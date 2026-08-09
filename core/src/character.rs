//! Who is on the account.
//!
//! A character is identified two ways and both are needed. The profile
//! endpoints take a realm *slug* and a lowercased name; the protected endpoint —
//! the only one that knows about gold — takes numeric realm and character ids
//! instead. Carrying both from the moment the roster is read is cheaper than
//! rediscovering the numeric pair later, and it is why [`Character`] looks
//! redundant.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a character is addressed on the profile endpoints.
///
/// Ordered, so a roster sorts stably by realm and then by name without the
/// caller inventing a comparison each time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CharacterKey {
    pub realm_slug: String,
    /// Lowercased. The endpoints 404 on a capitalised name.
    pub name: String,
}

impl CharacterKey {
    pub fn new(realm_slug: impl Into<String>, name: &str) -> Self {
        CharacterKey {
            realm_slug: realm_slug.into(),
            name: name.to_lowercase(),
        }
    }

    /// The name as a person would write it.
    ///
    /// The stored form is lowercased because the endpoints 404 otherwise, and
    /// that is a fact about URLs rather than about the character. Anywhere a
    /// key reaches prose — "aeltor earned this" — it needs putting back.
    ///
    /// Titlecasing rather than looking the roster up: this is used in sentences
    /// about characters who may have been deleted or transferred, which is
    /// exactly when the roster no longer has them.
    pub fn display_name(&self) -> String {
        let mut characters = self.name.chars();
        match characters.next() {
            Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Faction {
    Alliance,
    Horde,
    /// Pandaren before they choose, and anything Blizzard adds later.
    Neutral,
}

impl Faction {
    pub fn from_type(code: &str) -> Faction {
        match code {
            "ALLIANCE" => Faction::Alliance,
            "HORDE" => Faction::Horde,
            _ => Faction::Neutral,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Faction::Alliance => "Alliance",
            Faction::Horde => "Horde",
            Faction::Neutral => "Neutral",
        }
    }
}

/// One character, as the account index describes them.
///
/// This is the cheap summary that `/profile/user/wow` returns for everybody. The
/// expensive per-character detail — achievements, collections, quests — hangs
/// off it and is fetched only for the enrolled cohort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    pub key: CharacterKey,
    /// The numeric id, for the protected endpoint.
    pub id: u64,
    pub realm_id: u64,
    /// As Blizzard capitalises it, for showing to a person.
    pub display_name: String,
    pub realm_name: String,
    pub level: u8,
    pub class: String,
    pub race: String,
    pub faction: Faction,
    /// Which WoW licence under the Battle.net account this character sits on.
    ///
    /// One login can hold several, which is why the account profile returns
    /// `wow_accounts` in the plural. Collections are shared across them, so this
    /// matters for describing the account and not for deciding what is owned.
    pub wow_account_id: u64,
}

impl Character {
    /// How the character is written in the interface: `Somechar — Emerald Dream`.
    pub fn full_name(&self) -> String {
        format!("{} — {}", self.display_name, self.realm_name)
    }

    /// The path segment pair the protected-character endpoint wants.
    pub fn protected_id(&self) -> String {
        format!("{}-{}", self.realm_id, self.id)
    }
}

/// The expensive half of a character, fetched only for the enrolled cohort.
///
/// Every field is optional because every one of them comes from a different
/// endpoint that can independently be `Empty`, `Unchanged` or broken. A
/// character whose professions call failed still has an item level worth
/// showing, and a struct that made these mandatory would have to throw the
/// whole character away to say so.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    /// Averaged across everything worn, and the one people compare.
    pub item_level: Option<u16>,
    /// Item level counting only equipped slots. Lower than the average when a
    /// slot is empty, which is the case worth seeing.
    pub equipped_item_level: Option<u16>,
    pub spec: Option<String>,
    pub guild: Option<String>,
    /// Copper. From the protected endpoint — the plain profile has no money.
    pub money: Option<u64>,
    pub achievement_points: Option<u32>,
    /// When Blizzard last saw them log out, which is when everything else here
    /// was true.
    pub last_login: Option<DateTime<Utc>>,
    pub professions: Vec<Profession>,
    /// Best Mythic+ rating this season.
    pub mythic_rating: Option<u32>,
    /// Highest renown across the account-wide major factions.
    ///
    /// Account-wide since The War Within, so this describes the account rather
    /// than the character — shown as a fact, never counted as run progress.
    pub renown: Option<u32>,
    /// What is actually worn, slot by slot.
    ///
    /// Only the slots that hold something. An empty slot is an *absent* entry
    /// rather than an entry with no item, which is the same rule the rest of
    /// this struct follows — and the character page draws the absence, because
    /// an empty slot is the most useful thing on the page and folding it into
    /// an average is what hides it.
    #[serde(default)]
    pub equipment: Option<Vec<Equipped>>,
    /// Raid progress, newest instance last, as Blizzard orders it.
    ///
    /// Every boss this character has ever killed. Web API only — see
    /// [`Detail::raid_locks`] for the half an addon can answer.
    #[serde(default)]
    pub raids: Option<Vec<RaidTier>>,
    /// The raids this character is saved to *this week*.
    ///
    /// A different fact from [`Detail::raids`] and kept apart from it. The API
    /// reports a lifetime; the client knows only the current lockout, because
    /// that is all `GetSavedInstanceInfo` is. For an account with no Battle.net
    /// client this is the only raid progress there is, and for the tier being
    /// raided now it is the more current of the two — but folding it into the
    /// lifetime numbers would invent a decade of raiding out of one reset.
    #[serde(default)]
    pub raid_locks: Option<Vec<RaidLock>>,
    // No Great Vault field. There is no endpoint for it: the weekly rewards
    // frame is client-side state and the Profile API has never exposed it. The
    // mythic keystone profile gives this season's runs, which is the input to a
    // vault slot and not the slot itself. When this arrives it will come from
    // the addon, and it will be honest about being a snapshot from the last
    // logout like everything else here.
}

impl Detail {
    /// Take everything `fresh` actually answered, and keep the rest.
    ///
    /// The two sources answer overlapping halves of this struct and neither
    /// answers all of it. The addon knows the specialisation trees, the
    /// knowledge spent and this week's lockouts, and cannot know the Mythic+
    /// rating, the renown, the account's achievement points or a lifetime of
    /// raiding. The API is the other way round. So an addon read that assigned
    /// the whole struct would blank four fields every time somebody logged out,
    /// and they would come back on the next sync and go again on the next
    /// logout — the same rule as `save_collectibles` and `parse_professions`,
    /// which are both merges for exactly this reason.
    ///
    /// `None` is silence, not an answer. A source that did not speak about a
    /// field leaves what is there, and only a source that *did* replaces it.
    pub fn absorb(&mut self, fresh: Detail) {
        fn keep<T>(held: &mut Option<T>, fresh: Option<T>) {
            if fresh.is_some() {
                *held = fresh;
            }
        }

        keep(&mut self.item_level, fresh.item_level);
        keep(&mut self.equipped_item_level, fresh.equipped_item_level);
        keep(&mut self.spec, fresh.spec);
        keep(&mut self.guild, fresh.guild);
        keep(&mut self.money, fresh.money);
        keep(&mut self.achievement_points, fresh.achievement_points);
        keep(&mut self.last_login, fresh.last_login);
        keep(&mut self.mythic_rating, fresh.mythic_rating);
        keep(&mut self.renown, fresh.renown);
        keep(&mut self.equipment, fresh.equipment);
        keep(&mut self.raids, fresh.raids);
        keep(&mut self.raid_locks, fresh.raid_locks);

        if !fresh.professions.is_empty() {
            self.professions = fresh
                .professions
                .into_iter()
                .map(|mut fresh| {
                    let Some(held) = self.professions.iter().find(|held| held.name == fresh.name)
                    else {
                        return fresh;
                    };
                    // A tier is the API's and the trees are the addon's, and a
                    // profession row carries both or neither depending on who
                    // wrote it last.
                    if fresh.tier.is_none() {
                        fresh.tier.clone_from(&held.tier);
                    }
                    if fresh.specialisations.is_empty() {
                        fresh.specialisations.clone_from(&held.specialisations);
                    }
                    if fresh.knowledge == 0 {
                        fresh.knowledge = held.knowledge;
                    }
                    fresh
                })
                .collect();
        }
    }
}

/// One worn item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipped {
    /// Blizzard's own slot type — `HEAD`, `OFF_HAND`, `TABARD`. Kept as the
    /// key rather than the display name because it is the stable half, and
    /// because [`Equipped::SLOTS`] is written in it.
    pub slot: String,
    /// The slot as a person reads it, from the same response.
    pub slot_name: String,
    pub name: String,
    /// `None` for a cosmetic slot, which has no item level at all.
    ///
    /// Not zero, and not a guess. A shirt with a fabricated 1 in it would sort
    /// to the top of a list whose whole purpose is to put the weakest slot
    /// first.
    pub level: Option<u16>,
}

