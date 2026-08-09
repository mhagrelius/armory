//! A zone, and everything Armory knows about it.
//!
//! This is the join the rest of the application has been accumulating parts
//! for, and all of it meets on one key: Blizzard's `UiMapID`.
//!
//! * **What the place is** — the lore corpus in `data/zones/`, written from
//!   `warcraft.wiki.gg` and the Chronicle in Armory's own words.
//! * **What happened in its dungeons** — Blizzard's own Adventure Guide, which
//!   states a raid's premise rather than summarising its plot, and is the only
//!   source that does. Where the guide is blank — every raid older than Mists
//!   of Pandaria, which is most of the famous ones — `data/instances/` fills in.
//! * **What *you* did there** — the chronicle. Evenings, hours, deaths, rares,
//!   quests turned in with the game's own text, screenshots taken.
//!
//! The name is never the key. There are two Nagrands and two Shadowmoon
//! Valleys, on different continents in different expansions, and a page that
//! joined on the string would show one zone's history over another's evenings.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::adventure::{Encounter, Guide, Instance};
use crate::chronicle::{Happening, Session};
use crate::tally::{self, Counting, Tallies};

/// One zone's lore, as written in `data/zones/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lore {
    pub zone: String,
    /// `None` for the handful the wiki's own map table never listed — every
    /// zone newer than mid-2023, which the addon supplies instead.
    pub map: Option<u32>,
    pub expansion: String,
    pub summary: String,
    pub history: String,
    #[serde(default)]
    pub factions: Vec<String>,
    #[serde(default)]
    pub notable: Vec<Named>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub licence: String,
}

/// One instance's lore, for the raids Blizzard never wrote up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub instance: String,
    /// The Adventure Guide's own instance id, which is what this joins on.
    pub journal: u32,
    pub summary: String,
    pub history: String,
    /// What the place takes for granted that the game never tells you.
    ///
    /// The most useful field in the corpus and the one with no equivalent
    /// anywhere else: Karazhan assumes you know who Medivh was and never says.
    #[serde(default)]
    pub assumes: Option<String>,
    /// Where the sources conflict or the wiki hedges.
    ///
    /// Recorded rather than silently resolved, because every pass over this
    /// material rediscovered the same contradictions and threw the finding
    /// away. Naming them is what stops that happening again.
    #[serde(default)]
    pub disputed: Vec<String>,
    #[serde(default)]
    pub notable: Vec<Named>,
    #[serde(default)]
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    pub what: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub title: String,
    pub url: String,
}

/// A dungeon or raid as the page shows it: the guide's account, or ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delve {
    pub name: String,
    /// Blizzard's description where it has one, ours where it does not.
    pub description: String,
    /// True when the words above are Armory's rather than Blizzard's, which
    /// the page says out loud — the two are not interchangeable and a reader
    /// should know which they are looking at.
    pub ours: bool,
    pub assumes: Option<String>,
    pub disputed: Vec<String>,
    pub bosses: Vec<Encounter>,
}

/// What one evening in this place amounted to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub character: String,
    pub at: chrono::DateTime<chrono::Utc>,
    /// Quests turned in here, with the title the game gave them.
    pub quests: Vec<String>,
    pub deaths: Vec<String>,
    pub rares: Vec<String>,
}

/// Something that drops here and can actually be sold.
///
/// **Bind-on-Equip only**, and that is not a compromise — it is the question.
/// Most raid loot is Bind-on-Pickup and has no market at any price, so a list
/// that included it would be a list of things you cannot do anything with. What
/// is left is what people actually farm a place for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spoil {
    pub item: u32,
    /// The name, once one has been fetched. Ids arrive before names do.
    pub name: Option<String>,
    pub from: String,
    /// The cheapest it is listed for, in copper.
    pub cheapest: u64,
    /// Units listed across every auction of it.
    pub quantity: u32,
}

/// Everything about one place, assembled.
#[derive(Debug, Clone, Default)]
pub struct Place {
    pub map: u32,
    pub name: String,
    pub lore: Option<Lore>,
    pub delves: Vec<Delve>,
    pub visits: Vec<Visit>,
    /// Seconds this account has spent here, across every character.
    pub spent: u64,
    /// What has killed somebody here, most often first.
    pub killers: Vec<(String, u64)>,
    /// Bind-on-Equip drops with a price on them, dearest first.
    pub spoils: Vec<Spoil>,
}

