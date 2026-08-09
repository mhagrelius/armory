//! An evening, from the game's file to the page, with nothing mocked.
//!
//! Every step of the chronicle is unit tested where it lives. What no unit test
//! covers is the *join*: a file the addon wrote, parsed, filed, read back, and
//! drawn — and every one of those steps hands a slightly different shape to the
//! next. This drives the whole chain against a file in the shape WoW actually
//! writes.
//!
//! Nothing here talks to Anthropic. The request builder and the response reader
//! are pure functions with their own tests; what this asserts about them is the
//! thing a mocked HTTP call would not — that the brief handed to a model is
//! built from exactly what came out of the file, and that a key never ends up
//! anywhere it can be read.
//!
//! Needs a display for the page half — `./test.sh --headless` provides one.

use std::collections::HashMap;

use armory::model::addon::chronicle;
use armory::model::chronicle::{Entry, Session};
use armory::model::source::journal;
use armory::model::store::Store;
use armory::ui::ChroniclePage;

/// A per-character SavedVariables file with both of the addon's tables in it.
///
/// Both, on purpose: the chronicle is a second saved variable of the same
/// addon, so this is one file in practice and reading either half must not
/// disturb the other.
const SAVED: &str = r#"
ArmoryCollectorCharDB = {
	["format"] = 4,
	["name"] = "Somechar",
	["realm"] = "Emerald Dream",
	["level"] = 71,
	["class"] = "DRUID",
	["race"] = "Tauren",
	["faction"] = "Horde",
	["quests"] = { 100, 200 },
}
ArmoryChronicleDB = {
	["format"] = 1,
	["sessions"] = {
		{
			["startedAt"] = 1785000000,
			["endedAt"] = 1785009240,
			["name"] = "Somechar",
			["realm"] = "Emerald Dream",
			["class"] = "DRUID",
			["race"] = "Tauren",
			["faction"] = "Horde",
			["startLevel"] = 70,
			["endLevel"] = 71,
			["startMoney"] = 118204500,
			["endMoney"] = 121950300,
			["startItemLevel"] = 602.4,
			["endItemLevel"] = 606.1,
			["events"] = {
				{ 0, "zone", "Orgrimmar", "The Drag", "" },
				{ 420, "zone", "Nagrand", "Halaa", "" },
				{ 460, "accepted", "Hero of the Mag'har", "Garrosh has not left his tent.", "" },
				{ 1980, "quest", 9923, "Hero of the Mag'har", "You have given him back his father." },
				{ 1980, "questpay", 9923, 84500, 12400 },
				{ 2100, "level", 71, "Nagrand", "" },
				{ 3600, "encounter", "Durn the Hungerer", 0, 14 },
				{ 3900, "death", "Nagrand", "Halaa", "" },
				{ 4500, "encounter", "Durn the Hungerer", 1, 14 },
				{ 5400, "gained", "mount", "Talbuk Doe", "" },
				{ 6000, "sale", "Auction successful: Mycobloom", 3745800, "" },
				{ 7200, "with", "Velkurai", "", "" },
			},
		},
	},
}
"#;

fn read_one() -> Session {
    let mut sessions = chronicle::read(SAVED).expect("the file reads");
    assert_eq!(sessions.len(), 1, "one evening in the file");
    sessions.pop().expect("the evening")
}

#[test]
fn an_evening_survives_the_file_the_store_and_the_digest() {
    let session = read_one();
    let mut store = Store::in_memory().expect("a store");
    assert_eq!(
        store
            .save_sessions(std::slice::from_ref(&session))
            .expect("saved"),
        1
    );

    // Back out of SQLite, which is a JSON round trip through every variant.
    let stored = store.sessions(10).expect("sessions");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0], session, "the evening came back unchanged");

    let digest = stored[0].digest();
    assert_eq!(digest.display_name, "Somechar");
    // Orgrimmar then Nagrand, in that order, with the subzone kept.
    assert_eq!(
        digest
            .route
            .iter()
            .map(|stop| stop.zone.as_str())
            .collect::<Vec<_>>(),
        ["Orgrimmar", "Nagrand"]
    );
    assert_eq!(digest.route[1].within, ["Halaa"]);
    // Wiped on and then killed, so it was killed.
    assert_eq!(digest.felled, ["Durn the Hungerer"]);
    assert!(digest.lost_to.is_empty());
    assert_eq!(digest.levels, [(71, "Nagrand".to_string())]);
    assert_eq!(digest.purse, 3_745_800);
    assert!(digest.is_worth_writing());
}

