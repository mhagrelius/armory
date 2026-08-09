//! `/profile/...`: this account, and what its characters have done.
//!
//! Everything here is a snapshot written when a character logs out. Blizzard
//! staff put it plainly on the developer forums: profile data changes only when
//! the character has logged out of the game. There is no live view, and an
//! application that implies one is lying about the only thing it reports.
//!
//! That is also what makes syncing a large account affordable. Every endpoint
//! sets `Last-Modified` and honours `If-Modified-Since`, so a character who has
//! not played since the last sync answers `304` with no body — twenty-three
//! characters cost twenty-three round trips and almost no bytes.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, TimeZone, Utc};

use super::super::{parse_json, Outcome, Reason, Request, SourceId};
use super::{realm_slug, url, Namespace, Region};
use crate::achievement::{Criterion, CriterionKind, PrimaryData};
use crate::character::{
    Character, CharacterKey, Detail, Equipped, Faction, Profession, RaidDifficulty, RaidTier,
    Roster,
};

const SOURCE: SourceId = SourceId::BlizzardProfile;

/// Every character on the account, across every realm and every licence.
pub fn account(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(region, Namespace::Profile, "/profile/user/wow", &[]),
    )
}

fn character_path(key: &CharacterKey, suffix: &str) -> String {
    format!(
        "/profile/wow/character/{}/{}{}",
        key.realm_slug, key.name, suffix
    )
}

/// The character summary: item level, spec, guild, last logout.
pub fn summary(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(region, Namespace::Profile, &character_path(key, ""), &[]),
    )
}

/// Professions, and how far along each is.
pub fn professions(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/professions"),
            &[],
        ),
    )
}

/// This season's Mythic+ standing.
pub fn mythic_keystone(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/mythic-keystone-profile"),
            &[],
        ),
    )
}

/// Dungeons and raids this character has cleared.
///
/// Two endpoints rather than one because Blizzard splits them, and the
/// criteria that reference them do not care which is which.
pub fn dungeon_encounters(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/encounters/dungeons"),
            &[],
        ),
    )
}

pub fn raid_encounters(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/encounters/raids"),
            &[],
        ),
    )
}

/// What the character is wearing, slot by slot.
pub fn equipment(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/equipment"),
            &[],
        ),
    )
}

/// Gold, and the lifetime counters that go with it.
///
/// The one endpoint that takes numeric ids rather than a realm slug and a name,
/// and the only one that knows about money. Requires the token owner's own
/// character; anybody else's answers 403.
pub fn protected_character(region: Region, character: &Character) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &format!(
                "/profile/user/wow/protected-character/{}",
                character.protected_id()
            ),
            &[],
        ),
    )
}

/// A character's achievements, with the account's progress through each
/// criteria tree.
pub fn achievements(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/achievements"),
            &[],
        ),
    )
}

/// The per-character statistics that back a good share of achievement criteria.
pub fn statistics(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/achievements/statistics"),
            &[],
        ),
    )
}

/// Every quest this character has completed. Genuinely per character, which is
/// what makes a replayed character's progress visible at all.
pub fn completed_quests(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/quests/completed"),
            &[],
        ),
    )
}

/// Reputations. Account-wide since The War Within for most factions, which is
/// why what comes back needs the treatment in [`parse_reputations`].
pub fn reputations(region: Region, key: &CharacterKey) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Profile,
            &character_path(key, "/reputations"),
            &[],
        ),
    )
}

/// Read the account index into a roster.
pub fn parse_account(body: &[u8]) -> Outcome<Roster> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // `wow_accounts` is plural because one Battle.net login can hold several WoW
    // licences. Collections are shared across them, so this matters for
    // describing the account and never for deciding what is owned.
    let Some(accounts) = value.get("wow_accounts").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the account profile carried no wow_accounts".into(),
        ));
    };

    let mut characters = Vec::new();
    for account in accounts {
        let wow_account_id = account.get("id").and_then(|id| id.as_u64()).unwrap_or(0);
        let Some(list) = account.get("characters").and_then(|list| list.as_array()) else {
            continue;
        };
        for entry in list {
            if let Some(character) = read_character(entry, wow_account_id) {
                characters.push(character);
            }
        }
    }

    Outcome::of_collection(characters).map(Roster::new)
}

fn read_character(entry: &serde_json::Value, wow_account_id: u64) -> Option<Character> {
    let display_name = entry.get("name")?.as_str()?.to_string();
    let realm = entry.get("realm")?;
    let realm_name = realm.get("name")?.as_str()?.to_string();
    // The response carries the slug, but a locale that spells the realm
    // differently has been known to omit it. Deriving it is cheap insurance.
    let realm_slug = realm
        .get("slug")
        .and_then(|slug| slug.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| realm_slug(&realm_name));

    Some(Character {
        key: CharacterKey::new(realm_slug, &display_name),
        id: entry.get("id").and_then(|id| id.as_u64()).unwrap_or(0),
        realm_id: realm.get("id").and_then(|id| id.as_u64()).unwrap_or(0),
        display_name,
        realm_name,
        level: entry
            .get("level")
            .and_then(|level| level.as_u64())
            .unwrap_or(0) as u8,
        class: named(entry.get("playable_class")),
        race: named(entry.get("playable_race")),
        faction: entry
            .get("faction")
            .and_then(|faction| faction.get("type"))
            .and_then(|code| code.as_str())
            .map(Faction::from_type)
            .unwrap_or(Faction::Neutral),
        wow_account_id,
    })
}

