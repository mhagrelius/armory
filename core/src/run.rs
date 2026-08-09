//! The soft reset: replaying content an account already remembers.
//!
//! The account says done. The run has not done it. Reading a completion flag
//! would produce an empty backlog on day one, which is exactly the problem.
//!
//! But most of the account is not a problem, and the cheap path matters more
//! than the clever one. Three cases:
//!
//! - **Not yet earned by anyone.** The flag behaves as designed. A cohort
//!   character finishes it, it lights up. Read it and move on.
//! - **Already earned, by an enrolled character.** The run has it. Read it and
//!   move on.
//! - **Already earned, by someone outside the cohort.** The flag is permanently
//!   useless. It was set before the run began and will never change again,
//!   because a second character completing an account-wide achievement produces
//!   *no signal at all* — no per-character shadow copy, no second timestamp, no
//!   event. `earnedBy` names whoever earned it first and goes on naming them
//!   however many times the content is replayed.
//!
//! Only the third case is [`Standing::Poisoned`], and only poisoned goals reach
//! the expensive machinery in [`Bucket`]. On a fresh Battle.net account nothing
//! is poisoned and none of this costs anything.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::achievement::Evaluation;
use super::character::CharacterKey;
use super::cohort::Cohort;

/// Where an achievement stands relative to *this run*, as distinct from where
/// it stands relative to the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standing {
    /// Nobody on the account has it. The flag works normally from here.
    Unearned,
    /// Earned after the baseline was taken, so it belongs to the run by
    /// definition — nobody but the cohort has been playing.
    EarnedDuringRun { at: DateTime<Utc> },
    /// Earned before the baseline, by a character who is in the cohort. The run
    /// has it and nothing further needs computing.
    EarnedByCohort { by: CharacterKey },
    /// Earned before the baseline by someone outside the cohort, or by someone
    /// we cannot identify. The flag will never move again.
    ///
    /// `by` is `None` when the addon has not reported attribution. That is the
    /// pessimistic case and it is deliberately pessimistic: without
    /// `GetAchievementInfo`'s `earnedBy` the web API exposes no attribution at
    /// all, so every already-earned achievement has to be assumed poisoned. The
    /// addon is what shrinks this set to the ones that genuinely are.
    Poisoned { by: Option<CharacterKey> },
}

impl Standing {
    /// Decide where an achievement stands.
    ///
    /// `earned_by` comes from the addon and is `None` when it has not run.
    pub fn classify(
        completed_at: Option<DateTime<Utc>>,
        earned_by: Option<&CharacterKey>,
        cohort: &Cohort,
        baseline_taken_at: DateTime<Utc>,
    ) -> Standing {
        let Some(completed_at) = completed_at else {
            return Standing::Unearned;
        };

        // Anything finished since the baseline was taken belongs to the run.
        // There is nobody else playing this account, so attribution is not
        // needed and poisoning cannot appear after the fact.
        if completed_at >= baseline_taken_at {
            return Standing::EarnedDuringRun { at: completed_at };
        }

        match earned_by {
            Some(key) if cohort.contains(key) => Standing::EarnedByCohort { by: key.clone() },
            Some(key) => Standing::Poisoned {
                by: Some(key.clone()),
            },
            None => Standing::Poisoned { by: None },
        }
    }

    /// Whether the run already has this, with no further work.
    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            Standing::EarnedDuringRun { .. } | Standing::EarnedByCohort { .. }
        )
    }

    /// Whether the expensive classification in [`Bucket`] applies.
    pub fn is_poisoned(&self) -> bool {
        matches!(self, Standing::Poisoned { .. })
    }
}

/// Why a poisoned goal has been taken out of the run entirely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exclusion {
    /// An account-wide collectible the account already owns. Many bind-on-pickup
    /// mounts will not drop again at all for such an account, so this is not a
    /// difficulty rating — it is an impossibility.
    AlreadyOwned,
    /// A Feat of Strength, or content that no longer exists.
    Unrepeatable,
    /// Nothing measures it and nobody could honestly attest to it either.
    Unmeasurable,
    /// The user excluded it.
    ByHand,
}