#[test]
fn the_brief_is_built_from_the_file_and_carries_the_games_own_words() {
    // The quest text is the reason the addon records anything at all here. If
    // it stops reaching the brief, entries go back to being lists of titles —
    // and nothing else in the chain would fail.
    let digest = read_one().digest();
    let brief = journal::brief(&digest);

    assert!(brief.contains("Garrosh has not left his tent."), "{brief}");
    assert!(
        brief.contains("You have given him back his father."),
        "{brief}"
    );
    assert!(brief.contains("Hero of the Mag'har"), "{brief}");
    assert!(brief.contains("Somechar of Emerald Dream"), "{brief}");
    assert!(brief.contains("Talbuk Doe"), "{brief}");
    assert!(brief.contains("Velkurai"), "{brief}");
    // A wipe that ended in a kill is reported as the kill and nothing else, so
    // the model is not told to write about a defeat that was reversed.
    assert!(brief.contains("Bosses defeated"), "{brief}");
    assert!(!brief.contains("Fought and lost to"), "{brief}");
}

#[test]
fn the_request_goes_to_the_local_server_with_no_credential() {
    // The journal talks to a llama-server on this machine. Nothing leaves it,
    // and there is nothing that could.
    let digest = read_one().digest();
    let request = journal::write("http://127.0.0.1:8080", &digest);

    assert_eq!(request.url, "http://127.0.0.1:8080/v1/chat/completions");
    // Nothing to authenticate with, which is most of the point of pointing it
    // at a server on this machine.
    assert!(!request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("x-api-key")));
}

#[test]
fn an_entry_is_filed_against_the_evening_it_is_about() {
    let session = read_one();
    let mut store = Store::in_memory().expect("a store");
    store
        .save_sessions(std::slice::from_ref(&session))
        .expect("saved");

    let body = br#"{"model":"qwen3-30b","choices":[{"finish_reason":"stop","message":
        {"content":"{\"title\":\"What the Mag'har Sing\",\"entry\":\"I went to Halaa.\"}"}}]}"#;
    let written = journal::parse_written(body).found().expect("an entry");

    store
        .save_entry(&Entry {
            session: session.id(),
            title: written.title,
            body: written.body,
            model: written.model,
            written_at: chrono::Utc::now(),
        })
        .expect("saved");

    let entries = store.entries().expect("entries");
    assert_eq!(entries.len(), 1);
    let entry = entries.get(&session.id()).expect("filed under the evening");
    assert_eq!(entry.title, "What the Mag'har Sing");
    assert_eq!(entry.model, "qwen3-30b");
}

/// Initialise the toolkit once, whatever order the tests run in.
fn toolkit() -> bool {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    static OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    ONCE.call_once(|| {
        if gtk::init().is_ok() {
            adw::init().expect("libadwaita");
            OK.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    OK.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn the_page_draws_an_evening_written_and_unwritten_without_panicking() {
    // Both states and both filters, because the card is assembled differently
    // for each and a redraw from inside a callback is the shape that has
    // panicked in this codebase before.
    if !toolkit() {
        eprintln!("no display; run under ./test.sh --headless");
        return;
    }

    let session = read_one();
    let page = ChroniclePage::new();

    // No key yet: the card offers setup rather than a button that would fail.
    page.show(
        std::slice::from_ref(&session),
        &HashMap::new(),
        &[],
        false,
        &HashMap::new(),
    );
    // With one, but nothing written.
    page.show(
        std::slice::from_ref(&session),
        &HashMap::new(),
        &[],
        true,
        &HashMap::new(),
    );
    // Mid-write.
    page.set_writing(&session.id(), true);
    page.set_writing(&session.id(), false);

    let entries = HashMap::from([(
        session.id(),
        Entry {
            session: session.id(),
            title: "What the Mag'har Sing".into(),
            body: "I went to Halaa meaning only to clear the road.".into(),
            model: "claude-opus-5".into(),
            written_at: chrono::Utc::now(),
        },
    )]);
    page.show(
        std::slice::from_ref(&session),
        &entries,
        &[],
        true,
        &HashMap::new(),
    );

    // And with nothing at all, which is what every launch before the addon has
    // written starts as.
    page.show(&[], &HashMap::new(), &[], false, &HashMap::new());
}
