//! Mounts, pets and toys: what the account has, and what exists.
//!
//! Two halves that have to be joined. The profile side says what is collected,
//! account-wide — mounts, pets and toys are shared across every character, and
//! have been for years. The game data side says what exists at all. Missing is
//! the difference, and neither half alone can produce it.
//!
//! What Blizzard will not say is where anything comes from. `/data/wow/mount`
//! carries a `source` with a one-word `type` — `DROP`, `VENDOR`, `QUEST` — and
//! no NPC, no zone, no drop rate and no lockout. Pets have no structured source
//! field at all and toys are not a first-class type. That ceiling is why
//! [`Collectible::source`] is a coarse enum rather than a sentence, and why a
//! link out is what stands in for the sentence.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::super::{parse_json, Outcome, Reason, Request, SourceId};
use super::{url, Namespace, Region};
use crate::character::Faction;

const PROFILE: SourceId = SourceId::BlizzardProfile;
const DATA: SourceId = SourceId::BlizzardGameData;

/// Which collection something belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Kind {
    Mount,
    Pet,
    Toy,
    /// Housing decor, added to the API in 11.2.7 alongside Midnight's housing.
    ///
    /// The odd one out in one respect: decor is owned in *quantities* — a house
    /// wants six of the same chair — and Blizzard's collection response carries
    /// a count. Armory records owned or not owned, the same as the other three,
    /// because it tracks a collection rather than furnishes a house, and "have
    /// I got one of these" is the question a collection page answers.
    Decor,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Mount, Kind::Pet, Kind::Toy, Kind::Decor];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Mount => "Mounts",
            Kind::Pet => "Pets",
            Kind::Toy => "Toys",
            Kind::Decor => "Decor",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Kind::Mount => "mount",
            Kind::Pet => "pet",
            Kind::Toy => "toy",
            Kind::Decor => "decor",
        }
    }

    /// The Wowhead path for this kind.
    ///
    /// Linking is not scraping. Wowhead's terms forbid automated access and its
    /// robots.txt names the crawlers, so Armory fetches nothing from it — but a
    /// link a person clicks is a person visiting a website, which is the whole
    /// answer to "where does this come from" that Blizzard will not give.
    fn wowhead_path(self) -> &'static str {
        match self {
            Kind::Mount => "spell",
            Kind::Pet => "npc",
            // A piece of decor is usually backed by an item, and `link_id`
            // carries that item where the detail call has supplied one.
            Kind::Toy | Kind::Decor => "item",
        }
    }
}

/// How coarse Blizzard's answer to "where is this from" is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Source {
    Drop,
    Vendor,
    Quest,
    Achievement,
    Profession,
    Pvp,
    Promotion,
    /// The field was absent, which is common for anything old.
    #[default]
    Unknown,
}

impl Source {
    fn from_type(code: &str) -> Source {
        match code {
            "DROP" => Source::Drop,
            "VENDOR" => Source::Vendor,
            "QUEST" => Source::Quest,
            "ACHIEVEMENT" => Source::Achievement,
            "PROFESSION" | "TRADESKILL" => Source::Profession,
            "PVP" => Source::Pvp,
            "PROMOTION" | "TCG" | "STORE" => Source::Promotion,
            _ => Source::Unknown,
        }
    }

    /// Read a source out of the journals' `sourceText`.
    ///
    /// The in-game journals give a sentence — "Drop: Attumen the Huntsman,
    /// Karazhan" — where the web API gives one word or nothing. The sentence is
    /// kept whole for display; this only classifies its first clause, so the
    /// list can be sorted with the actionable entries first.
    ///
    /// Matching on the leading word rather than searching the whole string:
    /// "Vendor: sold near the Drop Zone" is a vendor, and a substring search
    /// would call it a drop.
    pub fn from_text(text: &str) -> Source {
        let head = text
            .split(&[':', '\n'][..])
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();

        match head.as_str() {
            "drop" | "world drop" => Source::Drop,
            // The Trading Post is a vendor that rotates. Its stock comes back,
            // so it is not the dead end that a trading-card mount is.
            "vendor" | "trading post" => Source::Vendor,
            "quest" => Source::Quest,
            "achievement" => Source::Achievement,
            "profession" => Source::Profession,
            "pvp" | "arena" | "battleground" => Source::Pvp,
            "promotion"
            | "trading card game"
            | "collector's edition"
            | "in-game shop"
            | "blizzard store"
            | "recruit-a-friend" => Source::Promotion,
            _ => Source::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Drop => "Drop",
            Source::Vendor => "Vendor",
            Source::Quest => "Quest",
            Source::Achievement => "Achievement",
            Source::Profession => "Profession",
            Source::Pvp => "PvP",
            Source::Promotion => "Promotion",
            Source::Unknown => "Unknown",
        }
    }