/// Pull the display name out of one of Blizzard's `{key, name, id}` references.
fn named(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(|value| value.get("name"))
        .and_then(|name| name.as_str())
        .unwrap_or_default()
        .to_string()
}

/// One achievement as the profile reports it: the criteria tree, and when — if
/// ever — the account finished it.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementProgress {
    pub id: u32,
    pub completed_at: Option<DateTime<Utc>>,
    pub criteria: Option<Criterion>,
}

/// Read a character's achievements.
///
/// An entry appears for any achievement with *any* progress, complete or not,
/// which is what makes partial progress visible without asking for it.
pub fn parse_achievements(body: &[u8]) -> Outcome<Vec<AchievementProgress>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("achievements").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the achievements response carried no achievements list".into(),
        ));
    };

    let progress = list
        .iter()
        .filter_map(|entry| {
            let id = entry
                .get("achievement")
                .and_then(|achievement| achievement.get("id"))
                .and_then(|id| id.as_u64())? as u32;
            Some(AchievementProgress {
                id,
                completed_at: entry
                    .get("completed_timestamp")
                    .and_then(|stamp| stamp.as_i64())
                    .and_then(millis),
                criteria: entry.get("criteria").and_then(read_criterion),
            })
        })
        .collect();

    Outcome::of_collection(progress)
}

/// Read one node of a criteria tree, recursing through `child_criteria`.
///
/// The kind is left [`CriterionKind::Unknown`] on purpose: the profile response
/// says how far along the account is and never what the criterion measures.
/// That meaning is joined on afterwards from the catalogue, and inventing it
/// here would be inventing it from nothing.
fn read_criterion(value: &serde_json::Value) -> Option<Criterion> {
    let id = value.get("id").and_then(|id| id.as_u64())?;
    Some(Criterion {
        id,
        kind: CriterionKind::Unknown,
        required: value
            .get("amount")
            .and_then(|amount| amount.as_u64())
            .unwrap_or(0),
        children: value
            .get("child_criteria")
            .and_then(|list| list.as_array())
            .map(|list| list.iter().filter_map(read_criterion).collect())
            .unwrap_or_default(),
    })
}

/// Blizzard stamps in milliseconds since the epoch.
fn millis(stamp: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(stamp).single()
}

/// Read the completed-quest id list.
pub fn parse_completed_quests(body: &[u8]) -> Outcome<HashSet<u32>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("quests").and_then(|list| list.as_array()) else {
        // A character who has completed nothing genuinely has no `quests` key,
        // so this is Empty rather than Stale — the one place in this file where
        // a missing collection is an answer rather than a broken parser.
        return Outcome::Empty;
    };

    let quests: HashSet<u32> = list
        .iter()
        .filter_map(|quest| quest.get("id").and_then(|id| id.as_u64()))
        .map(|id| id as u32)
        .collect();

    if quests.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(quests)
    }
}

/// Read the per-character statistics into a flat id-to-value map.
///
/// Statistics arrive nested under categories and sub-categories, and nothing
/// downstream cares about the tree — a criterion refers to a statistic by id.
pub fn parse_statistics(body: &[u8]) -> Outcome<HashMap<u32, f64>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(categories) = value.get("categories").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the statistics response carried no categories".into(),
        ));
    };

    let mut statistics = HashMap::new();
    for category in categories {
        collect_statistics(category, &mut statistics);
    }
    if statistics.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(statistics)
    }
}

fn collect_statistics(value: &serde_json::Value, into: &mut HashMap<u32, f64>) {
    if let Some(list) = value.get("statistics").and_then(|list| list.as_array()) {
        for statistic in list {
            let Some(id) = statistic.get("id").and_then(|id| id.as_u64()) else {
                continue;
            };
            let quantity = statistic
                .get("quantity")
                .and_then(|quantity| quantity.as_f64())
                .unwrap_or(0.0);
            into.insert(id as u32, quantity);
        }
    }
    if let Some(children) = value.get("sub_categories").and_then(|list| list.as_array()) {
        for child in children {
            collect_statistics(child, into);
        }
    }
}

/// One faction, as a person would read it.
///
/// The planner only ever wants an id and a number, which is what `standings`
/// is. This is the same data with the words attached, kept separately so the
/// hot path stays a map lookup and the page has something to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionStanding {
    pub faction: u32,
    pub name: String,
    /// What Blizzard calls the tier — `Exalted`, or a renown level's name.
    pub tier: String,
    /// Progress within the tier, and what it takes to leave it. Both zero for a
    /// standing that has no further to go.
    pub value: u64,
    pub max: u64,
    /// Renown level, for the factions that use renown instead of tiers.
    pub renown: u32,
    /// Whether Warbands handed this to the character rather than the character
    /// earning it.
    pub inherited: bool,
}

impl FactionStanding {
    /// How far through the current tier, if the tier has a width.
    ///
    /// `None` at the top: a maxed standing has no fraction, and drawing a full
    /// bar for it would look identical to one that happens to be at 99%.
    pub fn fraction(&self) -> Option<f64> {
        (self.max > 0).then(|| (self.value as f64 / self.max as f64).clamp(0.0, 1.0))
    }
}

/// A faction's standing, and whether it can be trusted as this character's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Reputations {
    pub standings: HashMap<u32, u32>,
    /// Factions whose standing is account-wide, and therefore may have been
    /// earned by anybody.
    pub inherited: HashSet<u32>,
    /// The same standings with their names and tiers, for showing.
    pub detail: Vec<FactionStanding>,
}