impl Exclusion {
    pub fn label(&self) -> &'static str {
        match self {
            Exclusion::AlreadyOwned => "already collected on this account",
            Exclusion::Unrepeatable => "cannot be earned again",
            Exclusion::Unmeasurable => "no way to measure progress",
            Exclusion::ByHand => "excluded by you",
        }
    }
}

/// How a poisoned goal is tracked, once it is known to be poisoned.
///
/// Unpoisoned goals never enter this classification at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Bucket {
    /// Every criterion resolves against per-character data. Progress is
    /// computed and a bar can honestly be drawn.
    Observable,
    /// No per-character signal exists, but a person knows whether they did it.
    Attestable,
    /// Out of the run.
    Excluded(Exclusion),
}

/// Someone saying they did it, because nothing else can say so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    pub character: CharacterKey,
    pub at: DateTime<Utc>,
}

/// One thing the run is trying to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub achievement_id: u32,
    pub standing: Standing,
    /// Only meaningful when the standing is poisoned.
    pub bucket: Bucket,
    /// Only meaningful when the bucket is attestable.
    pub attestation: Option<Attestation>,
    /// Whichever enrolled character the evaluation below was measured against.
    ///
    /// Not serialised, for the same reason as the evaluation: it is recomputed
    /// from primary data on every sync, and a name on disk would outlive the
    /// measurement it belongs to. Kept beside the figure rather than folded into
    /// it because `Evaluation` is `Copy` and a character key is not.
    #[serde(skip)]
    pub nearest: Option<crate::character::CharacterKey>,
    /// Only meaningful when the bucket is observable. Not serialised: it is
    /// recomputed from primary data on every sync, and a stale copy on disk
    /// would outlive the data it was derived from.
    #[serde(skip)]
    pub evaluation: Option<Evaluation>,
}

impl Goal {
    /// Whether the run has done this.
    pub fn is_done(&self) -> bool {
        if self.standing.is_settled() {
            return true;
        }
        if !self.standing.is_poisoned() {
            return false;
        }
        match &self.bucket {
            // An excluded goal is not done, it is gone. Counting it as done
            // would inflate the run exactly as badly as counting an alt's
            // reputation does.
            Bucket::Excluded(_) => false,
            Bucket::Attestable => self.attestation.is_some(),
            Bucket::Observable => self
                .evaluation
                .as_ref()
                .is_some_and(Evaluation::is_complete),
        }
    }

    /// Whether this goal is part of what the run is measured against.
    pub fn counts(&self) -> bool {
        !matches!(self.bucket, Bucket::Excluded(_)) || !self.standing.is_poisoned()
    }

    /// How far along, for a progress bar. `None` when no honest bar can be drawn.
    pub fn fraction(&self) -> Option<f64> {
        if self.is_done() {
            return Some(1.0);
        }
        match self.bucket {
            Bucket::Observable => self.evaluation.as_ref().and_then(|evaluation| {
                (evaluation.observable && !evaluation.inherited).then(|| evaluation.fraction())
            }),
            _ => None,
        }
    }
}

/// What the account had when the run began.
///
/// Immutable once taken. This is what makes "already owned, therefore excluded"
/// a decidable question rather than a moving one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub taken_at: DateTime<Utc>,
    /// Mount, pet and toy ids the account already had.
    pub collected: Vec<u32>,
    /// Achievement ids already complete, and when.
    pub completed: Vec<(u32, DateTime<Utc>)>,
}

/// A pass through the account's content, with its own idea of what is done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub name: String,
    pub baseline: Baseline,
    pub cohort: Cohort,
    pub goals: Vec<Goal>,
}

/// What the run has done, and what it has decided not to try.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    /// Goals the run is measured against, including the ones not yet done.
    pub counted: usize,
    /// Goals removed from the run because they cannot be earned again.
    pub excluded: usize,
    /// Goals awaiting a person's word, because nothing measures them.
    pub awaiting_attestation: usize,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.counted == 0 {
            return 0.0;
        }
        self.done as f64 / self.counted as f64
    }
}

