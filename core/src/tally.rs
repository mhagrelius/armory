//! Counters Armory keeps because nothing else does.
//!
//! Every other number in this application is read from somewhere: the profile
//! API says what is owned, the addon says what happened tonight, the auction
//! house says what things cost. These are different. They are things a
//! character has done *repeatedly, over months*, which no Blizzard system
//! records at any granularity worth having — Blizzard's statistics count a few
//! professions in the aggregate and no individual recipe, count no party
//! members at all, and forget a boss attempt the moment the pull ends.
//!
//! So they only exist because the addon has been adding one to them since it
//! was installed, which decides everything about how they are stored:
//!
//! * **One table, not one per kind.** A tally is a `(kind, key)` and a number.
//!   Five near-identical tables with five near-identical readers is what this
//!   file exists to prevent, and the second counter is the right time to
//!   prevent it rather than the fifth.
//! * **Merged by taking the larger count.** The addon's totals are already
//!   cumulative, so a write is normally a no-op — but a reinstalled addon
//!   starts at one, and a year of somebody's evenings must not be erased by a
//!   cleared folder. There is nowhere to get it back from.
//! * **Never purged.** Like `session` and `entry`, and for the same reason:
//!   the thirty-day term is a condition on data obtained through Blizzard's
//!   API, and none of this was.

use std::collections::HashMap;

use crate::character::CharacterKey;

/// What a tally counts.
///
/// A closed set. The addon and this file are both ours, so a row with a kind
/// this version does not know is a newer addon writing an older application's
/// folder — which is what `FORMAT` reports, and is better reported than
/// silently filed under something plausible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Counting {
    /// Times a recipe has been made.
    Recipe,
    /// Evenings a person has been in the party.
    Companion,
    /// Attempts at an encounter, won or lost.
    Attempt,
    /// Attempts that ended with the boss on the floor.
    Victory,
    /// Seconds spent in a zone, keyed by `UiMapID`.
    ///
    /// By the map rather than the name, because two zones share the name
    /// `Nagrand` and two more share `Shadowmoon Valley`. The label carries the
    /// name a person reads; the key is what the lore corpus and the chronicle
    /// both join on.
    Zone,
    /// Deaths, by what did it.
    Killer,
    /// Yards travelled, keyed by how.
    Distance,
    /// Flights taken, keyed by where from.
    Flight,
    /// Delves finished, keyed by tier.
    Delve,
    /// Quests taken from or handed to a particular NPC.
    Questgiver,
    /// Named rares put down, by name.
    ///
    /// Kept apart from [`Counting::Victory`], which is dungeon and raid
    /// encounters. A world rare raises no `ENCOUNTER_END` at all, so without
    /// this every world-drop mount would show nought attempts however many
    /// times its rare had been killed.
    Rare,
}

impl Counting {
    /// The token the addon writes.
    pub fn as_token(self) -> &'static str {
        match self {
            Counting::Recipe => "recipe",
            Counting::Companion => "companion",
            Counting::Attempt => "attempt",
            Counting::Victory => "victory",
            Counting::Zone => "zone",
            Counting::Killer => "killer",
            Counting::Distance => "distance",
            Counting::Flight => "flight",
            Counting::Delve => "delve",
            Counting::Questgiver => "questgiver",
            Counting::Rare => "rare",
        }
    }

    pub fn from_token(token: &str) -> Option<Counting> {
        [
            Counting::Recipe,
            Counting::Companion,
            Counting::Attempt,
            Counting::Victory,
            Counting::Zone,
            Counting::Killer,
            Counting::Distance,
            Counting::Flight,
            Counting::Delve,
            Counting::Questgiver,
            Counting::Rare,
        ]
        .into_iter()
        .find(|kind| kind.as_token() == token)
    }

    /// What a group of these is called on a page.
    pub fn title(self) -> &'static str {
        match self {
            Counting::Recipe => "At the workbench",
            Counting::Companion => "Alongside",
            Counting::Attempt => "Fought most",
            Counting::Victory => "Defeated most",
            Counting::Zone => "Where the time went",
            Counting::Killer => "Killed by",
            Counting::Distance => "Distance travelled",
            Counting::Flight => "Flights taken",
            Counting::Delve => "Delves finished",
            Counting::Questgiver => "Sent you out most",
            Counting::Rare => "Rares hunted down",
        }
    }

    /// The line under that title.
    pub fn description(self) -> &'static str {
        match self {
            Counting::Recipe => "Everything this character has ever made",
            Counting::Companion => "Who has been in the party, and how often",
            Counting::Attempt => "Bosses pulled, won or lost",
            Counting::Victory => "Bosses that went down",
            Counting::Zone => "Hours spent, by zone",
            Counting::Killer => "What has killed this character, and how often",
            Counting::Distance => "Ground covered since the addon was installed",
            Counting::Flight => "Where the flight paths were taken from",
            Counting::Delve => "How many, and at what tier",
            Counting::Questgiver => "Who keeps giving this character work",
            Counting::Rare => "Named rares, and how many times each",
        }
    }
}