impl Equipped {
    /// Every slot a character can fill, in the order Blizzard's own character
    /// sheet reads down.
    ///
    /// Here because the equipment response says nothing about what is *not*
    /// worn — an empty slot is simply not in the list — and "the off hand is
    /// empty" is the single most actionable line on the character page.
    pub const SLOTS: [(&'static str, &'static str); 16] = [
        ("HEAD", "Head"),
        ("NECK", "Neck"),
        ("SHOULDER", "Shoulder"),
        ("BACK", "Back"),
        ("CHEST", "Chest"),
        ("WRIST", "Wrist"),
        ("HANDS", "Hands"),
        ("WAIST", "Waist"),
        ("LEGS", "Legs"),
        ("FEET", "Feet"),
        ("FINGER_1", "Ring 1"),
        ("FINGER_2", "Ring 2"),
        ("TRINKET_1", "Trinket 1"),
        ("TRINKET_2", "Trinket 2"),
        ("MAIN_HAND", "Main Hand"),
        ("OFF_HAND", "Off Hand"),
    ];

    /// The slots that are worn for the look of the thing and carry no item
    /// level, which is why they are not in [`Equipped::SLOTS`] and are never
    /// counted as empty.
    pub const COSMETIC: [&'static str; 2] = ["SHIRT", "TABARD"];

    pub fn is_cosmetic(&self) -> bool {
        Self::COSMETIC.contains(&self.slot.as_str())
    }
}

/// One raid, and how far into it this character has got.
///
/// A "tier" here is a raid instance — Liberation of Undermine — rather than an
/// expansion, because that is the unit a person means by "the current tier" and
/// the unit the bosses are counted in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidTier {
    pub name: String,
    /// Which expansion it belongs to, for ordering and for saying which of
    /// these is the one being raided now.
    pub expansion: String,
    /// One entry per difficulty the character has set foot in. A difficulty
    /// never attempted is absent rather than zero-of-eight, for the usual
    /// reason: nothing attempted and nothing killed are different facts.
    pub difficulties: Vec<RaidDifficulty>,
}

