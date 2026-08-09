//! Watching the collector addon's file.
//!
//! WoW writes SavedVariables at logout or `/reload` and at no other time, so
//! there is nothing to poll and nothing to stream. A `gio::FileMonitor` on the
//! one file is the whole mechanism.
//!
//! The debounce is not decoration. The client writes the file in pieces and a
//! monitor reports each write, so reading on the first event reliably gets a
//! truncated table — which the parser correctly refuses, producing an error
//! about a perfectly healthy addon. Waiting for the writes to stop is what
//! makes the read see a whole file.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::model::addon::chronicle;
use crate::model::addon::collector::{self, Collected, CollectedCharacter, ReadError};
use crate::model::chronicle::Session;

/// How long to wait after the last write before reading.
///
/// WoW's logout write is a burst of appends. Two seconds is long past the end
/// of one and short enough that the roster updates while somebody is still
/// looking at the loading screen.
const SETTLE: Duration = Duration::from_secs(2);

/// A monitor on one addon file.
pub struct Watch {
    _monitor: gio::FileMonitor,
    /// The pending debounced read, cancelled and replaced on each new event.
    pending: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Watch {
    /// Watch `path`, calling `deliver` whenever it settles.
    ///
    /// Delivers once immediately if the file is already there, so an
    /// application that starts after the game has quit does not sit waiting for
    /// a write that already happened.
    pub fn new<F>(wow: &Path, account: &str, deliver: F) -> Result<Watch, glib::Error>
    where
        F: Fn(Result<Dump, ReadError>) + 'static,
    {
        let path = path(wow, account);
        let path = path.as_path();
        let file = gio::File::for_path(path);
        let monitor = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)?;

        let deliver = Rc::new(deliver);
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

        {
            let wow = wow.to_path_buf();
            let account = account.to_string();
            let deliver = Rc::clone(&deliver);
            let pending = Rc::clone(&pending);
            monitor.connect_changed(move |_, _, _, event| {
                // Every kind of change ends in the file being different, and
                // the debounce below collapses the burst either way, so there
                // is nothing to gain from filtering on the event type.
                let _ = event;

                if let Some(source) = pending.borrow_mut().take() {
                    source.remove();
                }

                let wow = wow.clone();
                let account = account.clone();
                let deliver = Rc::clone(&deliver);
                let slot = Rc::clone(&pending);
                let source = glib::timeout_add_local_once(SETTLE, move || {
                    slot.borrow_mut().take();
                    // Every per-character file was written by the same logout,
                    // so one watch on the account file is enough to know they
                    // all changed.
                    deliver(read_all(&wow, &account));
                });
                *pending.borrow_mut() = Some(source);
            });
        }

        if path.exists() {
            deliver(read_all(wow, account));
        }

        Ok(Watch {
            _monitor: monitor,
            pending,
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // A timeout that fires after the watch is gone would call a closure
        // holding a clone of the application and write into a run that has been
        // replaced.
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
    }
}

/// Everything the addon has written: the account file, and one per character.
pub struct Dump {
    pub collected: Collected,
    pub characters: Vec<CollectedCharacter>,
    /// Play sessions, from every character's file.
    ///
    /// The chronicle is a second saved variable of the same addon, so it lands
    /// in the same per-character file rather than a file of its own — which is
    /// why reading it costs nothing extra here and why the one watch already in
    /// place covers it.
    pub sessions: Vec<Session>,
}

/// Read the account file and every per-character file beside it.
///
/// They are all written by the same logout, so one watch on the account file is
/// enough to know the rest have changed too.
///
/// A character file that will not parse is skipped rather than failing the lot.
/// Losing one character off the roster is a much better outcome than losing the
/// roster, and the one that failed will be rewritten on its next logout. The
/// same goes one level down: a file whose collector half is unreadable may
/// still have a readable chronicle in it, and there is no reason to throw away
/// somebody's evenings over an unrelated table.
pub fn read_all(wow: &Path, account: &str) -> Result<Dump, ReadError> {
    let collected = read(&path(wow, account))?;

    let mut characters = Vec::new();
    let mut sessions = Vec::new();
    for file in crate::model::addon::character_files(wow, account, ADDON) {
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        let source = String::from_utf8_lossy(&bytes);
        if let Ok(character) = collector::read_character(&source) {
            characters.push(character);
        }
        if let Ok(recorded) = chronicle::read(&source) {
            sessions.extend(recorded);
        }
    }

    Ok(Dump {
        collected,
        characters,
        sessions,
    })
}

/// Read and parse the collector's account-wide file.
pub fn read(path: &Path) -> Result<Collected, ReadError> {
    // Read as bytes and lossily decode: SavedVariables is mostly ASCII with
    // `\ddd` escapes for anything else, but a hand-edited file can carry raw
    // UTF-8, and one bad byte should not lose the whole account's attributions.
    let bytes = std::fs::read(path)
        .map_err(|error| ReadError::Unparsable(format!("could not read the file: {error}")))?;
    collector::read(&String::from_utf8_lossy(&bytes))
}

/// Where the collector writes, given an install and an account.
pub fn path(wow: &Path, account: &str) -> PathBuf {
    crate::model::addon::account_saved_variables(wow, account, ADDON)
}

/// The addon's folder name, which is also its SavedVariables file name.
pub const ADDON: &str = "Armory_Collector";

/// Where the client writes screenshots.
pub fn screenshot_directory(wow: &Path) -> PathBuf {
    wow.join("Screenshots")
}

/// Every screenshot the client has written, with the time it landed.
///
/// The join the addon cannot make itself. `Screenshot()` returns nothing and no
/// API reports the filename, so the addon records *when* it fired and this
/// supplies the other half — the files, by modification time, which is when the
/// client finished writing them.
///
/// Bounded to `since`, because a folder that has been accumulating for a decade
/// is thousands of files and only tonight's are ever going to match.
pub fn screenshots_since(wow: &Path, since: DateTime<Utc>) -> Vec<(DateTime<Utc>, String)> {
    let Ok(entries) = std::fs::read_dir(screenshot_directory(wow)) else {
        return Vec::new();
    };

    let mut shots: Vec<(DateTime<Utc>, String)> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("jpg")
                    || extension.eq_ignore_ascii_case("jpeg")
                    || extension.eq_ignore_ascii_case("png")
                    || extension.eq_ignore_ascii_case("tga")
            })
        })
        .filter_map(|entry| {
            let modified: DateTime<Utc> = entry.metadata().ok()?.modified().ok()?.into();
            (modified >= since).then(|| (modified, entry.path().to_string_lossy().into_owned()))
        })
        .collect();

    shots.sort();
    shots
}
