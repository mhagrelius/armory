//! How long you have been trying.
//!
//! Blizzard publishes no drop chance for anything, anywhere. Every percentage
//! you have ever seen on a database site is an *estimate from observed kills* —
//! Wowhead ships an addon that records every loot event its users witness and
//! infers a rate from millions of samples. It is a measurement with a sample
//! size, not a fact, which is why the numbers drift and why obscure items carry
//! wilder ones than farmed items do.
//!
//! Armory does not have that data and will not be fetching it. What it has
//! instead is better for the question a person is actually asking. A global
//! one-in-a-hundred tells you nothing about whether tonight is the night; *"you
//! have killed Attumen the Huntsman forty-seven times and never seen it"* is
//! exact, it is about you, and nothing else in the world can tell you it.
//!
//! Two things already on disk make it: the collection knows what is missing and
//! where each thing drops from, and [`Counting::Victory`] has been counting
//! encounters put down. This joins them on the creature's name.

use std::collections::{HashMap, HashSet};

use crate::source::blizzard::collections::{Collectible, Kind, Source};
use crate::tally::{self, Counting, Tallies};

/// Something missing, what drops it, and how long you have been at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quarry {
    pub kind: Kind,
    pub id: u32,
    /// What you are after.
    pub name: String,
    /// What drops it, as the in-game journal names it.
    pub from: String,
    /// Where that is, where the journal says. `None` for a world drop, which
    /// names a creature and no place.
    pub place: Option<String>,
    /// Times this account has put that creature down **since the addon was
    /// installed**.
    ///
    /// Never a lifetime figure and it cannot be made into one: nothing in the
    /// game or the API records how many times anybody has killed anything. A
    /// character who farmed Karazhan for a decade before installing Armory
    /// starts at nought, and the page has to say so rather than imply the
    /// number means "ever".
    pub attempts: u32,
}

/// The creature a journal sentence names, and where it says to find it.
///
/// The sentence is the in-game journal's own — `Drop: Attumen the Huntsman,
/// Karazhan` — which is the single best reason the addon beats the web API for
/// collections, since the API says only the word `DROP`.
///
/// Deliberately narrow. Anything that is not a leading `Drop:` followed by a
/// name is refused rather than guessed at, because a wrong creature here is a
/// count of somebody else's kills attached to your mount.
pub fn dropped_by(description: &str) -> Option<(String, Option<String>)> {
    let (head, rest) = description.split_once(':')?;
    if !matches!(head.trim().to_ascii_lowercase().as_str(), "drop") {
        return None;
    }

    let rest = rest.trim();
    // `Creature, Place` — but a place is optional, and several journal lines
    // carry a third clause that is neither.
    let (who, place) = match rest.split_once(',') {
        Some((who, place)) => (who.trim(), Some(place.trim())),
        None => (rest, None),
    };

    if who.is_empty() {
        return None;
    }
    Some((
        who.to_string(),
        place.filter(|p| !p.is_empty()).map(str::to_string),
    ))
}

