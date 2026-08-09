//! Building a run's goals, and deciding what each one costs to track.
//!
//! This is where [`crate::run`]'s three cases become a list. Everything
//! here is a pure function over data that has already been fetched — no
//! network, no storage — which is what makes the classification testable
//! against the awkward cases rather than against whatever the account happens
//! to contain today.
//!
//! The order matters and it is the whole point. Standing is decided first,
//! because it is cheap and because it settles most goals outright. Only what is
//! left — the poisoned ones — reaches [`classify`], which is the expensive half.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use super::achievement::{evaluate, Criterion, CriterionKind, PrimaryData};
use super::character::CharacterKey;
use super::cohort::Cohort;
use super::provenance::Provenance;
use super::run::{Baseline, Bucket, Exclusion, Goal, Run, Standing};
use super::source::blizzard::gamedata::Achievement;
use super::source::blizzard::profile::AchievementProgress;

/// Everything the planner needs, gathered by the caller.
///
/// A struct rather than eight arguments because every one of these is fetched
/// separately and half of them can be missing — an account with no addon has no
/// attributions, and a catalogue that has not synced yet has no achievements.
/// Missing data must degrade the classification, never stop it.
#[derive(Debug, Default)]
pub struct Inputs {
    /// What the account has done, from `/achievements`.
    pub progress: Vec<AchievementProgress>,
    /// Names, points, and whether a thing can be earned twice.
    pub catalogue: HashMap<u32, Achievement>,
    /// Which character earned each account-wide achievement. Empty without the
    /// addon, which is what makes everything already-earned poisoned.
    pub attributions: HashMap<u32, CharacterKey>,
    /// Criterion id to what it measures, from the client database or the addon.
    pub criteria: HashMap<u64, CriterionKind>,
    /// Per-character primary data, for the enrolled cohort.
    pub primary: HashMap<CharacterKey, PrimaryData>,
    /// What each character has personally been observed earning.
    ///
    /// Merged into [`Inputs::primary`] before planning rather than being read
    /// straight from it, because the two arrive from different places at
    /// different times: primary data comes from a sync or a character file, and
    /// this comes from the account-wide addon table. Keeping them apart until
    /// the last moment means a sync that lands first does not wipe it.
    pub provenance: Provenance,
    /// Collectible ids the account already owns, from the baseline.
    pub owned: HashSet<u32>,
    /// Goals the user has excluded by hand. Kept across a rebuild, because a
    /// decision a person made should survive a resync.
    pub excluded_by_hand: HashSet<u32>,
}

/// How many times to re-resolve dependency chains.
///
/// A meta-achievement's criteria are other achievements, which may themselves
/// be metas. Each pass settles one more layer. Warcraft's chains are two or
/// three deep — a zone meta inside an expansion meta inside a "complete them
/// all" — so four passes reaches a fixpoint with room to spare, and the bound
/// is what stops a cycle in the data from spinning forever.
const CHAIN_PASSES: usize = 4;

/// Build the goal list for a run.
///
/// Two phases. The first classifies every goal and measures the ones that stand
/// alone. The second resolves dependency chains: a meta cannot be judged until
/// its parts have been, so the parts' outcomes are fed back in and the metas
/// re-measured, repeatedly, until nothing more moves.
pub fn plan(baseline: &Baseline, cohort: &Cohort, inputs: &Inputs) -> Vec<Goal> {
    let mut goals = plan_once(baseline, cohort, inputs, &HashSet::new());

    for _ in 0..CHAIN_PASSES {
        let done: HashSet<u32> = goals
            .iter()
            .filter(|goal| goal.is_done())
            .map(|goal| goal.achievement_id)
            .collect();

        let next = plan_once(baseline, cohort, inputs, &done);
        let settled: HashSet<u32> = next
            .iter()
            .filter(|goal| goal.is_done())
            .map(|goal| goal.achievement_id)
            .collect();

        let moved = settled != done;
        goals = next;
        if !moved {
            break;
        }
    }

    goals
}

