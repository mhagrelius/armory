//! One collection — mounts, or pets, or toys, or housing decor — as something a
//! collector can actually work through.
//!
//! Sixteen hundred mounts is not a list, it is a search problem, and the
//! question a collector asks is never "what are the first forty" but "have I
//! got the Karazhan one" or "what is left that drops". So: every entry,
//! illustrated, searched as you type, and **read in the order the work happens
//! in** — grouped by where a thing comes from, because an evening is spent on
//! one source at a time.
//!
//! ## Two panes
//!
//! The main column is only ever the collection: the three nearest to earning,
//! then the catalogue in its groups. Everything *about* the collection — the
//! standing, the toggle, the source list, the caveat about drop rates — is in
//! the rail. That is the almanac's rule and it is what took a summary, a filter
//! wrap and a caveat out of the top of the grid.
//!
//! ## Why grouping is a page state rather than a section header
//!
//! `GtkGridView` has no header factory. `GtkListView` gained one in GTK 4.12
//! and the grid did not, so a single grid cannot draw `DROPS` above its drops
//! and `VENDOR` above its vendors. Stacking one grid per source in a box
//! instead would draw every section header — and destroy the recycling the page
//! exists for, because a `GtkGridView` that is not its scroller's own child
//! sees its whole allocation as visible and realises every cell in it.
//!
//! So the groups are a *contact sheet*: each source shows [`GROUP_SHOWN`]
//! entries through a `GtkSliceListModel`, which is a cap the model applies and
//! not a list anybody built. Choosing a source — from the rail, or from the
//! group's own button — swaps the body to one grid that is the scroller's own
//! child, uncapped, recycling, and the same widget the whole catalogue used to
//! be. Nothing ever realises more than a screen and a half of cells.
//!
//! ## Already owned is not a tick
//!
//! In a run, owning something is the bad news: it is a thing that cannot be
//! earned again. The tick that used to sit on the artwork said the opposite, so
//! an owned entry is drawn dashed and dimmed with "already owned" under it —
//! the same treatment the run gives a spent goal.
//!
//! The pictures come from `render.worldofwarcraft.com` addressed by the
//! creature display id the collector addon already recorded — no request, no
//! quota, no token. Toys and decor are the exception and say so: both are
//! items, an item's art is addressed by a texture name nothing local knows, and
//! the answer costs one call to Blizzard each.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::almanac::{self, Bar};
use super::images::{Art, Images};
use crate::model::character::Faction;
use crate::model::rarity::Chances;
use crate::model::source::blizzard::collections::{Collectible, Kind, Source};
use crate::model::source::blizzard::{media, Region};
use crate::model::tally::{self, Tally};

/// How big the art is in a grid cell, in pixels.
///
/// Arithmetic rather than taste: seven columns in the main column of a 1100px
/// window is 520px of grid, and seven square tiles with the design's eleven
/// pixels between them is sixty-five each. Down from ninety-six, which is the
/// price of the rail; the renders are 600 square either way, and a smaller
/// decode is a smaller cache.
const ART: i32 = 64;

/// The artwork on one of the three cards at the top.
const CARD_ART: i32 = 58;

/// How wide the rail is. The same as every other page but the Run and the
/// Market, which carry two lists and a price book respectively.
const RAIL: f64 = 288.0;

/// How many entries a source group shows before it defers to its own page.
///
/// Three rows of seven. The cap is what keeps the contact sheet from realising
/// a thousand cells; the button under it is how the rest is reached.
const GROUP_SHOWN: u32 = 21;

/// Seven columns, as the design has it — and pinned at both ends rather than
/// given as a range.
///
/// A `GtkGridView` measures its height at `min-columns`, whatever width it is
/// then handed, so a grid free to widen reserves the room its narrowest layout
/// would have needed: four mounts in a group left three hundred pixels of
/// nothing under them. Pinning the two together is what makes a group as tall
/// as the rows it actually draws.
const COLUMNS: u32 = 7;

/// How many are named as closest to earning.
const CLOSEST: usize = 3;

/// The order the page reads in, and the order the rail lists.
///
/// Explicit rather than alphabetical. `Source::label` puts Achievement above
/// Drop, and a collection whose largest group is drops should not open on its
/// third largest.
const GROUPS: [Source; 8] = [
    Source::Drop,
    Source::Vendor,
    Source::Achievement,
    Source::Quest,
    Source::Profession,
    Source::Pvp,
    Source::Promotion,
    Source::Unknown,
];

/// Which entries the grid is showing.
///
/// Public only because it is named by a field on the private implementation
/// struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Showing {
    /// Everything in the catalogue, obtainable or not.
    All,
    Collected,
    /// Missing, and still gettable. A faction-locked mount on the wrong side
    /// and a trading-card mount are not a gap in this account's collection, and
    /// counting them as one overstates the backlog by a few hundred.
    #[default]
    Missing,
}

impl Showing {
    /// The rail's toggle, left to right.
    const ALL: [Showing; 3] = [Showing::Missing, Showing::Collected, Showing::All];

    fn from_name(name: &str) -> Showing {
        match name {
            "all" => Showing::All,
            "collected" => Showing::Collected,
            _ => Showing::Missing,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Showing::All => "All",
            Showing::Collected => "Collected",
            Showing::Missing => "Missing",
        }
    }

    /// What a count of this view is a count *of*.
    ///
    /// "318 missing" and "318 collected" are opposite facts and a group heading
    /// that said only "318" would be either, depending on a toggle in the other
    /// pane.
    fn counted(self) -> &'static str {
        match self {
            Showing::All => "in all",
            Showing::Collected => "collected",
            Showing::Missing => "missing",
        }
    }
}

/// A source group's heading in the main column.
fn heading(source: Source) -> &'static str {
    match source {
        Source::Drop => "DROPS",
        Source::Vendor => "VENDOR",
        Source::Achievement => "ACHIEVEMENTS",
        Source::Quest => "QUESTS",
        Source::Profession => "PROFESSIONS",
        Source::Pvp => "PVP",
        Source::Promotion => "PROMOTIONS",
        Source::Unknown => "UNRECORDED",
    }
}

/// A source in the rail's list.
///
/// `Source::label` says "Unknown", which reads as a property of the mount.
/// It is a gap in Blizzard's data, and the caveat at the foot of the rail is
/// where that is explained.
fn rail_label(source: Source) -> &'static str {
    match source {
        Source::Unknown => "Unrecorded",
        source => source.label(),
    }
}