impl Place {
    /// Whether there is anything at all to show.
    ///
    /// A zone with lore nobody has visited is worth a page; so is a zone
    /// somebody has lived in that Armory has no lore for. Neither is not.
    pub fn is_worth_showing(&self) -> bool {
        self.lore.is_some() || !self.visits.is_empty() || self.spent > 0
    }
}

/// The lore corpus, compiled in.
///
/// Shipped rather than fetched, and that is the whole reason it is Armory's own
/// prose: a zone page costs no request, works with no network, and needs no
/// licence from anybody to display. It is also why the wiki is summarised and
/// never pasted.
pub fn corpus() -> Vec<Lore> {
    const ZONES: &str = include_str!("../../data/zones.json");
    serde_json::from_str(ZONES).unwrap_or_default()
}

/// The raids Blizzard's own guide has nothing to say about, by journal id.
pub fn unwritten() -> HashMap<u32, Written> {
    const INSTANCES: &str = include_str!("../../data/instances.json");
    serde_json::from_str::<Vec<Written>>(INSTANCES)
        .unwrap_or_default()
        .into_iter()
        .map(|w| (w.journal, w))
        .collect()
}

/// Assemble one place from everything on hand.
///
/// `sessions` is filtered here rather than by the caller because the filter is
/// the interesting part: a session is not "in" a zone, it *passes through*
/// several, so what counts is the moments that happened while the character was
/// standing in this one.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    map: u32,
    lore: Option<&Lore>,
    guide: &Guide,
    written: &HashMap<u32, Written>,
    sessions: &[Session],
    tallies: &Tallies,
    items: &HashMap<u32, crate::source::blizzard::gamedata::Item>,
    market: &HashMap<u32, (u64, u32)>,
) -> Place {
    let mut place = Place {
        map,
        name: lore.map(|l| l.zone.clone()).unwrap_or_default(),
        lore: lore.cloned(),
        ..Place::default()
    };

    // An instance sits on its own map, so this is what a person standing
    // *inside* one sees. A zone page reaches its dungeons through the wiki's
    // own list, which is a separate question and not this one.
    if let Some(instance) = guide.at(map) {
        place.delves.push(delve(instance, guide, written));
        if place.name.is_empty() {
            place.name = instance.name.clone();
        }
        place.spoils = spoils(instance, guide, items, market);
    }

    for session in sessions {
        if let Some(visit) = visited(session, map) {
            place.visits.push(visit);
        }
    }
    place.visits.sort_by_key(|v| std::cmp::Reverse(v.at));

    let key = map.to_string();
    for counted in tallies.values() {
        for entry in tally::of(counted, Counting::Zone) {
            if entry.key == key {
                place.spent += entry.count;
                if place.name.is_empty() {
                    place.name.clone_from(&entry.label);
                }
            }
        }
    }

    place
}

/// One instance as the page shows it, preferring Blizzard's words to ours.
///
/// Ours only where the guide is silent, which is every raid older than Mists of
/// Pandaria — the Adventure Guide arrived then and nothing before it was
/// backfilled, so the blanks are Molten Core, Karazhan, Ulduar, Naxxramas and
/// the rest of the ones people actually want the story of.
fn delve(instance: &Instance, guide: &Guide, written: &HashMap<u32, Written>) -> Delve {
    let ours = written
        .get(&instance.id)
        .filter(|_| instance.description.is_empty());

    Delve {
        name: instance.name.clone(),
        description: match ours {
            Some(w) => w.history.clone(),
            None => instance.description.clone(),
        },
        ours: ours.is_some(),
        assumes: ours.and_then(|w| w.assumes.clone()),
        disputed: ours.map(|w| w.disputed.clone()).unwrap_or_default(),
        bosses: instance
            .encounters
            .iter()
            .filter_map(|id| guide.encounters.get(id).cloned())
            .collect(),
    }
}