/// One pass, against a fixed idea of what the run has already done.
fn plan_once(
    baseline: &Baseline,
    cohort: &Cohort,
    inputs: &Inputs,
    done: &HashSet<u32>,
) -> Vec<Goal> {
    let completed: HashMap<u32, DateTime<Utc>> = baseline.completed.iter().copied().collect();

    inputs
        .progress
        .iter()
        .map(|progress| {
            // The baseline's record of when something was finished beats the
            // live response: the live one moves as the run proceeds, and
            // standing has to be decided against a fixed point or a goal could
            // un-poison itself halfway through.
            let completed_at = completed
                .get(&progress.id)
                .copied()
                .or(progress.completed_at);

            let standing = Standing::classify(
                completed_at,
                inputs.attributions.get(&progress.id),
                cohort,
                baseline.taken_at,
            );

            let bucket = if standing.is_poisoned() {
                classify(progress, inputs)
            } else {
                // Never read for an unpoisoned goal. Observable is the neutral
                // value rather than a fourth "not applicable" variant that
                // every match would have to carry.
                Bucket::Observable
            };

            let evaluation = match (&bucket, standing.is_poisoned()) {
                (Bucket::Observable, true) => best_evaluation(progress, cohort, inputs, done),
                _ => None,
            };

            Goal {
                achievement_id: progress.id,
                standing,
                bucket,
                attestation: None,
                nearest: evaluation.as_ref().map(|(_, who)| who.clone()),
                evaluation: evaluation.map(|(evaluation, _)| evaluation),
            }
        })
        .collect()
}

/// Decide how a poisoned goal can be tracked.
///
/// Called only for poisoned goals. The order of the checks is the order of
/// cost: an exclusion is a lookup, observability needs the criteria tree, and
/// attestation is what is left.
pub fn classify(progress: &AchievementProgress, inputs: &Inputs) -> Bucket {
    if inputs.excluded_by_hand.contains(&progress.id) {
        return Bucket::Excluded(Exclusion::ByHand);
    }

    // A Feat of Strength or a legacy achievement can never be earned again by
    // anybody. Leaving it in the backlog would be leaving a row that can only
    // ever read zero.
    if inputs
        .catalogue
        .get(&progress.id)
        .is_some_and(|achievement| achievement.is_unrepeatable)
    {
        return Bucket::Excluded(Exclusion::Unrepeatable);
    }

    if inputs.owned.contains(&progress.id) {
        return Bucket::Excluded(Exclusion::AlreadyOwned);
    }

    // With no criteria tree at all there is nothing to measure and nothing to
    // hope for. That is attestable rather than excluded: a person may well
    // remember doing it, and excluding it would decide on their behalf.
    let Some(criteria) = &progress.criteria else {
        return Bucket::Attestable;
    };

    if is_observable(criteria, &inputs.criteria) {
        Bucket::Observable
    } else {
        Bucket::Attestable
    }
}

/// Whether every leaf of a criteria tree resolves to something measurable.
///
/// One unknown leaf is enough to make the whole tree unobservable. That is not
/// pessimism for its own sake: a partial count drawn as a progress bar is a
/// confident claim about a number nobody computed.
fn is_observable(criterion: &Criterion, catalogue: &HashMap<u64, CriterionKind>) -> bool {
    if criterion.children.is_empty() {
        return catalogue
            .get(&criterion.id)
            .copied()
            .unwrap_or(CriterionKind::Unknown)
            .is_observable();
    }
    criterion
        .children
        .iter()
        .all(|child| is_observable(child, catalogue))
}