/// What this account is still hunting, longest-suffering first.
///
/// Four things have to be true, and each removes most of the catalogue:
///
/// **It is missing.** A thing you own is not a hunt.
///
/// **The journal says what drops it.** The web API's bare `DROP` names no
/// creature, so an account that has never run the collector gets an empty list
/// rather than a wrong one.
///
/// **You have actually fought it.** An attempt count of nought is not a hunt
/// either — it is a thing you have never gone after, and there are thousands of
/// those.
///
/// Attempts are summed across every character, because a collection is
/// account-wide: the mount does not care which of them landed the killing blow.
pub fn hunting(catalogue: &[Collectible], owned: &HashSet<u32>, tallies: &Tallies) -> Vec<Quarry> {
    // Every character's kills, folded together and keyed by creature.
    let mut kills: HashMap<String, u32> = HashMap::new();
    for counted in tallies.values() {
        for kind in [Counting::Victory, Counting::Rare] {
            for entry in tally::of(counted, kind) {
                *kills.entry(entry.label.to_lowercase()).or_default() += entry.count as u32;
            }
        }
    }
    if kills.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<Quarry> = catalogue
        .iter()
        .filter(|entry| entry.source == Source::Drop)
        .filter(|entry| !owned.contains(&entry.id))
        .filter_map(|entry| {
            let (from, place) = dropped_by(entry.description.as_deref()?)?;
            let attempts = *kills.get(&from.to_lowercase())?;
            (attempts > 0).then(|| Quarry {
                kind: entry.kind,
                id: entry.id,
                name: entry.name.clone(),
                from,
                place,
                attempts,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.attempts
            .cmp(&a.attempts)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterKey;
    use crate::tally::Tally;

    fn missing(id: u32, name: &str, description: &str) -> Collectible {
        Collectible {
            kind: Kind::Mount,
            id,
            name: name.into(),
            source: Source::Drop,
            description: Some(description.into()),
            flavour: None,
            icon: None,
            display: None,
            faction: None,
            link_id: id,
            tradeable: None,
        }
    }

    fn killed(who: &[(&str, u64)]) -> Tallies {
        HashMap::from([(
            CharacterKey::new("emerald-dream", "Somechar"),
            who.iter()
                .map(|(name, count)| Tally {
                    kind: Counting::Victory,
                    key: name.to_string(),
                    label: name.to_string(),
                    count: *count,
                })
                .collect(),
        )])
    }

    #[test]
    fn a_journal_sentence_names_the_creature_and_the_place() {
        assert_eq!(
            dropped_by("Drop: Attumen the Huntsman, Karazhan"),
            Some(("Attumen the Huntsman".into(), Some("Karazhan".into())))
        );
        // A world drop names a creature and no place.
        assert_eq!(
            dropped_by("Drop: Time-Lost Proto-Drake"),
            Some(("Time-Lost Proto-Drake".into(), None))
        );
        // Anything that is not a drop is refused rather than guessed at — a
        // wrong creature means somebody else's kills counted against your mount.
        assert_eq!(dropped_by("Vendor: Katie Hunter, Elwynn Forest"), None);
        assert_eq!(dropped_by("Drop:"), None);
        assert_eq!(dropped_by("no colon at all"), None);
    }

    #[test]
    fn a_hunt_needs_a_thing_you_want_and_a_thing_you_have_fought() {
        let catalogue = vec![
            missing(1, "Fiery Warhorse", "Drop: Attumen the Huntsman, Karazhan"),
            missing(
                2,
                "Ashes of Al'ar",
                "Drop: Kael'thas Sunstrider, Tempest Keep",
            ),
            // Owned, so not a hunt.
            missing(
                3,
                "Swift White Hawkstrider",
                "Drop: Kael'thas Sunstrider, Magisters' Terrace",
            ),
            // Never fought, so not a hunt either — there are thousands of these.
            missing(4, "Invincible", "Drop: The Lich King, Icecrown Citadel"),
        ];
        let owned = HashSet::from([3]);
        let tallies = killed(&[("Attumen the Huntsman", 47), ("Kael'thas Sunstrider", 12)]);

        let hunts = hunting(&catalogue, &owned, &tallies);
        assert_eq!(hunts.len(), 2, "one owned, one never attempted");
        // Longest-suffering first.
        assert_eq!(hunts[0].name, "Fiery Warhorse");
        assert_eq!(hunts[0].attempts, 47);
        assert_eq!(hunts[0].place.as_deref(), Some("Karazhan"));
        assert_eq!(hunts[1].attempts, 12);
    }

    #[test]
    fn attempts_are_the_whole_account_because_the_collection_is() {
        // The mount does not care which character landed the killing blow.
        let catalogue = vec![missing(
            1,
            "Fiery Warhorse",
            "Drop: Attumen the Huntsman, Karazhan",
        )];
        let mut tallies = killed(&[("Attumen the Huntsman", 30)]);
        tallies.insert(
            CharacterKey::new("mannoroth", "Aeltor"),
            vec![Tally {
                kind: Counting::Victory,
                key: "Attumen the Huntsman".into(),
                label: "Attumen the Huntsman".into(),
                count: 17,
            }],
        );

        let hunts = hunting(&catalogue, &HashSet::new(), &tallies);
        assert_eq!(hunts[0].attempts, 47);
    }

    #[test]
    fn an_account_with_no_journal_gets_nothing_rather_than_something_wrong() {
        // The web API's source is the bare word DROP and names no creature, so
        // there is nothing to join on. Silence is the honest answer.
        let mut bare = missing(1, "Fiery Warhorse", "");
        bare.description = None;
        assert!(hunting(
            &[bare],
            &HashSet::new(),
            &killed(&[("Attumen the Huntsman", 47)])
        )
        .is_empty());
    }
}
