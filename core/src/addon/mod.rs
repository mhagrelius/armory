//! The companion addon's half of the conversation.
//!
//! An addon cannot open a socket and cannot read a file. SavedVariables is the
//! only channel out of the game and writing Lua into the addon's folder before
//! the game loads is the only channel in. Both are files, and both live here.
//!
//! What this buys is the attribution the web API does not have.
//! `GetAchievementInfo` returns `earnedBy` — which character originally earned
//! each account-wide achievement — and that is what decides whether a goal is
//! poisoned. Without it every already-earned achievement has to be assumed
//! poisoned, which drags a decade of completions through recomputation. The
//! addon does not make the run possible so much as it makes the run cheap.

pub mod chronicle;
pub mod collector;
pub mod lua;

use std::path::{Path, PathBuf};

/// Where the account-wide SavedVariables file for an addon lives.
///
/// `_retail_/WTF/Account/<ACCOUNT>/SavedVariables/<Addon>.lua`.
pub fn account_saved_variables(wow_path: &Path, account: &str, addon: &str) -> PathBuf {
    wow_path
        .join("WTF")
        .join("Account")
        .join(account)
        .join("SavedVariables")
        .join(format!("{addon}.lua"))
}

/// Where a character's own SavedVariables file lives.
pub fn character_saved_variables(
    wow_path: &Path,
    account: &str,
    realm: &str,
    character: &str,
    addon: &str,
) -> PathBuf {
    wow_path
        .join("WTF")
        .join("Account")
        .join(account)
        .join(realm)
        .join(character)
        .join("SavedVariables")
        .join(format!("{addon}.lua"))
}

/// The account folders under a WoW install.
///
/// There is usually one, and it is usually the Battle.net account name in
/// capitals. `SavedVariables` sits alongside them at the same level, so it is
/// skipped rather than mistaken for an account.
pub fn accounts(wow_path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(wow_path.join("WTF").join("Account")) else {
        return Vec::new();
    };
    let mut accounts: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "SavedVariables")
        .collect();
    accounts.sort();
    accounts
}

/// Where the addon itself is installed.
pub fn addon_directory(wow_path: &Path, addon: &str) -> PathBuf {
    wow_path.join("Interface").join("AddOns").join(addon)
}

/// Whether the collector addon is installed.
pub fn is_installed(wow_path: &Path, addon: &str) -> bool {
    addon_directory(wow_path, addon)
        .join(format!("{addon}.toc"))
        .is_file()
}

/// Every per-character SavedVariables file an addon has written.
///
/// The layout is `WTF/Account/<ACCOUNT>/<Realm>/<Character>/SavedVariables/`,
/// so this is a two-level walk. It is how the roster gets built with no web API
/// at all — one file per character you have logged in on since installing the
/// addon.
///
/// Characters you have never logged in on are simply absent, which is the trade
/// the API would otherwise solve.
pub fn character_files(wow_path: &Path, account: &str, addon: &str) -> Vec<PathBuf> {
    let base = wow_path.join("WTF").join("Account").join(account);
    let Ok(realms) = std::fs::read_dir(&base) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    for realm in realms.filter_map(|entry| entry.ok()) {
        // `SavedVariables` sits alongside the realm folders at this level and is
        // not one.
        if !realm.path().is_dir() || realm.file_name() == "SavedVariables" {
            continue;
        }
        let Ok(characters) = std::fs::read_dir(realm.path()) else {
            continue;
        };
        for character in characters.filter_map(|entry| entry.ok()) {
            if !character.path().is_dir() {
                continue;
            }
            let file = character
                .path()
                .join("SavedVariables")
                .join(format!("{addon}.lua"));
            if file.is_file() {
                files.push(file);
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_variables_land_where_the_client_writes_them() {
        let wow = Path::new("/games/wow/_retail_");
        assert_eq!(
            account_saved_variables(wow, "PLAYER1", "Armory_Collector"),
            Path::new("/games/wow/_retail_/WTF/Account/PLAYER1/SavedVariables/Armory_Collector.lua")
        );
        assert_eq!(
            character_saved_variables(wow, "PLAYER1", "Emerald Dream", "Somechar", "Armory_Collector"),
            Path::new(
                "/games/wow/_retail_/WTF/Account/PLAYER1/Emerald Dream/Somechar/SavedVariables/Armory_Collector.lua"
            )
        );
    }

    #[test]
    fn the_shared_saved_variables_folder_is_not_an_account() {
        // It sits at the same level as the account folders and would otherwise
        // be read as one, producing a phantom account with no characters.
        let wow = tempfile::tempdir().expect("a directory");
        let base = wow.path().join("WTF").join("Account");
        std::fs::create_dir_all(base.join("PLAYER1")).expect("created");
        std::fs::create_dir_all(base.join("SavedVariables")).expect("created");

        assert_eq!(accounts(wow.path()), ["PLAYER1"]);
    }

    #[test]
    fn an_install_with_no_wtf_folder_has_no_accounts_rather_than_failing() {
        assert!(accounts(Path::new("/nonexistent")).is_empty());
    }

    #[test]
    fn every_character_that_has_logged_in_leaves_a_file() {
        // This is the roster, with no web API involved. Characters never logged
        // in on are absent, which is the trade the API would otherwise solve.
        let wow = tempfile::tempdir().expect("a directory");
        let base = wow.path().join("WTF").join("Account").join("PLAYER1");

        for (realm, character) in [
            ("Emerald Dream", "Somechar"),
            ("Emerald Dream", "Velkurai"),
            ("Mannoroth", "Aeltor"),
        ] {
            let directory = base.join(realm).join(character).join("SavedVariables");
            std::fs::create_dir_all(&directory).expect("created");
            std::fs::write(directory.join("Armory_Collector.lua"), b"x").expect("written");
        }

        // The account-wide folder sits at the same level as the realms, and a
        // walk that treats it as one produces a phantom realm.
        std::fs::create_dir_all(base.join("SavedVariables")).expect("created");
        // A character who has the folder but not our file — every other addon
        // makes these.
        std::fs::create_dir_all(base.join("Thrall").join("Ulahae").join("SavedVariables"))
            .expect("created");

        let files = character_files(wow.path(), "PLAYER1", "Armory_Collector");
        assert_eq!(files.len(), 3);
        assert!(files
            .iter()
            .all(|path| path.ends_with("Armory_Collector.lua")));
    }

    #[test]
    fn an_account_with_no_characters_yet_yields_nothing_rather_than_failing() {
        assert!(
            character_files(Path::new("/nonexistent"), "PLAYER1", "Armory_Collector").is_empty()
        );
    }

    #[test]
    fn the_addon_counts_as_installed_only_once_its_toc_is_there() {
        // A directory an addon manager half-created is not an installed addon,
        // and treating it as one means silently waiting for a file that will
        // never be written.
        let wow = tempfile::tempdir().expect("a directory");
        let directory = addon_directory(wow.path(), "Armory_Collector");
        std::fs::create_dir_all(&directory).expect("created");
        assert!(!is_installed(wow.path(), "Armory_Collector"));

        std::fs::write(
            directory.join("Armory_Collector.toc"),
            b"## Interface: 110200",
        )
        .expect("written");
        assert!(is_installed(wow.path(), "Armory_Collector"));
    }
}