/// Read reputations, marking the ones Warbands made account-wide.
///
/// The War Within syncs most reputations to the furthest-progressed character on
/// the account, so a standing that arrives on a freshly levelled character was
/// very likely earned by somebody else years ago. The API offers no flag for
/// this, so the heuristic is the honest one available: a standing on a character
/// too low to have earned it is inherited, and inherited standings are reported
/// rather than counted.
pub fn parse_reputations(body: &[u8], character_level: u8) -> Outcome<Reputations> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("reputations").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the reputations response carried no reputations list".into(),
        ));
    };

    let mut standings = HashMap::new();
    let mut inherited = HashSet::new();
    let mut detail = Vec::new();

    for entry in list {
        let faction = entry.get("faction");
        let Some(id) = faction
            .and_then(|faction| faction.get("id"))
            .and_then(|id| id.as_u64())
        else {
            continue;
        };
        let standing = entry.get("standing");
        let raw = standing
            .and_then(|standing| standing.get("raw"))
            .and_then(|raw| raw.as_u64())
            .unwrap_or(0) as u32;
        let renown = standing
            .and_then(|standing| standing.get("renown_level"))
            .and_then(|renown| renown.as_u64())
            .unwrap_or(0);

        standings.insert(id as u32, raw);

        // A character below the level cap carrying renown could not have earned
        // it themselves; Warbands handed it to them. Levelled characters are
        // left alone, because at that point the standing is as likely theirs as
        // anyone's and guessing further would be inventing data.
        let is_inherited = renown > 0 && character_level < 70;
        if is_inherited {
            inherited.insert(id as u32);
        }

        detail.push(FactionStanding {
            faction: id as u32,
            name: faction
                .and_then(|faction| faction.get("name"))
                .and_then(|name| name.as_str())
                .unwrap_or_default()
                .to_string(),
            // Blizzard puts the word under `standing.name` for the old tiers
            // and leaves it out for renown, where the level is the word.
            tier: standing
                .and_then(|standing| standing.get("name"))
                .and_then(|name| name.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if renown > 0 {
                        format!("Renown {renown}")
                    } else {
                        String::new()
                    }
                }),
            value: standing
                .and_then(|standing| standing.get("value"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            max: standing
                .and_then(|standing| standing.get("max"))
                .and_then(|max| max.as_u64())
                .unwrap_or(0),
            renown: renown as u32,
            inherited: is_inherited,
        });
    }

    if standings.is_empty() {
        Outcome::Empty
    } else {
        detail.sort_by(|a, b| a.name.cmp(&b.name));
        Outcome::Found(Reputations {
            standings,
            inherited,
            detail,
        })
    }
}

/// Read the character summary into the detail fields it supplies.
pub fn parse_summary(body: &[u8]) -> Outcome<Detail> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // `id` is the one field every summary has. Its absence means the response
    // is not a character summary at all.
    if value.get("id").and_then(|id| id.as_u64()).is_none() {
        return Outcome::Stale(Reason::Malformed(
            "the character summary carried no id".into(),
        ));
    }

    Outcome::Found(Detail {
        item_level: value
            .get("average_item_level")
            .and_then(|level| level.as_u64())
            .map(|level| level as u16),
        equipped_item_level: value
            .get("equipped_item_level")
            .and_then(|level| level.as_u64())
            .map(|level| level as u16),
        spec: value
            .get("active_spec")
            .and_then(|spec| spec.get("name"))
            .and_then(|name| name.as_str())
            .map(str::to_string),
        guild: value
            .get("guild")
            .and_then(|guild| guild.get("name"))
            .and_then(|name| name.as_str())
            .map(str::to_string),
        achievement_points: value
            .get("achievement_points")
            .and_then(|points| points.as_u64())
            .map(|points| points as u32),
        last_login: value
            .get("last_login_timestamp")
            .and_then(|stamp| stamp.as_i64())
            .and_then(millis),
        ..Detail::default()
    })
}

/// Read professions.
///
/// Only the current expansion's tier is kept. A character with eight tiers of
/// Blacksmithing behind them is a character with one that matters, and showing
/// all of them turns a roster row into a paragraph.
pub fn parse_professions(body: &[u8]) -> Outcome<Vec<Profession>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let mut professions = Vec::new();
    for (key, is_primary) in [("primaries", true), ("secondaries", false)] {
        let Some(list) = value.get(key).and_then(|list| list.as_array()) else {
            continue;
        };
        for entry in list {
            let Some(name) = entry
                .get("profession")
                .and_then(|profession| profession.get("name"))
                .and_then(|name| name.as_str())
            else {
                continue;
            };

            // Tiers arrive oldest-first, so the current expansion's is last.
            let tier = entry
                .get("tiers")
                .and_then(|tiers| tiers.as_array())
                .and_then(|tiers| tiers.last());

            professions.push(Profession {
                name: name.to_string(),
                tier: tier
                    .and_then(|tier| tier.get("tier"))
                    .and_then(|tier| tier.get("name"))
                    .and_then(|name| name.as_str())
                    .map(str::to_string),
                skill: tier
                    .and_then(|tier| tier.get("skill_points"))
                    .and_then(|points| points.as_u64())
                    .map(|points| points as u16),
                max_skill: tier
                    .and_then(|tier| tier.get("max_skill_points"))
                    .and_then(|points| points.as_u64())
                    .map(|points| points as u16),
                is_primary,
                // The API knows nothing about either. Specialisation trees
                // and knowledge are addon-only, and the merge is what fills
                // them in — see `Roster` merging in `ui/application.rs`.
                specialisations: Vec::new(),
                knowledge: 0,
            });
        }
    }

    Outcome::of_collection(professions)
}