/// What drops here, can be sold, and has a price on it right now.
///
/// Three filters, and each removes most of what is left. The guide has to list
/// it; the item has to be sellable, which most raid loot is not; and somebody
/// has to have it on the auction house, without which there is no number to
/// show. An item whose name has not arrived yet is still listed — its id is
/// honest and a blank row would be worse.
fn spoils(
    instance: &Instance,
    guide: &Guide,
    items: &HashMap<u32, crate::source::blizzard::gamedata::Item>,
    market: &HashMap<u32, (u64, u32)>,
) -> Vec<Spoil> {
    let mut out: Vec<Spoil> = instance
        .encounters
        .iter()
        .filter_map(|id| guide.encounters.get(id))
        .flat_map(|encounter| {
            encounter.loot.iter().filter_map(move |item| {
                // Absent from `items` means the name has not been fetched, and
                // so has the binding — so it is unknown rather than sellable,
                // and an unknown is not shown. The alternative is offering
                // somebody a Bind-on-Pickup drop as a thing to sell.
                if !items.get(item)?.sellable {
                    return None;
                }
                let (cheapest, quantity) = *market.get(item)?;
                Some(Spoil {
                    item: *item,
                    name: items.get(item).map(|i| i.name.clone()),
                    from: encounter.name.trim_end_matches([',', ' ']).to_string(),
                    cheapest,
                    quantity,
                })
            })
        })
        .collect();

    out.sort_by_key(|spoil| std::cmp::Reverse(spoil.cheapest));
    out.dedup_by_key(|spoil| spoil.item);
    out
}

