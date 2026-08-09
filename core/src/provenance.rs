//! Who actually earned the account's account-wide progress.
//!
//! The run already knows that an account-wide *achievement* names whoever
//! earned it first and nobody after. This is the same problem one level down,
//! for the two things that do not even do that much.
//!
//! **Reputation.** The War Within syncs most standings across the Warband to
//! the furthest-progressed character, and Dragonflight's renown works the same
//! way. A character made yesterday reads Exalted and Renown 25 with factions it
//! has never met. [`crate::achievement::Evaluation::inherited`] already
//! refuses to count those, which is right and is not enough: a person replaying
//! the game *can* earn the equivalent of Exalted with a faction the account
//! maxed out in 2023, and the standing cannot move to record it because it was
//! already at the ceiling before they began.
//!
//! **Currency.** Worse, because a currency can arrive on a character three
//! ways, and only one of them is work: earned by this character, transferred
//! from another across the Warband, or simply already there when the run
//! started.
//!
//! ## What makes attribution possible
//!
//! One client, one character at a time. So a standing that rises between a
//! character's login and their logout rose *because of that character*, and the
//! addon needs no event and no localised string to know it — two snapshots and
//! a subtraction. Anything that changed while they were logged out was somebody
//! else's doing and is deliberately invisible, which is exactly right.
//!
//! ## What this deliberately will not do
//!
//! Guess. A currency that both rose and is Warband-transferable, on a currency
//! whose `totalEarned` the game does not maintain, is genuinely ambiguous — and
//! [`Origin::Unclear`] says so rather than picking the flattering answer. The
//! whole application's credibility rests on not inflating a run, and a
//! confident wrong attribution is worse here than an admitted unknown.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::character::CharacterKey;

/// What one character has personally earned with one faction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarnedReputation {
    /// Reputation points this character earned, paragon included.
    ///
    /// Cumulative across every session they have played since the addon was
    /// installed. It is not the standing — the standing may have been at the
    /// ceiling the whole time.
    pub points: u32,
    /// Renown levels this character earned.
    ///
    /// The number that means something to a person. A major faction's progress
    /// is a level plus a partial bar, and the bar alone would report a
    /// character who earned nine levels as having earned almost nothing.
    pub renown: u32,
    /// The highest renown level this character has personally seen. Not what
    /// they earned — what the account showed them — and kept so the two can be
    /// told apart on screen.
    pub renown_seen: u32,
    /// Whether the faction is account-wide at all.
    ///
    /// When it is not, the standing was already honest and this is a
    /// cross-check rather than the only truth.
    pub account_wide: bool,
}

/// What one character has personally gained of one currency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarnedCurrency {
    /// How much arrived while this character was logged in, by any means.
    pub gained: u64,
    /// How much of that the game itself calls earned.
    ///
    /// Only meaningful when [`EarnedCurrency::tracks_earned`] is true: the game
    /// maintains `totalEarned` for currencies with a moving maximum and returns
    /// a flat zero for everything else. Reading that zero as "earned nothing"
    /// is the mistake this field exists to prevent.
    pub earned: u64,
    /// Whether the game maintains an earned total for this currency at all.
    pub tracks_earned: bool,
    pub account_wide: bool,
    /// Whether the Warband can move it between characters, which is what makes
    /// a rise ambiguous.
    pub transferable: bool,
}

/// Where an amount on a character came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Origin {
    /// This character did the work. Either the thing cannot be transferred at
    /// all, or the game's own earned total rose to match.
    Earned,
    /// It arrived without being earned: more turned up than the game counted as
    /// earned, on something the Warband can move.
    Transferred,
    /// It was there before anybody was watching. Not this character's work, and
    /// not attributable to any other either.
    Existing,
    /// It rose, and there is no way to tell which. Said plainly rather than
    /// guessed: a confident wrong attribution inflates a run, which is the one
    /// failure that would make the whole application worthless.
    Unclear,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Earned => "Earned",
            Origin::Transferred => "Transferred",
            Origin::Existing => "Already held",
            Origin::Unclear => "Unclear",
        }
    }

    /// Whether a run may count this as progress.
    ///
    /// Only the first. Everything else is somebody else's work, or nobody's.
    pub fn counts(self) -> bool {
        matches!(self, Origin::Earned)
    }
}

