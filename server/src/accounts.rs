//! Which accounts the server holds, and where each one lives.
//!
//! One SQLite file per account, in a directory of its own. Isolation by file
//! rather than by a column on every table, and that is the whole of the
//! argument: two Battle.net accounts sharing one store would merge into a
//! roster with both sets of characters, collections added together, and a run
//! measuring a cohort drawn from both — and nothing would report an error,
//! because every merge rule here is written to fold two views of *one* account
//! together. A column would leave that one `WHERE` clause away from happening
//! anyway. Separate files cannot.
//!
//! It also makes deleting an account a thing that can be done: remove the
//! directory. There is no cascade to get wrong and nothing left behind in a
//! table somebody forgot to filter.

use std::path::{Path, PathBuf};

/// The longest an account name may be.
///
/// It becomes a directory name, and it is chosen by a person rather than
/// generated, so this is generous rather than tight.
pub const MAX_NAME: usize = 64;

/// The account a request that names none belongs to.
///
/// Every client sent no name at all before this existed, so their data is not
/// nameless — it is this.
pub const DEFAULT: &str = "default";

/// Turn a name off the network into a directory, or refuse it.
///
/// **This is the only thing between a string a client sent and the server's
/// filesystem**, so it allows rather than forbids: a name is letters, digits,
/// dash, underscore and dot, it is not empty, it is not longer than
/// [`MAX_NAME`], and it is not made only of dots. Anything else is refused.
///
/// Checking for `..` and rejecting it is the version of this that misses
/// `a/../../b`, an absolute path, a leading dash, and a name that is
/// `.` — which would hand out the parent directory. Listing what is allowed
/// has no such edges.
pub fn directory(root: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.len() > MAX_NAME {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    // `.` and `..` pass the character test and are the two that matter.
    if name.chars().all(|c| c == '.') {
        return None;
    }
    Some(root.join("accounts").join(name))
}

/// Every account the server holds, in a stable order.
///
/// Read off the filesystem rather than kept in a list beside it: the
/// directories are the truth, and a list would be a second one to keep level.
pub fn known(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root.join("accounts"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        // A directory that could not have been created through `directory` is
        // not one this server made, and is not listed as an account.
        .filter(|name| directory(root, name).is_some())
        .collect();
    found.sort();
    found
}

/// Move a pre-account database under the default account.
///
/// The server kept one store at `data/armory.db` before accounts existed.
/// Leaving it there would strand whatever is in it — tens of thousands of rows
/// a client has already pushed and will not push again, because its outbox is
/// drained and its seed mark is set. Moving it is the difference between an
/// upgrade and a silent reset.
pub fn adopt_old_store(root: &Path) -> std::io::Result<bool> {
    let old = root.join("armory.db");
    if !old.exists() {
        return Ok(false);
    }
    let Some(home) = directory(root, DEFAULT) else {
        return Ok(false);
    };
    if home.join("armory.db").exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&home)?;
    for suffix in ["", "-wal", "-shm"] {
        let from = root.join(format!("armory.db{suffix}"));
        if from.exists() {
            // The write-ahead log carries writes the database file does not.
            // Moving one without the other restores the account as it was some
            // moments earlier, which is worse than either alternative.
            std::fs::rename(&from, home.join(format!("armory.db{suffix}")))?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/var/lib/armory")
    }

    #[test]
    fn an_ordinary_name_becomes_a_directory_under_accounts() {
        assert_eq!(
            directory(&root(), "PLAYER1"),
            Some(PathBuf::from("/var/lib/armory/accounts/PLAYER1"))
        );
        assert!(directory(&root(), "second-account_2.0").is_some());
    }

    /// The refusal list is the specification. Each of these would otherwise
    /// name somewhere outside the accounts directory, or name the directory
    /// itself.
    #[test]
    fn a_name_that_climbs_out_of_the_accounts_directory_is_refused() {
        for bad in [
            "",
            "..",
            ".",
            "...",
            "../escape",
            "a/../../escape",
            "/etc/passwd",
            "a/b",
            "a\\b",
            "a b",
            "a\0b",
            "a\nb",
            "über",
            "a:b",
            "*",
            "~",
        ] {
            assert!(directory(&root(), bad).is_none(), "{bad:?} was allowed");
        }
    }

    #[test]
    fn a_name_longer_than_the_ceiling_is_refused() {
        assert!(directory(&root(), &"a".repeat(MAX_NAME)).is_some());
        assert!(directory(&root(), &"a".repeat(MAX_NAME + 1)).is_none());
    }

    #[test]
    fn the_old_single_store_is_adopted_rather_than_stranded() {
        let home = tempfile::tempdir().expect("a directory");
        let root = home.path();
        std::fs::write(root.join("armory.db"), b"an account").expect("written");
        std::fs::write(root.join("armory.db-wal"), b"and its recent writes").expect("written");

        assert!(adopt_old_store(root).expect("adopted"));

        let moved = root.join("accounts").join(DEFAULT);
        assert_eq!(
            std::fs::read(moved.join("armory.db")).expect("read"),
            b"an account"
        );
        assert!(
            moved.join("armory.db-wal").exists(),
            "the WAL was left behind"
        );
        assert!(!root.join("armory.db").exists());

        // And it is not done twice.
        assert!(!adopt_old_store(root).expect("second look"));
    }

    #[test]
    fn accounts_are_listed_from_the_directories_themselves() {
        let home = tempfile::tempdir().expect("a directory");
        let root = home.path();
        for name in ["beta", "alpha"] {
            std::fs::create_dir_all(root.join("accounts").join(name)).expect("made");
        }
        // A file rather than a directory is not an account.
        std::fs::write(root.join("accounts").join("notes.txt"), b"").expect("written");

        assert_eq!(known(root), vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn no_accounts_directory_is_no_accounts_rather_than_an_error() {
        let home = tempfile::tempdir().expect("a directory");
        assert!(known(home.path()).is_empty());
    }
}