    /// Whether a run could plausibly obtain this again.
    ///
    /// Promotional and trading-card mounts cannot be re-earned by anybody, so
    /// they leave a run rather than sitting in it as permanent zeroes — the same
    /// reasoning as a Feat of Strength.
    pub fn is_repeatable(self) -> bool {
        !matches!(self, Source::Promotion)
    }
}

/// One thing that can be collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collectible {
    pub kind: Kind,
    pub id: u32,
    pub name: String,
    pub source: Source,
    /// The sentence the in-game journal gives, markup stripped.
    ///
    /// `None` from the web API, which has no such text at all — a mount there
    /// carries the word `DROP` and nothing else. This is the single biggest
    /// reason the addon is the better source for collections.
    #[serde(default)]
    pub description: Option<String>,
    /// Flavour text: what the thing says about itself rather than where it came
    /// from. Journals only; the web API has none.
    #[serde(default)]
    pub flavour: Option<String>,
    /// The texture the game draws for this, as a FileDataID.
    ///
    /// Not a URL and not renderable on its own: the art lives inside the
    /// client's CASC archives. It is kept because it is the key any icon
    /// service is looked up by, and discarding it would mean another logout to
    /// get it back.
    #[serde(default)]
    pub icon: Option<u32>,
    /// The creature display, for the 3D render Blizzard's media endpoint
    /// serves. Mounts and pets only.
    #[serde(default)]
    pub display: Option<u32>,
    /// Which faction can use it at all, when only one can.
    ///
    /// A mount the other faction gets is not a gap in this account's
    /// collection, and counting it as missing overstates the backlog by a few
    /// hundred.
    #[serde(default)]
    pub faction: Option<Faction>,
    /// The id Wowhead indexes this under, which is not always `id`: a mount is
    /// indexed by its spell and a pet by its creature.
    pub link_id: u32,
    /// Whether this can be caged and traded. Pets only, and the journal is the
    /// only source: the web API's pet record does not say.
    ///
    /// `None` means nobody has told us, which is not the same as `Some(false)`.
    /// Most pets cannot be caged, so treating silence as "yes" would offer a
    /// collection's worth of things that cannot be sold, and treating it as
    /// "no" before the addon has ever run would offer nothing at all — the
    /// difference is what the resale page says when it is empty.
    #[serde(default)]
    pub tradeable: Option<bool>,
}

impl Collectible {
    /// Where a person can go to read about this.
    ///
    /// Linking is not scraping. Wowhead's terms forbid automated access and its
    /// robots.txt names the crawlers, so Armory fetches nothing from it — but a
    /// link a person clicks is a person visiting a website, and it is the whole
    /// answer to "what does this actually look like and how do I get it".
    /// `None` when the id space is a guess rather than a fact.
    ///
    /// A toy and a piece of decor are addressed on Wowhead by the *item* they
    /// wrap, and the index does not give one — until a detail call lands,
    /// `link_id` is the collection's own id in a completely different id
    /// space. Linking anyway sends somebody to a real, unrelated item: a
    /// person clicking a chair got a belt, which is the worst kind of wrong,
    /// because a plausible page reads as correct until you look at it.
    ///
    /// The earlier reasoning here was that "a link that lands on the wrong page
    /// is recoverable, and no link at all is not". That is true for a mount,
    /// where the wrong id lands on nothing and fails visibly. It is false for
    /// anything addressed by item, and this is the case it was wrong about.
    pub fn wowhead_url(&self) -> Option<String> {
        if self.item_id_is_guessed() {
            return None;
        }
        Some(format!(
            "https://www.wowhead.com/{}={}",
            self.kind.wowhead_path(),
            self.link_id
        ))
    }