impl EarnedCurrency {
    /// Where this character's holding of the currency came from.
    ///
    /// The reasoning, in order:
    ///
    /// * Nothing arrived → it was already there.
    /// * It cannot be transferred → whatever arrived was earned here. The
    ///   Warband has no way to have moved it.
    /// * The game tracks an earned total → believe it. What rose beyond the
    ///   earned figure came across from another character.
    /// * Otherwise → transferable, with no earned total to check it against.
    ///   Unknowable, and said so.
    pub fn origin(&self) -> Origin {
        if self.gained == 0 && self.earned == 0 {
            return Origin::Existing;
        }
        if !self.transferable {
            return Origin::Earned;
        }
        if self.tracks_earned {
            if self.earned >= self.gained {
                Origin::Earned
            } else {
                Origin::Transferred
            }
        } else {
            Origin::Unclear
        }
    }

    /// How much of it a run may count.
    pub fn creditable(&self) -> u64 {
        match self.origin() {
            Origin::Earned => self.gained.max(self.earned),
            // The part the game vouched for, and not a copper more.
            Origin::Transferred => self.earned,
            Origin::Existing | Origin::Unclear => 0,
        }
    }
}

/// Everything one character has been observed earning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Earned {
    /// Faction id to what this character earned with them.
    pub reputation: HashMap<u32, EarnedReputation>,
    /// Currency id to what this character gained of it.
    pub currency: HashMap<u32, EarnedCurrency>,
}

impl Earned {
    /// What this character earned with one faction.
    pub fn with(&self, faction: u32) -> EarnedReputation {
        self.reputation.get(&faction).copied().unwrap_or_default()
    }

    /// Whether this character has personally done anything with a faction.
    pub fn has_touched(&self, faction: u32) -> bool {
        self.with(faction).points > 0 || self.with(faction).renown > 0
    }
}

/// The whole account's answer to "who did this".
pub type Provenance = HashMap<CharacterKey, Earned>;

/// How much reputation the game asks for between one standing and the next.
///
/// Blizzard's classic ladder, unchanged since vanilla and not exposed as a
/// table anywhere an addon can read cheaply — the client knows it per faction
/// through `nextReactionThreshold`, but that is the *account's* next threshold,
/// which for an inherited standing is the wrong question entirely. What a
/// replay wants is "how much would this character have needed from nothing",
/// and that is this ladder.
///
/// Values are cumulative from Neutral, which is where a faction starts.
const LADDER: [(u8, &str, u32); 5] = [
    (5, "Friendly", 3_000),
    (6, "Honored", 9_000),
    (7, "Revered", 21_000),
    (8, "Exalted", 42_000),
    // Beyond Exalted there is only paragon, which has no standing of its own.
    (9, "Paragon", 42_000 + 10_000),
];

/// The standing this character's own earned reputation would have reached,
/// starting from Neutral.
///
/// **This is the number the soft reset is about.** The account may have been
/// Exalted with the Consortium since 2007; what a run wants to know is whether
/// *this* character has done the equivalent of the work, and the account's
/// standing cannot answer because it was already at the ceiling.
///
/// Renown is answered on levels rather than points, because that is the shape
/// it has: earning nine renown levels is nine levels of work whatever the
/// per-level thresholds happen to be.
pub fn standing_earned(earned: &EarnedReputation) -> (u8, &'static str) {
    if earned.renown > 0 {
        // A renown faction has no classic ladder. The level is the standing.
        return (earned.renown.min(255) as u8, "Renown");
    }

    let mut reached = (4u8, "Neutral");
    for (rank, name, threshold) in LADDER {
        if earned.points >= threshold {
            reached = (rank, name);
        }
    }
    reached
}

