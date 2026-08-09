//! Where Blizzard's art lives, and which of it can be had for nothing.
//!
//! Two quite different things are called "media" here, and the difference
//! decides how much of the interface can be illustrated.
//!
//! **Constructed URLs cost no request.** `render.worldofwarcraft.com` serves
//! creature renders addressed by creature display id, and icons addressed by
//! the game's own texture name. Both are public, unauthenticated and outside
//! the API quota, and the media endpoints below do nothing but hand back a URL
//! of exactly that shape. So anything whose display id or texture name is
//! already known is drawn without asking Blizzard first — which is every mount
//! and pet the collector addon has seen, and every class on the roster.
//!
//! **Everything else needs a call.** An item's icon is addressed by a texture
//! name nothing else tells us, and a character's portrait by a hash. Those go
//! through [`item`], [`achievement`] and [`character`], are one request each,
//! and answer with a URL rather than with bytes — so the response is small,
//! cacheable, and covered by the ordinary thirty-day expiry.
//!
//! Nothing here fetches an image. `ui::images` does that.

use super::super::{parse_json, Outcome, Request, SourceId};
use super::{url, Namespace, Region};

const DATA: SourceId = SourceId::BlizzardGameData;
const PROFILE: SourceId = SourceId::BlizzardProfile;

/// The render service. Regional, and the region has to match the namespace —
/// a US display id is not addressable on the EU host.
fn render_host(region: Region) -> String {
    format!("https://render.worldofwarcraft.com/{}", region.code())
}

/// A creature's portrait, addressed by its display id.
///
/// The one that matters most: a mount or pet the addon has seen carries a
/// creature display id, so the whole collection can be illustrated without a
/// single request. `zoom` is the only size the service publishes for creatures
/// — `big` and `small` answer 403 — and it is 600x600.
pub fn creature_render(region: Region, display_id: u32) -> String {
    format!(
        "{}/npcs/zoom/creature-display-{display_id}.jpg",
        render_host(region)
    )
}

/// An icon, addressed by the game's own texture name.
///
/// 56px is the only size served; the smaller ones answer 403. The name is the
/// lowercase texture name with no path and no extension, which is what every
/// media endpoint's `icon` asset resolves to anyway.
pub fn icon(region: Region, texture: &str) -> String {
    format!("{}/icons/56/{}.jpg", render_host(region), texture)
}

/// The class crest, for a roster row.
///
/// Class names come off the profile as display text, and the texture name is
/// that text lowercased with its spaces removed — `Death Knight` is
/// `classicon_deathknight`. Every one of the thirteen resolves.
pub fn class_icon(region: Region, class: &str) -> String {
    icon(region, &format!("classicon_{}", squash(class)))
}

/// The faction crest.
pub fn faction_icon(region: Region, faction: crate::character::Faction) -> Option<String> {
    use crate::character::Faction;
    match faction {
        Faction::Alliance => Some(icon(region, "ui_allianceicon")),
        Faction::Horde => Some(icon(region, "ui_hordeicon")),
        // Neutral is not a side and has no crest. An icon standing in for one
        // would be inventing an allegiance the character does not have.
        Faction::Neutral => None,
    }
}

/// Lowercase, and strip everything that is not a letter or a digit.
///
/// `Death Knight` becomes `deathknight`, `Demon Hunter` becomes `demonhunter`.
/// Texture names carry no spaces, no hyphens and no apostrophes.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

// -- the endpoints that have to be asked -------------------------------------

/// The path an item's media hangs off. Public because the answer to "which
/// item was this response about" is read back out of the URL — see [`media_id`].
pub const ITEM_MEDIA: &str = "/data/wow/media/item/";

/// The same, for an achievement's.
pub const ACHIEVEMENT_MEDIA: &str = "/data/wow/media/achievement/";

/// An item's icon. This is the only way to illustrate a toy: a toy is an item,
/// and an item's texture name appears nowhere else.
pub fn item(region: Region, item_id: u32) -> Request {
    Request::get(
        DATA,
        url(
            region,
            Namespace::Static,
            &format!("{ITEM_MEDIA}{item_id}"),
            &[],
        ),
    )
}

