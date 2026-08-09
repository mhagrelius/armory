//! The Adventure Guide: what a dungeon or raid actually was.
//!
//! The one thing about instanced content that is genuinely hard to find out is
//! *what the deal was*. A wiki article gives a plot summary written afterwards
//! by somebody who already knew the ending; the quests inside give you a
//! fragment at a time and are gone once turned in; and the encounter itself
//! tells you nothing at all if you were following somebody through it at speed.
//!
//! Blizzard's own Adventure Guide is the exception. It carries a paragraph per
//! instance and a paragraph per boss, written as the premise rather than the
//! summary — *why is this place a problem, and who is this*. It is first-party,
//! it comes through the API Armory is already licensed for, and nothing else
//! covers it.
//!
//! It also carries each encounter's loot table, which is the other half: it is
//! how an item is joined to a boss, a boss to an instance, and an instance —
//! through its `UiMapID` — to the zone a person was standing in.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One dungeon or raid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub id: u32,
    pub name: String,
    /// The instance's own `UiMapID`, where it has one.
    ///
    /// The same key a zone entry and a chronicle session join on, which is what
    /// lets an evening spent in Karazhan find Karazhan's description without
    /// matching on a name that four different things share.
    pub map: Option<u32>,
    /// Blizzard's own account of the place.
    pub description: String,
    /// `DUNGEON` or `RAID`, as the API spells it.
    pub expansion: Option<String>,
    /// Encounter ids, in the order the guide lists them.
    ///
    /// Duplicates are real and are kept: a raid with a faction-split wing
    /// lists the same boss twice under two ids, and collapsing them would lose
    /// the fact that there are two versions of the fight.
    pub encounters: Vec<u32>,
}

/// One boss, and what it drops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    pub id: u32,
    pub name: String,
    pub description: String,
    /// Item ids the guide lists for this encounter.
    ///
    /// Possibility, never probability. The guide says what *can* drop and
    /// Blizzard publishes no chance for any of it — see `model::hunt` for why
    /// that is fine and what Armory says instead.
    pub loot: Vec<u32>,
}

/// Everything the guide knows, as the application holds it.
#[derive(Debug, Clone, Default)]
pub struct Guide {
    pub instances: HashMap<u32, Instance>,
    pub encounters: HashMap<u32, Encounter>,
}

impl Guide {
    /// The instance a zone contains, by `UiMapID`.
    ///
    /// An instance sits on its own map rather than on the zone's, so this
    /// answers "what is this place" for somebody standing *inside* it — which
    /// is what the chronicle records when it notes an instance entry.
    pub fn at(&self, map: u32) -> Option<&Instance> {
        self.instances.values().find(|i| i.map == Some(map))
    }

    /// Which boss drops an item, if the guide says so.
    ///
    /// The first that lists it. An item that drops from several bosses in one
    /// raid is a real thing and the first is as good an answer as any, since
    /// the question this serves is "where do I go", not "which of them".
    pub fn drops(&self, item: u32) -> Option<(&Instance, &Encounter)> {
        self.instances.values().find_map(|instance| {
            instance.encounters.iter().find_map(|id| {
                let encounter = self.encounters.get(id)?;
                encounter
                    .loot
                    .contains(&item)
                    .then_some((instance, encounter))
            })
        })
    }

    /// Every item the guide lists as dropping anywhere.
    ///
    /// What the auction snapshot is filtered against, so that "worth looking
    /// for here" is bounded by what actually drops rather than by the market.
    pub fn loot(&self) -> std::collections::HashSet<u32> {
        self.encounters
            .values()
            .flat_map(|e| e.loot.iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guide() -> Guide {
        Guide {
            instances: HashMap::from([(
                63,
                Instance {
                    id: 63,
                    name: "Deadmines".into(),
                    map: Some(291),
                    description: "The Defias Brotherhood have taken the mines.".into(),
                    expansion: Some("DUNGEON".into()),
                    encounters: vec![89, 90],
                },
            )]),
            encounters: HashMap::from([
                (
                    89,
                    Encounter {
                        id: 89,
                        name: "Glubtok".into(),
                        description: "An ogre mage hired as head foreman.".into(),
                        loot: vec![2169, 5195],
                    },
                ),
                (
                    90,
                    Encounter {
                        id: 90,
                        name: "Helix Gearbreaker".into(),
                        description: "A goblin with a bomb.".into(),
                        loot: vec![5444],
                    },
                ),
            ]),
        }
    }

    #[test]
    fn an_instance_is_found_by_the_map_somebody_is_standing_on() {
        // Which is what the chronicle records, and why the join is not on a
        // name — several places in the game share one.
        let guide = guide();
        assert_eq!(guide.at(291).map(|i| i.name.as_str()), Some("Deadmines"));
        assert_eq!(guide.at(37), None);
    }

    #[test]
    fn an_item_is_traced_back_to_the_boss_and_the_place() {
        let guide = guide();
        let (instance, encounter) = guide.drops(5195).expect("a source");
        assert_eq!(instance.name, "Deadmines");
        assert_eq!(encounter.name, "Glubtok");
        // Something the guide does not list has no source, which is silence
        // rather than a guess — plenty of gear drops off trash.
        assert_eq!(guide.drops(99_999), None);
    }

    #[test]
    fn the_loot_set_is_what_the_market_is_filtered_against() {
        assert_eq!(guide().loot().len(), 3);
    }
}
