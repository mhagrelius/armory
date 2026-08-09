//! No page may be wider than the window is prepared to give it.
//!
//! This exists because the market page grew past it and what fell off the right
//! edge was the window's own close button. The chain: the browse toolbar's
//! minimum width is the switcher plus the search field, that made the market
//! page the widest of the ten places, the places stack takes its size from the
//! page that is open, and a content pane wider than the window overflows to the
//! right — carrying the header bar, and the window controls at the end of it,
//! past the edge of the screen. Nothing warns except one libadwaita message on
//! stderr, and nothing in the type system has an opinion at all.
//!
//! The numbers here are the window's, not this test's: [`WINDOW`] is the
//! default size `ArmoryWindow` opens at and [`SIDEBAR`] is the places sidebar's
//! `min_sidebar_width`. What is left is what a page gets, rail and all.
//!
//! Needs a display — `./test.sh --headless` provides one.

use adw::prelude::*;
use armory::model::character::Faction;
use armory::model::market::{Crafting, Listed};
use armory::model::source::blizzard::collections::{Collectible, Kind, Source};
use armory::model::source::blizzard::Region;
use armory::model::tally::{Counting, Tally};
use armory::ui::{CollectionPage, Images, MarketPage};

use std::collections::HashSet;

/// The window's default width, from `ArmoryWindow::set_default_size`.
const WINDOW: i32 = 1180;

/// The places sidebar's `min_sidebar_width`.
const SIDEBAR: i32 = 200;

/// Initialise the toolkit once, whatever order the tests run in.
fn toolkit() -> bool {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    static OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    ONCE.call_once(|| {
        if gtk::init().is_ok() {
            adw::init().expect("libadwaita");
            // The stylesheet is not decoration here. Half of what sets a
            // minimum width is in it — `.al-segment.al-fixed` is a `min-width`
            // and the switcher is three of them — so a page measured without it
            // is not the page anybody sees, and measures narrower than it is.
            if let Some(display) = gtk::gdk::Display::default() {
                armory::ui::load_stylesheet(&display);
            }
            OK.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    OK.load(std::sync::atomic::Ordering::SeqCst)
}

fn listing(item_id: u32) -> Listed {
    Listed {
        item_id,
        name: None,
        // The cap, because a price cell that grew with its contents rather than
        // ellipsizing would be a floor that only appears on a rich realm.
        cheapest: 9_999_999_999,
        quantity: 411_466,
        listings: 3,
        tenth: 9_999_999_999,
        median: 9_999_999_999,
        sold: 0,
        span_hours: 24,
    }
}

/// What a page is allowed to need, and why.
fn budget() -> i32 {
    WINDOW - SIDEBAR
}

/// Every page in one test, because GTK may only be used from one thread and
/// `cargo test` runs the functions in a file on several.
#[test]
fn no_page_is_wider_than_the_window_it_opens_in() {
    if !toolkit() {
        eprintln!("no display; run under ./test.sh --headless");
        return;
    }
    the_market_page_fits();
    the_collection_page_fits();
}

/// The market page, with all three tabs built and the browse table full.
fn the_market_page_fits() {
    let page = MarketPage::new();
    // Through `show` as well as `show_market`, because the switcher's tally and
    // the realm caption are both written by it and both sit in the toolbar row
    // that sets the floor.
    page.show(
        &[],
        &[(61, "Emerald Dream".into()), (13, "Mannoroth".into())],
        Some(2_500_000_000),
        &[],
        &[],
        &Crafting::default(),
    );
    let rows: Vec<Listed> = (0..600).map(|index| listing(2770 + index)).collect();
    page.show_market(0, &rows, &HashSet::new());
    while gtk::glib::MainContext::default().iteration(false) {}

    let (minimum, _, _, _) = page.measure(gtk::Orientation::Horizontal, -1);
    let budget = budget();
    assert!(
        minimum <= budget,
        "the market page needs {minimum}px and a {WINDOW}px window has {budget}px to give it \
         once the places sidebar has its {SIDEBAR}px — the content pane overflows to the right \
         and takes the window controls with it"
    );
}

/// The collection page, with the three closest-to-earning cards carrying the
/// longest line they can.
///
/// Those cards are homogeneous and the line on them is a boss's name, so a
/// raid boss with a long one is a floor under three cards at once. This caught
/// it at 1569px the first time the line was added.
fn the_collection_page_fits() {
    let images = Images::new();
    let page = CollectionPage::new(Kind::Mount, &images);

    let catalogue: Vec<Collectible> = [
        (1, "Ashes of Al'ar", "Kael'thas Sunstrider, Tempest Keep"),
        (2, "Invincible's Reins", "The Lich King, Icecrown Citadel"),
        (
            3,
            "Reins of the Twilight Harbinger",
            "Halion the Twilight Destroyer, The Ruby Sanctum",
        ),
    ]
    .into_iter()
    .map(|(id, name, whence)| Collectible {
        kind: Kind::Mount,
        id,
        name: name.into(),
        source: Source::Drop,
        description: Some(format!("Drop: {whence}")),
        flavour: None,
        icon: None,
        display: None,
        faction: None,
        link_id: id,
        tradeable: None,
    })
    .collect();

    let attempts: Vec<Tally> = [
        "Kael'thas Sunstrider",
        "The Lich King",
        "Halion the Twilight Destroyer",
    ]
    .into_iter()
    .map(|label| Tally {
        kind: Counting::Attempt,
        key: label.into(),
        label: label.into(),
        count: 5_318_008,
    })
    .collect();

    page.show(
        &catalogue,
        &HashSet::new(),
        Faction::Horde,
        Region::Us,
        &attempts,
        &Default::default(),
    );
    while gtk::glib::MainContext::default().iteration(false) {}

    let (minimum, _, _, _) = page.measure(gtk::Orientation::Horizontal, -1);
    let budget = budget();
    assert!(
        minimum <= budget,
        "the mounts page needs {minimum}px and a {WINDOW}px window has {budget}px to give it \
         once the places sidebar has its {SIDEBAR}px"
    );
}