    /// Whether `link_id` is still the collection id standing in for an item.
    ///
    /// The index sets `link_id` to the collection id, and a detail call
    /// replaces it with the real one — so the two being equal means no detail
    /// has landed. Only meaningful for the kinds addressed by item; a mount's
    /// `link_id` legitimately equals its id when the spell is absent, and a
    /// mount is not linked by item anyway.
    pub fn item_id_is_guessed(&self) -> bool {
        matches!(self.kind, Kind::Toy | Kind::Decor) && self.link_id == self.id
    }

    /// The Warcraft Wiki, which is community-run and reads better for lore.
    pub fn wiki_url(&self) -> String {
        format!(
            "https://warcraft.wiki.gg/wiki/Special:Search?search={}",
            super::encode(&self.name)
        )
    }

    /// The item this is, for anything addressed by item.
    ///
    /// A toy and a piece of decor are both wrappers around an item, and the item
    /// is what an icon is looked up by. A mount is a spell and a pet a creature,
    /// so for those the collection id is the only key there is — and neither
    /// needs one, because both are drawn from a creature display for nothing.
    pub fn item_id(&self) -> u32 {
        match self.kind {
            Kind::Toy | Kind::Decor => self.link_id,
            Kind::Mount | Kind::Pet => self.id,
        }
    }

    /// The item this is, only where that is known.
    ///
    /// What an icon lookup should use. Asking the media service for the icon of
    /// item 5 when 5 is a decor id fetches a real icon for the wrong thing,
    /// which is how a chair came to be drawn as a belt.
    pub fn known_item_id(&self) -> Option<u32> {
        (!self.item_id_is_guessed()).then(|| self.item_id())
    }

    /// Whether this account can obtain it at all.
    ///
    /// A faction-locked mount is not missing from a collection that could never
    /// have held it.
    pub fn obtainable_by(&self, faction: Faction) -> bool {
        match self.faction {
            Some(only) if only != faction => false,
            _ => self.source.is_repeatable(),
        }
    }

    /// Fold in what another reading of the same thing knows.
    ///
    /// Two sources describe every collectible and they know different things.
    /// The in-game journal has the sentence, the flavour text, the icon, the
    /// creature display and the faction lock; the web API has a name and one
    /// word. Whichever arrives second must not flatten the other — an index
    /// sync landing after a logout would otherwise take the artwork off every
    /// mount in the collection, because the API has no `display` to give and
    /// writing `None` over one is indistinguishable from learning there is
    /// none.
    ///
    /// So this is richest-wins per field rather than newest-wins per record.
    pub fn merge(&mut self, other: &Collectible) {
        if self.name.is_empty() {
            self.name.clone_from(&other.name);
        }
        if self.source == Source::Unknown {
            self.source = other.source;
        }
        if self.description.as_deref().unwrap_or_default().is_empty() {
            self.description.clone_from(&other.description);
        }
        if self.flavour.as_deref().unwrap_or_default().is_empty() {
            self.flavour.clone_from(&other.flavour);
        }
        self.icon = self.icon.or(other.icon);
        self.display = self.display.or(other.display);
        self.faction = self.faction.or(other.faction);
        // The journal is the only source for this, so a web-API row landing
        // second must not write its `None` over an answer.
        self.tradeable = self.tradeable.or(other.tradeable);
        // A link to the collection id is the fallback the index sets when it
        // has nothing better. A real one — a mount's spell, a toy's item — is
        // never equal to it.
        if self.link_id == self.id && other.link_id != other.id {
            self.link_id = other.link_id;
        }
    }
}

// -- what the account has ----------------------------------------------------

