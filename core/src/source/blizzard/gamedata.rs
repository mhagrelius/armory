//! `/data/...`: the catalogue.
//!
//! What an achievement is called, what it is worth, and what category it sits
//! in. Deliberately *not* where a criterion's meaning comes from: the public
//! achievement endpoint returns the criteria tree's shape and requirements and
//! never the asset each node measures. That mapping is in the client database
//! and in the game's own Lua, and [`crate::achievement::CriterionKind`]
//! is filled in from there.

use super::super::{parse_json, Outcome, Reason, Request, SourceId};
use super::{url, Namespace, Region};
use crate::adventure::{Encounter, Instance};

const SOURCE: SourceId = SourceId::BlizzardGameData;

/// One achievement, as the catalogue describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Achievement {
    pub id: u32,
    pub name: String,
    pub category: String,
    pub points: u32,
    pub description: String,
    /// Feats of Strength and the like. These can never be earned again and are
    /// excluded from a run rather than left in it as permanent zeroes.
    pub is_unrepeatable: bool,
}

/// Every achievement id the game has.
pub fn achievement_index(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            "/data/wow/achievement/index",
            &[],
        ),
    )
}

/// One achievement.
pub fn achievement(region: Region, id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            &format!("/data/wow/achievement/{id}"),
            &[],
        ),
    )
}

/// Every dungeon and raid the Adventure Guide knows.
pub fn instance_index(region: Region) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            "/data/wow/journal-instance/index",
            &[],
        ),
    )
}

/// One instance: what it is, and which encounters are in it.
pub fn instance(region: Region, id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            &format!("/data/wow/journal-instance/{id}"),
            &[],
        ),
    )
}

/// One encounter: its lore, and what it drops.
pub fn encounter(region: Region, id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            &format!("/data/wow/journal-encounter/{id}"),
            &[],
        ),
    )
}

/// One item, for its name.
///
/// The reverse of [`item_search`], and the only direction that works from an
/// auction listing: a listing carries an item id and nothing else, and no
/// endpoint turns a list of ids into a list of names. So the browser fills them
/// in one call at a time, the way `ui/images.rs` fills in artwork.
pub fn item(region: Region, id: u32) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            &format!("/data/wow/item/{id}"),
            &[],
        ),
    )
}

/// The instance index, as `(id, name)`.
pub fn parse_instance_index(body: &[u8]) -> Outcome<Vec<(u32, String)>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let Some(list) = value.get("instances").and_then(|l| l.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the instance index carried no instances".into(),
        ));
    };
    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| {
                Some((
                    entry.get("id")?.as_u64()? as u32,
                    entry.get("name")?.as_str()?.to_string(),
                ))
            })
            .collect(),
    )
}