/// An achievement's icon.
pub fn achievement(region: Region, id: u32) -> Request {
    Request::get(
        DATA,
        url(
            region,
            Namespace::Static,
            &format!("{ACHIEVEMENT_MEDIA}{id}"),
            &[],
        ),
    )
}

/// What a media URL was built for: the inverse of [`item`] and [`achievement`].
///
/// A cached media response is held under its URL and says nothing about which
/// item or achievement it describes — the body is an `assets` array and no
/// more. So reading a session's worth of artwork back out of the response cache
/// means reading the id off the front of the key, which is why the two
/// directions live next to each other and share the path constants.
pub fn media_id(url: &str, path: &str) -> Option<u32> {
    url.split_once(path)?
        .1
        // The query string carries the namespace and the locale.
        .split(['?', '&', '/'])
        .next()?
        .parse()
        .ok()
}

/// A creature display's render, asked for rather than constructed.
///
/// [`creature_render`] answers the same question for nothing, and is what the
/// application uses. This exists for the case the constructed form stops
/// resolving: the endpoint is the contract, the URL shape is an observation.
pub fn creature_display(region: Region, display_id: u32) -> Request {
    Request::get(
        DATA,
        url(
            region,
            Namespace::Static,
            &format!("/data/wow/media/creature-display/{display_id}"),
            &[],
        ),
    )
}

/// A character's portraits: an avatar, an inset, and a full render.
///
/// Profile namespace, and the only art here that needs a signed-in account.
/// Without one the roster falls back to class crests, which is a smaller
/// picture of the same fact rather than a blank.
pub fn character(region: Region, key: &crate::character::CharacterKey) -> Request {
    Request::get(
        PROFILE,
        url(
            region,
            Namespace::Profile,
            &format!(
                "/profile/wow/character/{}/{}/character-media",
                key.realm_slug, key.name
            ),
            &[],
        ),
    )
}

/// Which picture of a character is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Portrait {
    /// The square head-and-shoulders, for a list row.
    Avatar,
    /// The waist-up crop, for a card.
    Inset,
    /// The full-body render, for a header.
    Main,
}

impl Portrait {
    /// The asset key Blizzard files this under.
    ///
    /// `main-raw` rather than `main`: the plain one is composited onto a scene
    /// background, which is a picture of a place with a character in it. The
    /// raw one has an alpha channel and sits on the application's own
    /// background, which is what a portrait in a header bar has to do.
    fn key(self) -> &'static str {
        match self {
            Portrait::Avatar => "avatar",
            Portrait::Inset => "inset",
            Portrait::Main => "main-raw",
        }
    }
}

/// Read a URL out of a media response.
///
/// Every media endpoint answers with the same `assets` array of key/value
/// pairs. Asking by key rather than taking the first: an achievement response
/// has one asset and a character response has four, and taking the first of
/// those four is whichever Blizzard happened to order first.
pub fn parse_asset(body: &[u8], key: &str) -> Outcome<String> {
    let value = match parse_json::<String>(DATA, body) {
        Ok(value) => value,
        Err(outcome) => return outcome,
    };

    let found = value
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset.get("key").and_then(|k| k.as_str()) == Some(key))
        })
        .and_then(|asset| asset.get("value"))
        .and_then(|value| value.as_str());

    match found {
        Some(url) => Outcome::Found(url.to_string()),
        // An entry with no art is an answer, not a fault. Plenty of items have
        // never had an icon assigned.
        None => Outcome::Empty,
    }
}

/// Read an icon URL out of a media response.
pub fn parse_icon(body: &[u8]) -> Outcome<String> {
    parse_asset(body, "icon")
}