/// One difficulty of one raid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidDifficulty {
    /// `Normal`, `Heroic`, `Mythic`, `Raid Finder`.
    pub name: String,
    pub defeated: u16,
    pub total: u16,
    /// The last boss to fall, and when. `None` when nothing has.
    pub last_kill: Option<(String, DateTime<Utc>)>,
}

/// One raid this character is saved to for the current reset.
///
/// From the game client, which is the only thing that knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaidLock {
    pub name: String,
    /// `Heroic`, `Mythic` — the game's own wording.
    pub difficulty: String,
    pub defeated: u16,
    pub total: u16,
}

impl RaidTier {
    /// The last kill anywhere in this raid, at any difficulty.
    pub fn last_kill(&self) -> Option<(&str, &DateTime<Utc>, &str)> {
        self.difficulties
            .iter()
            .filter_map(|difficulty| {
                difficulty
                    .last_kill
                    .as_ref()
                    .map(|(boss, at)| (boss.as_str(), at, difficulty.name.as_str()))
            })
            .max_by_key(|(_, at, _)| **at)
    }
}

/// One profession and how far along it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profession {
    pub name: String,
    /// The current expansion's tier, which is the one that matters.
    pub tier: Option<String>,
    pub skill: Option<u16>,
    pub max_skill: Option<u16>,
    /// Whether this is a primary profession rather than cooking or fishing.
    pub is_primary: bool,
    /// Specialisation trees, and whether each has been opened.
    ///
    /// Addon-only, and there is no endpoint that could supply it. Two
    /// characters with Alchemy at 100 can have spent a year of knowledge in
    /// completely different places, and nothing outside the game says which.
    #[serde(default)]
    pub specialisations: Vec<(String, bool)>,
    /// How much knowledge this profession has ever been given.
    ///
    /// The total earned where the game tracks it and the unspent balance
    /// otherwise — see the addon. Zero means "not a specialised profession, or
    /// a client that predates them", which is why it does not suppress the
    /// profession itself.
    #[serde(default)]
    pub knowledge: u32,
}