/// How much chance stands between the account and one of these.
///
/// **Armory has no drop rates.** There is no AllTheThings integration and no
/// rarity source, so nothing here knows that Invincible is one in a hundred,
/// and inventing a figure about somebody's own odds is the mistake
/// `Resale::floor` exists to avoid. What the page *does* know is whether chance
/// is involved at all — a vendor mount is gold and a quest reward is time,
/// where a raid drop is a coin flipped until it lands. Ranking on that is the
/// strongest true statement available, and it is why the cards carry a sentence
/// where the design carried a rate.
///
/// `None` is a source with chance in it, which sorts last and gets no gold.
fn certainty(source: Source) -> Option<u8> {
    match source {
        Source::Vendor => Some(0),
        Source::Quest => Some(1),
        Source::Achievement => Some(2),
        Source::Profession => Some(3),
        Source::Pvp => Some(4),
        Source::Drop | Source::Promotion | Source::Unknown => None,
    }
}

/// What can honestly be said, in a line, about how one of these is earned.
fn no_chance(source: Source) -> Option<&'static str> {
    match source {
        Source::Vendor => Some("SOLD, NOT DROPPED"),
        Source::Quest => Some("A QUEST, NOT A ROLL"),
        Source::Achievement => Some("EARNED, NOT ROLLED"),
        Source::Profession => Some("MADE, NOT DROPPED"),
        Source::Pvp => Some("WON, NOT ROLLED"),
        _ => None,
    }
}

/// The day the weekly lockout turns over, which is the clock a collector plans
/// a raid week around. Blizzard resets the Americas on Tuesday, Europe on
/// Wednesday and the Asian regions on Thursday.
fn reset_day(region: Region) -> &'static str {
    match region {
        Region::Us => "TUESDAY",
        Region::Eu => "WEDNESDAY",
        Region::Kr | Region::Tw => "THURSDAY",
    }
}

// -- one entry, as an object the list model can hold -------------------------

mod entry {
    use super::*;

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct Entry {
            pub collectible: RefCell<Option<Collectible>>,
            pub owned: Cell<bool>,
            pub obtainable: Cell<bool>,
            /// Lowercased once, at build time. Recomputing it inside a filter
            /// that runs over sixteen hundred entries on every keystroke is the
            /// difference between a search that keeps up with typing and one
            /// that does not.
            pub haystack: RefCell<String>,
            pub art: RefCell<Option<String>>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for Entry {
            const NAME: &'static str = "ArmoryCollectionEntry";
            type Type = super::Entry;
        }

        impl ObjectImpl for Entry {}
    }

    glib::wrapper! {
        pub struct Entry(ObjectSubclass<imp::Entry>);
    }

    impl Entry {
        pub fn new(
            collectible: &Collectible,
            owned: bool,
            faction: Faction,
            region: Region,
        ) -> Self {
            let entry: Self = glib::Object::builder().build();
            let imp = entry.imp();

            imp.haystack.replace(
                format!(
                    "{} {} {}",
                    collectible.name,
                    collectible.source.label(),
                    collectible.description.as_deref().unwrap_or_default()
                )
                .to_lowercase(),
            );
            imp.art.replace(art_url(collectible, region));
            imp.owned.set(owned);
            imp.obtainable.set(collectible.obtainable_by(faction));
            imp.collectible.replace(Some(collectible.clone()));
            entry
        }

        pub fn collectible(&self) -> Collectible {
            self.imp()
                .collectible
                .borrow()
                .clone()
                .expect("an entry always holds one")
        }

        pub fn owned(&self) -> bool {
            self.imp().owned.get()
        }

        pub fn obtainable(&self) -> bool {
            self.imp().obtainable.get()
        }

        pub fn art(&self) -> Option<String> {
            self.imp().art.borrow().clone()
        }

        pub fn set_art(&self, url: &str) {
            self.imp().art.replace(Some(url.to_string()));
        }

        pub fn matches(&self, needle: &str) -> bool {
            needle.is_empty() || self.imp().haystack.borrow().contains(needle)
        }

        /// The item this entry is, where that is actually known.
        ///
        /// Zero when it is not. The index gives a toy or a piece of decor the
        /// collection's own id as a stand-in for the item, and asking the media
        /// service for the icon of item 5 when 5 is a decor id fetches a real
        /// icon for the wrong thing — which is how a chair came to be drawn as
        /// a belt.
        pub fn item_id(&self) -> u32 {
            self.collectible().known_item_id().unwrap_or(0)
        }
    }

    /// Where the picture for one entry lives, if it can be worked out for free.
    ///
    /// A creature display id is all the render service needs, and the addon
    /// records one for every mount and pet the client knows. A toy has none —
    /// it is an item, drawn from an icon addressed by texture name — so it
    /// comes back `None` here and is filled in later by whoever can afford the
    /// call.
    fn art_url(collectible: &Collectible, region: Region) -> Option<String> {
        collectible
            .display
            .filter(|display| *display > 0)
            .map(|display| media::creature_render(region, display))
    }
}

pub use entry::Entry;

// -- the pieces the page keeps a handle on ------------------------------------
//
// Both are `pub` for the same reason [`Showing`] is: they are named by fields on
// the private implementation struct, which the subclass macro makes public.

/// One source's worth of the catalogue, as a contact sheet in the main column.
#[derive(Clone)]
pub struct Group {
    source: Source,
    widget: gtk::Box,
    count: gtk::Label,
    more: gtk::Button,
    /// Everything in the group, before the slice caps what is drawn. This is
    /// what the heading counts, so it says how much work there is rather than
    /// how much of it fits.
    held: gtk::FilterListModel,
}

/// One source in the rail's "WHERE THEY COME FROM".
#[derive(Clone)]
pub struct RailRow {
    source: Source,
    button: gtk::Button,
    name: gtk::Label,
    count: gtk::Label,
}

