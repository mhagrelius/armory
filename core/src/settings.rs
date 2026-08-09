//! What the application remembers between runs, other than data.
//!
//! Not the client secret. That goes to the keyring through `ui::keyring`, and
//! keeping it out of this struct is what stops it being written to a file by
//! accident when something new gets added here later.
//!
//! The client *id* does live here. It is not a secret — it travels in the
//! authorize URL in plain sight — and pairing it with the secret in the same
//! store would mean opening the keyring to answer "is this set up at all".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::source::blizzard::Region;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub region: Region,
    /// The API client the user registered. Empty until they have.
    pub client_id: String,
    /// Where WoW is installed, once found or chosen.
    pub wow_path: Option<PathBuf>,
    /// Which `WTF/Account/<NAME>` folder to read, when the install has more
    /// than one.
    ///
    /// One is the normal case. A second appears when a second Battle.net
    /// login has used the same install, and the two hold different
    /// characters, different collections and different achievements — so
    /// reading the wrong one is not a near miss, it is somebody else's
    /// account. Recorded on first run so it is visible and can be changed,
    /// rather than guessed afresh every launch.
    pub wow_account: Option<String>,
    /// Whether the person has chosen to go without a Battle.net client.
    ///
    /// Separate from `client_id` being empty, because those mean different
    /// things: empty is "not set up yet" and this is "set up, deliberately,
    /// without one". Onboarding reappears for the first and not the second.
    pub addon_only: bool,
    /// Whether a new evening gets written up without being asked.
    ///
    /// **On.** It was off while entries went to a hosted API, because a default
    /// that spends somebody's money as a side effect of launching an
    /// application is not a default worth having. Entries are written by a
    /// `llama-server` on this machine now: nothing is billed and nothing
    /// leaves, so the only cost is some seconds of a GPU that is sitting there
    /// anyway — and a journal you have to remember to write is a journal that
    /// does not get written.
    pub journal_automatic: bool,
    /// Where that server is.
    ///
    /// Not a secret and not a credential, so it lives here rather than in the
    /// keyring — it is an address, and it travels in plain sight.
    pub journal_server: String,
    /// Where this account is shared to, if it is.
    ///
    /// `http://nas.example:8084`. Empty means Armory keeps to itself,
    /// which is the default and the whole of what a single machine needs.
    ///
    /// The token is not here — it is in the keyring beside the Battle.net
    /// secret. **Both or neither**: an address with no token cannot
    /// authenticate and a token with no address has nowhere to go, and doing
    /// half of it silently is how you get an installation that looks
    /// configured and never syncs.
    pub sync_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            region: Region::Us,
            client_id: String::new(),
            wow_path: None,
            wow_account: None,
            addon_only: false,
            journal_automatic: true,
            journal_server: crate::source::journal::DEFAULT_SERVER.to_string(),
            sync_url: String::new(),
        }
    }
}

impl Settings {
    /// Whether onboarding has got far enough to sign in.
    pub fn is_registered(&self) -> bool {
        !self.client_id.trim().is_empty()
    }

    /// Read settings, falling back to defaults for anything missing.
    ///
    /// A settings file that will not parse is not worth stopping for: the
    /// application is usable with defaults, and refusing to start because one
    /// key went bad is a worse outcome than losing a preference.
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(path, json)
    }
}

/// Everywhere a WoW retail install might be on a machine where the game runs
/// under Wine or Proton.
///
/// The path *inside* a prefix is always the same; what differs is which
/// launcher made the prefix. These are the layouts the standalone Battle.net
/// prefix, Lutris, Steam's compatdata and Bottles produce.
pub fn wow_search_paths(home: &Path) -> Vec<PathBuf> {
    const INSIDE_PREFIX: &str = "drive_c/Program Files (x86)/World of Warcraft/_retail_";

    [
        "Games/battlenet/compatdata/pfx",
        "Games/battle-net/compatdata/pfx",
        "Games/battlenet/pfx",
        "Games/wow/pfx",
        ".wine",
        ".local/share/lutris/prefixes/battlenet",
        ".var/app/com.usebottles.bottles/data/bottles/bottles/battlenet",
    ]
    .iter()
    .map(|prefix| home.join(prefix).join(INSIDE_PREFIX))
    .collect()
}

/// The first search path that actually holds a `WTF` folder.
pub fn find_wow(home: &Path) -> Option<PathBuf> {
    wow_search_paths(home)
        .into_iter()
        .find(|path| path.join("WTF").is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_is_unregistered_and_defaults_to_the_americas() {
        let settings = Settings::default();
        assert!(!settings.is_registered());
        assert_eq!(settings.region, Region::Us);
    }

    #[test]
    fn settings_round_trip_through_a_directory_that_did_not_exist() {
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("nested").join("settings.json");

        let settings = Settings {
            region: Region::Eu,
            client_id: "abc123".into(),
            wow_path: Some(PathBuf::from("/games/wow")),
            wow_account: Some("PLAYER1".into()),
            addon_only: false,
            journal_automatic: false,
            journal_server: "http://127.0.0.1:9090".into(),
            sync_url: "http://nas.example:8084".into(),
        };
        settings.save(&path).expect("saved");
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn an_unreadable_settings_file_falls_back_rather_than_stopping_the_application() {
        // Losing a preference is a much better outcome than refusing to start.
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("settings.json");
        std::fs::write(&path, b"{ not json").expect("written");

        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn a_missing_settings_file_is_simply_defaults() {
        assert_eq!(
            Settings::load(Path::new("/nonexistent/settings.json")),
            Settings::default()
        );
    }

    #[test]
    fn no_secret_has_a_field_to_be_written_into() {
        // The keyring holds the Battle.net client secret. This is the guard
        // against somebody later adding a convenient `client_secret` or
        // `api_key` here and quietly putting a credential in a plain file —
        // `journal_server` is an address and belongs here; a key would not.
        let json = serde_json::to_string(&Settings::default()).expect("serialised");
        for banned in ["secret", "api_key", "apikey", "token"] {
            assert!(!json.contains(banned), "{banned} in {json}");
        }
    }

    #[test]
    fn a_wow_install_is_found_by_its_wtf_folder() {
        // The marker is WTF rather than the directory itself: an empty
        // `_retail_` left behind by an uninstall would otherwise match, and the
        // addon channel would then silently have nowhere to write.
        let home = tempfile::tempdir().expect("a directory");
        assert_eq!(find_wow(home.path()), None);

        let install = home
            .path()
            .join("Games/battlenet/compatdata/pfx")
            .join("drive_c/Program Files (x86)/World of Warcraft/_retail_");
        std::fs::create_dir_all(install.join("WTF")).expect("created");

        assert_eq!(find_wow(home.path()), Some(install));
    }
}