/// Every character on the account, across every realm and every licence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub characters: Vec<Character>,
}

impl Roster {
    pub fn new(mut characters: Vec<Character>) -> Self {
        characters.sort_by(|a, b| a.key.cmp(&b.key));
        Roster { characters }
    }

    pub fn get(&self, key: &CharacterKey) -> Option<&Character> {
        self.characters.iter().find(|c| &c.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.characters.is_empty()
    }

    pub fn len(&self) -> usize {
        self.characters.len()
    }

    /// The realms the account has characters on, in display order, deduplicated.
    ///
    /// Each of these is a separate auction house for everything except
    /// commodities, which is why the roster is what decides the realms the
    /// auction view offers.
    pub fn realms(&self) -> Vec<(String, String)> {
        let mut realms: Vec<(String, String)> = self
            .characters
            .iter()
            .map(|c| (c.key.realm_slug.clone(), c.realm_name.clone()))
            .collect();
        realms.sort();
        realms.dedup();
        realms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn character(realm: &str, name: &str) -> Character {
        Character {
            key: CharacterKey::new(super::super::source::blizzard::realm_slug(realm), name),
            id: 1,
            realm_id: 2,
            display_name: name.to_string(),
            realm_name: realm.to_string(),
            level: 80,
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            wow_account_id: 1,
        }
    }

    #[test]
    fn a_key_lowercases_the_name_because_the_endpoints_do() {
        // `/profile/wow/character/emerald-dream/Somechar` is a 404. The lowercase
        // spelling is not cosmetic.
        let key = CharacterKey::new("emerald-dream", "Somechar");
        assert_eq!(key.name, "somechar");
    }

    #[test]
    fn a_key_reads_back_as_a_name_when_it_reaches_prose() {
        // The lowercase form is a fact about URLs, not about the character, and
        // "aeltor earned this" is a sentence nobody should see.
        assert_eq!(
            CharacterKey::new("mannoroth", "Aeltor").display_name(),
            "Aeltor"
        );
        assert_eq!(CharacterKey::new("x", "").display_name(), "");
    }

    #[test]
    fn a_roster_sorts_by_realm_then_name() {
        let roster = Roster::new(vec![
            character("Thrall", "Ulahae"),
            character("Emerald Dream", "Velkurai"),
            character("Emerald Dream", "Atulak"),
        ]);
        let order: Vec<&str> = roster
            .characters
            .iter()
            .map(|c| c.display_name.as_str())
            .collect();
        assert_eq!(order, ["Atulak", "Velkurai", "Ulahae"]);
    }

    #[test]
    fn realms_are_deduplicated_because_each_one_is_an_auction_house() {
        let roster = Roster::new(vec![
            character("Emerald Dream", "Atulak"),
            character("Emerald Dream", "Velkurai"),
            character("Mannoroth", "Aeltor"),
        ]);
        assert_eq!(
            roster.realms(),
            [
                ("emerald-dream".to_string(), "Emerald Dream".to_string()),
                ("mannoroth".to_string(), "Mannoroth".to_string()),
            ]
        );
    }

    #[test]
    fn the_protected_endpoint_wants_the_numeric_pair() {
        // Realm id and character id, not the slug and name every other endpoint
        // takes. Getting this wrong is a 404 that looks like a missing feature.
        let mut c = character("Dalaran", "Moodivh");
        c.realm_id = 3684;
        c.id = 12345;
        assert_eq!(c.protected_id(), "3684-12345");
    }

    #[test]
    fn absorbing_the_addons_answer_keeps_what_only_the_api_knows() {
        // This was a real wipe. The addon's `Detail` has no Mythic+ rating, no
        // renown, no account achievement points and no lifetime raiding, and
        // assigning it over the held struct blanked all four every time
        // somebody logged out — they came back on the next sync and went again
        // on the next logout.
        let mut held = Detail {
            item_level: Some(639),
            mythic_rating: Some(2418),
            renown: Some(80),
            achievement_points: Some(28_940),
            raids: Some(vec![RaidTier {
                name: "Liberation of Undermine".into(),
                expansion: "The War Within".into(),
                difficulties: Vec::new(),
            }]),
            ..Detail::default()
        };

        held.absorb(Detail {
            item_level: Some(641),
            money: Some(1_234_500),
            raid_locks: Some(vec![RaidLock {
                name: "Liberation of Undermine".into(),
                difficulty: "Heroic".into(),
                defeated: 2,
                total: 8,
            }]),
            ..Detail::default()
        });

        assert_eq!(held.item_level, Some(641), "the newer answer wins");
        assert_eq!(
            held.money,
            Some(1_234_500),
            "and a field only it has arrives"
        );
        assert_eq!(held.mythic_rating, Some(2418), "silence is not an answer");
        assert_eq!(held.renown, Some(80));
        assert_eq!(held.achievement_points, Some(28_940));
        assert!(
            held.raids.is_some(),
            "a lifetime of raiding survives a logout"
        );
        assert!(held.raid_locks.is_some(), "and this week's lockouts arrive");
    }

    #[test]
    fn a_profession_keeps_the_half_the_other_source_wrote() {
        let mut held = Detail {
            professions: vec![Profession {
                name: "Alchemy".into(),
                tier: Some("Khaz Algar Alchemy".into()),
                skill: Some(100),
                max_skill: Some(100),
                is_primary: true,
                specialisations: Vec::new(),
                knowledge: 0,
            }],
            ..Detail::default()
        };

        // The addon's half: the trees and the knowledge, and no tier.
        held.absorb(Detail {
            professions: vec![Profession {
                name: "Alchemy".into(),
                tier: None,
                skill: Some(100),
                max_skill: Some(100),
                is_primary: true,
                specialisations: vec![("Potion Mastery".into(), true)],
                knowledge: 42,
            }],
            ..Detail::default()
        });

        let alchemy = &held.professions[0];
        assert_eq!(alchemy.tier.as_deref(), Some("Khaz Algar Alchemy"));
        assert_eq!(alchemy.knowledge, 42);
        assert_eq!(alchemy.specialisations.len(), 1);
    }

    #[test]
    fn the_last_kill_is_the_latest_across_every_difficulty() {
        let tier = RaidTier {
            name: "Liberation of Undermine".into(),
            expansion: "The War Within".into(),
            difficulties: vec![
                RaidDifficulty {
                    name: "Normal".into(),
                    defeated: 8,
                    total: 8,
                    last_kill: Some(("Mug'Zee".into(), Utc.timestamp_opt(1_753_800, 0).unwrap())),
                },
                RaidDifficulty {
                    name: "Heroic".into(),
                    defeated: 2,
                    total: 8,
                    last_kill: Some(("Vexie".into(), Utc.timestamp_opt(1_754_000, 0).unwrap())),
                },
            ],
        };
        let (boss, _, difficulty) = tier.last_kill().expect("a last kill");
        assert_eq!(boss, "Vexie");
        assert_eq!(difficulty, "Heroic");
    }
}