/// Read the current season's Mythic+ rating.
pub fn parse_mythic_keystone(body: &[u8]) -> Outcome<u32> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    match value
        .get("current_mythic_rating")
        .and_then(|rating| rating.get("rating"))
        .and_then(|rating| rating.as_f64())
    {
        Some(rating) => Outcome::Found(rating as u32),
        // A character who has run no keys this season has no rating. That is an
        // answer about the character, not a broken parser.
        None => Outcome::Empty,
    }
}

/// Read gold out of the protected-character response.
pub fn parse_protected(body: &[u8]) -> Outcome<u64> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    match value.get("money").and_then(|money| money.as_u64()) {
        Some(money) => Outcome::Found(money),
        None => Outcome::Stale(Reason::Malformed(
            "the protected character carried no money".into(),
        )),
    }
}

/// Read the encounter ids a character has completed.
///
/// The response nests expansions, instances and modes; nothing downstream cares
/// about the tree, because a criterion refers to an encounter by id.
pub fn parse_encounters(body: &[u8]) -> Outcome<HashSet<u32>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // A character who has cleared nothing has no `expansions` key at all.
    let Some(expansions) = value.get("expansions").and_then(|list| list.as_array()) else {
        return Outcome::Empty;
    };

    let mut encounters = HashSet::new();
    for expansion in expansions {
        collect_encounters(expansion, &mut encounters);
    }
    if encounters.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(encounters)
    }
}

fn collect_encounters(value: &serde_json::Value, into: &mut HashSet<u32>) {
    // The nesting is expansion → instances → modes → progress → encounters, and
    // `progress` is an object where the rest are arrays. Rather than encode that
    // shape — which Blizzard has changed before — this walks whichever of the
    // known keys is present, in whichever form it is present.
    for key in ["instances", "modes", "progress", "encounters", "instance"] {
        let Some(child) = value.get(key) else {
            continue;
        };
        match child.as_array() {
            Some(list) => {
                for entry in list {
                    collect_encounters(entry, into);
                }
            }
            None => collect_encounters(child, into),
        }
    }

    if let Some(id) = value
        .get("encounter")
        .and_then(|encounter| encounter.get("id"))
        .and_then(|id| id.as_u64())
    {
        // `completed_count` is present and zero for an encounter that is listed
        // but never killed, which is not the same as having cleared it. Absent
        // means the response did not break it down, and being listed at all is
        // then the evidence.
        let killed = value
            .get("completed_count")
            .and_then(|count| count.as_u64())
            .unwrap_or(1);
        if killed > 0 {
            into.insert(id as u32);
        }
    }
}

/// Read what a character is wearing.
///
/// Only what is *worn*: an empty slot is simply not in `equipped_items`, and
/// this does not invent a row for it. Which slots exist is
/// [`Equipped::SLOTS`], and the character page is what subtracts one list from
/// the other — the parser's job is to report what came back.
///
/// A cosmetic slot has no `level` at all, which is why the field is an
/// `Option` and not a zero: the list is sorted by item level and a fabricated
/// number would sort a tabard above a real weak slot.
pub fn parse_equipment(body: &[u8]) -> Outcome<Vec<Equipped>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(items) = value.get("equipped_items").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the equipment response carried no equipped_items".into(),
        ));
    };

    let worn: Vec<Equipped> = items
        .iter()
        .filter_map(|item| {
            let slot = item.get("slot")?;
            Some(Equipped {
                slot: slot.get("type")?.as_str()?.to_string(),
                slot_name: slot
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap_or_default()
                    .to_string(),
                name: item.get("name")?.as_str()?.to_string(),
                level: item
                    .get("level")
                    .and_then(|level| level.get("value"))
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u16),
            })
        })
        .collect();

    // A character wearing nothing at all is a real answer and a naked one, but
    // it is also what a parser that has stopped understanding the response
    // produces. `Empty` is the variant that tells those apart downstream.
    if worn.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(worn)
    }
}

/// Read raid progress, one instance at a time.
///
/// The same body [`parse_encounters`] walks for ids, read for what a person
/// would say about it: which raid, which difficulty, how many of the bosses,
/// and who fell last. A difficulty the character has never set foot in is
/// absent rather than nought-of-eight — never attempted and wiped on the first
/// boss are different facts and only one of them is progress.
pub fn parse_raids(body: &[u8]) -> Outcome<Vec<RaidTier>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(expansions) = value.get("expansions").and_then(|list| list.as_array()) else {
        return Outcome::Empty;
    };

    let mut tiers = Vec::new();
    for expansion in expansions {
        let name = expansion
            .get("expansion")
            .and_then(|expansion| expansion.get("name"))
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_string();

        let Some(instances) = expansion.get("instances").and_then(|list| list.as_array()) else {
            continue;
        };
        for instance in instances {
            let Some(tier) = read_raid(instance, &name) else {
                continue;
            };
            tiers.push(tier);
        }
    }

    if tiers.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(tiers)
    }
}

