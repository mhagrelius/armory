//! Artwork has to be asked for in the order somebody sees it.
//!
//! Toys are the only collection whose pictures cost a request each — a toy is
//! an item, and an item's texture name appears nowhere the client exposes — so
//! a budget of a hundred and twenty a sync decides which two thousand toys get
//! illustrated and in what order. `art_wanted` used to read the backing store,
//! whose order is whatever `store::collapse_toys` left behind: highest item id
//! first, which is the newest toys in the game. The grid sorts by name. So the
//! icons that arrived were scattered through the page with no pattern to them,
//! and the top of it stayed blank however many syncs ran.
//!
//! That is a widget fact — the order comes from a filter model and a sorter,
//! not from anything under `model/` — so only building the widget shows it.
//! Needs a display; `./test.sh --headless` provides one.
//!
//! One `#[test]`, several assertions. GTK objects belong to the thread that
//! made them and the test harness runs test functions in parallel, so a second
//! `#[test]` in this binary is a segfault rather than a second test.

use armory::model::character::Faction;
use armory::model::source::blizzard::collections::{Collectible, Kind, Source};
use armory::model::source::blizzard::Region;
use armory::ui::{CollectionPage, Images};

use std::collections::{HashMap, HashSet};

/// A toy, named and backed by an item.
///
/// The two ids differ the way they really do: the toy box knows an item in the
/// hundreds of thousands and the web API a toy in the low thousands. An icon is
/// looked up by the item, so a page that offers the collection id sends a sync
/// asking Blizzard about something that is not a toy.
fn toy(id: u32, item_id: u32, name: &str) -> Collectible {
    Collectible {
        kind: Kind::Toy,
        id,
        name: name.to_string(),
        source: Source::Drop,
        description: None,
        flavour: None,
        icon: None,
        // A toy has no creature display, which is the whole reason its picture
        // has to be bought.
        display: None,
        faction: None,
        link_id: item_id,
        tradeable: None,
    }
}

fn page(catalogue: &[Collectible], images: &Images) -> CollectionPage {
    let page = CollectionPage::new(Kind::Toy, images);
    page.show(
        catalogue,
        &HashSet::new(),
        Faction::Horde,
        Region::Us,
        &[],
        &Default::default(),
    );
    page
}

#[test]
fn the_art_budget_is_spent_on_what_the_grid_is_showing() {
    if gtk::init().is_err() {
        eprintln!("no display; skipping");
        return;
    }
    adw::init().expect("libadwaita");
    let images = Images::new();

    // Deliberately the shape `collapse_toys` produces: descending item id, in
    // an order that has nothing to do with the alphabet.
    let catalogue = vec![
        toy(3, 300_003, "Zephyr"),
        toy(2, 200_002, "Muradin's Favor"),
        toy(1, 100_001, "Ancient Amber"),
    ];

    // Two of the three, so the cap is doing something and this is not simply
    // everything in whatever order it was found.
    assert_eq!(
        page(&catalogue, &images).art_wanted(2),
        vec![100_001, 200_002],
        "the budget goes to the top of the page, by item id and not collection id"
    );

    // The model decides the order; it does not decide the extent. Nothing here
    // is owned, so the collected view is empty — and "Fetch Missing Artwork"
    // still has to mean every picture that is missing rather than every picture
    // on the tab somebody happens to be looking at. Stopping at the model is
    // how a run of it fetched the missing third of a catalogue and silently
    // refused the collected two thirds.
    let collected = page(&catalogue, &images);
    collected.set_showing("collected");
    assert_eq!(collected.art_wanted(usize::MAX).len(), 3);

    // And nothing is asked for twice, however many models it appears in.
    let both = page(&catalogue, &images);
    let asked = both.art_wanted(usize::MAX);
    assert_eq!(asked.len(), 3, "{asked:?} has a duplicate");

    // What a launch restored has to come off the list, or the first sync of
    // every session spends its whole budget re-earning URLs already in hand.
    let filled = page(&catalogue, &images);
    let mut art = HashMap::new();
    art.insert(100_001, "https://render/amber.jpg".to_string());
    filled.set_art(&art);
    assert_eq!(filled.art_wanted(120), vec![200_002, 300_003]);
}
