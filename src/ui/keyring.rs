//! The client secret, in the login keyring.
//!
//! Battle.net has no PKCE and no public-client mode, so the authorization code
//! flow needs a client secret. Armory has none of its own — the user registers
//! their own API client — but it has to hold theirs, and a secret in a JSON file
//! next to the settings is a secret in a backup, in a sync folder and in a
//! screen share.
//!
//! This talks to `org.freedesktop.secrets` directly over D-Bus rather than
//! through libsecret. The service is the same one either way; going through
//! `gio::DBusConnection` means no `libsecret-1-dev` to build against and no
//! extra module in the Flatpak, which is the same reasoning that put `soup3` in
//! this tree rather than a Rust HTTP client. The cost is the session handshake
//! below, which is thirty lines and has not changed since the specification was
//! written.
//!
//! The session uses the `plain` algorithm. That negotiates no encryption for
//! the D-Bus leg, which matters not at all here: the bus is a unix socket owned
//! by this user, and the alternative is a Diffie-Hellman exchange to hide a
//! value from a channel nobody else can read.

use std::collections::HashMap;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const SERVICE: &str = "org.freedesktop.secrets";
const PATH: &str = "/org/freedesktop/secrets";
const DEFAULT_COLLECTION: &str = "/org/freedesktop/secrets/aliases/default";

/// The attribute every item Armory stores carries, so its own items can be
/// found again without walking the whole keyring.
const APPLICATION: &str = "us.hagreli.Armory";

/// Why the keyring could not be used.
///
/// A keyring that is locked or absent is not a fault in Armory, and it reads
/// differently to one that refused: the first is answered by unlocking it or
/// installing a keyring, the second is a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyringError {
    /// No secret service on the bus at all — a session with no keyring daemon.
    Unavailable,
    /// The service answered, and said no.
    Refused(String),
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyringError::Unavailable => write!(
                f,
                "no keyring is running, so the client secret cannot be stored securely"
            ),
            KeyringError::Refused(detail) => write!(f, "the keyring refused: {detail}"),
        }
    }
}

type Result<T> = std::result::Result<T, KeyringError>;

/// A connection to the secret service, with a session open.
pub struct Keyring {
    connection: gio::DBusConnection,
    session: glib::variant::ObjectPath,
}

