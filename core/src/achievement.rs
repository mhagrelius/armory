//! Achievement criteria, and what they can be measured against.
//!
//! An achievement is a tree of criteria. Blizzard's *profile* response carries
//! the account's progress through that tree — `amount` counters and
//! `is_completed` flags, nested through `child_criteria` — which is enough to
//! say how far along the account is and useless for saying how far along a
//! particular character is once the account has already finished.
//!
//! What the profile response does not carry is *what each criterion measures*.
//! There is no asset id in the public API: a criterion knows it needs 100 of
//! something and not that the something is quests in Nagrand. That mapping lives
//! in the client database (`Criteria.Type` and `Criteria.Asset`, published as
//! CSV by wago.tools) and in the game's own Lua
//! (`GetAchievementCriteriaInfo` returns `criteriaType` and `assetID`).
//!
//! So [`Criterion::kind`] is filled in from a catalogue, and where it cannot be,
//! the criterion is honestly [`CriterionKind::Unknown`] rather than guessed. An
//! unknown criterion does not make an achievement untrackable — it makes it
//! *unobservable*, which is a different and much smaller claim, and it is what
//! sends a goal to attestation instead of to a wrong progress bar.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::provenance::EarnedReputation;

/// What a criterion is actually counting.
///
/// The numeric criteria types come from the client database. Only the ones
/// below are claimed, because a wrong mapping is worse than a missing one: a
/// missing mapping sends a goal to attestation, and a wrong one draws a
/// confident progress bar over a number that means something else. The list
/// grows as types are confirmed against real data, never by inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CriterionKind {
    /// Type 27. Asset is a quest id, and `/quests/completed` answers it per
    /// character — this is the single most valuable mapping in the table,
    /// because questing is what a replayed character mostly does.
    Quest(u32),
    /// Type 0. Asset is a creature id. Not directly answerable per character;
    /// kept distinct from `Unknown` because the statistics endpoint sometimes
    /// carries a matching counter.
    Creature(u32),
    /// Type 8. Asset is another achievement id, so the criterion recurses into
    /// whatever that achievement's own standing turns out to be.
    Achievement(u32),
    /// Type 46. Asset is a faction id. Answerable through `/reputations`, with
    /// the Warband caveat that makes the answer suspect — see
    /// [`Evaluation::inherited`].
    Reputation(u32),
    /// Backed by a per-character statistic, which `/achievements/statistics`
    /// answers directly.
    Statistic(u32),
    /// A dungeon or raid encounter, answerable through `/encounters`.
    Encounter(u32),
    /// The catalogue had no entry, or the type is one we do not claim to
    /// understand. Not a failure — a boundary.
    Unknown,
}

impl CriterionKind {
    /// Read a kind from the client database's `(Type, Asset)` pair.
    pub fn from_catalogue(criteria_type: u32, asset: u32) -> CriterionKind {
        match criteria_type {
            0 => CriterionKind::Creature(asset),
            8 => CriterionKind::Achievement(asset),
            27 => CriterionKind::Quest(asset),
            46 => CriterionKind::Reputation(asset),
            _ => CriterionKind::Unknown,
        }
    }

    /// Whether a character's own data can answer this criterion.
    pub fn is_observable(self) -> bool {
        matches!(
            self,
            CriterionKind::Quest(_)
                | CriterionKind::Statistic(_)
                | CriterionKind::Encounter(_)
                | CriterionKind::Reputation(_)
                | CriterionKind::Achievement(_)
        )
    }
}

/// One node of an achievement's criteria tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterion {
    pub id: u64,
    pub kind: CriterionKind,
    /// How many are needed. Zero means the criterion is a flag rather than a
    /// counter, and one of it is enough.
    pub required: u64,
    pub children: Vec<Criterion>,
}

impl Criterion {
    pub fn leaf(id: u64, kind: CriterionKind, required: u64) -> Self {
        Criterion {
            id,
            kind,
            required,
            children: Vec::new(),
        }
    }

    /// How many of this node must be satisfied, treating a flag as one.
    fn threshold(&self) -> u64 {
        self.required.max(1)
    }
}

