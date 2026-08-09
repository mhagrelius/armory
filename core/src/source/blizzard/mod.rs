//! Blizzard's Battle.net API: the OAuth endpoints, the profile of an account,
//! and the game data catalogue.
//!
//! Split by what each half answers rather than by URL prefix. [`oauth`] gets a
//! token, [`profile`] asks about this account, [`gamedata`] asks about the game,
//! [`collections`] needs both halves at once — what exists, and what is owned —
//! and [`media`] says where the art for any of it lives. None of them opens a
//! socket.

pub mod auctions;
pub mod collections;
pub mod gamedata;
pub mod media;
pub mod oauth;
pub mod profile;

use std::fmt;

/// Which regional API to talk to.
///
/// A Battle.net account's characters all live in one region, and the region
/// decides the host, the namespace suffix and which auction house is being
/// priced. There is no cross-region anything — not trade, not commodities, not
/// character transfer — so this is chosen once and threaded through.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    #[default]
    Us,
    Eu,
    Kr,
    Tw,
}

impl Region {
    pub const ALL: [Region; 4] = [Region::Us, Region::Eu, Region::Kr, Region::Tw];

    /// The two-letter code, which is also the namespace suffix.
    pub fn code(self) -> &'static str {
        match self {
            Region::Us => "us",
            Region::Eu => "eu",
            Region::Kr => "kr",
            Region::Tw => "tw",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Region::Us => "Americas",
            Region::Eu => "Europe",
            Region::Kr => "Korea",
            Region::Tw => "Taiwan",
        }
    }

    pub fn from_code(code: &str) -> Option<Region> {
        Region::ALL
            .into_iter()
            .find(|region| region.code().eq_ignore_ascii_case(code))
    }

    /// The API host. China is deliberately absent: it runs on a different host
    /// under a different operator and has never been production-ready for
    /// third parties.
    pub fn api_host(self) -> String {
        format!("https://{}.api.blizzard.com", self.code())
    }

    /// The default locale for the region, used when the user has not chosen.
    pub fn default_locale(self) -> &'static str {
        match self {
            Region::Us => "en_US",
            Region::Eu => "en_GB",
            Region::Kr => "ko_KR",
            Region::Tw => "zh_TW",
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which slice of the API a request is addressed to.
///
/// Blizzard versions these — responses echo `static-11.1.5_60123-us` — but a
/// staff answer on the developer forums confirms the version segment is
/// optional and the bare form is stable. The bare form is what is sent, because
/// pinning a build would break the application on every patch day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    /// Catalogue data that changes on patch days.
    Static,
    /// Data that changes hourly or faster: realms, auctions, the token price.
    Dynamic,
    /// This account and its characters.
    Profile,
}

impl Namespace {
    pub fn qualified(self, region: Region) -> String {
        let prefix = match self {
            Namespace::Static => "static",
            Namespace::Dynamic => "dynamic",
            Namespace::Profile => "profile",
        };
        format!("{prefix}-{}", region.code())
    }
}

/// Build an API URL with its namespace and locale already attached.
///
/// Every Blizzard call needs both, and forgetting the namespace produces a 404
/// that reads like a missing character rather than a missing parameter.
pub fn url(region: Region, namespace: Namespace, path: &str, extra: &[(&str, &str)]) -> String {
    let mut url = format!(
        "{}{}?namespace={}&locale={}",
        region.api_host(),
        path,
        namespace.qualified(region),
        region.default_locale()
    );
    for (name, value) in extra {
        url.push('&');
        url.push_str(name);
        url.push('=');
        url.push_str(&encode(value));
    }
    url
}

/// Percent-encode one query parameter value.
///
/// Hand-rolled rather than pulled in, because the only values that reach it are
/// realm slugs, character names and item names — and `glib::Uri::escape_string`
/// would drag a GLib type into the half of the tree that deliberately has none.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Turn a realm's display name into the slug the API wants.
///
/// "Emerald Dream" is `emerald-dream`. Every profile call takes the slug, and
/// the responses carry both, so this is only needed when a name arrives from
/// somewhere that is not the API — the addon, or a person typing.
pub fn realm_slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut previous_dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if character == '\'' {
            // Apostrophes vanish rather than becoming separators: Zul'jin is
            // `zuljin`, not `zul-jin`.
            continue;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_always_carries_its_namespace_and_locale() {
        // A call without a namespace 404s, and the 404 reads like a character
        // who does not exist. That failure is expensive enough to assert on.
        let built = url(Region::Us, Namespace::Profile, "/profile/user/wow", &[]);
        assert!(built.starts_with("https://us.api.blizzard.com/profile/user/wow?"));
        assert!(built.contains("namespace=profile-us"));
        assert!(built.contains("locale=en_US"));
    }

    #[test]
    fn namespaces_are_unversioned() {
        // Responses echo `static-11.1.5_60123-us`. Sending that back would pin
        // the application to one patch and break it on the next.
        assert_eq!(Namespace::Static.qualified(Region::Eu), "static-eu");
        assert_eq!(Namespace::Dynamic.qualified(Region::Us), "dynamic-us");
    }

    #[test]
    fn realm_names_slug_the_way_the_api_spells_them() {
        assert_eq!(realm_slug("Emerald Dream"), "emerald-dream");
        assert_eq!(realm_slug("Mannoroth"), "mannoroth");
        // Apostrophes disappear rather than splitting the slug.
        assert_eq!(realm_slug("Zul'jin"), "zuljin");
        assert_eq!(realm_slug("Aerie Peak"), "aerie-peak");
    }

    #[test]
    fn parameters_are_encoded() {
        let built = url(
            Region::Us,
            Namespace::Static,
            "/data/wow/search/item",
            &[("name.en_US", "Reins of the Onyxian Drake")],
        );
        assert!(built.contains("name.en_US=Reins%20of%20the%20Onyxian%20Drake"));
    }

    #[test]
    fn a_region_round_trips_through_its_code() {
        for region in Region::ALL {
            assert_eq!(Region::from_code(region.code()), Some(region));
        }
        assert_eq!(Region::from_code("US"), Some(Region::Us));
        assert_eq!(Region::from_code("cn"), None);
    }
}