impl Keyring {
    /// Open the keyring.
    ///
    /// Synchronous, deliberately: this runs once at startup and once at the end
    /// of onboarding, and a keyring that is going to prompt will prompt whether
    /// or not the call was spelled asynchronously.
    pub fn open() -> Result<Keyring> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|_| KeyringError::Unavailable)?;

        let reply = connection
            .call_sync(
                Some(SERVICE),
                PATH,
                "org.freedesktop.Secret.Service",
                "OpenSession",
                Some(&("plain", glib::Variant::from("")).to_variant()),
                Some(glib::VariantTy::new("(vo)").expect("a type")),
                gio::DBusCallFlags::NONE,
                2000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| {
                // A session bus with nothing answering that name is the common
                // case on a headless box, and it is not an error to report as a
                // failure of the keyring.
                if error.matches(gio::DBusError::ServiceUnknown) {
                    KeyringError::Unavailable
                } else {
                    KeyringError::Refused(error.to_string())
                }
            })?;

        let session: glib::variant::ObjectPath = reply
            .child_value(1)
            .get()
            .ok_or_else(|| KeyringError::Refused("the session had no path".into()))?;

        Ok(Keyring {
            connection,
            session,
        })
    }

    /// Store a secret under `key`, replacing any previous one.
    pub fn store(&self, key: &str, secret: &str, label: &str) -> Result<()> {
        let attributes = attributes(key);

        let mut properties: HashMap<String, glib::Variant> = HashMap::new();
        properties.insert(
            "org.freedesktop.Secret.Item.Label".into(),
            label.to_variant(),
        );
        properties.insert(
            "org.freedesktop.Secret.Item.Attributes".into(),
            attributes.to_variant(),
        );

        // `(oayays)`: the session, no parameters, the value, and its type.
        let value = glib::Variant::tuple_from_iter([
            self.session.to_variant(),
            Vec::<u8>::new().to_variant(),
            secret.as_bytes().to_vec().to_variant(),
            "text/plain".to_variant(),
        ]);

        self.connection
            .call_sync(
                Some(SERVICE),
                DEFAULT_COLLECTION,
                "org.freedesktop.Secret.Collection",
                "CreateItem",
                Some(&glib::Variant::tuple_from_iter([
                    properties.to_variant(),
                    value,
                    // Replace, so re-running onboarding does not leave a
                    // second item behind that a later read might find first.
                    true.to_variant(),
                ])),
                Some(glib::VariantTy::new("(oo)").expect("a type")),
                gio::DBusCallFlags::NONE,
                5000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| KeyringError::Refused(error.to_string()))?;

        Ok(())
    }

    /// Look a secret up. `Ok(None)` means the keyring works and has nothing.
    pub fn lookup(&self, key: &str) -> Result<Option<String>> {
        let reply = self
            .connection
            .call_sync(
                Some(SERVICE),
                PATH,
                "org.freedesktop.Secret.Service",
                "SearchItems",
                Some(&glib::Variant::tuple_from_iter([
                    attributes(key).to_variant()
                ])),
                Some(glib::VariantTy::new("(aoao)").expect("a type")),
                gio::DBusCallFlags::NONE,
                5000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| KeyringError::Refused(error.to_string()))?;

        let unlocked: Vec<glib::variant::ObjectPath> =
            reply.child_value(0).get().unwrap_or_default();
        let Some(item) = unlocked.first() else {
            // A locked item is not read here. Unlocking prompts, and prompting
            // during a background sync is worse than reporting that the secret
            // is not available yet.
            return Ok(None);
        };

        let reply = self
            .connection
            .call_sync(
                Some(SERVICE),
                PATH,
                "org.freedesktop.Secret.Service",
                "GetSecrets",
                Some(&glib::Variant::tuple_from_iter([
                    vec![item.clone()].to_variant(),
                    self.session.to_variant(),
                ])),
                Some(glib::VariantTy::new("(a{o(oayays)})").expect("a type")),
                gio::DBusCallFlags::NONE,
                5000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| KeyringError::Refused(error.to_string()))?;

        // `a{o(oayays)}` keyed by item path. One item was asked for, so the
        // first entry is the answer; the value sits third inside the secret
        // struct, after the session and the (unused, with a plain session)
        // parameters.
        let secrets = reply.child_value(0);
        let Some(entry) = (secrets.n_children() > 0).then(|| secrets.child_value(0)) else {
            return Ok(None);
        };
        let value: Vec<u8> = entry
            .child_value(1)
            .child_value(2)
            .get()
            .unwrap_or_default();
        Ok(Some(String::from_utf8_lossy(&value).into_owned()))
    }

    /// Forget a secret. Used when someone signs out or re-registers a client.
    pub fn clear(&self, key: &str) -> Result<()> {
        let reply = self
            .connection
            .call_sync(
                Some(SERVICE),
                PATH,
                "org.freedesktop.Secret.Service",
                "SearchItems",
                Some(&glib::Variant::tuple_from_iter([
                    attributes(key).to_variant()
                ])),
                Some(glib::VariantTy::new("(aoao)").expect("a type")),
                gio::DBusCallFlags::NONE,
                5000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| KeyringError::Refused(error.to_string()))?;

        let mut paths: Vec<glib::variant::ObjectPath> =
            reply.child_value(0).get().unwrap_or_default();
        paths.extend(
            reply
                .child_value(1)
                .get::<Vec<glib::variant::ObjectPath>>()
                .unwrap_or_default(),
        );

        for path in paths {
            let _ = self.connection.call_sync(
                Some(SERVICE),
                path.as_str(),
                "org.freedesktop.Secret.Item",
                "Delete",
                None,
                Some(glib::VariantTy::new("(o)").expect("a type")),
                gio::DBusCallFlags::NONE,
                5000,
                gio::Cancellable::NONE,
            );
        }
        Ok(())
    }
}

fn attributes(key: &str) -> HashMap<String, String> {
    HashMap::from([
        ("application".to_string(), APPLICATION.to_string()),
        ("key".to_string(), key.to_string()),
    ])
}

/// The key the Battle.net client secret is stored under.
pub const CLIENT_SECRET: &str = "battlenet-client-secret";

/// The key the access token is stored under.
///
/// A bearer token is a credential, so it lives beside the secret rather than in
/// the settings file. It lasts a day, and keeping it is the difference between
/// relaunching the application and signing in through a browser again — which
/// matters more here than it would elsewhere, because Blizzard issues no
/// refresh token to do it quietly.
pub const ACCESS_TOKEN: &str = "battlenet-access-token";

/// The key the sync server's shared secret is stored under.
///
/// In the keyring rather than beside the server address in `settings.json`,
/// which is where Brain and Planner keep theirs. The difference is that Armory
/// already has a keyring for the Battle.net secret, and one credential in
/// plain text next to another one that is not would be a decision nobody made
/// on purpose. The cost is the same as the Battle.net secret's: the field
/// cannot be pre-filled, so it is empty on every launch and the row says
/// whether one is held.
pub const SYNC_TOKEN: &str = "sync-token";

/// The key a refresh token would be stored under.
///
/// Unused so far, and deliberately kept: Blizzard staff say refresh tokens are
/// not issued while the discovery document lists the grant and some responses
/// carry the field. When one turns out to work, this is where it goes.
#[allow(dead_code)]
pub const REFRESH_TOKEN: &str = "battlenet-refresh-token";
