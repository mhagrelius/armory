//! Enrolling a character must not crash the application.
//!
//! This exists because it did. The toggle handler took a `RefMut` on the cohort
//! and then shadowed the guard with a clone, so the `drop` that looked like it
//! released the borrow dropped the clone instead — and the redraw that follows
//! reads the same cell. Every attempt to enrol anybody panicked.
//!
//! Nothing in the type system catches that: `RefCell` moves the check to
//! runtime, which is the whole point of it, and the shadowing made the mistake
//! invisible on the page. Only running the thing finds it, so the thing is run.
//!
//! Needs a display and a session bus — `./test.sh --headless` provides both.

use adw::prelude::*;
use armory::model::character::{Character, CharacterKey, Faction, Roster};
use armory::model::cohort::Cohort;
use armory::model::source::blizzard::Region;
use armory::ui::{Images, RosterPage, Warband};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

fn character(realm: &str, name: &str) -> Character {
    Character {
        key: CharacterKey::new(realm, name),
        id: 1,
        realm_id: 2,
        display_name: name.to_string(),
        realm_name: realm.to_string(),
        level: 80,
        class: "Druid".into(),
        race: "Tauren".into(),
        faction: Faction::Horde,
        wow_account_id: 1,
    }
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

/// Drive the page the way a person does: flip the switch, and let the handler
/// redraw the page from inside its own callback.
///
/// The redraw is the part that matters. A handler that mutates state and then
/// asks the page to rebuild is exactly the shape that panicked, and a test that
/// only called the handler without redrawing would have passed against the
/// broken version.
#[test]
fn enrolling_a_character_redraws_without_panicking() {
    if !toolkit() {
        eprintln!("no display; run under ./test.sh --headless");
        return;
    }

    let roster = Roster::new(vec![
        character("emerald-dream", "Somechar"),
        character("mannoroth", "Aeltor"),
    ]);
    let cohort = Rc::new(RefCell::new(Cohort::new()));
    let images = Images::new();
    let page = RosterPage::new(&images);

    {
        let cohort = Rc::clone(&cohort);
        let roster = roster.clone();
        let page_for_redraw = page.clone();
        page.connect_toggled(move |key| {
            // Mutate, release, redraw — the discipline the application code has
            // to keep. Holding the borrow across `show` is the bug.
            let snapshot = {
                let mut cohort = cohort.borrow_mut();
                cohort.toggle(&key);
                cohort.clone()
            };
            page_for_redraw.show(
                &roster,
                &snapshot,
                &HashMap::new(),
                &HashMap::new(),
                &Warband::default(),
                Region::Us,
            );
        });
    }

    page.show(
        &roster,
        &cohort.borrow(),
        &HashMap::new(),
        &HashMap::new(),
        &Warband::default(),
        Region::Us,
    );

    let switch = find_switch(page.upcast_ref::<gtk::Widget>()).expect("a switch");
    switch.set_active(true);

    assert_eq!(cohort.borrow().len(), 1, "the toggle reached the handler");

    // And back off again, which redraws a second time from a populated state.
    let switch = find_switch(page.upcast_ref::<gtk::Widget>()).expect("a switch");
    switch.set_active(false);
    assert_eq!(cohort.borrow().len(), 0);
}

/// The first `GtkSwitch` anywhere under a widget.
///
/// A character card is a box rather than an `AdwSwitchRow` since the roster was
/// redrawn as a grid, so this looks for the control itself.
fn find_switch(widget: &gtk::Widget) -> Option<gtk::Switch> {
    if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
        return Some(switch.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_switch(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