/// One instance, with the encounters it contains.
///
/// `map` is a `UiMapID` — the same key a zone entry and a chronicle session
/// join on, which is what lets an evening spent in Karazhan find Karazhan's own
/// description without matching on a name.
pub fn parse_instance(body: &[u8]) -> Outcome<Instance> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let (Some(id), Some(name)) = (
        value.get("id").and_then(|v| v.as_u64()),
        value.get("name").and_then(|v| v.as_str()),
    ) else {
        return Outcome::Stale(Reason::Malformed("the instance had no id or name".into()));
    };

    Outcome::Found(Instance {
        id: id as u32,
        name: name.to_string(),
        map: value
            .get("map")
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_u64())
            .map(|id| id as u32),
        // Blizzard's own account of what the place is. The single reason this
        // endpoint is worth calling: nothing else says plainly what the deal
        // with a given raid was, and the wiki's version is a plot summary
        // written afterwards rather than the premise the game gives you.
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        expansion: value
            .get("category")
            .and_then(|c| c.get("type"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        // Encounter ids only; each needs its own call for its lore and loot.
        // Duplicates are real — a raid with a faction-split wing lists the same
        // boss twice under two ids — so they are kept as the API gives them.
        encounters: value
            .get("encounters")
            .and_then(|l| l.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|e| Some(e.get("id")?.as_u64()? as u32))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// One encounter, with its lore and the items it drops.
pub fn parse_encounter(body: &[u8]) -> Outcome<Encounter> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let (Some(id), Some(name)) = (
        value.get("id").and_then(|v| v.as_u64()),
        value.get("name").and_then(|v| v.as_str()),
    ) else {
        return Outcome::Stale(Reason::Malformed("the encounter had no id or name".into()));
    };

    Outcome::Found(Encounter {
        id: id as u32,
        name: name.to_string(),
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        // The nested `item` is the real item; the outer `id` is the journal's
        // own row number and joins to nothing.
        loot: value
            .get("items")
            .and_then(|l| l.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|e| Some(e.get("item")?.get("id")?.as_u64()? as u32))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// What one item response says about the item.
///
/// The name is what a browser needs; the binding is what decides whether an
/// item can be *sold* at all, which is most of the answer to "is this worth
/// looking for". Both come from the one call, so asking for the second costs
/// nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    /// `false` for Bind-on-Pickup.
    ///
    /// Absent binding means freely tradeable — most reagents carry none at all
    /// — so silence is a yes here, unusually. `ON_ACQUIRE` is the only value
    /// that stops a thing reaching the auction house.
    pub sellable: bool,
    pub quality: Option<String>,
}

/// One item's name, binding and quality.
pub fn parse_item(body: &[u8]) -> Outcome<Item> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    let Some(name) = value
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
    else {
        return Outcome::Stale(Reason::Malformed(
            "the item response carried no name".into(),
        ));
    };

    let binding = value
        .get("preview_item")
        .and_then(|p| p.get("binding"))
        .and_then(|b| b.get("type"))
        .and_then(|t| t.as_str());

    Outcome::Found(Item {
        name: name.to_string(),
        sellable: binding != Some("ON_ACQUIRE"),
        quality: value
            .get("quality")
            .and_then(|q| q.get("type"))
            .and_then(|t| t.as_str())
            .map(str::to_string),
    })
}

/// The name off an item response.
pub fn parse_item_name(body: &[u8]) -> Outcome<String> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };
    match value.get("name").and_then(|name| name.as_str()) {
        Some(name) if !name.is_empty() => Outcome::Found(name.to_string()),
        // An item with no name is a response shape that has changed, not an
        // item nobody named. Reported rather than cached as an empty string,
        // which would look like a name that had already been fetched.
        _ => Outcome::Stale(Reason::Malformed(
            "the item response carried no name".into(),
        )),
    }
}

/// Look an item up by name.
///
/// The only way to turn "Mycobloom" into 197794, and so the only way somebody
/// adds a price watch without knowing an item id. The search endpoints take a
/// locale-suffixed field name — `name.en_US` — because an item has a name per
/// locale and Blizzard will not guess which one is being searched.
pub fn item_search(region: Region, name: &str) -> Request {
    Request::get(
        SOURCE,
        url(
            region,
            Namespace::Static,
            "/data/wow/search/item",
            &[
                (&format!("name.{}", region.default_locale()), name),
                ("orderby", "id"),
                // A name fragment matches hundreds of items. Anyone who has to
                // scroll past twenty-five to find theirs is better served by
                // typing more of the name.
                ("_pageSize", "25"),
            ],
        ),
    )
}

/// Read a search response into ids and names.
pub fn parse_item_search(body: &[u8], locale: &str) -> Outcome<Vec<(u32, String)>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(results) = value.get("results").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the item search carried no results".into(),
        ));
    };

    Outcome::of_collection(
        results
            .iter()
            .filter_map(|result| {
                let data = result.get("data")?;
                let name = data.get("name")?;
                Some((
                    data.get("id")?.as_u64()? as u32,
                    // Search results carry every locale's name in one object,
                    // unlike every other endpoint, which resolves against the
                    // `locale` parameter and returns a bare string.
                    name.get(locale)
                        .or_else(|| name.get("en_US"))
                        .and_then(|name| name.as_str())?
                        .to_string(),
                ))
            })
            .collect(),
    )
}