/// One raid instance, with a row per difficulty that was attempted.
fn read_raid(instance: &serde_json::Value, expansion: &str) -> Option<RaidTier> {
    let name = instance.get("instance")?.get("name")?.as_str()?.to_string();

    let difficulties = instance
        .get("modes")?
        .as_array()?
        .iter()
        .filter_map(|mode| {
            let progress = mode.get("progress")?;
            // The last kill is a per-encounter stamp, so the raid's is the
            // latest of them. Blizzard does not report one for the instance.
            let last_kill = progress
                .get("encounters")
                .and_then(|list| list.as_array())
                .and_then(|encounters| {
                    encounters
                        .iter()
                        .filter(|encounter| {
                            encounter
                                .get("completed_count")
                                .and_then(|count| count.as_u64())
                                .unwrap_or(0)
                                > 0
                        })
                        .filter_map(|encounter| {
                            let boss = encounter.get("encounter")?.get("name")?.as_str()?;
                            let at = millis(encounter.get("last_kill_timestamp")?.as_i64()?)?;
                            Some((boss.to_string(), at))
                        })
                        .max_by_key(|(_, at)| *at)
                });

            Some(RaidDifficulty {
                name: mode.get("difficulty")?.get("name")?.as_str()?.to_string(),
                defeated: progress.get("completed_count")?.as_u64()? as u16,
                total: progress.get("total_count")?.as_u64()? as u16,
                last_kill,
            })
        })
        .collect::<Vec<_>>();

    if difficulties.is_empty() {
        return None;
    }
    Some(RaidTier {
        name,
        expansion: expansion.to_string(),
        difficulties,
    })
}

/// The highest renown across the account-wide major factions.
///
/// Account-wide since The War Within, so this describes the account rather than
/// the character. Reported as a fact and never counted as run progress — see
/// [`parse_reputations`] for the half of this that matters.
pub fn highest_renown(reputations: &serde_json::Value) -> Option<u32> {
    reputations
        .get("reputations")?
        .as_array()?
        .iter()
        .filter_map(|entry| {
            entry
                .get("standing")?
                .get("renown_level")?
                .as_u64()
                .map(|renown| renown as u32)
        })
        .max()
}

