//! The seam between "what to ask" and "asking it".
//!
//! Every source here is a pair of pure functions: one builds a [`Request`], the
//! other turns a response body into an [`Outcome`]. Nothing here opens a socket
//! — `ui::http` does that, and it is the only file in the tree that does. That
//! is what makes every source, every malformed response and every failure mode
//! testable from a recorded fixture with no network and no display.
//!
//! There is deliberately no `Source` trait. Blizzard answers "who is on this
//! account and what have they done"; the catalogue sources answer "and where
//! does this thing come from"; they share the request/parse shape and nothing
//! else. That shape is expressed by the two types below rather than by a
//! supertype nobody would implement twice.

pub mod blizzard;
pub mod journal;

use std::fmt;

/// Who is being asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceId {
    /// The OAuth endpoints: token exchange and refresh.
    BattleNetOAuth,
    /// `/profile/...` — this account and its characters.
    BlizzardProfile,
    /// `/data/...` — the catalogue, and the auction house.
    BlizzardGameData,
    /// The local `llama-server`, which writes the journal entries.
    ///
    /// The only source here that is not a game database, the only one on this
    /// machine, and the only one whose status codes mean something other than
    /// what Blizzard's mean — see the note in `ui::http`.
    Journal,
}

impl SourceId {
    pub const ALL: [SourceId; 4] = [
        SourceId::BattleNetOAuth,
        SourceId::BlizzardProfile,
        SourceId::BlizzardGameData,
        SourceId::Journal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SourceId::BattleNetOAuth => "Battle.net sign-in",
            SourceId::BlizzardProfile => "Character profiles",
            SourceId::BlizzardGameData => "Game data",
            SourceId::Journal => "Journal entries",
        }
    }

    /// Whether this is one of Blizzard's endpoints.
    ///
    /// `ui::http` reads a handful of status codes as things that are true of
    /// Blizzard and false everywhere else — a 403 is a privacy setting there
    /// and an ordinary refusal anywhere else — so it has to be able to ask.
    pub fn is_blizzard(self) -> bool {
        !matches!(self, SourceId::Journal)
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One HTTP call, described but not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub source: SourceId,
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Form-encoded body, for the token endpoint. Nothing else here posts.
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

impl Request {
    pub fn get(source: SourceId, url: impl Into<String>) -> Self {
        Request {
            source,
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn post(source: SourceId, url: impl Into<String>, body: impl Into<String>) -> Self {
        Request {
            source,
            method: Method::Post,
            url: url.into(),
            headers: vec![(
                "Content-Type".into(),
                "application/x-www-form-urlencoded".into(),
            )],
            body: Some(body.into()),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    /// Attach the bearer token.
    ///
    /// Blizzard stopped accepting `?access_token=` on 2024-09-30; the header is
    /// now the only way to authenticate a call.
    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }

    /// Ask the server to answer `304` if nothing has changed since `stamp`.
    ///
    /// This is the whole reason syncing twenty-three characters is affordable.
    /// Profile data changes only when a character logs out, so most of a sync is
    /// a conditional request that costs one round trip and no body.
    pub fn if_modified_since(self, stamp: &str) -> Self {
        self.header("If-Modified-Since", stamp)
    }

    /// The cache key for this request.
    ///
    /// The URL is the key: two requests that differ only in a header are, for
    /// every source here, the same question asked with a fresher token.
    pub fn cache_key(&self) -> &str {
        &self.url
    }
}

/// Why a source could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Timeout,
    RateLimited,
    Http(u16),
    /// The body arrived and did not parse.
    Malformed(String),
    /// Never asked: no client registered, no token, or switched off.
    NotConfigured(String),
    /// The token expired or the user withdrew consent. Distinct from
    /// [`Reason::NotConfigured`] because the fix is different — sign in again,
    /// rather than set the application up.
    Unauthorised(String),
    /// The user has third-party data sharing turned off on their Battle.net
    /// account. Nothing the application does will fix this, and saying "HTTP
    /// 403" instead would send someone hunting for a bug that is a checkbox.
    SharingDisabled,
    /// The service answered, and said no, and said why in its own words.
    ///
    /// Distinct from every other variant here because it is neither a fault nor
    /// something to retry: an exhausted credit balance and a request the safety
    /// classifiers declined are both real answers, and both are things only the
    /// person running the application can act on. Carrying the service's own
    /// sentence is the point — "HTTP 400" sends somebody looking for a bug in
    /// Armory and "your credit balance is too low" does not.
    Declined(String),
    Network(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Timeout => write!(f, "timed out"),
            Reason::RateLimited => write!(f, "rate limited"),
            Reason::Http(code) => write!(f, "HTTP {code}"),
            Reason::Malformed(what) => write!(f, "unreadable response: {what}"),
            Reason::NotConfigured(what) => write!(f, "not set up: {what}"),
            Reason::Unauthorised(what) => write!(f, "not signed in: {what}"),
            Reason::SharingDisabled => write!(
                f,
                "this account has third-party data sharing turned off in its \
                 Battle.net privacy settings"
            ),
            Reason::Declined(what) => write!(f, "{what}"),
            Reason::Network(what) => write!(f, "{what}"),
        }
    }
}

/// What a source came back with.
///
/// The distinction between [`Outcome::Empty`] and [`Outcome::Stale`] is the
/// whole reason this is not an `Option` or a `Result`. A character who has
/// collected no mounts and a mounts parser that has stopped understanding the
/// response both produce an empty list; if they are the same value, a broken
/// parser silently empties a collection and makes a run look finished. The
/// application would then be confidently, quietly wrong about the one thing it
/// exists to say.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome<T> {
    /// The source answered and had something.
    Found(T),
    /// The source answered and genuinely has nothing.
    Empty,
    /// Nothing has changed since the stamp we sent. Not an error and not an
    /// answer — the answer we already hold is still current.
    Unchanged,
    /// The source could not be reached or refused.
    Unusable(Reason),
    /// The source answered, but not in a shape we recognise.
    Stale(Reason),
}

impl<T> Outcome<T> {
    pub fn found(self) -> Option<T> {
        match self {
            Outcome::Found(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_found(&self) -> bool {
        matches!(self, Outcome::Found(_))
    }

    /// Why this source contributed nothing, if it should be reported as a gap.
    ///
    /// [`Outcome::Empty`] and [`Outcome::Unchanged`] return `None`: a source
    /// that answered and had nothing is not a gap, and neither is one that
    /// confirmed our copy is current.
    ///
    /// The [`Reason`] comes back whole rather than as a formatted string,
    /// because the caller has to tell "you never set this up" from "your
    /// sign-in lapsed" from "this actually broke". All three read differently
    /// and only the last is a fault.
    pub fn gap(&self) -> Option<Reason> {
        match self {
            Outcome::Found(_) | Outcome::Empty | Outcome::Unchanged => None,
            Outcome::Unusable(reason) => Some(reason.clone()),
            Outcome::Stale(reason) => Some(Reason::Malformed(format!("check failed — {reason}"))),
        }
    }

    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Outcome<U> {
        match self {
            Outcome::Found(value) => Outcome::Found(f(value)),
            Outcome::Empty => Outcome::Empty,
            Outcome::Unchanged => Outcome::Unchanged,
            Outcome::Unusable(reason) => Outcome::Unusable(reason),
            Outcome::Stale(reason) => Outcome::Stale(reason),
        }
    }

    /// `Found` when the collection has anything in it, `Empty` when it does not.
    pub fn of_collection(items: Vec<T>) -> Outcome<Vec<T>> {
        if items.is_empty() {
            Outcome::Empty
        } else {
            Outcome::Found(items)
        }
    }
}

/// Parse a body as JSON, mapping a parse failure to [`Outcome::Stale`].
///
/// A source that answers `200` with something that is not JSON has changed
/// shape under us; that is the definition of stale, and it is never `Empty`.
pub fn parse_json<T>(source: SourceId, body: &[u8]) -> Result<serde_json::Value, Outcome<T>> {
    serde_json::from_slice(body).map_err(|error| {
        Outcome::Stale(Reason::Malformed(format!(
            "{source} sent non-JSON: {error}"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_unchanged_are_answers_and_stale_is_a_gap() {
        // The distinction the whole enum exists for.
        assert_eq!(Outcome::<u8>::Empty.gap(), None);
        assert_eq!(Outcome::<u8>::Unchanged.gap(), None);
        assert_eq!(Outcome::Found(1u8).gap(), None);
        assert!(Outcome::<u8>::Unusable(Reason::Timeout)
            .gap()
            .is_some_and(|gap| gap.to_string().contains("timed out")));
        assert!(
            Outcome::<u8>::Stale(Reason::Malformed("no criteria".into()))
                .gap()
                .is_some_and(|gap| gap.to_string().contains("check failed"))
        );
    }

    #[test]
    fn sharing_disabled_reads_as_a_setting_not_a_status_code() {
        // A 403 here is a privacy checkbox on the user's account, and telling
        // someone "HTTP 403" sends them hunting for a bug in the application.
        let text = Reason::SharingDisabled.to_string();
        assert!(text.contains("privacy"), "{text}");
        assert!(!text.contains("403"), "{text}");
    }

    #[test]
    fn a_non_json_body_is_stale_rather_than_empty() {
        // Blizzard's edge answers HTML during maintenance, with a 200. Treating
        // that as "no characters" would empty the roster.
        let outcome: Outcome<u8> = match parse_json(SourceId::BlizzardProfile, b"<html>nope</html>")
        {
            Err(outcome) => outcome,
            Ok(_) => panic!("html parsed as json"),
        };
        assert!(matches!(outcome, Outcome::Stale(_)));
    }

    #[test]
    fn a_collection_of_nothing_is_empty() {
        assert_eq!(Outcome::of_collection(Vec::<u8>::new()), Outcome::Empty);
        assert_eq!(Outcome::of_collection(vec![1u8]), Outcome::Found(vec![1]));
    }

    #[test]
    fn a_request_keys_its_cache_entry_on_the_url_not_the_token() {
        // Tokens last a day and the same question asked with a fresher one is
        // still the same question.
        let a = Request::get(SourceId::BlizzardProfile, "https://example/x").bearer("one");
        let b = Request::get(SourceId::BlizzardProfile, "https://example/x").bearer("two");
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn a_post_carries_a_form_body_and_says_so() {
        let request = Request::post(SourceId::BattleNetOAuth, "https://example/token", "a=b");
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.body.as_deref(), Some("a=b"));
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "Content-Type" && value.contains("form-urlencoded")));
    }
}