pub fn collected(region: Region, kind: Kind) -> Request {
    let path = match kind {
        Kind::Mount => "/profile/user/wow/collections/mounts",
        Kind::Pet => "/profile/user/wow/collections/pets",
        Kind::Toy => "/profile/user/wow/collections/toys",
        Kind::Decor => "/profile/user/wow/collections/decor",
    };
    Request::get(PROFILE, url(region, Namespace::Profile, path, &[]))
}

/// Read the ids the account already has.
///
/// The responses nest differently — Blizzard names the list after the
/// collection and the id after the thing — so each is picked apart on its own
/// rather than through one shape that would have to be a lie about the rest.
pub fn parse_collected(body: &[u8], kind: Kind) -> Outcome<HashSet<u32>> {
    let value = match parse_json(PROFILE, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let (list_keys, item_key): (&[&str], &str) = match kind {
        Kind::Mount => (&["mounts"], "mount"),
        Kind::Pet => (&["pets"], "species"),
        Kind::Toy => (&["toys"], "toy"),
        // Housing names its list `decor_items` in the game-data index. The
        // profile side is unconfirmed — this account has never furnished a
        // house, so the endpoint answers 404 and there is nothing to record a
        // fixture from — so both spellings are accepted.
        Kind::Decor => (&["decor_items", "decor"], "decor"),
    };

    let Some(list) = list_keys
        .iter()
        .find_map(|key| value.get(key))
        .and_then(|list| list.as_array())
    else {
        return Outcome::Stale(Reason::Malformed(format!(
            "the {} response carried no {} list",
            list_keys[0], list_keys[0]
        )));
    };

    let ids: HashSet<u32> = list
        .iter()
        .filter_map(|entry| {
            entry
                .get(item_key)
                .and_then(|item| item.get("id"))
                // Decor's entries were still settling when this was written and
                // may carry the id at the top level beside a quantity rather
                // than nested. Falling back is cheap; guessing wrong and
                // reporting an empty collection is not.
                .or_else(|| entry.get("id"))
                .and_then(|id| id.as_u64())
        })
        .map(|id| id as u32)
        .collect();

    if ids.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Found(ids)
    }
}

// -- what exists -------------------------------------------------------------

pub fn index(region: Region, kind: Kind) -> Request {
    let path = match kind {
        Kind::Mount => "/data/wow/mount/index",
        Kind::Pet => "/data/wow/pet/index",
        Kind::Toy => "/data/wow/toy/index",
        Kind::Decor => "/data/wow/decor/index",
    };
    Request::get(DATA, url(region, Namespace::Static, path, &[]))
}

pub fn detail(region: Region, kind: Kind, id: u32) -> Request {
    let path = match kind {
        Kind::Mount => format!("/data/wow/mount/{id}"),
        Kind::Pet => format!("/data/wow/pet/{id}"),
        Kind::Toy => format!("/data/wow/toy/{id}"),
        Kind::Decor => format!("/data/wow/decor/{id}"),
    };
    Request::get(DATA, url(region, Namespace::Static, &path, &[]))
}