/// One counter: how many times this character did this particular thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub kind: Counting,
    /// What is being counted, as the addon keys it — a spell id, a zone name,
    /// a person's name.
    pub key: String,
    /// The same thing said the way a person says it.
    ///
    /// Separate from the key because a recipe is keyed by a spell id nobody
    /// wants to read, and because a key must not change when Blizzard renames
    /// something or a character transfers realms.
    pub label: String,
    pub count: u64,
}

/// Every counter, per character.
pub type Tallies = HashMap<CharacterKey, Vec<Tally>>;

/// One character's counters of one kind, biggest first.
///
/// The whole set is held as one flat list per character because that is how it
/// is stored and how it is written; a page wanting one kind asks here rather
/// than making the store answer eight questions.
pub fn of(tallies: &[Tally], kind: Counting) -> Vec<&Tally> {
    let mut wanted: Vec<&Tally> = tallies.iter().filter(|tally| tally.kind == kind).collect();
    wanted.sort_by_key(|tally| (std::cmp::Reverse(tally.count), tally.label.clone()));
    wanted
}

/// The shortest name worth matching a drop against.
///
/// Three letters matches half the game by accident. Blizzard has encounters
/// called `Ick` and rares called `Zul`, and a description mentioning either as
/// part of a longer word would claim a tally that is not about it.
const NAME_FLOOR: usize = 5;

/// How many times this account has fought whatever drops a thing.
///
/// **This is a count of attempts and never a drop rate.** Armory has no rates
/// and cannot get any: Blizzard publishes none, AllTheThings is not parsed and
/// Wowhead's terms forbid fetching it. What the addon *does* have is every pull
/// this account has made, which nothing in the game or the API keeps — Blizzard
/// forgets an attempt the moment the encounter ends. So the honest line is
/// "thirty-one tries" with no odds beside it, and a collector reading it knows
/// exactly what it means.
///
/// The join is on the sentence the in-game journal gives a collectible — "Drop:
/// Attumen the Huntsman, Karazhan" — against the encounter and rare names the
/// addon counted. Substring, because the sentence carries the zone and other
/// punctuation around the name, and the *longest* match wins so that a boss
/// whose name contains another's is not credited to the shorter one.
///
/// Returns what was fought as well as the count. A card that says "31 TRIES"
/// and does not say what was tried is a number without a referent, and the
/// referent is the thing somebody has to go and do again.
pub fn attempts_at(description: Option<&str>, tallies: &[Tally]) -> Option<(String, u64)> {
    let sentence = description?.to_lowercase();
    tallies
        .iter()
        .filter(|tally| matches!(tally.kind, Counting::Attempt | Counting::Rare))
        .filter(|tally| tally.label.chars().count() >= NAME_FLOOR)
        .filter(|tally| sentence.contains(&tally.label.to_lowercase()))
        .max_by_key(|tally| (tally.label.chars().count(), tally.count))
        .map(|tally| (tally.label.clone(), tally.count))
}

/// The same question asked of a whole account rather than one character.
///
/// A mount is account-wide, so every character's pulls at the boss that drops
/// it are pulls at that mount. Summed per name rather than taking the largest,
/// because two characters raiding the same boss is twice the rolls.
pub fn account_attempts(tallies: &Tallies) -> Vec<Tally> {
    let mut totals: HashMap<(Counting, String), Tally> = HashMap::new();
    for tally in tallies.values().flatten() {
        if !matches!(tally.kind, Counting::Attempt | Counting::Rare) {
            continue;
        }
        totals
            .entry((tally.kind, tally.label.clone()))
            .and_modify(|held| held.count += tally.count)
            .or_insert_with(|| tally.clone());
    }
    totals.into_values().collect()
}

/// A duration in hours and minutes, for the zone tallies.
///
/// Rounded to the minute below an hour and to the hour above about a day: "17
/// hours" is the fact, and "17 hours 3 minutes" is a spreadsheet.
pub fn spent(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes < 60 {
        return crate::chronicle::plural(minutes as usize, "minute", "minutes");
    }
    let hours = minutes / 60;
    if hours >= 24 {
        return crate::chronicle::plural(hours as usize, "hour", "hours");
    }
    let rest = minutes % 60;
    if rest == 0 {
        crate::chronicle::plural(hours as usize, "hour", "hours")
    } else {
        format!("{hours} hr {rest} min")
    }
}