// -- the page ----------------------------------------------------------------

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CollectionPage {
        /// Set once, at construction. A `Kind` has no sensible default — there
        /// is no such thing as a collection of nothing in particular — so this
        /// is the cell that says "not yet" rather than inventing one.
        pub kind: std::cell::OnceCell<Kind>,
        pub images: RefCell<Option<Images>>,
        pub region: Cell<Region>,

        pub store: RefCell<Option<gtk::gio::ListStore>>,
        pub filter: RefCell<Option<gtk::CustomFilter>>,
        /// Filtered and ordered: the collection as the page reads it, top to
        /// bottom. Every group hangs off this, and so does `art_wanted`.
        pub sorted: RefCell<Option<gtk::SortListModel>>,
        /// Which source the one-source page is showing.
        pub focused: RefCell<Option<gtk::CustomFilter>>,
        pub grid: RefCell<Option<gtk::GridView>>,
        pub groups: RefCell<Vec<super::Group>>,
        pub body: RefCell<Option<gtk::Stack>>,
        pub split: RefCell<Option<adw::OverlaySplitView>>,

        pub closest_block: RefCell<Option<gtk::Box>>,
        pub closest_cards: RefCell<Option<gtk::Box>>,
        /// The three named as closest, held so fresh artwork can redraw their
        /// cards without the application being asked for a catalogue it has
        /// already handed over.
        pub closest: RefCell<Vec<super::Entry>>,
        /// Every attempt this account has made at anything, merged across
        /// characters. What a card joins a drop against.
        pub attempts: RefCell<Vec<Tally>>,
        /// Drop chances out of an installed Rarity, empty when there is none.
        pub chances: RefCell<Chances>,
        /// Their ids, which is what marks their tiles in the grid.
        pub notable: RefCell<HashSet<u32>>,

        pub focus_heading: RefCell<Option<gtk::Label>>,
        pub focus_count: RefCell<Option<gtk::Label>>,

        pub search: RefCell<Option<gtk::SearchBar>>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub segments: RefCell<Option<gtk::Box>>,
        pub rail_rows: RefCell<Vec<super::RailRow>>,
        pub figure: RefCell<Option<gtk::Label>>,
        pub denominator: RefCell<Option<gtk::Label>>,
        pub bar: RefCell<Option<super::Bar>>,
        pub note: RefCell<Option<gtk::Label>>,

        /// The search text and the toggles, lowercased and resolved once so the
        /// filter closure reads them rather than the widgets.
        pub needle: RefCell<String>,
        pub showing: Cell<Showing>,
        pub source: Cell<Option<Source>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CollectionPage {
        const NAME: &'static str = "ArmoryCollectionPage";
        type Type = super::CollectionPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for CollectionPage {}
    impl WidgetImpl for CollectionPage {}
    impl BinImpl for CollectionPage {}
}

glib::wrapper! {
    pub struct CollectionPage(ObjectSubclass<imp::CollectionPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CollectionPage {
    pub fn new(kind: Kind, images: &Images) -> Self {
        let page: Self = glib::Object::builder().build();
        let _ = page.imp().kind.set(kind);
        *page.imp().images.borrow_mut() = Some(images.clone());
        page.build();
        page
    }

    pub fn kind(&self) -> Kind {
        self.imp().kind.get().copied().unwrap_or(Kind::Mount)
    }

    fn build(&self) {
        let imp = self.imp();

        let store = gtk::gio::ListStore::new::<Entry>();
        let filter = gtk::CustomFilter::new(|_| true);
        let sorter = gtk::CustomSorter::new(Self::compare);

        let filtered = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let sorted = gtk::SortListModel::new(Some(filtered), Some(sorter));

        let factory = self.factory();

        // Two things can be true of an empty grid and they need different
        // words: a catalogue that has never synced, and a search with no hits.
        let body = gtk::Stack::new();
        body.set_vexpand(true);
        body.add_named(&self.groups_page(&sorted, &factory), Some("groups"));
        body.add_named(&self.focus_page(&sorted, &factory), Some("source"));
        body.add_named(&self.nothing_synced(), Some("unsynced"));
        body.add_named(&self.no_results(), Some("empty"));

        let column = almanac::column(14);
        column.add_css_class("al-main");
        column.append(&self.closest_block());
        column.append(&body);

        let split = almanac::split(&column, &almanac::rail_pane(&self.rail()), RAIL);

        let search = self.search_bar();
        let view = adw::ToolbarView::builder().content(&split).build();
        view.add_top_bar(&search);

        *imp.store.borrow_mut() = Some(store);
        *imp.filter.borrow_mut() = Some(filter);
        *imp.sorted.borrow_mut() = Some(sorted);
        *imp.body.borrow_mut() = Some(body);
        *imp.split.borrow_mut() = Some(split);
        *imp.search.borrow_mut() = Some(search);

        self.set_child(Some(&view));
        self.reapply();
    }

    /// Source group first, then name.
    ///
    /// Fixed rather than offered as a choice. The page *is* the grouping now,
    /// so a second ordering would be a grid whose headings no longer bracket
    /// their own entries — and `art_wanted` reads this order as the order
    /// somebody sees, which only holds while there is one of them.
    fn compare(a: &glib::Object, b: &glib::Object) -> gtk::Ordering {
        let (Some(a), Some(b)) = (a.downcast_ref::<Entry>(), b.downcast_ref::<Entry>()) else {
            return gtk::Ordering::Equal;
        };
        let one = a.collectible();
        let two = b.collectible();

        let rank = |source: Source| {
            GROUPS
                .iter()
                .position(|held| *held == source)
                .unwrap_or(GROUPS.len())
        };
        rank(one.source)
            .cmp(&rank(two.source))
            .then_with(|| one.name.to_lowercase().cmp(&two.name.to_lowercase()))
            .then_with(|| one.id.cmp(&two.id))
            .into()
    }

    // -- the main column ------------------------------------------------------

    /// The three nearest to earning, above the catalogue and outside its
    /// scroller: they are the answer to "what should I do tonight" and scrolling
    /// them away to reach the grid would hide the one part of the page that is
    /// advice rather than inventory.
    fn closest_block(&self) -> gtk::Box {
        let block = almanac::column(10);
        block.set_margin_top(18);
        block.set_margin_start(28);
        block.set_margin_end(24);
        block.set_visible(false);

        let cards = almanac::row(11);
        cards.set_homogeneous(true);

        block.append(&almanac::section(&format!(
            "CLOSEST TO EARNING — LOCKOUT RESETS {}",
            reset_day(self.imp().region.get())
        )));
        block.append(&cards);

        *self.imp().closest_block.borrow_mut() = Some(block.clone());
        *self.imp().closest_cards.borrow_mut() = Some(cards);
        block
    }

    /// One of the three, and what can honestly be said about earning it.
    fn closest_card(&self, entry: &Entry) -> gtk::Box {
        let collectible = entry.collectible();
        // Two different gold lines, and neither is a drop rate. The first says
        // chance is not in the way at all; the second says how many times this
        // account has already fought the thing that drops it. The design asked
        // for "1 IN 100 · 31 TRIES" and only the second half of that exists —
        // Blizzard publishes no rates, AllTheThings is not parsed and Wowhead
        // may not be fetched, so the odds are quietly dropped rather than
        // guessed at.
        let fought = tally::attempts_at(
            collectible.description.as_deref(),
            &self.imp().attempts.borrow(),
        );
        let odds = self.imp().chances.borrow().one_in(&collectible);
        let line = match no_chance(collectible.source) {
            Some(line) => Some(line.to_string()),
            // `1 IN 100 · 31 TRIES`, and either half alone when only one is
            // known. The odds are Rarity's estimate and the tries are this
            // account's own count — two different kinds of fact, which is why
            // the tooltip says which is which rather than letting the line
            // read as one measurement.
            None => match (odds, fought.as_ref()) {
                (Some(one_in), Some((_, tries))) => Some(format!(
                    "1 IN {one_in}\n{}",
                    almanac::plural(*tries as usize, "TRY", "TRIES").to_uppercase()
                )),
                (Some(one_in), None) => Some(format!("1 IN {one_in}")),
                (None, Some((_, tries))) => {
                    Some(almanac::plural(*tries as usize, "TRY", "TRIES").to_uppercase())
                }
                (None, None) => None,
            },
        };

        // Gold only where there is something gold to say. The design tints the
        // cards carrying an estimate and leaves the rest plain, and with no
        // rates at all the distinction that survives is whether chance is in
        // the way — or whether somebody has been going after it anyway.
        let card = if line.is_some() {
            almanac::earned_card(0)
        } else {
            almanac::card(0)
        };
        card.add_css_class("al-activatable");

        let row = almanac::row(11);
        let art = Art::new(CARD_ART, self.placeholder());
        art.set_valign(gtk::Align::Center);
        if let Some(images) = self.imp().images.borrow().as_ref() {
            art.show(images, entry.art().as_deref(), CARD_ART);
        }
        row.append(&art);

        let text = almanac::column(2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);

        let name = almanac::label(&collectible.name, &["al-row-title"]);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name);

        let where_ = almanac::label(&whence(&collectible), &["al-caption"]);
        where_.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&where_);

        if let Some(line) = &line {
            let claim = almanac::mono(line, &["al-price", "al-gold"]);
            claim.set_tooltip_text(Self::claim_tooltip(odds, fought.as_ref()).as_deref());
            claim.set_margin_top(3);
            // Two lines rather than one, when there are two things to say.
            // The design has `1 IN 100 · 31 TRIES` on a single line at a width
            // these cards do not have — three of them abreast in a main column
            // truncated it to `1 IN 20 · 58 …`, which loses the half that is
            // about you.
            //
            // Ellipsized as well, because it is still a floor under three
            // homogeneous cards and through them under the whole window —
            // `tests/width.rs` caught that at 1569px against a budget of 980.
            claim.set_xalign(0.0);
            claim.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&claim);
        }
        row.append(&text);
        card.append(&row);

        let owned = entry.owned();
        let art_url = entry.art();
        let page = self.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |gesture, _, _, _| {
            if let Some(widget) = gesture.widget() {
                super::collectible_dialog::present(
                    &widget,
                    &collectible,
                    owned,
                    page.imp().images.borrow().as_ref(),
                    art_url.as_deref(),
                );
            }
        });
        card.add_controller(click);
        card
    }

    /// What the gold line on a card actually means, said in full.
    ///
    /// The two halves come from different places and are worth different
    /// amounts, so the tooltip names both rather than letting a single line
    /// read as one measurement. The odds are somebody else's estimate — Rarity
    /// reads Wowhead's observed rates and guesses, and a user can override any
    /// of them. The tries are this account's own count, and are the only figure
    /// here that Armory watched happen.
    fn claim_tooltip(odds: Option<u32>, fought: Option<&(String, u64)>) -> Option<String> {
        match (odds, fought) {
            (Some(one_in), Some((boss, tries))) => Some(format!(
                "Roughly a one in {one_in} chance, estimated by the Rarity addon \
                 — Blizzard publishes no drop rates. This account has pulled \
                 {boss} {tries} times, which is Armory's own count.",
            )),
            (Some(one_in), None) => Some(format!(
                "Roughly a one in {one_in} chance, estimated by the Rarity addon \
                 — Blizzard publishes no drop rates.",
            )),
            (None, Some((boss, tries))) => Some(format!(
                "This account has pulled {boss} {tries} times. No drop rate for \
                 this one: install the Rarity addon and Armory will read its \
                 estimate from your own copy.",
            )),
            (None, None) => None,
        }
    }

    /// The contact sheet: every source that has anything in it, capped.
    fn groups_page(
        &self,
        sorted: &gtk::SortListModel,
        factory: &gtk::SignalListItemFactory,
    ) -> gtk::ScrolledWindow {
        let column = almanac::column(16);
        column.set_valign(gtk::Align::Start);
        column.set_margin_start(28);
        column.set_margin_end(24);
        column.set_margin_bottom(24);

        let mut groups = Vec::new();
        for source in GROUPS {
            let held = gtk::FilterListModel::new(
                Some(sorted.clone()),
                Some(gtk::CustomFilter::new(move |object| {
                    object
                        .downcast_ref::<Entry>()
                        .is_some_and(|entry| entry.collectible().source == source)
                })),
            );
            // The cap is the model's, not a list anybody built: a slice asks
            // its child for twenty-one items and never for the other three
            // hundred.
            let shown = gtk::SliceListModel::new(Some(held.clone()), 0, GROUP_SHOWN);
            let grid = self.grid(&gtk::NoSelection::new(Some(shown)), factory);
            grid.set_vexpand(false);

            let count = almanac::label("", &["al-footnote"]);
            let title = almanac::row(10);
            title.set_valign(gtk::Align::Baseline);
            title.append(&almanac::section(heading(source)));
            title.append(&count);

            let more = gtk::Button::builder()
                .halign(gtk::Align::Start)
                .visible(false)
                .build();
            more.add_css_class("flat");
            let page = self.clone();
            more.connect_clicked(move |_| page.choose_source(Some(source)));

            let widget = almanac::column(10);
            widget.append(&title);
            widget.append(&grid);
            widget.append(&more);
            widget.set_visible(false);
            column.append(&widget);

            groups.push(Group {
                source,
                widget,
                count,
                more,
                held,
            });
        }
        *self.imp().groups.borrow_mut() = groups;

        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build()
    }

    /// One source, whole. The grid is the scroller's own child here, which is
    /// what makes it recycle: three hundred drops cost the same as thirty.
    fn focus_page(
        &self,
        sorted: &gtk::SortListModel,
        factory: &gtk::SignalListItemFactory,
    ) -> gtk::Box {
        let focused = gtk::CustomFilter::new(|_| true);
        let model = gtk::NoSelection::new(Some(gtk::FilterListModel::new(
            Some(sorted.clone()),
            Some(focused.clone()),
        )));
        let grid = self.grid(&model, factory);
        grid.set_vexpand(true);

        let heading = almanac::section("");
        let count = almanac::label("", &["al-footnote"]);

        let back = gtk::Button::builder()
            .label("Every source")
            .halign(gtk::Align::End)
            .hexpand(true)
            .build();
        back.add_css_class("flat");
        let page = self.clone();
        back.connect_clicked(move |_| page.choose_source(None));

        let title = almanac::row(10);
        title.set_valign(gtk::Align::Baseline);
        title.set_margin_start(28);
        title.set_margin_end(24);
        title.append(&heading);
        title.append(&count);
        title.append(&back);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&grid)
            .build();
        scroller.set_margin_start(24);
        scroller.set_margin_end(20);
        scroller.set_margin_bottom(20);

        let column = almanac::column(10);
        column.append(&title);
        column.append(&scroller);

        let imp = self.imp();
        *imp.focused.borrow_mut() = Some(focused);
        *imp.grid.borrow_mut() = Some(grid);
        *imp.focus_heading.borrow_mut() = Some(heading);
        *imp.focus_count.borrow_mut() = Some(count);
        column
    }

    /// A grid over some slice of the collection.
    fn grid(
        &self,
        model: &impl IsA<gtk::SelectionModel>,
        factory: &gtk::SignalListItemFactory,
    ) -> gtk::GridView {
        let grid = gtk::GridView::builder()
            .model(model)
            .factory(factory)
            .max_columns(COLUMNS)
            .min_columns(COLUMNS)
            .single_click_activate(true)
            .build();
        // A `GtkGridView` carries the `view` style class and so paints the list
        // background under the tiles, which on this page is a panel the design
        // does not have: the artwork sits on the page ground with nothing
        // behind it. `.collection-grid` puts the ground back.
        grid.add_css_class("collection-grid");

        let page = self.clone();
        grid.connect_activate(move |grid, position| {
            let Some(entry) = grid
                .model()
                .and_then(|model| model.item(position))
                .and_downcast::<Entry>()
            else {
                return;
            };
            super::collectible_dialog::present(
                grid,
                &entry.collectible(),
                entry.owned(),
                page.imp().images.borrow().as_ref(),
                entry.art().as_deref(),
            );
        });
        grid
    }

    // -- the rail -------------------------------------------------------------

    fn rail(&self) -> gtk::Box {
        let rail = almanac::rail_column();
        let imp = self.imp();

        // The standing.
        let figure = almanac::mono("0", &["al-figure-large"]);
        let denominator = almanac::label("", &["al-caption"]);
        denominator.set_valign(gtk::Align::Baseline);

        let standing = almanac::row(8);
        standing.set_valign(gtk::Align::Baseline);
        standing.append(&figure);
        standing.append(&denominator);

        let bar = Bar::new(6);
        let note = almanac::caption("");
        note.set_visible(false);

        let block = almanac::column(9);
        block.append(&standing);
        block.append(&bar.widget);
        block.append(&note);
        rail.append(&block);

        // Which of them to look at. Full width, because the rail has nothing
        // else on its line and a control huddled left in a 288px pane reads as
        // unfinished.
        let page = self.clone();
        let segments = almanac::segments(&Showing::ALL.map(Showing::label), 0, move |index| {
            let chosen = Showing::ALL.get(index).copied().unwrap_or_default();
            page.imp().showing.set(chosen);
            page.reapply();
        });
        segments.set_halign(gtk::Align::Fill);
        segments.set_homogeneous(true);
        rail.append(&segments);

        // Where they come from, which is also the filter: choosing one is what
        // opens its own page in the main column.
        let list = almanac::column(7);
        let mut rows = Vec::new();
        for source in GROUPS {
            let name = almanac::label(rail_label(source), &[]);
            name.set_hexpand(true);
            let count = almanac::mono("", &["al-price"]);
            count.set_halign(gtk::Align::End);

            let line = almanac::row(9);
            line.append(&name);
            line.append(&count);

            let button = gtk::Button::builder().child(&line).build();
            button.add_css_class("flat");
            let page = self.clone();
            button.connect_clicked(move |_| {
                let already = page.imp().source.get() == Some(source);
                page.choose_source((!already).then_some(source));
            });
            button.set_visible(false);
            list.append(&button);

            rows.push(RailRow {
                source,
                button,
                name,
                count,
            });
        }
        rail.append(&almanac::titled("WHERE THEY COME FROM", &list));

        rail.append(&almanac::hairline());

        // The caveat, and it is not the design's. The mock credits Rarity for
        // drop rates and promises AllTheThings for sources; Armory has neither,
        // so the rail says what is actually behind every figure on the page
        // rather than crediting an addon nothing here reads.
        rail.append(&almanac::caption(
            "There are no drop rates here. A source is Blizzard's one word and \
             whatever sentence the collector addon recorded — the odds would \
             need AllTheThings, which is not parsed. Opening an entry links out \
             to Wowhead for the rest.",
        ));

        *imp.figure.borrow_mut() = Some(figure);
        *imp.denominator.borrow_mut() = Some(denominator);
        *imp.bar.borrow_mut() = Some(bar);
        *imp.note.borrow_mut() = Some(note);
        *imp.segments.borrow_mut() = Some(segments);
        *imp.rail_rows.borrow_mut() = rows;
        rail
    }

    // -- chrome ---------------------------------------------------------------

    fn search_bar(&self) -> gtk::SearchBar {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(match self.kind() {
                Kind::Mount => "Search mounts",
                Kind::Pet => "Search pets",
                Kind::Toy => "Search toys",
                Kind::Decor => "Search decor",
            })
            .hexpand(true)
            .build();

        let page = self.clone();
        entry.connect_search_changed(move |entry| {
            page.imp()
                .needle
                .replace(entry.text().trim().to_lowercase());
            page.reapply();
        });

        let bar = gtk::SearchBar::builder()
            .child(
                &adw::Clamp::builder()
                    .maximum_size(560)
                    .child(&entry)
                    .build(),
            )
            .build();
        bar.connect_entry(&entry);

        *self.imp().entry.borrow_mut() = Some(entry);
        bar
    }

    /// The icon standing in for a picture that has not arrived.
    fn placeholder(&self) -> &'static str {
        match self.kind() {
            Kind::Mount => "starred-symbolic",
            Kind::Pet => "emblem-favorite-symbolic",
            Kind::Toy => "applications-games-symbolic",
            Kind::Decor => "user-home-symbolic",
        }
    }

    fn nothing_synced(&self) -> gtk::Widget {
        adw::StatusPage::builder()
            .icon_name(self.placeholder())
            .title("Nothing synced yet")
            .description(if self.kind() == Kind::Decor {
                // Decor is the one collection the addon has nothing to say
                // about. Sending somebody to install one would be sending them
                // to fix a problem it cannot fix.
                "Sync to fetch the housing catalogue and what your account has of it. \
                 Decor came to the API with Midnight's housing and is the one \
                 collection here that needs a Battle.net client — the collector addon \
                 does not read it."
            } else {
                "Sync to fetch what your account has collected and what exists to \
                 collect. Logging out once with the collector addon installed brings \
                 the whole catalogue in one go, with the artwork and the in-game \
                 source text the web API has no field for."
            })
            .vexpand(true)
            .build()
            .upcast()
    }

    fn no_results(&self) -> gtk::Widget {
        let status = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("No matches")
            .description("Nothing here fits the search and filters.")
            .vexpand(true)
            .build();

        let clear = gtk::Button::builder()
            .label("Clear Filters")
            .halign(gtk::Align::Center)
            .build();
        clear.add_css_class("pill");
        let page = self.clone();
        clear.connect_clicked(move |_| page.clear_filters());
        status.set_child(Some(&clear));

        status.upcast()
    }

    // -- the cells -------------------------------------------------------------

    fn factory(&self) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        let placeholder = self.placeholder();

        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            let art = Art::new(ART, placeholder);
            art.add_css_class("al-tile");
            art.set_halign(gtk::Align::Center);

            // Two lines always, whether or not the name needs them. A label
            // that shrinks to one puts the line underneath it at a different
            // height from its neighbours, and a grid whose captions do not line
            // up reads as broken rather than as varied.
            let name = gtk::Label::builder()
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .lines(2)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .justify(gtk::Justification::Center)
                .valign(gtk::Align::Start)
                .max_width_chars(14)
                // `max_width_chars` caps where the name wraps. `width_chars`
                // would *also* floor it at fourteen characters, and seven of
                // those floors side by side is what set the minimum width of
                // the whole window — the grid could not shrink, so the rail was
                // pushed off the right edge. The tile's floor is its artwork.
                .height_request(30)
                .build();
            name.add_css_class("al-tile-name");

            // "already owned", or the one word about why a missing entry is not
            // really missing. Never a source: the group heading above it has
            // just said where everything under it comes from.
            let note = gtk::Label::builder()
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(14)
                .build();
            note.add_css_class("al-footnote");

            let cell = almanac::column(6);
            cell.set_margin_top(6);
            cell.set_margin_bottom(6);
            cell.set_margin_start(5);
            cell.set_margin_end(5);
            cell.append(&art);
            cell.append(&name);
            cell.append(&note);
            cell.add_css_class("collection-cell");

            item.set_child(Some(&cell));
        });

        let page = self.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(entry) = item.item().and_downcast::<Entry>() else {
                return;
            };
            let Some(cell) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            page.dress(&cell, &entry);
        });

        factory
    }

    /// Put one entry into one recycled cell.
    fn dress(&self, cell: &gtk::Box, entry: &Entry) {
        let Some(art) = cell.first_child().and_downcast::<Art>() else {
            return;
        };
        let Some(name) = art.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(note) = name.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };

        let collectible = entry.collectible();
        let imp = self.imp();

        if let Some(images) = imp.images.borrow().as_ref() {
            art.show(images, entry.art().as_deref(), ART);
        }

        let notable = imp.notable.borrow().contains(&collectible.id);
        // Owned is only worth marking where both kinds are on screen. Under
        // "Collected" every entry is owned, so dashing the lot says nothing and
        // makes a page of six hundred harder to read than it needs to be.
        let spent = entry.owned() && imp.showing.get() != Showing::Collected;

        art.remove_css_class("al-notable");
        art.remove_css_class("al-spent");
        name.remove_css_class("al-gold");
        name.remove_css_class("al-spent");
        if notable {
            art.add_css_class("al-notable");
            name.add_css_class("al-gold");
        } else if spent {
            art.add_css_class("al-spent");
            name.add_css_class("al-spent");
        }

        name.set_label(if collectible.name.is_empty() {
            // An id with no name is an ownership sync that outran the
            // catalogue. Saying so beats drawing a blank cell.
            "Not named yet"
        } else {
            collectible.name.as_str()
        });
        name.set_tooltip_text(Some(&tooltip(&collectible)));

        // "Unobtainable" is only ever said of something not had. It means "you
        // cannot get this", which is not a thing to tell somebody about a mount
        // already in their collection — plenty of what a long-standing account
        // owns is no longer obtainable, and that is a boast rather than a
        // warning.
        note.set_label(match (spent, entry.owned(), entry.obtainable()) {
            (true, _, _) => "already owned",
            (_, false, false) => "unobtainable",
            _ => "",
        });
    }

    // -- driving the model -----------------------------------------------------

    /// Rebuild the filter from the current search text and toggle.
    ///
    /// The source is deliberately not in here: it is the *grouping*, and the
    /// group models each hang their own source filter off this one.
    fn reapply(&self) {
        let imp = self.imp();
        let Some(filter) = imp.filter.borrow().clone() else {
            return;
        };

        let needle = imp.needle.borrow().clone();
        let showing = imp.showing.get();

        filter.set_filter_func(move |object| {
            let Some(entry) = object.downcast_ref::<Entry>() else {
                return false;
            };
            let keep = match showing {
                Showing::All => true,
                Showing::Collected => entry.owned(),
                Showing::Missing => !entry.owned() && entry.obtainable(),
            };
            keep && entry.matches(&needle)
        });

        self.refresh_groups();
        self.retitle();
    }

    /// Show one source's own page, or go back to the contact sheet.
    fn choose_source(&self, source: Option<Source>) {
        let imp = self.imp();
        imp.source.set(source);

        if let Some(focused) = imp.focused.borrow().as_ref() {
            focused.set_filter_func(move |object| match source {
                None => false,
                Some(source) => object
                    .downcast_ref::<Entry>()
                    .is_some_and(|entry| entry.collectible().source == source),
            });
        }
        self.refresh_groups();
        self.retitle();
    }

    /// Every heading, count and rail row, from models that have already
    /// filtered themselves.
    fn refresh_groups(&self) {
        let imp = self.imp();
        let showing = imp.showing.get();
        let chosen = imp.source.get();
        let counted = showing.counted();

        let groups = imp.groups.borrow().clone();
        for group in &groups {
            let held = group.held.n_items();
            group.widget.set_visible(held > 0);
            group.count.set_label(&format!(
                "{} {counted}",
                almanac::thousands(u64::from(held))
            ));
            group.more.set_visible(held > GROUP_SHOWN);
            group
                .more
                .set_label(&format!("All {}", almanac::thousands(u64::from(held))));
        }

        // Eight of them, looked up by scanning. A map would want `Source: Hash`
        // and that is a derive on a model type for the sake of a widget.
        let held_by = |wanted: Source| {
            groups
                .iter()
                .find(|group| group.source == wanted)
                .map(|group| group.held.n_items())
                .unwrap_or(0)
        };

        for row in imp.rail_rows.borrow().iter() {
            let held = held_by(row.source);
            row.button.set_visible(held > 0);
            row.count.set_label(&almanac::thousands(u64::from(held)));

            let active = chosen == Some(row.source);
            for label in [&row.name, &row.count] {
                if active {
                    label.add_css_class("al-source-active");
                } else {
                    label.remove_css_class("al-source-active");
                }
            }
        }

        if let (Some(source), Some(heading_label), Some(count)) = (
            chosen,
            imp.focus_heading.borrow().clone(),
            imp.focus_count.borrow().clone(),
        ) {
            heading_label.set_label(heading(source));
            count.set_label(&format!(
                "{} {counted}",
                almanac::thousands(u64::from(held_by(source)))
            ));
        }
    }

    fn clear_filters(&self) {
        let imp = self.imp();
        imp.needle.replace(String::new());

        if let Some(entry) = imp.entry.borrow().as_ref() {
            entry.set_text("");
        }
        self.choose_source(None);
        self.reapply();
    }

    /// Swap between the two grids and whichever emptiness applies.
    fn retitle(&self) {
        let imp = self.imp();
        let held = imp
            .store
            .borrow()
            .as_ref()
            .map(|store| store.n_items())
            .unwrap_or(0);
        let showing = imp
            .sorted
            .borrow()
            .as_ref()
            .map(|model| model.n_items())
            .unwrap_or(0);

        if let Some(body) = imp.body.borrow().as_ref() {
            body.set_visible_child_name(match (held, showing, imp.source.get()) {
                (0, _, _) => "unsynced",
                (_, 0, _) => "empty",
                (_, _, Some(_)) => "source",
                _ => "groups",
            });
        }
        // A rail saying "0 of 0" over a page that has never synced reads as a
        // broken collection rather than an empty one, and the three cards above
        // an unsynced grid have nothing to be closest to.
        if let Some(split) = imp.split.borrow().as_ref() {
            split.set_show_sidebar(held > 0);
        }
        if let Some(block) = imp.closest_block.borrow().as_ref() {
            block.set_visible(held > 0 && !imp.closest.borrow().is_empty());
        }
    }

    // -- what the application hands over ---------------------------------------

    /// Draw a whole collection.
    pub fn show(
        &self,
        catalogue: &[Collectible],
        owned: &HashSet<u32>,
        faction: Faction,
        region: Region,
        attempts: &[Tally],
        chances: &Chances,
    ) {
        let imp = self.imp();
        imp.region.set(region);
        *imp.attempts.borrow_mut() = attempts.to_vec();
        *imp.chances.borrow_mut() = chances.clone();
        let Some(store) = imp.store.borrow().clone() else {
            return;
        };

        let entries: Vec<Entry> = catalogue
            .iter()
            .map(|collectible| {
                Entry::new(
                    collectible,
                    owned.contains(&collectible.id),
                    faction,
                    region,
                )
            })
            .collect();

        // Splice rather than remove-then-append: one signal, one relayout, and
        // the scroll position survives a redraw that changed nothing visible.
        store.splice(0, store.n_items(), &entries);

        self.pick_closest(&entries);
        self.set_counts(catalogue, owned, faction);
        self.reapply();
    }

    /// The three the page opens by recommending.
    ///
    /// Missing, obtainable, and ordered by how little chance stands in the way.
    /// See [`certainty`]: with no rarity source there is no "closest" that is a
    /// measurement, so this is the nearest honest thing — the entries whose
    /// cost is time or gold rather than a roll.
    fn pick_closest(&self, entries: &[Entry]) {
        let mut wanted: Vec<&Entry> = entries
            .iter()
            .filter(|entry| !entry.owned() && entry.obtainable())
            .collect();
        wanted.sort_by_key(|entry| {
            let collectible = entry.collectible();
            (
                certainty(collectible.source).unwrap_or(u8::MAX),
                collectible.name.to_lowercase(),
                collectible.id,
            )
        });

        let closest: Vec<Entry> = wanted.into_iter().take(CLOSEST).cloned().collect();
        let imp = self.imp();
        *imp.notable.borrow_mut() = closest.iter().map(|entry| entry.collectible().id).collect();
        *imp.closest.borrow_mut() = closest;
        self.draw_closest();
    }

    fn draw_closest(&self) {
        let imp = self.imp();
        let Some(cards) = imp.closest_cards.borrow().clone() else {
            return;
        };
        while let Some(child) = cards.first_child() {
            cards.remove(&child);
        }
        for entry in imp.closest.borrow().iter() {
            cards.append(&self.closest_card(entry));
        }
    }

    /// Fill in the art for entries whose picture had to be asked for.
    ///
    /// Toys and decor, in practice: both are items, and an item's art is
    /// addressed by a texture name nothing local knows. The map is item id to
    /// render URL and arrives from the application as the media calls land, a
    /// few at a time.
    pub fn set_art(&self, art: &HashMap<u32, String>) {
        if art.is_empty() {
            return;
        }
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };

        let mut landed = false;
        for position in 0..store.n_items() {
            let Some(entry) = store.item(position).and_downcast::<Entry>() else {
                continue;
            };
            if entry.art().is_some() {
                continue;
            }
            if let Some(url) = art.get(&entry.item_id()) {
                entry.set_art(url);
                landed = true;
                // The cell holding it is not necessarily realised, so this
                // nudges the model rather than the widget.
                store.items_changed(position, 1, 1);
            }
        }
        // The three cards are widgets rather than cells and no model change
        // reaches them, so a toy that has just earned an icon would otherwise
        // keep its placeholder until the next sync redrew the page.
        if landed {
            self.draw_closest();
        }
    }

    fn set_counts(&self, catalogue: &[Collectible], owned: &HashSet<u32>, faction: Faction) {
        let imp = self.imp();
        let collected = catalogue
            .iter()
            .filter(|entry| owned.contains(&entry.id))
            .count();
        let obtainable = catalogue
            .iter()
            .filter(|entry| !owned.contains(&entry.id) && entry.obtainable_by(faction))
            .count();
        let unobtainable = catalogue
            .len()
            .saturating_sub(collected)
            .saturating_sub(obtainable);

        // The denominator is what this account could ever hold, which is the
        // same rule the run applies to its own ring: counting the impossible
        // produces a bar that can never fill, and a bar that can never fill is
        // one nobody looks at twice. The line underneath says what was left
        // out, so the difference from the catalogue's own total is visible
        // rather than quietly applied.
        let countable = collected + obtainable;
        let fraction = if countable == 0 {
            0.0
        } else {
            collected as f64 / countable as f64
        };

        if let Some(figure) = imp.figure.borrow().as_ref() {
            figure.set_label(&almanac::thousands(collected as u64));
        }
        if let Some(denominator) = imp.denominator.borrow().as_ref() {
            denominator.set_label(&format!("of {}", almanac::thousands(countable as u64)));
        }
        if let Some(bar) = imp.bar.borrow().as_ref() {
            bar.set(fraction, 300);
        }
        if let Some(note) = imp.note.borrow().as_ref() {
            note.set_visible(unobtainable > 0);
            note.set_label(&format!(
                "{} more can never be taken again by this account, and are left out of the count.",
                almanac::thousands(unobtainable as u64)
            ));
        }
    }

    /// The search bar, so the window's header toggle can drive it.
    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    /// Choose which entries are shown: `missing`, `collected` or `all`.
    ///
    /// The rail's toggle is the way a person does this. This is the way a
    /// screenshot does it, so the collected state gets looked at as often as
    /// the one the page opens on.
    pub fn set_showing(&self, name: &str) {
        let wanted = Showing::from_name(name);
        let Some(index) = Showing::ALL.iter().position(|held| *held == wanted) else {
            return;
        };
        // Through the segment rather than around it: the toggle group is what
        // holds which one is on, and setting the field alone would leave the
        // rail claiming a view the grid is not showing.
        let Some(segments) = self.imp().segments.borrow().clone() else {
            return;
        };
        let mut child = segments.first_child();
        for _ in 0..index {
            child = child.and_then(|widget| widget.next_sibling());
        }
        if let Some(button) = child.and_downcast::<gtk::ToggleButton>() {
            button.set_active(true);
        }
    }

    /// Which entries have no picture yet and are worth asking Blizzard about.
    ///
    /// Capped by the caller, because this is one request per entry and a
    /// catalogue of two thousand toys would be two thousand of them.
    ///
    /// The page's own order first, then everything else. Which one matters
    /// depends on who is asking, and both callers are real:
    ///
    /// A **sync** has a budget of a hundred and twenty and wants them spent
    /// where somebody will see them — filtered how they filtered, grouped and
    /// ordered how the page reads, top down. The backing store's order is
    /// whatever `collapse_toys` left behind, which is highest item id first and
    /// has nothing to do with the page.
    ///
    /// **Fetch Missing Artwork** has no budget and means every picture that is
    /// missing, not every picture on the current view. The page opens on
    /// `Missing`, so stopping at the filtered model would quietly refuse to
    /// illustrate anything already collected — a third of the catalogue, and
    /// the half a person is most likely to go looking at.
    ///
    /// So the shown model decides the order and the store decides the extent.
    pub fn art_wanted(&self, limit: usize) -> Vec<u32> {
        let imp = self.imp();
        let shown = imp
            .sorted
            .borrow()
            .clone()
            .map(|model| model.upcast::<gtk::gio::ListModel>());
        let held = imp
            .store
            .borrow()
            .clone()
            .map(|store| store.upcast::<gtk::gio::ListModel>());

        let mut wanted = Vec::new();
        let mut seen = HashSet::new();
        for model in [shown, held].into_iter().flatten() {
            for position in 0..model.n_items() {
                if wanted.len() >= limit {
                    return wanted;
                }
                let Some(entry) = model.item(position).and_downcast::<Entry>() else {
                    continue;
                };
                if entry.art().is_some() {
                    continue;
                }
                // The same entry is in both models. Asking twice is a wasted
                // request against somebody's hourly quota.
                let item = entry.item_id();
                if item > 0 && seen.insert(item) {
                    wanted.push(item);
                }
            }
        }
        wanted
    }
}