/// Read one of a character's portraits out of a character-media response.
pub fn parse_portrait(body: &[u8], want: Portrait) -> Outcome<String> {
    parse_asset(body, want.key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{CharacterKey, Faction};

    #[test]
    fn a_creature_render_is_addressed_by_display_id_and_costs_nothing() {
        // The whole reason the collection can be illustrated: no request, no
        // quota, and the id is already in hand from the addon.
        assert_eq!(
            creature_render(Region::Us, 2404),
            "https://render.worldofwarcraft.com/us/npcs/zoom/creature-display-2404.jpg"
        );
        assert!(
            creature_render(Region::Eu, 1).starts_with("https://render.worldofwarcraft.com/eu/")
        );
    }

    #[test]
    fn a_class_crest_is_its_name_with_the_spaces_taken_out() {
        // `Death Knight` and `Demon Hunter` are the two that would otherwise
        // 404, and they are the two a roster is most likely to contain.
        assert!(class_icon(Region::Us, "Death Knight").ends_with("classicon_deathknight.jpg"));
        assert!(class_icon(Region::Us, "Demon Hunter").ends_with("classicon_demonhunter.jpg"));
        assert!(class_icon(Region::Us, "Evoker").ends_with("classicon_evoker.jpg"));
    }

    #[test]
    fn neutral_has_no_crest_because_it_is_not_a_side() {
        assert!(faction_icon(Region::Us, Faction::Horde).is_some());
        assert!(faction_icon(Region::Us, Faction::Alliance).is_some());
        assert_eq!(faction_icon(Region::Us, Faction::Neutral), None);
    }

    #[test]
    fn an_asset_is_found_by_key_rather_than_by_position() {
        // A character answers with four assets and Blizzard's order is not
        // contractual. Taking the first would hand a list row a full-body
        // render whenever the order changed.
        let body = br#"{"assets":[
            {"key":"inset","value":"https://render/inset.jpg"},
            {"key":"avatar","value":"https://render/avatar.jpg"},
            {"key":"main-raw","value":"https://render/main-raw.png"}]}"#;
        assert_eq!(
            parse_portrait(body, Portrait::Avatar).found().as_deref(),
            Some("https://render/avatar.jpg")
        );
        assert_eq!(
            parse_portrait(body, Portrait::Main).found().as_deref(),
            Some("https://render/main-raw.png")
        );
    }

    #[test]
    fn a_portrait_asks_for_the_raw_render_not_the_composited_one() {
        // The composited one is a picture of a place with a character in it,
        // and it does not sit on an application background.
        assert_eq!(Portrait::Main.key(), "main-raw");
    }

    #[test]
    fn an_entry_with_no_art_is_empty_rather_than_broken() {
        // Plenty of items have never had an icon assigned, and that is an
        // answer about the item.
        assert_eq!(parse_icon(br#"{"assets":[]}"#), Outcome::Empty);
        assert_eq!(
            parse_icon(br#"{"assets":[{"key":"zoom","value":"x"}]}"#),
            Outcome::Empty
        );
    }

    #[test]
    fn a_media_url_gives_back_the_id_it_was_built_for() {
        // The response cache is keyed by URL and the body names no item, so
        // this is the only thing that says which toy a stored icon belongs to.
        // A drift between the two directions would restore artwork onto the
        // wrong entries, or onto none.
        let request = item(Region::Us, 86571);
        assert_eq!(media_id(&request.url, ITEM_MEDIA), Some(86571));

        let request = achievement(Region::Eu, 4956);
        assert_eq!(media_id(&request.url, ACHIEVEMENT_MEDIA), Some(4956));

        // An item URL is not an achievement URL, and answering one for the
        // other would put a mount's icon on a goal.
        assert_eq!(
            media_id(&item(Region::Us, 86571).url, ACHIEVEMENT_MEDIA),
            None
        );
        assert_eq!(
            media_id("https://us.api.blizzard.com/data/wow/toy/1", ITEM_MEDIA),
            None
        );
    }

    #[test]
    fn item_media_is_static_and_character_media_is_profile() {
        // A wrong namespace 404s, and the 404 reads like a missing item.
        assert!(item(Region::Us, 32566).url.contains("namespace=static-us"));
        assert!(achievement(Region::Us, 4956)
            .url
            .contains("namespace=static-us"));

        let key = CharacterKey::new("mannoroth", "Aeltor");
        let request = character(Region::Us, &key);
        assert!(request.url.contains("namespace=profile-us"));
        assert!(request
            .url
            .contains("/profile/wow/character/mannoroth/aeltor/character-media"));
    }
}