impl Run {
    /// Count the run up.
    ///
    /// Excluded goals are outside the denominator, not zeroes inside it. A
    /// backlog that can never reach 100% is one nobody looks at twice.
    /// Which character each closed goal is owed to, counted.
    ///
    /// Two kinds of credit and they are not the same strength. An
    /// [`Attestation`] is somebody saying "I did this", which is a claim about
    /// a person; `nearest` is only whichever enrolled character the evaluation
    /// happened to be measured against, which is the closest thing to
    /// attribution the measured goals have. Attestation wins where there is
    /// one, because a stated answer beats an inferred one.
    ///
    /// A goal credited to nobody is left out rather than shared around. The
    /// counts here are therefore a floor and do not add up to the run's total,
    /// which is the honest shape: most of a run is account-wide work nothing
    /// can pin on one character, and dividing it evenly would invent an answer.
    pub fn credited(&self) -> HashMap<CharacterKey, usize> {
        let mut credit: HashMap<CharacterKey, usize> = HashMap::new();
        for goal in &self.goals {
            if !goal.counts() || !goal.is_done() {
                continue;
            }
            let who = goal
                .attestation
                .as_ref()
                .map(|attestation| attestation.character.clone())
                .or_else(|| goal.nearest.clone());
            if let Some(who) = who {
                *credit.entry(who).or_default() += 1;
            }
        }
        credit
    }

    pub fn progress(&self) -> Progress {
        let mut progress = Progress::default();
        for goal in &self.goals {
            if !goal.counts() {
                progress.excluded += 1;
                continue;
            }
            progress.counted += 1;
            if goal.is_done() {
                progress.done += 1;
            } else if goal.bucket == Bucket::Attestable && goal.standing.is_poisoned() {
                progress.awaiting_attestation += 1;
            }
        }
        progress
    }