/// A distance in yards, said the way a person would.
///
/// Miles above a mile, because "1,760 yards" is a number and "a mile" is a
/// distance. The game's yard is close enough to a real one that the conversion
/// is not a lie.
pub fn far(yards: u64) -> String {
    const MILE: u64 = 1_760;
    if yards < MILE {
        return format!("{yards} yards");
    }
    let miles = yards as f64 / MILE as f64;
    if miles < 10.0 {
        format!("{miles:.1} miles")
    } else {
        format!("{} miles", miles.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(kind: Counting, label: &str, count: u64) -> Tally {
        Tally {
            kind,
            key: label.to_string(),
            label: label.to_string(),
            count,
        }
    }

    #[test]
    fn one_kind_is_picked_out_of_the_flat_list_biggest_first() {
        let held = vec![
            tally(Counting::Recipe, "Algari Mana Potion", 6),
            tally(Counting::Companion, "Velkurai", 34),
            tally(Counting::Recipe, "Flask of Alchemical Chaos", 412),
        ];

        let made = of(&held, Counting::Recipe);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].label, "Flask of Alchemical Chaos");
        assert_eq!(made[1].label, "Algari Mana Potion");
        assert_eq!(of(&held, Counting::Zone), Vec::<&Tally>::new());
    }

    #[test]
    fn a_kind_this_version_does_not_know_is_refused_rather_than_guessed() {
        assert_eq!(Counting::from_token("recipe"), Some(Counting::Recipe));
        assert_eq!(Counting::from_token("delve-tier"), None);
    }

    #[test]
    fn time_and_distance_are_said_the_way_a_person_says_them() {
        assert_eq!(spent(90), "1 minute");
        assert_eq!(spent(3_600), "1 hour");
        assert_eq!(spent(5_400), "1 hr 30 min");
        // Past a day the minutes are noise.
        assert_eq!(spent(200_000), "55 hours");

        assert_eq!(far(400), "400 yards");
        assert_eq!(far(3_520), "2.0 miles");
        assert_eq!(far(100_000), "57 miles");
    }

    fn attempt(label: &str, count: u64) -> Tally {
        Tally {
            kind: Counting::Attempt,
            key: label.into(),
            label: label.into(),
            count,
        }
    }

    #[test]
    fn a_drop_is_joined_to_the_boss_the_addon_counted_pulls_at() {
        // Nothing in the game or the API keeps this. Blizzard forgets a pull the
        // moment the encounter ends, so "thirty-one tries" exists only because
        // the addon was there for all thirty-one.
        let tallies = [attempt("Attumen the Huntsman", 31), attempt("Vexie", 4)];
        let (fought, tries) =
            attempts_at(Some("Drop: Attumen the Huntsman, Karazhan"), &tallies).expect("a match");
        assert_eq!(fought, "Attumen the Huntsman");
        assert_eq!(tries, 31);
    }

    #[test]
    fn the_longest_name_wins_so_one_boss_is_not_credited_to_another() {
        let tallies = [
            attempt("Halion", 9),
            attempt("Halion the Twilight Destroyer", 2),
        ];
        let (fought, tries) = attempts_at(
            Some("Drop: Halion the Twilight Destroyer, Ruby Sanctum"),
            &tallies,
        )
        .expect("a match");
        assert_eq!(fought, "Halion the Twilight Destroyer");
        assert_eq!(tries, 2);
    }

    #[test]
    fn a_short_name_is_not_matched_at_all() {
        // `Ick` is a real encounter and `Zul` is a real rare, and either would
        // match half the descriptions in the game as a substring.
        let tallies = [attempt("Ick", 40)];
        assert_eq!(
            attempts_at(Some("Drop: Sickly Gazelle, Mulgore"), &tallies),
            None
        );
    }

    #[test]
    fn a_collectible_with_no_sentence_has_nothing_to_join_on() {
        // The web API gives a mount the word `DROP` and no sentence at all,
        // which is the single biggest reason the addon is the better source.
        let tallies = [attempt("Attumen the Huntsman", 31)];
        assert_eq!(attempts_at(None, &tallies), None);
    }

    #[test]
    fn two_characters_raiding_the_same_boss_is_twice_the_rolls() {
        // A mount is account-wide, so both characters' pulls are pulls at it.
        let tallies = Tallies::from([
            (
                CharacterKey::new("emerald-dream", "Somechar"),
                vec![attempt("Vexie", 7)],
            ),
            (
                CharacterKey::new("mannoroth", "Aeltor"),
                vec![attempt("Vexie", 5)],
            ),
        ]);
        let merged = account_attempts(&tallies);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].count, 12);
    }
}