/// Read one achievement out of the catalogue.
pub fn parse_achievement(body: &[u8]) -> Outcome<Achievement> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(id) = value.get("id").and_then(|id| id.as_u64()) else {
        return Outcome::Stale(Reason::Malformed("an achievement with no id".into()));
    };

    let category = value.get("category");
    let category_name = category
        .and_then(|category| category.get("name"))
        .and_then(|name| name.as_str())
        .unwrap_or_default()
        .to_string();

    // Blizzard flags the category rather than the achievement, and the flag is
    // the only machine-readable signal that something can never be earned again.
    let is_unrepeatable = category
        .and_then(|category| category.get("is_guild_category"))
        .and_then(|flag| flag.as_bool())
        .unwrap_or(false)
        || category_name.contains("Feats of Strength")
        || category_name.contains("Legacy");

    Outcome::Found(Achievement {
        id: id as u32,
        name: value
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_string(),
        category: category_name,
        points: value
            .get("points")
            .and_then(|points| points.as_u64())
            .unwrap_or(0) as u32,
        description: value
            .get("description")
            .and_then(|text| text.as_str())
            .unwrap_or_default()
            .to_string(),
        is_unrepeatable,
    })
}

/// Read the achievement index into a list of ids.
pub fn parse_achievement_index(body: &[u8]) -> Outcome<Vec<u32>> {
    let value = match parse_json(SOURCE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(list) = value.get("achievements").and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(
            "the achievement index carried no achievements".into(),
        ));
    };

    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| entry.get("id").and_then(|id| id.as_u64()))
            .map(|id| id as u32)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_achievement_is_read_whole() {
        let body = br#"{"id": 4956, "name": "Loremaster of Kalimdor", "points": 50,
                        "description": "Complete the Kalimdor quest achievements.",
                        "category": {"id": 97, "name": "Quests"}}"#;
        let achievement = parse_achievement(body).found().expect("an achievement");
        assert_eq!(achievement.id, 4956);
        assert_eq!(achievement.name, "Loremaster of Kalimdor");
        assert_eq!(achievement.points, 50);
        assert_eq!(achievement.category, "Quests");
        assert!(!achievement.is_unrepeatable);
    }

    #[test]
    fn a_feat_of_strength_is_flagged_as_unrepeatable() {
        // These can never be earned again, so they leave the run rather than
        // sitting in it forever as zeroes.
        let body = br#"{"id": 1, "name": "Gone", "category": {"name": "Feats of Strength"}}"#;
        let achievement = parse_achievement(body).found().expect("an achievement");
        assert!(achievement.is_unrepeatable);
    }

    #[test]
    fn a_search_result_carries_every_locales_name_at_once() {
        // Unlike every other endpoint, which resolves against the `locale`
        // parameter and returns a bare string. Reading this one the usual way
        // yields nothing and looks like an item with no name.
        let body = br#"{"page":1,"results":[
            {"data":{"id":197794,"name":{"en_US":"Mycobloom","de_DE":"Pilzbluete"}}},
            {"data":{"id":210796,"name":{"en_US":"Crystalline Powder"}}}]}"#;
        let found = parse_item_search(body, "en_US").found().expect("results");
        assert_eq!(found[0], (197794, "Mycobloom".to_string()));
        assert_eq!(found[1].0, 210796);

        // A locale Blizzard has no name in falls back rather than dropping the
        // item out of the results.
        let found = parse_item_search(body, "ko_KR").found().expect("results");
        assert_eq!(found[0].1, "Mycobloom");
    }

    #[test]
    fn a_search_names_the_locale_it_is_searching_in() {
        // `name=` alone matches nothing; the field has to carry the locale.
        let request = item_search(Region::Us, "Mycobloom");
        assert!(
            request.url.contains("name.en_US=Mycobloom"),
            "{}",
            request.url
        );
        assert!(item_search(Region::Eu, "x").url.contains("name.en_GB="));
    }

    #[test]
    fn the_catalogue_is_asked_for_in_the_static_namespace() {
        // Static, not profile: a wrong namespace 404s and the 404 reads like a
        // missing achievement.
        assert!(achievement(Region::Us, 4956)
            .url
            .contains("namespace=static-us"));
    }
}