/// The per-character data a criterion can be measured against.
///
/// Every field here is genuinely per character — that is the entire selection
/// criterion for what belongs in this struct. Anything account-wide would defeat
/// the purpose, because account-wide is the problem being worked around.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrimaryData {
    /// From `/quests/completed`, or the addon's
    /// `C_QuestLog.GetAllCompletedQuestIDs()`.
    pub quests: HashSet<u32>,
    /// From `/achievements/statistics`.
    pub statistics: HashMap<u32, f64>,
    /// From `/encounters/dungeons` and `/encounters/raids`.
    pub encounters: HashSet<u32>,
    /// From `/reputations`. Standing value per faction id.
    pub reputations: HashMap<u32, u32>,
    /// Faction ids whose standing exceeds anything this character could have
    /// earned during the run, because The War Within made most reputations
    /// account-wide and synced them to the furthest-progressed character.
    pub inherited_reputations: HashSet<u32>,
    /// What this character has personally been observed earning, per faction.
    ///
    /// The way out of the trap the field above describes. An inherited standing
    /// is unusable as a measurement *and* impossible to improve on — it was at
    /// the ceiling before the run began, so no amount of work can move it. This
    /// is the work, counted as it arrived, and it is what lets a run say "this
    /// character has earned the equivalent of Exalted" about a faction the
    /// account maxed out in 2023.
    ///
    /// From the addon and from nowhere else: no endpoint attributes a point of
    /// reputation to a character.
    pub earned_reputations: HashMap<u32, EarnedReputation>,
    /// Achievement ids *the run* has done.
    ///
    /// Not the account's — the account has almost everything, which is the
    /// whole problem. This is what makes a meta-achievement resolvable: its
    /// criteria are other achievements, and whether the run has those is a
    /// question about the run.
    ///
    /// Shared across the cohort rather than held per character, because an
    /// achievement is account-wide and a meta does not care which of your
    /// characters earned the parts.
    pub achievements_done: HashSet<u32>,
}

/// What evaluating a criterion tree against one character produced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evaluation {
    /// How much of the requirement this character has met.
    pub progress: u64,
    /// How much is needed.
    pub required: u64,
    /// Whether every criterion in the tree could actually be answered.
    ///
    /// False means the number above is a floor, not a measurement, and it must
    /// not be drawn as a progress bar.
    pub observable: bool,
    /// Whether any part of the answer came from a value the cohort did not
    /// earn. A reputation criterion satisfied by an alt from 2023 is not
    /// progress; reporting it as progress is the failure that would make the
    /// whole run meaningless.
    pub inherited: bool,
}

impl Evaluation {
    pub fn is_complete(&self) -> bool {
        self.observable && !self.inherited && self.progress >= self.required
    }

    pub fn fraction(&self) -> f64 {
        if self.required == 0 {
            return 0.0;
        }
        (self.progress as f64 / self.required as f64).clamp(0.0, 1.0)
    }
}

/// Measure a criteria tree against one character's own data.
///
/// A parent node's progress is how many of its children are satisfied, which is
/// how Blizzard's own counters behave: "complete 10 of these quests" is a parent
/// requiring 10 with a child per quest.
pub fn evaluate(criterion: &Criterion, data: &PrimaryData) -> Evaluation {
    if criterion.children.is_empty() {
        return evaluate_leaf(criterion, data);
    }

    let mut satisfied = 0;
    let mut observable = true;
    let mut inherited = false;

    for child in &criterion.children {
        let evaluation = evaluate(child, data);
        observable &= evaluation.observable;
        inherited |= evaluation.inherited;
        if evaluation.is_complete() {
            satisfied += 1;
        }
    }

    // A parent with no explicit requirement needs all of its children. That is
    // the common case: a meta-achievement lists the things it is made of and
    // wants every one.
    let required = if criterion.required == 0 {
        criterion.children.len() as u64
    } else {
        criterion.required
    };

    Evaluation {
        progress: satisfied,
        required,
        observable,
        inherited,
    }
}