/// The second line of one of the three cards: where the thing comes from, in
/// the journal's own words where there are any.
///
/// The addon records "Vendor: Unger Statforth / Zone: Wetlands" and the web API
/// records the word `VENDOR`, so this takes the sentence when there is one and
/// falls back to the word.
fn whence(collectible: &Collectible) -> String {
    let sentence = collectible
        .description
        .as_deref()
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    if sentence.is_empty() {
        collectible.source.label().to_string()
    } else {
        sentence
    }
}

/// What hovering a cell says.
///
/// The cell shows a name over one word. The journal's sentence — "Drop: Lord
/// Aurius Rivendare, Stratholme" — is the thing worth reading and does not fit,
/// so it is a tooltip rather than a truncation.
fn tooltip(collectible: &Collectible) -> String {
    let mut lines = vec![collectible.name.clone()];
    match collectible.description.as_deref() {
        Some(text) if !text.is_empty() => {
            lines.extend(text.lines().map(str::to_string));
        }
        _ if collectible.source != Source::Unknown => {
            lines.push(collectible.source.label().to_string());
        }
        _ => lines.push("No source recorded".to_string()),
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collectible(id: u32, name: &str, source: Source) -> Collectible {
        Collectible {
            kind: Kind::Mount,
            id,
            name: name.to_string(),
            source,
            description: None,
            flavour: None,
            icon: None,
            display: Some(1000 + id),
            faction: None,
            link_id: id,
            tradeable: None,
        }
    }

    #[test]
    fn a_tooltip_prefers_the_journals_sentence_to_blizzards_one_word() {
        let mut entry = collectible(6, "Brown Horse", Source::Vendor);
        assert_eq!(tooltip(&entry), "Brown Horse\nVendor");

        entry.description = Some("Vendor: Unger Statforth\nZone: Wetlands".into());
        assert_eq!(
            tooltip(&entry),
            "Brown Horse\nVendor: Unger Statforth\nZone: Wetlands"
        );
    }

    #[test]
    fn an_entry_with_no_source_says_so_rather_than_showing_the_word_unknown() {
        // "Unknown" reads as a property of the mount. It is a gap in Blizzard's
        // data, and the rail's caveat is where that is explained.
        let entry = collectible(12, "Reins of the Raven Lord", Source::Unknown);
        assert!(tooltip(&entry).ends_with("No source recorded"));
        assert_eq!(rail_label(Source::Unknown), "Unrecorded");
    }

    #[test]
    fn every_source_has_a_group_to_be_read_under() {
        // A source missing from the grouping is a set of entries with no
        // heading over them and no row in the rail — reachable only by search,
        // which is exactly the silent hole the old "Any source" escape hatch
        // existed to prevent.
        for source in [
            Source::Drop,
            Source::Vendor,
            Source::Quest,
            Source::Achievement,
            Source::Profession,
            Source::Pvp,
            Source::Promotion,
            Source::Unknown,
        ] {
            assert!(GROUPS.contains(&source), "{source:?} has no group");
        }
    }

    #[test]
    fn nothing_claims_a_chance_it_cannot_measure() {
        // The whole of the deviation from the design, pinned: a drop has no
        // gold line because Armory has no idea what it drops at, and a vendor
        // does because "you buy it" is true without a rate.
        assert_eq!(certainty(Source::Drop), None);
        assert_eq!(no_chance(Source::Drop), None);
        assert_eq!(no_chance(Source::Promotion), None);
        assert!(no_chance(Source::Vendor).is_some());
        // And the ranking puts the ones with no chance in them first.
        assert!(certainty(Source::Vendor) < certainty(Source::Pvp));
    }

    #[test]
    fn the_reset_day_is_the_regions_own() {
        // A collector plans a raid week around this, and quoting Tuesday at
        // somebody in Europe is worse than saying nothing.
        assert_eq!(reset_day(Region::Us), "TUESDAY");
        assert_eq!(reset_day(Region::Eu), "WEDNESDAY");
        assert_eq!(reset_day(Region::Kr), "THURSDAY");
    }
}