    /// The goals that need the expensive treatment: recomputation from primary
    /// data. Everything else is a flag read.
    pub fn poisoned(&self) -> impl Iterator<Item = &Goal> {
        self.goals.iter().filter(|goal| goal.standing.is_poisoned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::achievement::Evaluation;

    fn key(realm: &str, name: &str) -> CharacterKey {
        CharacterKey::new(realm, name)
    }

    fn cohort() -> Cohort {
        Cohort::from(vec![key("emerald-dream", "Somechar")])
    }

    fn baseline_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn before() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2016-03-04T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn after() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-02T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    #[test]
    fn an_unearned_achievement_needs_no_special_handling() {
        // The common case, and the cheap one: the flag works, so use it.
        let standing = Standing::classify(None, None, &cohort(), baseline_at());
        assert_eq!(standing, Standing::Unearned);
        assert!(!standing.is_poisoned());
    }

    #[test]
    fn anything_earned_since_the_baseline_belongs_to_the_run() {
        // Nobody else is playing the account, so attribution is unnecessary —
        // and this is what stops poisoning appearing after the run starts.
        let standing = Standing::classify(Some(after()), None, &cohort(), baseline_at());
        assert!(standing.is_settled());
        assert!(!standing.is_poisoned());
    }

    #[test]
    fn an_enrolled_character_having_earned_it_settles_it() {
        let standing = Standing::classify(
            Some(before()),
            Some(&key("emerald-dream", "Somechar")),
            &cohort(),
            baseline_at(),
        );
        assert!(standing.is_settled());
        assert!(!standing.is_poisoned());
    }

    #[test]
    fn an_outsider_having_earned_it_poisons_it() {
        // Aeltor earned this in 2016 and is not in the cohort. The flag is lit
        // and will never move again, whatever Somechar does.
        let standing = Standing::classify(
            Some(before()),
            Some(&key("mannoroth", "Aeltor")),
            &cohort(),
            baseline_at(),
        );
        assert!(standing.is_poisoned());
        assert!(!standing.is_settled());
    }

    #[test]
    fn without_attribution_everything_already_earned_is_assumed_poisoned() {
        // This is the cost of not running the addon, and it is the argument for
        // building the addon second: without `earnedBy` the pessimistic
        // assumption is the only sound one, and it drags every old achievement
        // through recomputation.
        let standing = Standing::classify(Some(before()), None, &cohort(), baseline_at());
        assert_eq!(standing, Standing::Poisoned { by: None });
    }

    fn poisoned_goal(bucket: Bucket) -> Goal {
        Goal {
            achievement_id: 1,
            standing: Standing::Poisoned {
                by: Some(key("mannoroth", "Aeltor")),
            },
            bucket,
            attestation: None,
            nearest: None,
            evaluation: None,
        }
    }

    #[test]
    fn an_excluded_goal_leaves_the_denominator_rather_than_sitting_in_it_as_a_zero() {
        // A backlog that can never reach 100% is one nobody looks at twice.
        let run = Run {
            name: "Fresh start".into(),
            baseline: Baseline {
                taken_at: baseline_at(),
                collected: Vec::new(),
                completed: Vec::new(),
            },
            cohort: cohort(),
            goals: vec![
                poisoned_goal(Bucket::Excluded(Exclusion::AlreadyOwned)),
                poisoned_goal(Bucket::Attestable),
            ],
        };

        let progress = run.progress();
        assert_eq!(progress.excluded, 1);
        assert_eq!(progress.counted, 1);
        assert_eq!(progress.awaiting_attestation, 1);
        assert_eq!(progress.done, 0);
    }

    #[test]
    fn an_excluded_goal_is_gone_rather_than_done() {
        // Counting it as done would inflate the run just as badly as counting
        // an alt's reputation.
        let goal = poisoned_goal(Bucket::Excluded(Exclusion::AlreadyOwned));
        assert!(!goal.is_done());
        assert!(!goal.counts());
    }

    #[test]
    fn attestation_is_what_completes_a_goal_nothing_can_measure() {
        let mut goal = poisoned_goal(Bucket::Attestable);
        assert!(!goal.is_done());
        goal.attestation = Some(Attestation {
            character: key("emerald-dream", "Somechar"),
            at: after(),
        });
        assert!(goal.is_done());
    }

    #[test]
    fn an_observable_goal_completes_on_its_evaluation() {
        let mut goal = poisoned_goal(Bucket::Observable);
        goal.evaluation = Some(Evaluation {
            progress: 10,
            required: 10,
            observable: true,
            inherited: false,
        });
        assert!(goal.is_done());
        assert_eq!(goal.fraction(), Some(1.0));
    }

    #[test]
    fn an_inherited_evaluation_draws_no_progress_bar() {
        // Showing three-quarters of a bar filled by an alt's reputation is the
        // exact lie this whole module exists to avoid.
        let mut goal = poisoned_goal(Bucket::Observable);
        goal.evaluation = Some(Evaluation {
            progress: 8,
            required: 10,
            observable: true,
            inherited: true,
        });
        assert!(!goal.is_done());
        assert_eq!(goal.fraction(), None);
    }

    #[test]
    fn an_unobservable_evaluation_draws_no_progress_bar_either() {
        let mut goal = poisoned_goal(Bucket::Observable);
        goal.evaluation = Some(Evaluation {
            progress: 3,
            required: 4,
            observable: false,
            inherited: false,
        });
        assert_eq!(goal.fraction(), None);
    }

    #[test]
    fn only_poisoned_goals_reach_the_expensive_path() {
        let run = Run {
            name: "Fresh start".into(),
            baseline: Baseline {
                taken_at: baseline_at(),
                collected: Vec::new(),
                completed: Vec::new(),
            },
            cohort: cohort(),
            goals: vec![
                Goal {
                    achievement_id: 1,
                    standing: Standing::Unearned,
                    bucket: Bucket::Observable,
                    attestation: None,
                    nearest: None,
                    evaluation: None,
                },
                poisoned_goal(Bucket::Observable),
            ],
        };
        assert_eq!(run.poisoned().count(), 1);
    }
}