/// Assemble what a character's own data can answer.
pub fn primary_data(
    quests: HashSet<u32>,
    statistics: HashMap<u32, f64>,
    reputations: Reputations,
    encounters: HashSet<u32>,
) -> PrimaryData {
    PrimaryData {
        quests,
        statistics,
        encounters,
        reputations: reputations.standings,
        inherited_reputations: reputations.inherited,
        // Not from here. No endpoint attributes a point of reputation to a
        // character, so this is the addon's answer and the planner merges it
        // in — same as `achievements_done` below.
        earned_reputations: HashMap::new(),
        // Filled in by the planner, which is the only thing that knows what the
        // run has done as opposed to what the account has.
        achievements_done: HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_index_yields_every_character_across_every_licence() {
        // `wow_accounts` is plural, and a roster that reads only the first
        // licence silently loses characters.
        let body = br#"{
            "id": 1,
            "wow_accounts": [
              {"id": 11, "characters": [
                {"id": 5, "name": "Somechar", "level": 80,
                 "playable_class": {"name": "Druid"},
                 "playable_race": {"name": "Tauren"},
                 "faction": {"type": "HORDE"},
                 "realm": {"id": 61, "name": "Emerald Dream", "slug": "emerald-dream"}}
              ]},
              {"id": 12, "characters": [
                {"id": 6, "name": "Aeltor", "level": 70,
                 "playable_class": {"name": "Warrior"},
                 "playable_race": {"name": "Orc"},
                 "faction": {"type": "HORDE"},
                 "realm": {"id": 13, "name": "Mannoroth", "slug": "mannoroth"}}
              ]}
            ]
        }"#;

        let roster = parse_account(body).found().expect("a roster");
        assert_eq!(roster.len(), 2);
        assert_eq!(roster.characters[0].display_name, "Somechar");
        assert_eq!(roster.characters[0].key.name, "somechar");
        assert_eq!(roster.characters[1].wow_account_id, 12);
    }

    #[test]
    fn a_realm_with_no_slug_gets_one_derived() {
        let body = br#"{"wow_accounts":[{"id":1,"characters":[
            {"id":5,"name":"Velkurai","level":70,
             "realm":{"id":61,"name":"Emerald Dream"}}]}]}"#;
        let roster = parse_account(body).found().expect("a roster");
        assert_eq!(roster.characters[0].key.realm_slug, "emerald-dream");
    }

    #[test]
    fn a_missing_wow_accounts_key_is_stale_not_empty() {
        // An account with no characters still carries the key. Its absence means
        // the response changed shape, and calling that "no characters" would
        // empty the roster on screen.
        assert!(matches!(parse_account(br#"{"id":1}"#), Outcome::Stale(_)));
    }

    #[test]
    fn achievements_carry_partial_criteria_progress() {
        // The reason the criteria tree is worth reading at all: an entry appears
        // for anything with any progress, complete or not.
        let body = br#"{"achievements":[
            {"id": 1, "achievement": {"id": 1, "name": "Done"},
             "completed_timestamp": 1467331200000,
             "criteria": {"id": 10, "amount": 3, "is_completed": true,
                          "child_criteria": [
                            {"id": 11, "is_completed": true},
                            {"id": 12, "is_completed": false}]}},
            {"id": 2, "achievement": {"id": 2, "name": "Partway"}}
        ]}"#;

        let list = parse_achievements(body).found().expect("achievements");
        assert_eq!(list.len(), 2);

        let done = &list[0];
        assert!(done.completed_at.is_some());
        let criteria = done.criteria.as_ref().expect("a criteria tree");
        assert_eq!(criteria.required, 3);
        assert_eq!(criteria.children.len(), 2);

        // An unfinished achievement is still listed, with no timestamp.
        assert_eq!(list[1].completed_at, None);
    }

    #[test]
    fn the_profile_never_says_what_a_criterion_measures() {
        // It reports progress and not meaning. Inventing a kind here would be
        // inventing it from nothing, and a wrong kind draws a confident bar over
        // a number that means something else.
        let body = br#"{"achievements":[{"id":1,"achievement":{"id":1},
            "criteria":{"id":10,"amount":3,"is_completed":false}}]}"#;
        let list = parse_achievements(body).found().expect("achievements");
        assert_eq!(
            list[0].criteria.as_ref().unwrap().kind,
            CriterionKind::Unknown
        );
    }

    #[test]
    fn a_character_who_has_completed_no_quests_is_empty_not_stale() {
        // The one place a missing collection is genuinely an answer.
        assert_eq!(parse_completed_quests(br#"{}"#), Outcome::Empty);
    }

    #[test]
    fn completed_quests_flatten_to_a_set_of_ids() {
        let body = br#"{"quests":[{"id":100},{"id":200},{"id":300}]}"#;
        let quests = parse_completed_quests(body).found().expect("quests");
        assert_eq!(quests.len(), 3);
        assert!(quests.contains(&200));
    }

    #[test]
    fn statistics_flatten_out_of_their_category_tree() {
        // Criteria refer to a statistic by id; nothing downstream wants the
        // tree, and walking it at every lookup would be work done twice.
        let body = br#"{"categories":[
            {"id": 1, "name": "Character",
             "statistics": [{"id": 10, "quantity": 42.0}],
             "sub_categories": [
                {"id": 2, "statistics": [{"id": 20, "quantity": 7.0}],
                 "sub_categories": [
                    {"id": 3, "statistics": [{"id": 30, "quantity": 1.0}]}]}]}
        ]}"#;

        let statistics = parse_statistics(body).found().expect("statistics");
        assert_eq!(statistics.len(), 3);
        assert_eq!(statistics.get(&30), Some(&1.0));
    }

    #[test]
    fn renown_on_a_low_character_is_marked_inherited() {
        // Warbands handed it to them; they did not earn it. Counting it as this
        // character's progress is the failure that would make a run meaningless.
        let body = br#"{"reputations":[
            {"faction": {"id": 2570}, "standing": {"raw": 4200, "renown_level": 20}}
        ]}"#;
        let reputations = parse_reputations(body, 20).found().expect("reputations");
        assert!(reputations.inherited.contains(&2570));
        assert_eq!(reputations.standings.get(&2570), Some(&4200));
    }

    #[test]
    fn renown_on_a_levelled_character_is_left_alone() {
        // At the cap the standing is as plausibly theirs as anyone's, and
        // guessing further would be inventing data.
        let body = br#"{"reputations":[
            {"faction": {"id": 2570}, "standing": {"raw": 4200, "renown_level": 20}}
        ]}"#;
        let reputations = parse_reputations(body, 80).found().expect("reputations");
        assert!(reputations.inherited.is_empty());
    }

    #[test]
    fn a_plain_reputation_with_no_renown_is_never_inherited() {
        let body = br#"{"reputations":[
            {"faction": {"id": 69}, "standing": {"raw": 21000}}
        ]}"#;
        let reputations = parse_reputations(body, 20).found().expect("reputations");
        assert!(reputations.inherited.is_empty());
    }

    #[test]
    fn a_standing_keeps_its_words_as_well_as_its_number() {
        // The planner wants an id and a number; a page wants a name and a tier.
        // Both come out of one parse rather than two.
        let body = br#"{"reputations":[
            {"faction": {"id": 69, "name": "Darnassus"},
             "standing": {"raw": 21000, "value": 5000, "max": 21000, "name": "Revered"}},
            {"faction": {"id": 2570, "name": "Dream Wardens"},
             "standing": {"raw": 4200, "renown_level": 20}}
        ]}"#;
        let reputations = parse_reputations(body, 80).found().expect("reputations");

        // Alphabetical, because a page shows them in a column and Blizzard's
        // order is by internal id.
        let names: Vec<&str> = reputations
            .detail
            .iter()
            .map(|standing| standing.name.as_str())
            .collect();
        assert_eq!(names, ["Darnassus", "Dream Wardens"]);

        let darnassus = &reputations.detail[0];
        assert_eq!(darnassus.tier, "Revered");
        assert!((darnassus.fraction().expect("a fraction") - 5000.0 / 21000.0).abs() < 1e-9);

        // Renown factions have no tier word of their own, so the level is one.
        assert_eq!(reputations.detail[1].tier, "Renown 20");
        assert_eq!(reputations.detail[1].renown, 20);
    }

    #[test]
    fn a_maxed_standing_has_no_fraction_rather_than_a_full_bar() {
        // A full bar and a bar at ninety-nine per cent look the same, and only
        // one of them means there is nothing left to do.
        let body = br#"{"reputations":[
            {"faction": {"id": 69, "name": "Darnassus"},
             "standing": {"raw": 42999, "value": 0, "max": 0, "name": "Exalted"}}
        ]}"#;
        let reputations = parse_reputations(body, 80).found().expect("reputations");
        assert_eq!(reputations.detail[0].fraction(), None);
    }

    #[test]
    fn a_summary_yields_the_fields_a_roster_row_shows() {
        let body = br#"{"id": 5, "name": "Somechar", "level": 80,
            "average_item_level": 642, "equipped_item_level": 639,
            "achievement_points": 21450,
            "last_login_timestamp": 1785000000000,
            "active_spec": {"name": "Restoration"},
            "guild": {"name": "Dream Team"}}"#;

        let detail = parse_summary(body).found().expect("a summary");
        assert_eq!(detail.item_level, Some(642));
        assert_eq!(detail.equipped_item_level, Some(639));
        assert_eq!(detail.spec.as_deref(), Some("Restoration"));
        assert_eq!(detail.guild.as_deref(), Some("Dream Team"));
        assert_eq!(detail.achievement_points, Some(21450));
        assert!(detail.last_login.is_some());
    }

    #[test]
    fn a_guildless_character_is_not_a_broken_summary() {
        // Most alts have no guild. Every field on Detail is optional for
        // exactly this reason.
        let detail = parse_summary(br#"{"id": 5, "average_item_level": 600}"#)
            .found()
            .expect("a summary");
        assert_eq!(detail.guild, None);
        assert_eq!(detail.item_level, Some(600));
    }

    #[test]
    fn a_response_with_no_id_is_stale() {
        assert!(matches!(
            parse_summary(br#"{"name": "x"}"#),
            Outcome::Stale(_)
        ));
    }

    #[test]
    fn only_the_current_tier_of_a_profession_is_kept() {
        // A character with eight tiers of Blacksmithing behind them has one that
        // matters. Tiers arrive oldest-first, so the last is the current one.
        let body = br#"{"primaries": [
            {"profession": {"name": "Blacksmithing"},
             "tiers": [
               {"tier": {"name": "Classic Blacksmithing"}, "skill_points": 300,
                "max_skill_points": 300},
               {"tier": {"name": "Khaz Algar Blacksmithing"}, "skill_points": 84,
                "max_skill_points": 100}]}],
          "secondaries": [
            {"profession": {"name": "Cooking"},
             "tiers": [{"tier": {"name": "Khaz Algar Cooking"}, "skill_points": 40,
                        "max_skill_points": 100}]}]}"#;

        let professions = parse_professions(body).found().expect("professions");
        assert_eq!(professions.len(), 2);
        assert_eq!(professions[0].name, "Blacksmithing");
        assert_eq!(
            professions[0].tier.as_deref(),
            Some("Khaz Algar Blacksmithing")
        );
        assert_eq!(professions[0].skill, Some(84));
        assert!(professions[0].is_primary);
        assert!(!professions[1].is_primary);
    }

    #[test]
    fn a_character_with_no_professions_is_empty() {
        assert_eq!(parse_professions(br#"{}"#), Outcome::Empty);
    }

    #[test]
    fn no_keys_this_season_is_empty_rather_than_a_rating_of_zero() {
        // Zero is a rating somebody earned. Absent is a character who has not
        // played the content, and the two read differently.
        assert_eq!(
            parse_mythic_keystone(br#"{"current_period": {}}"#),
            Outcome::Empty
        );
        assert_eq!(
            parse_mythic_keystone(br#"{"current_mythic_rating": {"rating": 2418.7}}"#),
            Outcome::Found(2418)
        );
    }

    #[test]
    fn gold_comes_from_the_protected_endpoint_and_nowhere_else() {
        assert_eq!(
            parse_protected(br#"{"character": {}, "money": 91234567}"#),
            Outcome::Found(91234567)
        );
    }

    #[test]
    fn encounters_flatten_out_of_their_nesting() {
        // Expansions, instances, modes and progress, and a criterion only ever
        // refers to an encounter by id.
        let body = br#"{"expansions": [
            {"expansion": {"name": "Current"},
             "instances": [
               {"instance": {"name": "A Dungeon"},
                "modes": [
                  {"difficulty": {"type": "MYTHIC"},
                   "progress": {"encounters": [
                     {"encounter": {"id": 2600}, "completed_count": 3},
                     {"encounter": {"id": 2601}, "completed_count": 1}]}}]}]}]}"#;

        let encounters = parse_encounters(body).found().expect("encounters");
        assert!(encounters.contains(&2600));
        assert!(encounters.contains(&2601));
        assert_eq!(encounters.len(), 2);
    }

    #[test]
    fn an_encounter_listed_but_never_killed_does_not_count_as_cleared() {
        let body = br#"{"expansions": [{"instances": [{"modes": [{"progress":
            {"encounters": [{"encounter": {"id": 2600}, "completed_count": 0}]}}]}]}]}"#;
        assert_eq!(parse_encounters(body), Outcome::Empty);
    }

    #[test]
    fn a_character_who_has_cleared_nothing_is_empty_not_stale() {
        assert_eq!(parse_encounters(br#"{"character": {}}"#), Outcome::Empty);
    }

    #[test]
    fn renown_reports_the_highest_the_account_reached() {
        // Account-wide since The War Within, so this describes the account. It
        // is shown as a fact and never counted as run progress.
        let value: serde_json::Value = serde_json::from_slice(
            br#"{"reputations": [
                {"faction": {"id": 1}, "standing": {"renown_level": 12}},
                {"faction": {"id": 2}, "standing": {"renown_level": 25}},
                {"faction": {"id": 3}, "standing": {"raw": 21000}}]}"#,
        )
        .expect("json");
        assert_eq!(highest_renown(&value), Some(25));
    }

    #[test]
    fn requests_are_addressed_the_way_the_endpoints_want() {
        let key = CharacterKey::new("emerald-dream", "Somechar");
        let request = achievements(Region::Us, &key);
        assert!(request
            .url
            .contains("/profile/wow/character/emerald-dream/somechar/achievements"));
        assert!(request.url.contains("namespace=profile-us"));
    }

    #[test]
    fn the_protected_endpoint_is_addressed_by_numbers_not_by_name() {
        // Realm id and character id. Every other endpoint takes the slug and
        // the name, and mixing them up is a 404 that reads like a bug.
        let character = Character {
            key: CharacterKey::new("emerald-dream", "Somechar"),
            id: 12345,
            realm_id: 3684,
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            level: 80,
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            wow_account_id: 1,
        };
        let request = protected_character(Region::Us, &character);
        assert!(
            request
                .url
                .contains("/profile/user/wow/protected-character/3684-12345"),
            "{}",
            request.url
        );
    }

    // -- equipment ------------------------------------------------------------

    #[test]
    fn equipment_reads_the_slot_the_name_and_the_level() {
        let body = br#"{"equipped_items": [
            {"slot": {"type": "HEAD", "name": "Head"}, "name": "Helm of the Broken",
             "level": {"value": 639, "display_string": "Item Level 639"}},
            {"slot": {"type": "FINGER_1", "name": "Ring 1"}, "name": "Band of Oshu'gun",
             "level": {"value": 626}}
        ]}"#;
        let worn = parse_equipment(body).found().expect("equipment");
        assert_eq!(worn.len(), 2);
        assert_eq!(worn[0].slot, "HEAD");
        assert_eq!(worn[0].name, "Helm of the Broken");
        assert_eq!(worn[0].level, Some(639));
        assert_eq!(worn[1].slot_name, "Ring 1");
    }

    #[test]
    fn a_cosmetic_slot_has_no_item_level_rather_than_a_zero() {
        // The character page sorts on this and puts the weakest slot first. A
        // fabricated nought would put the tabard there and hide the real
        // answer, which is the one thing that list exists to show.
        let body = br#"{"equipped_items": [
            {"slot": {"type": "TABARD", "name": "Tabard"}, "name": "Tabard of the Kurenai"}
        ]}"#;
        let worn = parse_equipment(body).found().expect("equipment");
        assert_eq!(worn[0].level, None);
        assert!(worn[0].is_cosmetic());
    }

    #[test]
    fn an_empty_slot_is_absent_rather_than_reported_empty() {
        // Blizzard does not list a slot with nothing in it, and neither does
        // this. Which slots exist is `Equipped::SLOTS`, and putting the two
        // together is the page's job — a parser that invented a row here would
        // be inventing the most useful fact on the page.
        let body = br#"{"equipped_items": [
            {"slot": {"type": "HEAD", "name": "Head"}, "name": "Helm", "level": {"value": 600}}
        ]}"#;
        let worn = parse_equipment(body).found().expect("equipment");
        assert_eq!(worn.len(), 1);
        assert!(!worn.iter().any(|item| item.slot == "OFF_HAND"));
    }

    #[test]
    fn a_naked_character_is_empty_and_a_broken_response_is_stale() {
        assert_eq!(
            parse_equipment(br#"{"equipped_items": []}"#),
            Outcome::Empty
        );
        assert!(matches!(
            parse_equipment(br#"{"character": {}}"#),
            Outcome::Stale(_)
        ));
    }

    // -- raids ----------------------------------------------------------------

    #[test]
    fn raids_are_read_per_instance_and_per_difficulty() {
        let body = br#"{"expansions": [{"expansion": {"name": "The War Within"},
          "instances": [{"instance": {"id": 1296, "name": "Liberation of Undermine"},
            "modes": [
              {"difficulty": {"type": "NORMAL", "name": "Normal"}, "status": {"type": "COMPLETE"},
               "progress": {"completed_count": 8, "total_count": 8, "encounters": [
                 {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 3,
                  "last_kill_timestamp": 1750000000000},
                 {"encounter": {"id": 2640, "name": "Mug'Zee"}, "completed_count": 1,
                  "last_kill_timestamp": 1753800000000}]}},
              {"difficulty": {"type": "HEROIC", "name": "Heroic"}, "status": {"type": "IN_PROGRESS"},
               "progress": {"completed_count": 2, "total_count": 8, "encounters": [
                 {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 1,
                  "last_kill_timestamp": 1754000000000}]}}]}]}]}"#;
        let tiers = parse_raids(body).found().expect("raids");
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "Liberation of Undermine");
        assert_eq!(tiers[0].expansion, "The War Within");
        assert_eq!(tiers[0].difficulties.len(), 2);
        assert_eq!(tiers[0].difficulties[1].defeated, 2);
        assert_eq!(tiers[0].difficulties[1].total, 8);
        // The instance's last kill is the latest across every difficulty, not
        // the last row in the response.
        let (boss, _, difficulty) = tiers[0].last_kill().expect("a last kill");
        assert_eq!(boss, "Vexie");
        assert_eq!(difficulty, "Heroic");
    }

    #[test]
    fn a_difficulty_never_entered_carries_no_last_kill() {
        let body = br#"{"expansions": [{"expansion": {"name": "The War Within"},
          "instances": [{"instance": {"name": "Liberation of Undermine"},
            "modes": [{"difficulty": {"type": "MYTHIC", "name": "Mythic"},
              "progress": {"completed_count": 0, "total_count": 8, "encounters": [
                {"encounter": {"id": 2639, "name": "Vexie"}, "completed_count": 0}]}}]}]}]}"#;
        let tiers = parse_raids(body).found().expect("raids");
        assert_eq!(tiers[0].difficulties[0].defeated, 0);
        assert_eq!(tiers[0].difficulties[0].last_kill, None);
        assert_eq!(tiers[0].last_kill(), None);
    }

    #[test]
    fn a_character_who_has_never_raided_is_empty() {
        assert_eq!(parse_raids(br#"{"character": {}}"#), Outcome::Empty);
    }
}
