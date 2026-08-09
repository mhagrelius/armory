//! Opening a character and enrolling one are two different gestures.
//!
//! They sit a few pixels apart on the same card, and the first version put a
//! click gesture on the whole card — which fires for a mouse and for nothing
//! else, and has to be careful not to also fire when somebody presses the
//! switch. The card's activatable half is a `GtkButton` and the switch is its
//! sibling, so the two cannot overlap by construction. This is what holds that
//! true: pressing one must not do the other's work.
//!
//! Needs a display — `./test.sh --headless` provides one.

use adw::prelude::*;
use armory::model::character::{Character, CharacterKey, Faction, Roster};
use armory::model::cohort::Cohort;
use armory::model::source::blizzard::Region;
use armory::ui::{Images, RosterPage, Warband};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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

/// Walk the page for the widgets a person would press.
fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget, out: &mut Vec<T>) {
    if let Ok(found) = widget.clone().downcast::<T>() {
        out.push(found);
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        find(&current, out);
        child = current.next_sibling();
    }
}

/// Names the page reported, in the order it reported them.
type Reported = Rc<RefCell<Vec<String>>>;

fn page_with_one_character() -> (RosterPage, Reported, Reported) {
    let images = Images::new();
    let page = RosterPage::new(&images);
    page.show(
        &Roster::new(vec![character("emerald-dream", "Somechar")]),
        &Cohort::new(),
        &HashMap::new(),
        &HashMap::new(),
        &Warband::default(),
        Region::Us,
    );

    let opened: Reported = Rc::default();
    let toggled: Reported = Rc::default();
    {
        let opened = Rc::clone(&opened);
        page.connect_open_character(move |key| opened.borrow_mut().push(key.name.clone()));
    }
    {
        let toggled = Rc::clone(&toggled);
        page.connect_toggled(move |key| toggled.borrow_mut().push(key.name.clone()));
    }
    (page, opened, toggled)
}

/// Both gestures in one test, because GTK may only be used from one thread and
/// `cargo test` runs the functions in a file on several.
#[test]
fn opening_a_character_and_enrolling_one_are_different_gestures() {
    if !toolkit() {
        eprintln!("no display; run under ./test.sh --headless");
        return;
    }

    // Pressing the card opens the character, and enrols nobody.
    let (page, opened, toggled) = page_with_one_character();

    let mut buttons: Vec<gtk::Button> = Vec::new();
    find(page.upcast_ref::<gtk::Widget>(), &mut buttons);
    let card = buttons
        .iter()
        .find(|button| button.has_css_class("al-card-button"))
        .expect("a card to press");
    card.emit_clicked();

    assert_eq!(opened.borrow().as_slice(), ["somechar"]);
    assert!(
        toggled.borrow().is_empty(),
        "opening a character must not enrol them"
    );
    assert!(page.showing_character(), "and it pushes the character page");

    // Flipping the switch enrols them, and opens nothing.
    let (page, opened, toggled) = page_with_one_character();

    let mut switches: Vec<gtk::Switch> = Vec::new();
    find(page.upcast_ref::<gtk::Widget>(), &mut switches);
    switches.first().expect("a switch").set_active(true);

    assert_eq!(toggled.borrow().as_slice(), ["somechar"]);
    assert!(
        opened.borrow().is_empty(),
        "enrolling a character must not open their page"
    );
    assert!(!page.showing_character());
}