/// What a character did while standing in one particular zone.
///
/// The moments are walked in order with the current map carried along, because
/// a quest turn-in does not say where it happened — only the zone moment before
/// it does. A session that never entered this map answers `None` rather than an
/// empty visit, so a page does not fill with evenings that went somewhere else.
fn visited(session: &Session, map: u32) -> Option<Visit> {
    let mut here = false;
    let mut visit = Visit {
        character: session.display_name.clone(),
        at: session.started_at,
        quests: Vec::new(),
        deaths: Vec::new(),
        rares: Vec::new(),
    };
    let mut ever = false;

    for moment in &session.moments {
        match &moment.what {
            Happening::Arrived { map: at, .. } => {
                here = *at == Some(map);
                ever |= here;
            }
            Happening::Completed { title, .. } if here => visit.quests.push(title.clone()),
            Happening::Died { to, .. } if here => {
                visit
                    .deaths
                    .push(to.clone().unwrap_or_else(|| "something".into()));
            }
            Happening::Rare { name, .. } if here => visit.rares.push(name.clone()),
            _ => {}
        }
    }

    ever.then_some(visit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chronicle::Moment;

    fn moment(at: u32, what: Happening) -> Moment {
        Moment { at, what }
    }

    fn session(moments: Vec<Moment>) -> Session {
        Session {
            character: crate::character::CharacterKey::new("emerald-dream", "Somechar"),
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: crate::character::Faction::Horde,
            started_at: chrono::Utc::now(),
            ended_at: chrono::Utc::now(),
            start_level: 70,
            end_level: 70,
            start_money: 0,
            end_money: 0,
            start_item_level: 600,
            end_item_level: 600,
            moments,
            risen: Vec::new(),
            travelled: 0,
            longest_fight: 0,
        }
    }

    #[test]
    fn a_quest_belongs_to_the_zone_the_character_was_standing_in() {
        // A turn-in does not say where it happened. Only the zone moment before
        // it does, which is why the moments are walked in order.
        let session = session(vec![
            moment(
                0,
                Happening::Arrived {
                    zone: "Nagrand".into(),
                    subzone: None,
                    map: Some(107),
                },
            ),
            moment(
                10,
                Happening::Completed {
                    quest: 1,
                    title: "In Nagrand".into(),
                    story: None,
                },
            ),
            moment(
                20,
                Happening::Arrived {
                    zone: "Zangarmarsh".into(),
                    subzone: None,
                    map: Some(102),
                },
            ),
            moment(
                30,
                Happening::Completed {
                    quest: 2,
                    title: "In Zangarmarsh".into(),
                    story: None,
                },
            ),
        ]);

        let nagrand = visited(&session, 107).expect("was in Nagrand");
        assert_eq!(nagrand.quests, ["In Nagrand"]);
        let marsh = visited(&session, 102).expect("was in Zangarmarsh");
        assert_eq!(marsh.quests, ["In Zangarmarsh"]);
        // Somewhere the character never went is not an empty visit.
        assert_eq!(visited(&session, 999), None);
    }

    #[test]
    fn our_words_are_used_only_where_blizzards_are_missing_and_are_marked() {
        let mut guide = Guide::default();
        let karazhan = Instance {
            id: 745,
            name: "Karazhan".into(),
            map: Some(532),
            // Every raid older than Mists has this empty.
            description: String::new(),
            expansion: None,
            encounters: vec![],
        };
        guide.instances.insert(745, karazhan.clone());

        let written = HashMap::from([(
            745,
            Written {
                instance: "Karazhan".into(),
                journal: 745,
                history: "Medivh's tower, and what became of it.".into(),
                assumes: Some("That you know who Medivh was.".into()),
                ..Written::default()
            },
        )]);

        let ours = delve(&karazhan, &guide, &written);
        assert!(ours.ours, "the page must say whose words these are");
        assert_eq!(ours.description, "Medivh's tower, and what became of it.");
        assert!(ours.assumes.is_some());

        // Where Blizzard did write one, theirs wins and nothing is marked.
        let mut modern = karazhan.clone();
        modern.description = "The Defias have taken the mines.".into();
        let theirs = delve(&modern, &guide, &written);
        assert!(!theirs.ours);
        assert_eq!(theirs.description, "The Defias have taken the mines.");
        assert_eq!(theirs.assumes, None);
    }

    #[test]
    fn the_corpus_compiles_in_and_joins_on_the_map() {
        // Shipped rather than fetched, which is why it is our own prose.
        let corpus = corpus();
        assert!(corpus.len() > 100, "{} zones", corpus.len());

        // Every entry with a map id must have a distinct one, or two zones
        // would share a page — which is the whole reason the key is not a name.
        let mapped: Vec<u32> = corpus.iter().filter_map(|l| l.map).collect();
        let mut unique = mapped.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(mapped.len(), unique.len(), "two zones claim one map");

        // The two Nagrands are the case this exists for.
        let nagrands: Vec<&Lore> = corpus.iter().filter(|l| l.zone == "Nagrand").collect();
        assert_eq!(nagrands.len(), 1, "the Draenor one is titled differently");

        let raids = unwritten();
        assert_eq!(raids.len(), 21);
        // Karazhan is the clearest case of the field this corpus exists for.
        assert!(raids[&745].assumes.is_some());
    }

    #[test]
    fn only_what_can_actually_be_sold_is_worth_looking_for() {
        use crate::source::blizzard::gamedata::Item;

        let mut guide = Guide::default();
        guide.instances.insert(
            745,
            Instance {
                id: 745,
                name: "Karazhan".into(),
                map: Some(532),
                description: "…".into(),
                expansion: None,
                encounters: vec![1],
            },
        );
        guide.encounters.insert(
            1,
            Encounter {
                id: 1,
                name: "Attumen the Huntsman, ".into(),
                description: String::new(),
                loot: vec![10, 20, 30, 40],
            },
        );

        let item = |name: &str, sellable| Item {
            name: name.into(),
            sellable,
            quality: None,
        };
        let items = HashMap::from([
            (10, item("Fiery Warhorse's Reins", false)), // BoP — no market at any price
            (20, item("Worn Cloak", true)),
            (30, item("Rich Cloak", true)),
            // 40 has no entry at all: the name has not been fetched, so the
            // binding is unknown and it must not be offered as sellable.
        ]);
        let market = HashMap::from([
            (10, (5_000_000u64, 1u32)),
            (20, (1_000, 9)),
            (30, (90_000, 2)),
            (40, (400_000, 1)),
        ]);

        let instance = guide.instances[&745].clone();
        let spoils = spoils(&instance, &guide, &items, &market);
        assert_eq!(spoils.len(), 2, "the BoP and the unknown are both out");
        // Dearest first, and the boss name loses Blizzard's trailing comma.
        assert_eq!(spoils[0].name.as_deref(), Some("Rich Cloak"));
        assert_eq!(spoils[0].from, "Attumen the Huntsman");
        assert_eq!(spoils[1].name.as_deref(), Some("Worn Cloak"));
    }

    #[test]
    fn a_place_with_nothing_in_it_is_not_worth_a_page() {
        assert!(!Place::default().is_worth_showing());

        let mut lived_in = Place {
            spent: 3_600,
            ..Place::default()
        };
        assert!(lived_in.is_worth_showing(), "somewhere you spent an hour");

        lived_in.spent = 0;
        lived_in.lore = Some(Lore::default());
        assert!(lived_in.is_worth_showing(), "somewhere with a history");
    }
}