/// Read an index into ids and names.
///
/// The index gives no source at all, so what comes out of here is a catalogue
/// of names. Sources arrive one call at a time from [`parse_detail`], which is
/// why the collections page fills in its "where from" column gradually.
pub fn parse_index(body: &[u8], kind: Kind) -> Outcome<Vec<Collectible>> {
    let value = match parse_json(DATA, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    // Not the path, and not the singular: `/data/wow/decor/index` answers with
    // `decor_items`, which is the one list key in this API that is not simply
    // the plural of the thing. Guessing it cost a whole sync — the response
    // arrived, all 267 KB and 1,861 entries of it, and parsed to nothing.
    let list_key = match kind {
        Kind::Mount => "mounts",
        Kind::Pet => "pets",
        Kind::Toy => "toys",
        Kind::Decor => "decor_items",
    };

    let Some(list) = value.get(list_key).and_then(|list| list.as_array()) else {
        return Outcome::Stale(Reason::Malformed(format!(
            "the {list_key} index carried no {list_key}"
        )));
    };

    Outcome::of_collection(
        list.iter()
            .filter_map(|entry| {
                let id = entry.get("id").and_then(|id| id.as_u64())? as u32;
                Some(Collectible {
                    kind,
                    id,
                    name: entry
                        .get("name")
                        .and_then(|name| name.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    source: Source::Unknown,
                    // None of this exists in the web API. The index gives an
                    // id and a name and stops.
                    description: None,
                    flavour: None,
                    icon: None,
                    display: None,
                    faction: None,
                    // Until the detail call lands, the collection id is the best
                    // link available. Wrong for mounts, which Wowhead indexes by
                    // spell — but a link that lands on the wrong page is
                    // recoverable, and no link at all is not.
                    link_id: id,
                    // The web API does not say, and silence is not "no".
                    tradeable: None,
                })
            })
            .collect(),
    )
}

/// Read one collectible's detail, including whatever source Blizzard admits to.
pub fn parse_detail(body: &[u8], kind: Kind) -> Outcome<Collectible> {
    let value = match parse_json(DATA, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let Some(id) = value.get("id").and_then(|id| id.as_u64()) else {
        return Outcome::Stale(Reason::Malformed("a collectible with no id".into()));
    };
    let id = id as u32;

    // Mounts are indexed on Wowhead by the spell that summons them and pets by
    // their creature, neither of which is the collection id.
    let link_id = match kind {
        Kind::Mount => value
            .get("source_spell")
            .or_else(|| value.get("spell"))
            .and_then(|spell| spell.get("id"))
            .and_then(|id| id.as_u64())
            .map(|id| id as u32)
            .unwrap_or(id),
        Kind::Pet => value
            .get("creature_display")
            .and_then(|display| display.get("id"))
            .and_then(|id| id.as_u64())
            .map(|id| id as u32)
            .unwrap_or(id),
        // Both are backed by an item, and the item is what Wowhead indexes and
        // what an icon can be looked up by.
        Kind::Toy | Kind::Decor => value
            .get("item")
            .and_then(|item| item.get("id"))
            .and_then(|id| id.as_u64())
            .map(|id| id as u32)
            .unwrap_or(id),
    };

    Outcome::Found(Collectible {
        kind,
        id,
        // A toy has no name of its own. It is a wrapper around an item, and the
        // name is the item's — reading it the way a mount's is read yields
        // nothing, which is how a catalogue of a hundred and fifty nameless
        // toys got written.
        name: value
            .get("name")
            .or_else(|| value.get("item").and_then(|item| item.get("name")))
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_string(),
        source: value
            .get("source")
            .and_then(|source| source.get("type"))
            .and_then(|code| code.as_str())
            .map(Source::from_type)
            .unwrap_or_default(),
        // The web API has no such text, no flavour, no icon and no faction
        // restriction. Only the in-game journals do.
        description: None,
        flavour: None,
        icon: None,
        display: value
            .get("creature_display")
            .and_then(|display| display.get("id"))
            .and_then(|id| id.as_u64())
            .map(|id| id as u32),
        faction: None,
        link_id,
        tradeable: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decor_is_a_collection_like_any_other() {
        // Housing arrived with an index, a detail call and an account-wide
        // owned list — the same three shapes mounts have had for years — so it
        // is a fourth `Kind` rather than a feature of its own.
        assert!(index(Region::Us, Kind::Decor)
            .url
            .contains("/data/wow/decor/index"));
        assert!(detail(Region::Us, Kind::Decor, 42)
            .url
            .contains("/data/wow/decor/42"));
        assert!(collected(Region::Us, Kind::Decor)
            .url
            .contains("/profile/user/wow/collections/decor"));
    }

    #[test]
    fn the_decor_index_calls_its_list_decor_items() {
        // Recorded from a real response. Every other index here is named for
        // the plural of the thing; this one is not, and reading it as `decor`
        // parses 1,861 entries into an empty catalogue without erroring.
        let body = br#"{"_links":{},"decor_items":[
            {"key":{"href":"x"},"name":"Lorewalker's Bookcase","id":300},
            {"key":{"href":"y"},"name":"Zandalari War Torch","id":301}]}"#;
        let catalogue = parse_index(body, Kind::Decor).found().expect("decor");
        assert_eq!(catalogue.len(), 2);
        assert_eq!(catalogue[0].name, "Lorewalker's Bookcase");
        assert_eq!(catalogue[0].kind, Kind::Decor);
    }

    #[test]
    fn a_collectible_with_no_item_yet_is_not_linked_at_all() {
        // A chair sent somebody to a belt. The index gives decor and toys the
        // collection's own id as a stand-in, which is a real id in a different
        // space — so the link lands on an unrelated item and reads as correct.
        let mut chair = Collectible {
            kind: Kind::Decor,
            id: 5,
            name: "Sturdy Chair".into(),
            source: Source::Unknown,
            description: None,
            flavour: None,
            icon: None,
            display: None,
            faction: None,
            link_id: 5,
            tradeable: None,
        };
        assert!(chair.item_id_is_guessed());
        assert_eq!(chair.wowhead_url(), None);
        assert_eq!(chair.known_item_id(), None);

        // Once a detail call supplies the real item, both answer.
        chair.link_id = 246_810;
        assert!(!chair.item_id_is_guessed());
        assert_eq!(chair.known_item_id(), Some(246_810));
        assert_eq!(
            chair.wowhead_url().as_deref(),
            Some("https://www.wowhead.com/item=246810")
        );
    }

    #[test]
    fn a_piece_of_decor_links_by_the_item_it_is() {
        // Decor is backed by an item, and the item is what Wowhead indexes and
        // what an icon can be looked up by — the same as a toy.
        let body = br#"{"id":300,"name":"Lorewalker's Bookcase",
            "item":{"key":{"href":"z"},"name":"Lorewalker's Bookcase","id":246810},
            "source":{"type":"VENDOR","name":"Vendor"}}"#;
        let entry = parse_detail(body, Kind::Decor).found().expect("decor");
        assert_eq!(entry.id, 300);
        assert_eq!(entry.link_id, 246810);
        assert_eq!(entry.source, Source::Vendor);
        assert_eq!(
            entry.wowhead_url().as_deref(),
            Some("https://www.wowhead.com/item=246810")
        );
    }

    #[test]
    fn owned_decor_is_read_whether_the_id_is_nested_or_not() {
        // The other three nest the id under a key named for the thing. Decor
        // was new when this was written and may carry it at the top level
        // beside a quantity; reading only one shape and finding nothing would
        // report an empty collection rather than an unrecognised response.
        let nested = br#"{"decor":[{"decor":{"name":"Bookcase","id":300}}]}"#;
        assert_eq!(
            parse_collected(nested, Kind::Decor).found(),
            Some(HashSet::from([300]))
        );

        let flat = br#"{"decor":[{"id":300,"quantity":4}]}"#;
        assert_eq!(
            parse_collected(flat, Kind::Decor).found(),
            Some(HashSet::from([300]))
        );
    }

    #[test]
    fn each_collection_nests_its_ids_differently() {
        // Blizzard names the list after the collection and the id after the
        // thing, and the three do not agree. One shared shape would be a lie
        // about two of them.
        assert_eq!(
            parse_collected(
                br#"{"mounts":[{"mount":{"id":6}},{"mount":{"id":7}}]}"#,
                Kind::Mount
            )
            .found()
            .map(|ids| ids.len()),
            Some(2)
        );
        assert!(
            parse_collected(br#"{"pets":[{"species":{"id":42}}]}"#, Kind::Pet)
                .found()
                .is_some_and(|ids| ids.contains(&42))
        );
        assert!(
            parse_collected(br#"{"toys":[{"toy":{"id":9}}]}"#, Kind::Toy)
                .found()
                .is_some_and(|ids| ids.contains(&9))
        );
    }

    #[test]
    fn a_missing_list_is_stale_rather_than_an_empty_collection() {
        // Reporting "you have no mounts" because the response changed shape
        // would make a whole collection look lost.
        assert!(matches!(
            parse_collected(br#"{"character":{}}"#, Kind::Mount),
            Outcome::Stale(_)
        ));
    }

    #[test]
    fn an_index_yields_names_but_never_sources() {
        // The index carries no source at all, which is why the page fills its
        // "where from" column in gradually rather than all at once.
        let body = br#"{"mounts":[{"id":6,"name":"Brown Horse"},{"id":7,"name":"Grey Ram"}]}"#;
        let mounts = parse_index(body, Kind::Mount).found().expect("mounts");
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].name, "Brown Horse");
        assert_eq!(mounts[0].source, Source::Unknown);
    }

    #[test]
    fn a_mount_links_by_its_spell_rather_than_its_collection_id() {
        // Wowhead indexes a mount by the spell that summons it. Linking by the
        // collection id lands on an unrelated page.
        let body = br#"{"id":6,"name":"Brown Horse","source":{"type":"VENDOR"},
                        "source_spell":{"id":458}}"#;
        let mount = parse_detail(body, Kind::Mount).found().expect("a mount");
        assert_eq!(mount.link_id, 458);
        assert_eq!(
            mount.wowhead_url().as_deref(),
            Some("https://www.wowhead.com/spell=458")
        );
        assert_eq!(mount.source, Source::Vendor);
    }

    #[test]
    fn a_mount_with_no_spell_still_gets_a_link() {
        // A link that lands on the wrong page is recoverable; no link is not.
        let mount = parse_detail(br#"{"id":6,"name":"Old"}"#, Kind::Mount)
            .found()
            .expect("a mount");
        assert_eq!(mount.link_id, 6);
    }

    #[test]
    fn a_missing_source_is_unknown_rather_than_assumed() {
        // Common for anything old. Blizzard simply omits the field.
        let mount = parse_detail(br#"{"id":6,"name":"Old"}"#, Kind::Mount)
            .found()
            .expect("a mount");
        assert_eq!(mount.source, Source::Unknown);
    }

    #[test]
    fn a_promotional_mount_can_never_be_earned_again() {
        // Same reasoning as a Feat of Strength: it leaves the run rather than
        // sitting in it as a permanent zero.
        assert!(!Source::Promotion.is_repeatable());
        assert!(Source::Drop.is_repeatable());
        assert!(Source::Unknown.is_repeatable());
    }

    #[test]
    fn the_journals_sentence_classifies_by_its_first_clause() {
        // The in-game journals give "Drop: Attumen the Huntsman, Karazhan"
        // where the web API gives "DROP" or nothing at all.
        assert_eq!(
            Source::from_text("Drop: Attumen the Huntsman, Karazhan"),
            Source::Drop
        );
        assert_eq!(
            Source::from_text("Vendor: Katie Hunter, Elwynn Forest"),
            Source::Vendor
        );
        assert_eq!(Source::from_text("Trading Card Game"), Source::Promotion);
        assert_eq!(Source::from_text(""), Source::Unknown);
    }

    #[test]
    fn a_vendor_near_a_drop_zone_is_still_a_vendor() {
        // Matching the leading clause rather than searching the whole string.
        // A substring search would call this a drop and sort it wrongly.
        assert_eq!(
            Source::from_text("Vendor: sold near the Drop Zone"),
            Source::Vendor
        );
    }

    #[test]
    fn the_trading_post_is_a_vendor_rather_than_a_dead_end() {
        // Its stock rotates and comes back, so it is not the permanent
        // impossibility that a trading-card mount is.
        assert_eq!(Source::from_text("Trading Post"), Source::Vendor);
        assert!(Source::from_text("Trading Post").is_repeatable());
    }

    #[test]
    fn collections_are_asked_for_at_the_account_and_not_per_character() {
        // Mounts, pets and toys have been account-wide for years. Asking each
        // character would be twenty-three copies of one answer.
        let request = collected(Region::Us, Kind::Mount);
        assert!(request.url.contains("/profile/user/wow/collections/mounts"));
        assert!(!request.url.contains("/profile/wow/character/"));
    }

    #[test]
    fn the_catalogue_is_static_and_the_collection_is_profile() {
        assert!(index(Region::Us, Kind::Pet)
            .url
            .contains("namespace=static-us"));
        assert!(collected(Region::Us, Kind::Pet)
            .url
            .contains("namespace=profile-us"));
    }
}