/// How far through the current standing this character's own work has taken
/// them, as a fraction, or `None` at the top of the ladder.
pub fn fraction_earned(earned: &EarnedReputation) -> Option<f64> {
    if earned.renown > 0 {
        return None;
    }
    let mut floor = 0u32;
    for (_, _, threshold) in LADDER {
        if earned.points >= threshold {
            floor = threshold;
        } else {
            let span = threshold - floor;
            if span == 0 {
                return None;
            }
            return Some(f64::from(earned.points - floor) / f64::from(span));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_currency_that_cannot_be_transferred_was_earned_here() {
        // The Warband has no way to have moved it, so whatever arrived while
        // this character was logged in is this character's work.
        let held = EarnedCurrency {
            gained: 400,
            earned: 0,
            tracks_earned: false,
            account_wide: false,
            transferable: false,
        };
        assert_eq!(held.origin(), Origin::Earned);
        assert_eq!(held.creditable(), 400);
    }

    #[test]
    fn a_transferable_currency_is_believed_where_the_game_counts_earnings() {
        // More arrived than was earned, on something the Warband can move: the
        // difference came across from another character.
        let mixed = EarnedCurrency {
            gained: 1_000,
            earned: 600,
            tracks_earned: true,
            account_wide: true,
            transferable: true,
        };
        assert_eq!(mixed.origin(), Origin::Transferred);
        // Only the part the game vouched for.
        assert_eq!(mixed.creditable(), 600);

        let honest = EarnedCurrency {
            earned: 1_000,
            ..mixed
        };
        assert_eq!(honest.origin(), Origin::Earned);
        assert_eq!(honest.creditable(), 1_000);
    }

    #[test]
    fn a_transferable_currency_with_no_earned_total_is_admitted_as_unknown() {
        // The game maintains `totalEarned` only for currencies with a moving
        // maximum. Reading its flat zero as "earned nothing" would be wrong,
        // and calling the whole amount earned would inflate a run.
        let ambiguous = EarnedCurrency {
            gained: 500,
            earned: 0,
            tracks_earned: false,
            account_wide: true,
            transferable: true,
        };
        assert_eq!(ambiguous.origin(), Origin::Unclear);
        assert_eq!(ambiguous.creditable(), 0);
        assert!(!Origin::Unclear.counts());
    }

    #[test]
    fn a_currency_that_never_moved_was_already_there() {
        assert_eq!(EarnedCurrency::default().origin(), Origin::Existing);
        assert_eq!(EarnedCurrency::default().creditable(), 0);
    }

    #[test]
    fn earned_reputation_reaches_a_standing_of_its_own() {
        // The whole point. The account may have been Exalted with these people
        // since 2007; this says whether *this* character has done the work.
        let done = EarnedReputation {
            points: 42_000,
            account_wide: true,
            ..EarnedReputation::default()
        };
        assert_eq!(standing_earned(&done), (8, "Exalted"));

        let partway = EarnedReputation {
            points: 12_000,
            ..EarnedReputation::default()
        };
        assert_eq!(standing_earned(&partway), (6, "Honored"));

        let nothing = EarnedReputation::default();
        assert_eq!(standing_earned(&nothing), (4, "Neutral"));
    }

    #[test]
    fn renown_is_counted_in_levels_because_that_is_its_shape() {
        // Earning nine renown levels is nine levels of work, whatever the
        // per-level thresholds happen to be — and the partial bar alone would
        // report almost nothing.
        let renowned = EarnedReputation {
            points: 2_400,
            renown: 9,
            renown_seen: 25,
            account_wide: true,
        };
        assert_eq!(standing_earned(&renowned), (9, "Renown"));
        // What the account showed them is kept apart from what they earned.
        assert_eq!(renowned.renown_seen, 25);
        assert_eq!(fraction_earned(&renowned), None);
    }

    #[test]
    fn a_fraction_measures_progress_through_the_standing_being_worked_on() {
        let halfway = EarnedReputation {
            // Friendly is 3,000 and Honored is 9,000, so 6,000 is halfway.
            points: 6_000,
            ..EarnedReputation::default()
        };
        assert_eq!(standing_earned(&halfway), (5, "Friendly"));
        assert_eq!(fraction_earned(&halfway), Some(0.5));
    }

    #[test]
    fn a_character_who_has_done_nothing_with_a_faction_says_so() {
        let mut earned = Earned::default();
        assert!(!earned.has_touched(2_600));

        earned.reputation.insert(
            2_600,
            EarnedReputation {
                points: 250,
                ..EarnedReputation::default()
            },
        );
        assert!(earned.has_touched(2_600));
    }
}