fn evaluate_leaf(criterion: &Criterion, data: &PrimaryData) -> Evaluation {
    let required = criterion.threshold();
    let unobservable = Evaluation {
        progress: 0,
        required,
        observable: false,
        inherited: false,
    };

    match criterion.kind {
        CriterionKind::Quest(quest) => Evaluation {
            progress: u64::from(data.quests.contains(&quest)),
            required,
            observable: true,
            inherited: false,
        },
        CriterionKind::Encounter(encounter) => Evaluation {
            progress: u64::from(data.encounters.contains(&encounter)),
            required,
            observable: true,
            inherited: false,
        },
        CriterionKind::Statistic(statistic) => match data.statistics.get(&statistic) {
            Some(value) => Evaluation {
                progress: value.max(0.0) as u64,
                required,
                observable: true,
                inherited: false,
            },
            // The character has never done the thing at all, so the statistic
            // is absent rather than zero. That is still an observation.
            None => Evaluation {
                progress: 0,
                required,
                observable: true,
                inherited: false,
            },
        },
        CriterionKind::Reputation(faction) => {
            let standing = data.reputations.get(&faction).copied().unwrap_or(0);
            let earned = data.earned_reputations.get(&faction).copied();

            // An inherited standing is not a measurement *and* cannot become
            // one: it was at the ceiling before the run began, so no amount of
            // work will move it. What can move is what this character has been
            // observed earning, and where the addon has been watching, that is
            // the honest number to measure against.
            //
            // The distinction still stands where nothing has been observed.
            // Falling back to the account's standing there would be exactly the
            // inflation this whole field exists to prevent.
            if data.inherited_reputations.contains(&faction) {
                let earned = earned.unwrap_or_default();
                let observed = u64::from(earned.points);
                return Evaluation {
                    progress: observed,
                    required,
                    observable: true,
                    // No longer inherited once this character's own work covers
                    // the requirement — at that point the criterion is being
                    // answered by what they did, not by what they were given.
                    inherited: observed < required,
                };
            }

            Evaluation {
                progress: u64::from(standing),
                required,
                observable: true,
                inherited: false,
            }
        }
        // A meta-achievement's criteria are other achievements, and whether the
        // *run* has those is a question about the run rather than about the
        // account. This is what makes a dependency chain resolvable.
        CriterionKind::Achievement(achievement) => Evaluation {
            progress: u64::from(data.achievements_done.contains(&achievement)),
            required,
            observable: true,
            inherited: false,
        },
        CriterionKind::Creature(_) | CriterionKind::Unknown => unobservable,
    }
}