/// Measure a goal against whichever enrolled character is furthest along, and
/// say which one that was.
///
/// A run is about the cohort rather than about one character, so a goal a
/// person is working on across two alts should show the better of the two.
/// Taking the maximum is the only reading that does not punish having a roster.
///
/// The character comes back with the measurement because the figure is
/// meaningless without them: "eleven quests to go" is a fact about somebody in
/// particular, and a page that shows the number without the name invites it
/// being read as the account's.
fn best_evaluation(
    progress: &AchievementProgress,
    cohort: &Cohort,
    inputs: &Inputs,
    done: &HashSet<u32>,
) -> Option<(super::achievement::Evaluation, CharacterKey)> {
    let criteria = progress.criteria.as_ref()?;
    let tree = super::achievement::with_catalogue(criteria, &inputs.criteria);

    cohort
        .keys()
        .filter_map(|key| inputs.primary.get(key).map(|data| (key, data)))
        .map(|(key, data)| {
            // Two things folded into each character's own data at the last
            // moment, both because they arrive from somewhere else.
            //
            // What the *run* has done: an achievement is account-wide, so a
            // meta does not care which character earned its parts, only whether
            // the run has them.
            //
            // And what this character personally earned, which is the only
            // thing that can measure a reputation the account maxed out before
            // the run began. Keyed per character, so it must be looked up here
            // rather than baked into the primary data by whoever built it.
            let data = PrimaryData {
                achievements_done: done.clone(),
                earned_reputations: inputs
                    .provenance
                    .get(key)
                    .map(|earned| earned.reputation.clone())
                    .unwrap_or_default(),
                ..data.clone()
            };
            (evaluate(&tree, &data), key.clone())
        })
        .max_by(|(a, _), (b, _)| {
            // Complete beats incomplete; otherwise the further along wins. An
            // inherited evaluation loses to anything genuine, because counting
            // an alt's reputation as the cohort's best is exactly the inflation
            // this whole module exists to avoid.
            a.is_complete()
                .cmp(&b.is_complete())
                .then_with(|| b.inherited.cmp(&a.inherited))
                .then_with(|| {
                    a.fraction()
                        .partial_cmp(&b.fraction())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        })
}

/// Re-measure an existing run's observable goals against fresh primary data.
///
/// Standing and bucket are left alone. Poisoning is decided once at baseline
/// and cannot change; a bucket the user has overruled must not be silently
/// re-decided; and attestations are a person's word, which no sync overrides.
pub fn remeasure(run: &mut Run, inputs: &Inputs) {
    // Chains again: a meta's parts may have been finished since the last pass,
    // and re-measuring the meta without them would leave it stuck.
    let mut done: HashSet<u32> = run
        .goals
        .iter()
        .filter(|goal| goal.is_done())
        .map(|goal| goal.achievement_id)
        .collect();

    let by_id: HashMap<u32, &AchievementProgress> = inputs
        .progress
        .iter()
        .map(|progress| (progress.id, progress))
        .collect();

    for goal in &mut run.goals {
        if goal.bucket != Bucket::Observable || !goal.standing.is_poisoned() {
            continue;
        }
        let Some(progress) = by_id.get(&goal.achievement_id) else {
            continue;
        };
        let best = best_evaluation(progress, &run.cohort, inputs, &done);
        goal.nearest = best.as_ref().map(|(_, who)| who.clone());
        goal.evaluation = best.map(|(evaluation, _)| evaluation);
        if goal.is_done() {
            done.insert(goal.achievement_id);
        }
    }
}

/// Take a baseline from what the account currently has.
pub fn take_baseline(
    progress: &[AchievementProgress],
    collected: &HashSet<u32>,
    at: DateTime<Utc>,
) -> Baseline {
    Baseline {
        taken_at: at,
        collected: {
            let mut owned: Vec<u32> = collected.iter().copied().collect();
            owned.sort_unstable();
            owned
        },
        completed: {
            let mut completed: Vec<(u32, DateTime<Utc>)> = progress
                .iter()
                .filter_map(|entry| Some((entry.id, entry.completed_at?)))
                .collect();
            completed.sort_unstable();
            completed
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("a timestamp")
            .to_utc()
    }

    fn baseline() -> Baseline {
        Baseline {
            taken_at: at("2026-06-01T00:00:00Z"),
            collected: Vec::new(),
            completed: Vec::new(),
        }
    }

    fn somechar() -> CharacterKey {
        CharacterKey::new("emerald-dream", "Somechar")
    }

    fn aeltor() -> CharacterKey {
        CharacterKey::new("mannoroth", "Aeltor")
    }

    fn cohort() -> Cohort {
        Cohort::from(vec![somechar()])
    }

    /// An achievement made of three quests.
    fn quest_achievement(id: u32, completed_at: Option<&str>) -> AchievementProgress {
        AchievementProgress {
            id,
            completed_at: completed_at.map(at),
            criteria: Some(Criterion {
                id: 900,
                kind: CriterionKind::Unknown,
                required: 0,
                children: vec![
                    Criterion::leaf(901, CriterionKind::Unknown, 1),
                    Criterion::leaf(902, CriterionKind::Unknown, 1),
                    Criterion::leaf(903, CriterionKind::Unknown, 1),
                ],
            }),
        }
    }

    fn quest_criteria() -> HashMap<u64, CriterionKind> {
        HashMap::from([
            (901, CriterionKind::Quest(1)),
            (902, CriterionKind::Quest(2)),
            (903, CriterionKind::Quest(3)),
        ])
    }

    fn primary(quests: &[u32]) -> HashMap<CharacterKey, PrimaryData> {
        HashMap::from([(
            somechar(),
            PrimaryData {
                quests: quests.iter().copied().collect(),
                ..PrimaryData::default()
            },
        )])
    }

    #[test]
    fn an_unearned_achievement_is_planned_without_touching_the_expensive_path() {
        let inputs = Inputs {
            progress: vec![quest_achievement(1, None)],
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);

        assert_eq!(goals[0].standing, Standing::Unearned);
        assert!(!goals[0].standing.is_poisoned());
        // No evaluation was computed, because none was needed.
        assert!(goals[0].evaluation.is_none());
    }

    #[test]
    fn an_outsiders_achievement_is_poisoned_and_measured_from_primary_data() {
        // The heart of it: the account finished this in 2016 on a character
        // nobody enrolled, so the flag is dead — but Somechar's own quest list
        // still moves.
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            criteria: quest_criteria(),
            primary: primary(&[1, 2]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);

        assert!(goals[0].standing.is_poisoned());
        assert_eq!(goals[0].bucket, Bucket::Observable);
        let evaluation = goals[0].evaluation.expect("an evaluation");
        assert_eq!(evaluation.progress, 2);
        assert_eq!(evaluation.required, 3);
        assert!(!goals[0].is_done());
    }

    #[test]
    fn an_enrolled_characters_achievement_is_settled_and_never_measured() {
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, somechar())]),
            criteria: quest_criteria(),
            primary: primary(&[]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);

        assert!(goals[0].standing.is_settled());
        assert!(goals[0].is_done());
        // Nothing was computed: it did not need to be.
        assert!(goals[0].evaluation.is_none());
    }

    #[test]
    fn without_the_addon_everything_already_earned_falls_to_the_expensive_path() {
        // The cost of not running the collector, made visible. No attributions
        // means every old achievement is assumed poisoned.
        let inputs = Inputs {
            progress: vec![
                quest_achievement(1, Some("2016-03-04T00:00:00Z")),
                quest_achievement(2, Some("2017-03-04T00:00:00Z")),
            ],
            criteria: quest_criteria(),
            primary: primary(&[]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);
        assert_eq!(goals.iter().filter(|g| g.standing.is_poisoned()).count(), 2);
    }

    #[test]
    fn a_criterion_the_catalogue_cannot_explain_sends_the_goal_to_attestation() {
        // One unmapped leaf out of three. A bar drawn over the other two would
        // be a claim about a number nobody computed.
        let mut criteria = quest_criteria();
        criteria.remove(&903);

        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            criteria,
            primary: primary(&[1, 2]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);

        assert_eq!(goals[0].bucket, Bucket::Attestable);
        assert_eq!(goals[0].fraction(), None);
    }

    #[test]
    fn an_achievement_with_no_criteria_at_all_is_attestable_not_excluded() {
        // Nothing measures it, but a person may well remember doing it, and
        // excluding it would be deciding that on their behalf.
        let inputs = Inputs {
            progress: vec![AchievementProgress {
                id: 1,
                completed_at: Some(at("2016-03-04T00:00:00Z")),
                criteria: None,
            }],
            attributions: HashMap::from([(1, aeltor())]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);
        assert_eq!(goals[0].bucket, Bucket::Attestable);
    }

    #[test]
    fn a_feat_of_strength_leaves_the_run_entirely() {
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            catalogue: HashMap::from([(
                1,
                Achievement {
                    id: 1,
                    name: "Gone".into(),
                    category: "Feats of Strength".into(),
                    points: 0,
                    description: String::new(),
                    is_unrepeatable: true,
                },
            )]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);

        assert_eq!(goals[0].bucket, Bucket::Excluded(Exclusion::Unrepeatable));
        assert!(!goals[0].counts());
    }

    #[test]
    fn a_hand_exclusion_beats_everything_else() {
        // A decision the person made outranks anything derived, and it survives
        // a resync.
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            criteria: quest_criteria(),
            primary: primary(&[1, 2, 3]),
            excluded_by_hand: HashSet::from([1]),
            ..Inputs::default()
        };
        let goals = plan(&baseline(), &cohort(), &inputs);
        assert_eq!(goals[0].bucket, Bucket::Excluded(Exclusion::ByHand));
    }

    #[test]
    fn the_furthest_along_character_in_the_cohort_is_the_one_that_counts() {
        // A run is about the cohort, not one character. Taking anything but the
        // best would punish having a roster.
        let cohort = Cohort::from(vec![somechar(), aeltor()]);
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            // Earned by somebody in neither position — a third character.
            attributions: HashMap::from([(1, CharacterKey::new("dalaran", "Moodivh"))]),
            criteria: quest_criteria(),
            primary: HashMap::from([
                (
                    somechar(),
                    PrimaryData {
                        quests: HashSet::from([1]),
                        ..PrimaryData::default()
                    },
                ),
                (
                    aeltor(),
                    PrimaryData {
                        quests: HashSet::from([1, 2, 3]),
                        ..PrimaryData::default()
                    },
                ),
            ]),
            ..Inputs::default()
        };

        let goals = plan(&baseline(), &cohort, &inputs);
        assert!(goals[0].is_done(), "Aeltor finished it during the run");
    }

    #[test]
    fn a_baseline_completion_beats_the_live_response() {
        // The live timestamp moves as the run proceeds. Deciding standing
        // against it would let a goal un-poison itself halfway through.
        let mut base = baseline();
        base.completed = vec![(1, at("2016-03-04T00:00:00Z"))];

        let inputs = Inputs {
            // The live response now claims it was finished after the baseline.
            progress: vec![quest_achievement(1, Some("2026-07-01T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            criteria: quest_criteria(),
            ..Inputs::default()
        };

        let goals = plan(&base, &cohort(), &inputs);
        assert!(
            goals[0].standing.is_poisoned(),
            "the baseline says 2016, and the baseline is the fixed point"
        );
    }

    #[test]
    fn remeasuring_moves_progress_and_leaves_decisions_alone() {
        let inputs = Inputs {
            progress: vec![quest_achievement(1, Some("2016-03-04T00:00:00Z"))],
            attributions: HashMap::from([(1, aeltor())]),
            criteria: quest_criteria(),
            primary: primary(&[1]),
            ..Inputs::default()
        };
        let mut run = Run {
            name: "Fresh start".into(),
            baseline: baseline(),
            cohort: cohort(),
            goals: plan(&baseline(), &cohort(), &inputs),
        };
        assert_eq!(run.goals[0].evaluation.expect("one").progress, 1);

        // Two more quests done since.
        let later = Inputs {
            primary: primary(&[1, 2, 3]),
            ..inputs
        };
        remeasure(&mut run, &later);

        assert_eq!(run.goals[0].evaluation.expect("one").progress, 3);
        assert!(run.goals[0].is_done());
        assert!(run.goals[0].standing.is_poisoned(), "standing never moves");
    }

    #[test]
    fn remeasuring_never_overwrites_an_attestation() {
        // A person's word is not something a sync gets to revisit.
        let inputs = Inputs {
            progress: vec![AchievementProgress {
                id: 1,
                completed_at: Some(at("2016-03-04T00:00:00Z")),
                criteria: None,
            }],
            attributions: HashMap::from([(1, aeltor())]),
            ..Inputs::default()
        };
        let mut run = Run {
            name: "Fresh start".into(),
            baseline: baseline(),
            cohort: cohort(),
            goals: plan(&baseline(), &cohort(), &inputs),
        };
        run.goals[0].attestation = Some(super::super::run::Attestation {
            character: somechar(),
            at: at("2026-07-20T00:00:00Z"),
        });

        remeasure(&mut run, &inputs);
        assert!(run.goals[0].attestation.is_some());
        assert!(run.goals[0].is_done());
    }

    #[test]
    fn a_meta_resolves_once_its_parts_do() {
        // The dependency chain. A meta's criteria are other achievements, so it
        // cannot be judged until they have been — and one pass over the list
        // would leave it stuck at zero forever.
        let part = |id: u32| AchievementProgress {
            id,
            completed_at: Some(at("2016-03-04T00:00:00Z")),
            criteria: Some(Criterion::leaf(
                u64::from(id) * 10,
                CriterionKind::Unknown,
                1,
            )),
        };

        let inputs = Inputs {
            progress: vec![
                part(1),
                part(2),
                // The meta: needs both of the above.
                AchievementProgress {
                    id: 3,
                    completed_at: Some(at("2016-03-04T00:00:00Z")),
                    criteria: Some(Criterion {
                        id: 30,
                        kind: CriterionKind::Unknown,
                        required: 0,
                        children: vec![
                            Criterion::leaf(31, CriterionKind::Unknown, 1),
                            Criterion::leaf(32, CriterionKind::Unknown, 1),
                        ],
                    }),
                },
            ],
            attributions: HashMap::from([(1, aeltor()), (2, aeltor()), (3, aeltor())]),
            criteria: HashMap::from([
                (10, CriterionKind::Quest(101)),
                (20, CriterionKind::Quest(102)),
                (31, CriterionKind::Achievement(1)),
                (32, CriterionKind::Achievement(2)),
            ]),
            primary: primary(&[101, 102]),
            ..Inputs::default()
        };

        let goals = plan(&baseline(), &cohort(), &inputs);
        let meta = goals.iter().find(|goal| goal.achievement_id == 3).unwrap();

        assert!(goals
            .iter()
            .find(|g| g.achievement_id == 1)
            .unwrap()
            .is_done());
        assert!(goals
            .iter()
            .find(|g| g.achievement_id == 2)
            .unwrap()
            .is_done());
        assert!(meta.is_done(), "the meta follows its parts");
    }

    #[test]
    fn a_meta_stays_open_while_a_part_is_outstanding() {
        let inputs = Inputs {
            progress: vec![
                AchievementProgress {
                    id: 1,
                    completed_at: Some(at("2016-03-04T00:00:00Z")),
                    criteria: Some(Criterion::leaf(10, CriterionKind::Unknown, 1)),
                },
                AchievementProgress {
                    id: 3,
                    completed_at: Some(at("2016-03-04T00:00:00Z")),
                    criteria: Some(Criterion {
                        id: 30,
                        kind: CriterionKind::Unknown,
                        required: 0,
                        children: vec![Criterion::leaf(31, CriterionKind::Unknown, 1)],
                    }),
                },
            ],
            attributions: HashMap::from([(1, aeltor()), (3, aeltor())]),
            criteria: HashMap::from([
                (10, CriterionKind::Quest(101)),
                (31, CriterionKind::Achievement(1)),
            ]),
            // The part's quest has not been done.
            primary: primary(&[]),
            ..Inputs::default()
        };

        let goals = plan(&baseline(), &cohort(), &inputs);
        assert!(!goals
            .iter()
            .find(|g| g.achievement_id == 3)
            .unwrap()
            .is_done());
    }

    #[test]
    fn a_cycle_in_the_chain_terminates_rather_than_spinning() {
        // Two metas that each require the other. Blizzard's data should never
        // contain this, and the bound on the passes is what makes "should" not
        // matter.
        let inputs = Inputs {
            progress: vec![
                AchievementProgress {
                    id: 1,
                    completed_at: Some(at("2016-03-04T00:00:00Z")),
                    criteria: Some(Criterion::leaf(10, CriterionKind::Unknown, 1)),
                },
                AchievementProgress {
                    id: 2,
                    completed_at: Some(at("2016-03-04T00:00:00Z")),
                    criteria: Some(Criterion::leaf(20, CriterionKind::Unknown, 1)),
                },
            ],
            attributions: HashMap::from([(1, aeltor()), (2, aeltor())]),
            criteria: HashMap::from([
                (10, CriterionKind::Achievement(2)),
                (20, CriterionKind::Achievement(1)),
            ]),
            primary: primary(&[]),
            ..Inputs::default()
        };

        let goals = plan(&baseline(), &cohort(), &inputs);
        assert_eq!(goals.len(), 2);
        assert!(goals.iter().all(|goal| !goal.is_done()));
    }

    #[test]
    fn a_baseline_records_what_was_finished_and_when() {
        let progress = vec![
            quest_achievement(1, Some("2016-03-04T00:00:00Z")),
            quest_achievement(2, None),
        ];
        let baseline = take_baseline(
            &progress,
            &HashSet::from([100, 200]),
            at("2026-06-01T00:00:00Z"),
        );

        assert_eq!(baseline.completed.len(), 1);
        assert_eq!(baseline.completed[0].0, 1);
        assert_eq!(baseline.collected, [100, 200]);
    }
}