/// Attach catalogue kinds to a criteria tree read from the profile response.
///
/// The profile gives structure and progress; the catalogue gives meaning. They
/// are joined on the criterion id, and a criterion the catalogue has never heard
/// of keeps [`CriterionKind::Unknown`] rather than borrowing its parent's kind.
pub fn with_catalogue(criterion: &Criterion, catalogue: &HashMap<u64, CriterionKind>) -> Criterion {
    Criterion {
        id: criterion.id,
        kind: catalogue
            .get(&criterion.id)
            .copied()
            .unwrap_or(CriterionKind::Unknown),
        required: criterion.required,
        children: criterion
            .children
            .iter()
            .map(|child| with_catalogue(child, catalogue))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quests(ids: &[u32]) -> PrimaryData {
        PrimaryData {
            quests: ids.iter().copied().collect(),
            ..PrimaryData::default()
        }
    }

    #[test]
    fn a_quest_criterion_reads_the_characters_own_completed_list() {
        // The whole point: the account finished this years ago, the character
        // has not, and `/quests/completed` is per character so it still moves.
        let criterion = Criterion::leaf(1, CriterionKind::Quest(42), 1);
        assert!(evaluate(&criterion, &quests(&[42])).is_complete());
        assert!(!evaluate(&criterion, &quests(&[7])).is_complete());
    }

    #[test]
    fn a_parent_counts_how_many_children_are_satisfied() {
        let criterion = Criterion {
            id: 100,
            kind: CriterionKind::Unknown,
            required: 2,
            children: vec![
                Criterion::leaf(1, CriterionKind::Quest(1), 1),
                Criterion::leaf(2, CriterionKind::Quest(2), 1),
                Criterion::leaf(3, CriterionKind::Quest(3), 1),
            ],
        };

        let evaluation = evaluate(&criterion, &quests(&[1, 3]));
        assert_eq!(evaluation.progress, 2);
        assert_eq!(evaluation.required, 2);
        assert!(evaluation.is_complete());
    }

    #[test]
    fn a_parent_with_no_requirement_wants_all_of_its_children() {
        // Meta-achievements list what they are made of and want every one.
        let criterion = Criterion {
            id: 100,
            kind: CriterionKind::Unknown,
            required: 0,
            children: vec![
                Criterion::leaf(1, CriterionKind::Quest(1), 1),
                Criterion::leaf(2, CriterionKind::Quest(2), 1),
            ],
        };
        let evaluation = evaluate(&criterion, &quests(&[1]));
        assert_eq!(evaluation.required, 2);
        assert!(!evaluation.is_complete());
        assert!(evaluate(&criterion, &quests(&[1, 2])).is_complete());
    }

    #[test]
    fn one_unknown_child_makes_the_whole_tree_unobservable() {
        // A floor is not a measurement. Drawing three-quarters of a progress bar
        // over a tree whose fourth branch is unreadable is a confident lie, and
        // this is the assertion that keeps such a goal out of the observable
        // bucket entirely.
        let criterion = Criterion {
            id: 100,
            kind: CriterionKind::Unknown,
            required: 0,
            children: vec![
                Criterion::leaf(1, CriterionKind::Quest(1), 1),
                Criterion::leaf(2, CriterionKind::Unknown, 1),
            ],
        };
        let evaluation = evaluate(&criterion, &quests(&[1]));
        assert!(!evaluation.observable);
        assert!(!evaluation.is_complete());
    }

    #[test]
    fn an_inherited_reputation_is_never_progress() {
        // Warbands sync reputation to the furthest-progressed character, so a
        // standing of 42000 may belong to an alt nobody enrolled. Counting it
        // would inflate the run, which is the one failure that makes the whole
        // thing worthless.
        let criterion = Criterion::leaf(1, CriterionKind::Reputation(2170), 42000);
        let data = PrimaryData {
            reputations: HashMap::from([(2170, 42000)]),
            inherited_reputations: HashSet::from([2170]),
            ..PrimaryData::default()
        };

        let evaluation = evaluate(&criterion, &data);
        assert!(evaluation.inherited);
        assert!(!evaluation.is_complete(), "an alt's grind is not the run's");
    }

    #[test]
    fn a_character_can_earn_the_equivalent_of_a_standing_the_account_already_had() {
        // The whole reason provenance exists. The account has been Exalted with
        // these people since 2023, so the standing was at the ceiling before
        // the run began and *cannot move* however much work is done. Refusing
        // to count it is right; leaving it permanently unmeasurable is not.
        //
        // What can be measured is what this character was observed earning.
        let criterion = Criterion::leaf(1, CriterionKind::Reputation(2170), 42000);
        let inherited = HashSet::from([2170]);

        // Part way. Still inherited, because their own work has not covered it.
        let partway = PrimaryData {
            reputations: HashMap::from([(2170, 42000)]),
            inherited_reputations: inherited.clone(),
            earned_reputations: HashMap::from([(
                2170,
                EarnedReputation {
                    points: 21_000,
                    account_wide: true,
                    ..EarnedReputation::default()
                },
            )]),
            ..PrimaryData::default()
        };
        let evaluation = evaluate(&criterion, &partway);
        assert!(evaluation.inherited);
        assert!(!evaluation.is_complete());
        // And the bar moves, which it could never do before.
        assert_eq!(evaluation.progress, 21_000);

        // All the way. Their own work now covers the requirement, so the
        // criterion is being answered by what they did rather than by what
        // they were handed — and it stops being inherited.
        let done = PrimaryData {
            earned_reputations: HashMap::from([(
                2170,
                EarnedReputation {
                    points: 42_000,
                    account_wide: true,
                    ..EarnedReputation::default()
                },
            )]),
            ..partway
        };
        let evaluation = evaluate(&criterion, &done);
        assert!(!evaluation.inherited, "they did the work themselves");
        assert!(evaluation.is_complete());
    }

    #[test]
    fn an_inherited_standing_with_nothing_observed_stays_worth_nothing() {
        // The guard on the change above. Falling back to the account's standing
        // when the addon has seen nothing would be exactly the inflation this
        // module exists to prevent — and "no addon" is the common case.
        let criterion = Criterion::leaf(1, CriterionKind::Reputation(2170), 42000);
        let data = PrimaryData {
            reputations: HashMap::from([(2170, 42000)]),
            inherited_reputations: HashSet::from([2170]),
            ..PrimaryData::default()
        };

        let evaluation = evaluate(&criterion, &data);
        assert_eq!(evaluation.progress, 0);
        assert!(evaluation.inherited);
        assert!(!evaluation.is_complete());
    }

    #[test]
    fn an_uninherited_reputation_counts_normally() {
        let criterion = Criterion::leaf(1, CriterionKind::Reputation(2170), 42000);
        let data = PrimaryData {
            reputations: HashMap::from([(2170, 42000)]),
            ..PrimaryData::default()
        };
        assert!(evaluate(&criterion, &data).is_complete());
    }

    #[test]
    fn inheritance_propagates_up_the_tree() {
        // A meta whose only tainted branch is three levels down is still tainted.
        let criterion = Criterion {
            id: 100,
            kind: CriterionKind::Unknown,
            required: 0,
            children: vec![Criterion {
                id: 10,
                kind: CriterionKind::Unknown,
                required: 0,
                children: vec![Criterion::leaf(1, CriterionKind::Reputation(5), 1)],
            }],
        };
        let data = PrimaryData {
            reputations: HashMap::from([(5, 100)]),
            inherited_reputations: HashSet::from([5]),
            ..PrimaryData::default()
        };
        assert!(evaluate(&criterion, &data).inherited);
    }

    #[test]
    fn a_missing_statistic_is_zero_and_still_an_observation() {
        // Never having killed the thing is a fact about the character, not a
        // gap in our data.
        let criterion = Criterion::leaf(1, CriterionKind::Statistic(1337), 10);
        let evaluation = evaluate(&criterion, &PrimaryData::default());
        assert!(evaluation.observable);
        assert_eq!(evaluation.progress, 0);
    }

    #[test]
    fn the_catalogue_supplies_meaning_and_never_invents_it() {
        // The profile response gives structure; the catalogue gives kinds. A
        // criterion the catalogue has not heard of stays Unknown rather than
        // inheriting anything from its parent.
        let tree = Criterion {
            id: 100,
            kind: CriterionKind::Unknown,
            required: 0,
            children: vec![
                Criterion::leaf(1, CriterionKind::Unknown, 1),
                Criterion::leaf(2, CriterionKind::Unknown, 1),
            ],
        };
        let catalogue = HashMap::from([(1u64, CriterionKind::Quest(500))]);

        let joined = with_catalogue(&tree, &catalogue);
        assert_eq!(joined.children[0].kind, CriterionKind::Quest(500));
        assert_eq!(joined.children[1].kind, CriterionKind::Unknown);
    }

    #[test]
    fn only_the_criteria_types_we_have_confirmed_are_claimed() {
        assert_eq!(
            CriterionKind::from_catalogue(27, 500),
            CriterionKind::Quest(500)
        );
        assert_eq!(
            CriterionKind::from_catalogue(46, 2170),
            CriterionKind::Reputation(2170)
        );
        // An unclaimed type degrades to Unknown rather than being guessed at.
        assert_eq!(
            CriterionKind::from_catalogue(119, 9),
            CriterionKind::Unknown
        );
    }

    #[test]
    fn a_fraction_never_exceeds_one() {
        let evaluation = Evaluation {
            progress: 30,
            required: 10,
            observable: true,
            inherited: false,
        };
        assert_eq!(evaluation.fraction(), 1.0);
    }
}
