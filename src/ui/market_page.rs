//! Prices, as three questions that are not the same question.
//!
//! **Browse** is the whole commodity file as it stands right now — tens of
//! thousands of rows and a search box. **Watching** is the only place in the
//! application where a line over time exists. **Crafting** is the money
//! question: what the account's recipe books are worth, what its spare pets are
//! worth, and what it is missing that somebody is selling.
//!
//! Splitting them is not decoration. A sparkline on a browse row would claim a
//! history that does not exist: browsing reads `snapshot`, which is *now* and
//! is replaced whole every hour, and only a watched item has a `price` series
//! behind it. Drawing the same chart on both would make the expensive half look
//! free and the free half look like evidence.
//!
//! Opt-in twice — per connected realm and per item — because five realms of
//! non-commodity auctions is roughly 3 GB a day of raw JSON, and ingesting all
//! of it to answer questions nobody asked is how a desktop application becomes
//! a service.
//!
//! Blizzard publishes no price history, so everything under Watching was
//! accumulated locally by diffing hourly snapshots. It has a 30-day horizon
//! because the API terms require one; the page says so rather than quietly
//! keeping more, and "we need more history" is answered by richer readings
//! inside the window rather than by older ones.
//!
//! Every figure here is the market's rather than the account's, so gold is
//! spent narrowly: the sorted column, the price of a thing the account could
//! actually make or sell, and nothing else.

use adw::prelude::*;
use adw::subclass::prelude::*;
use chrono::{DateTime, Utc};
use gtk::glib;
use std::cell::RefCell;
use std::collections::HashSet;

use super::almanac;
use crate::model::market::{self, Crafting, Listed, Making, Offer, Resale, Unmeasured};
use crate::model::source::blizzard::collections::Kind;

/// Called when someone stops watching an item, or a realm.
type UnwatchHandler = Box<dyn Fn(u32)>;
/// Called when someone asks to add a watch.
type AddHandler = Box<dyn Fn()>;
/// Told to start keeping a history for an item found by browsing.
type WatchHandler = Box<dyn Fn(u32, String)>;
/// Called with a name the browser could not match against what it holds.
type LookUpHandler = Box<dyn Fn(String)>;
/// Called with the realm whose market the browser should read.
type BrowseHandler = Box<dyn Fn(u32)>;

/// The three tabs, by the name their stack pages carry.
const TABS: [&str; 3] = ["browse", "watching", "crafting"];

/// How wide this page's rail is. Wider than the 288 the other pages use,
/// because all three of its rails carry a price book or a legend rather than a
/// standing.
const RAIL: f64 = 312.0;

/// A watched item's chart.
const SPARK: (i32, i32) = (150, 52);

/// The browser's column widths. The item name takes whatever is left.
const CHEAPEST: i32 = 92;
const LISTED: i32 = 120;
const SIZE: i32 = 104;
const SELLING: i32 = 96;

/// How many rows the browser puts in the list model at once.
///
/// A `GtkListView` recycles widgets and would hold the whole market happily,
/// but the *model* is rebuilt on every keystroke and thirty thousand
/// `GObject`s per character typed is what makes a search stop keeping up.
/// Somebody who has not narrowed past this has not found what they wanted yet.
const BROWSE_SHOWN: usize = 500;

/// How many bargains, spares and crafts to list.
///
/// A realm with a busy auction house can have a hundred missing pets up at
/// once, and a hundred rows is a list nobody reads.
const SHOWN_OFFERS: usize = 6;
const RANKED_SHOWN: usize = 12;

/// The 30-day ceiling, in days. A term of the API licence, not a cache policy,
/// and a series that has reached it says so rather than looking like a series
/// that happens to start there.
const TERM: i64 = 30;

/// What is known about one watched item on one realm.
pub struct Quote {
    pub item_id: u32,
    pub name: String,
    /// Realm id, or zero for region-wide commodities.
    pub realm: u32,
    pub realm_name: String,
    pub history: Vec<(DateTime<Utc>, u64, u32)>,
}

impl Quote {
    fn latest(&self) -> Option<(DateTime<Utc>, u64, u32)> {
        self.history.last().copied()
    }

    /// How the price has moved across what we hold.
    ///
    /// `None` when there is only one observation: a change needs two, and
    /// showing 0% for a single reading would claim a stable market we have not
    /// watched long enough to have seen.
    fn change(&self) -> Option<f64> {
        let first = self.history.first()?.1 as f64;
        let last = self.history.last()?.1 as f64;
        if self.history.len() < 2 || first == 0.0 {
            return None;
        }
        Some((last - first) / first)
    }

    /// The span actually observed, in whole days, never the thirty the store
    /// may keep. A realm watched since Tuesday has four days of evidence.
    fn days(&self) -> i64 {
        match (self.history.first(), self.history.last()) {
            (Some(first), Some(last)) => (last.0 - first.0).num_days().max(1),
            _ => 1,
        }
    }

    /// The same span in hours, which is what turns a count into a rate.
    ///
    /// Rounding an evening's worth of readings up to a day and dividing by that
    /// would report a market as a quarter as busy as it was.
    fn hours(&self) -> u32 {
        match (self.history.first(), self.history.last()) {
            (Some(first), Some(last)) => (last.0 - first.0).num_hours().max(1) as u32,
            _ => 1,
        }
    }

    fn since(&self) -> Option<DateTime<Utc>> {
        self.history.first().map(|(at, _, _)| *at)
    }

    /// Units that left the listings across the window.
    ///
    /// Only the falls, which is the same inference `record_prices` makes and
    /// the only one available: Blizzard records no sale anywhere, so stock
    /// disappearing between two snapshots is all there is. A quantity going up
    /// is somebody listing more and says nothing about demand.
    fn moved(&self) -> u32 {
        self.history
            .windows(2)
            .filter_map(|pair| pair[0].2.checked_sub(pair[1].2))
            .sum()
    }

    fn prices(&self) -> Vec<f64> {
        self.history
            .iter()
            .map(|(_, price, _)| *price as f64)
            .collect()
    }
}

mod imp {
    use super::*;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::collections::HashSet;

    #[derive(Default)]
    pub struct MarketPage {
        pub on_unwatch: RefCell<Option<super::UnwatchHandler>>,
        pub on_unwatch_realm: RefCell<Option<super::UnwatchHandler>>,
        pub on_add_item: RefCell<Option<super::AddHandler>>,
        pub on_add_realm: RefCell<Option<super::AddHandler>>,
        pub on_watch: RefCell<Option<super::WatchHandler>>,

        // -- the three tabs, and the one switcher that drives them -----------
        pub tabs: RefCell<Option<gtk::Stack>>,
        pub rails: RefCell<Option<gtk::Stack>>,
        /// What the toolbar carries to the right of the switcher, which is a
        /// different control on each tab.
        pub trailing: RefCell<Option<gtk::Stack>>,
        pub segments: RefCell<Vec<gtk::ToggleButton>>,
        /// The watch count, which is its own pill rather than part of a label.
        pub tally: RefCell<Option<gtk::Label>>,

        // -- browse ----------------------------------------------------------
        /// Asked to turn a name somebody typed into item ids.
        pub on_look_up: RefCell<Option<super::LookUpHandler>>,
        /// The last name looked up, so a keystroke does not ask twice.
        pub looked_up: RefCell<String>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub model: RefCell<Option<gtk::gio::ListStore>>,
        pub selection: RefCell<Option<gtk::SingleSelection>>,
        /// The table, and the empty state that stands in for it.
        pub table: RefCell<Option<gtk::Stack>>,
        pub footer: RefCell<Option<gtk::Label>>,
        pub realm_label: RefCell<Option<gtk::Label>>,
        /// The realm picker, and the menu rebuilt into it as realms come and go.
        pub realm_button: RefCell<Option<gtk::MenuButton>>,
        pub realm_menu: RefCell<Option<gtk::gio::Menu>>,
        /// Asked to browse a different realm.
        pub on_browse_realm: RefCell<Option<super::BrowseHandler>>,
        pub browse_rail: RefCell<Option<gtk::Box>>,
        /// The whole snapshot, held so a keystroke re-filters in memory rather
        /// than going back to the store thirty thousand rows at a time.
        pub market: RefCell<Vec<Listed>>,
        pub needle: RefCell<String>,
        pub browsing: Cell<u32>,
        pub realms: RefCell<Vec<(u32, String)>>,
        pub watched: RefCell<HashSet<u32>>,
        /// Whichever row's book the rail is showing.
        pub selected: RefCell<Option<Listed>>,

        // -- watching and crafting -------------------------------------------
        pub watching: RefCell<Option<gtk::Box>>,
        pub watching_rail: RefCell<Option<gtk::Box>>,
        pub crafting: RefCell<Option<gtk::Box>>,
        pub crafting_rail: RefCell<Option<gtk::Box>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MarketPage {
        const NAME: &'static str = "ArmoryMarketPage";
        type Type = super::MarketPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for MarketPage {}
    impl WidgetImpl for MarketPage {}
    impl BinImpl for MarketPage {}
}

/// One row of the browser, as a `GObject` so a list model can hold it.
///
/// The same shape as the collections page's `Entry` and for the same reason: a
/// realm's commodity market is tens of thousands of rows, and a `GtkListView`
/// over a list model recycles widgets where a box of rows would build every one
/// of them.
mod row {
    use super::*;

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct Row {
            pub listed: RefCell<Option<Listed>>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for Row {
            const NAME: &'static str = "ArmoryMarketRow";
            type Type = super::Row;
        }

        impl ObjectImpl for Row {}
    }

    glib::wrapper! {
        pub struct Row(ObjectSubclass<imp::Row>);
    }

    impl Row {
        pub fn new(listed: &Listed) -> Self {
            let row: Self = glib::Object::builder().build();
            *row.imp().listed.borrow_mut() = Some(listed.clone());
            row
        }

        pub fn listed(&self) -> Listed {
            self.imp().listed.borrow().clone().unwrap_or(Listed {
                item_id: 0,
                name: None,
                cheapest: 0,
                quantity: 0,
                listings: 0,
                tenth: 0,
                median: 0,
                sold: 0,
                span_hours: 0,
            })
        }
    }
}

use row::Row;

glib::wrapper! {
    pub struct MarketPage(ObjectSubclass<imp::MarketPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for MarketPage {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketPage {
    pub fn new() -> Self {
        let page: Self = glib::Object::builder().build();
        page.build();
        page
    }

    fn build(&self) {
        let imp = self.imp();

        // One switcher above three tabs, rather than a switcher per tab: they
        // are three questions about one market, and three separate controls
        // would have to be kept in step for no gain.
        let toolbar = almanac::row(12);
        toolbar.set_margin_top(14);
        toolbar.set_margin_bottom(12);
        toolbar.set_margin_start(28);
        toolbar.set_margin_end(24);
        toolbar.append(&self.switcher());

        // Each of the three stacks below sizes to the tab that is open, not to
        // the widest of the three. A homogeneous stack takes the maximum over
        // every child, so Watching's chart-and-price row was setting the
        // minimum width of Browse — and through it of the whole window, which
        // is how the rail ended up pushed off the right edge of the screen.
        let trailing = gtk::Stack::builder()
            .hexpand(true)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        trailing.add_named(&self.browse_controls(), Some("browse"));
        trailing.add_named(&self.watching_controls(), Some("watching"));
        trailing.add_named(&self.crafting_controls(), Some("crafting"));
        toolbar.append(&trailing);

        let tabs = gtk::Stack::builder()
            .vexpand(true)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        tabs.add_named(&self.browse_body(), Some("browse"));
        tabs.add_named(&self.scrolling("watching"), Some("watching"));
        tabs.add_named(&self.scrolling("crafting"), Some("crafting"));

        let main = almanac::column(0);
        main.add_css_class("al-main");
        main.append(&toolbar);
        main.append(&tabs);

        let rails = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        // Before the browse rail, because `draw_browse_rail` re-appends it and
        // cannot build what does not exist yet.
        self.realm_picker();
        rails.add_named(&self.rail("browse"), Some("browse"));
        rails.add_named(&self.rail("watching"), Some("watching"));
        rails.add_named(&self.rail("crafting"), Some("crafting"));

        *imp.tabs.borrow_mut() = Some(tabs);
        *imp.rails.borrow_mut() = Some(rails.clone());
        *imp.trailing.borrow_mut() = Some(trailing);

        self.set_child(Some(&almanac::split(&main, &rails, RAIL)));

        // Drawn once here, so a page nobody has handed data to yet is an empty
        // state rather than an empty rectangle.
        self.show(&[], &[], None, &[], &[], &Crafting::default());
        self.refill();
    }

    /// Three equal segments, and the watch count as a mark of its own.
    ///
    /// The count is a pill beside the word rather than part of it, because
    /// "Watching 14" reads as a label that grew a number and the number is the
    /// thing that changes.
    fn switcher(&self) -> gtk::Box {
        let page = self.clone();
        let track = almanac::segments(&["Browse", "Watching", "Crafting"], 0, move |index| {
            page.open_tab(TABS[index.min(TABS.len() - 1)]);
        });

        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        let mut child = track.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Ok(button) = widget.downcast::<gtk::ToggleButton>() {
                button.add_css_class("al-fixed");
                buttons.push(button);
            }
        }

        if let Some(button) = buttons.get(1) {
            let content = almanac::row(7);
            content.set_halign(gtk::Align::Center);
            content.append(&almanac::label("Watching", &[]));
            let tally = almanac::mono("0", &["al-chip"]);
            content.append(&tally);
            button.set_child(Some(&content));
            *self.imp().tally.borrow_mut() = Some(tally);
        }

        *self.imp().segments.borrow_mut() = buttons;
        // Centred rather than filling, so the switcher is the same height on
        // all three tabs whatever the control beside it happens to be.
        track.set_valign(gtk::Align::Center);
        track
    }

    /// Open one of the three. The switcher, the body and the rail all move
    /// together, so there is one call for it rather than three.
    fn open_tab(&self, name: &str) {
        let imp = self.imp();
        if let Some(tabs) = imp.tabs.borrow().as_ref() {
            tabs.set_visible_child_name(name);
        }
        if let Some(rails) = imp.rails.borrow().as_ref() {
            rails.set_visible_child_name(name);
        }
        if let Some(trailing) = imp.trailing.borrow().as_ref() {
            trailing.set_visible_child_name(name);
        }
        if let Some(index) = TABS.iter().position(|tab| *tab == name) {
            if let Some(button) = imp.segments.borrow().get(index) {
                button.set_active(true);
            }
        }
    }

    /// Show the browse half rather than the watch list.
    pub fn show_browsing(&self) {
        self.open_tab("browse");
    }

    /// No search bar for the header to drive, deliberately.
    ///
    /// Search is the primary control on Browse and it is on the page, always
    /// visible, in the toolbar. A `GtkSearchBar` around it was drawing its own
    /// background behind the field and standing the whole toolbar up taller
    /// than the switcher beside it — the row grew on Browse and shrank again on
    /// the other two tabs, which is a control that changes shape for no reason
    /// a person can see.
    ///
    /// Returning `None` also hides the header's search button on this page,
    /// which is right: a magnifying glass that reveals a field already on
    /// screen is a second control for one job.
    pub fn search(&self) -> Option<gtk::SearchBar> {
        None
    }

    /// Told to start keeping a history for an item somebody found by browsing.
    pub fn connect_watch<F: Fn(u32, String) + 'static>(&self, handler: F) {
        *self.imp().on_watch.borrow_mut() = Some(Box::new(handler));
    }

    /// Asked to turn a typed name into item ids.
    ///
    /// The auction house names nothing — a listing is an id — so a search over
    /// what the browser holds can only match the items whose names have already
    /// been fetched, one call at a time, in the background. Typing "copper ore"
    /// into a market of thirty thousand ids therefore found nothing at all,
    /// which is not a market with no copper ore in it; it is a market that has
    /// not been told what copper ore is called.
    ///
    /// Blizzard has an endpoint that goes the other way. This is the page
    /// asking for it.
    pub fn connect_look_up<F: Fn(String) + 'static>(&self, handler: F) {
        *self.imp().on_look_up.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_unwatched<F: Fn(u32) + 'static>(&self, handler: F) {
        *self.imp().on_unwatch.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_realm_unwatched<F: Fn(u32) + 'static>(&self, handler: F) {
        *self.imp().on_unwatch_realm.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_add_item<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_add_item.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_add_realm<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_add_realm.borrow_mut() = Some(Box::new(handler));
    }

    /// Told which market somebody wants to read.
    pub fn connect_browse_realm<F: Fn(u32) + 'static>(&self, handler: F) {
        *self.imp().on_browse_realm.borrow_mut() = Some(Box::new(handler));
    }

    // -- browse ---------------------------------------------------------------

    /// The toolbar's browse half: the search field, and nothing else.
    ///
    /// The realm picker used to sit here beside it and could not be read. This
    /// row is the widest thing on the widest page — the switcher alone is most
    /// of a main column's budget — so it is always at its floor at the default
    /// window size, and a control at its floor beside a search field at its
    /// floor is an ellipsis. The picker is in the rail, which is where the page
    /// keeps what it is showing rather than what to do with it.
    fn browse_controls(&self) -> gtk::Widget {
        let controls = almanac::row(12);

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search the market")
            .hexpand(true)
            // A floor rather than a size, and `hexpand` is what decides how
            // wide it actually is — on any real window this field is the widest
            // thing in the row. The floor is only ever reached where there is
            // nothing to spare, and there it is paid for by the whole window:
            // this row is the widest thing on the widest page, so every
            // character here is a character the window cannot give back.
            // Twelve was a comfortable size wearing a floor's clothes.
            .width_chars(4)
            .build();
        let page = self.clone();
        entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            *page.imp().needle.borrow_mut() = text.clone();
            page.refill();
            page.look_up(&text);
        });
        controls.append(&entry);

        *self.imp().entry.borrow_mut() = Some(entry);
        controls.upcast()
    }

    /// Which market is being read, and the way to read another one.
    ///
    /// A menu rather than a `GtkDropDown`, because a drop-down sizes its button
    /// to the widest name in the list and the rail is pinned at
    /// [`RAIL`] — a realm with a long name would push the price book about.
    /// Built once and re-appended, because the rail is torn down and rebuilt
    /// every time the selected row changes and the picker is not about the row.
    fn realm_picker(&self) -> gtk::MenuButton {
        let realm = almanac::label("", &["al-caption"]);
        realm.set_valign(gtk::Align::Center);
        realm.set_xalign(0.0);
        realm.set_hexpand(true);
        realm.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let menu = gtk::gio::Menu::new();
        let button = gtk::MenuButton::builder()
            .child(&realm)
            .tooltip_text("Which market to browse")
            .menu_model(&menu)
            .always_show_arrow(true)
            .build();
        button.add_css_class("flat");

        // An action group on the page rather than on the application, because
        // which market is being *looked* at is this page's business — the
        // application is told after the fact, the same way the search field
        // tells it. The target is the realm id, so one action answers however
        // many realms somebody watches.
        let actions = gtk::gio::SimpleActionGroup::new();
        let choose = gtk::gio::SimpleAction::new("realm", Some(glib::VariantTy::UINT32));
        let page = self.clone();
        choose.connect_activate(move |_, target| {
            let Some(realm) = target.and_then(glib::Variant::get::<u32>) else {
                return;
            };
            if let Some(handler) = page.imp().on_browse_realm.borrow().as_ref() {
                handler(realm);
            }
        });
        actions.add_action(&choose);
        self.insert_action_group("market", Some(&actions));

        let imp = self.imp();
        *imp.realm_label.borrow_mut() = Some(realm);
        *imp.realm_button.borrow_mut() = Some(button.clone());
        *imp.realm_menu.borrow_mut() = Some(menu);
        button
    }

    /// Rebuild the realm menu, and say which market is open.
    ///
    /// Region-wide first and always, because it is the commodity market — every
    /// stackable trade good in the game is there and on no realm — and because
    /// an account that watches no realm at all still has it. The realms follow
    /// in the order they were watched.
    ///
    /// A realm's own listings are shown *with* the region-wide ones, so these
    /// are not exclusive choices: picking a realm adds its gear to the
    /// commodities rather than replacing them, which is what the auction house
    /// in the game does. The caption says which realm, not which set.
    fn draw_realm_picker(&self, realms: &[(u32, String)]) {
        let imp = self.imp();
        let browsing = imp.browsing.get();

        if let Some(menu) = imp.realm_menu.borrow().as_ref() {
            menu.remove_all();
            for (realm, name) in
                std::iter::once(&(0, "Region-wide commodities".to_string())).chain(realms)
            {
                let item = gtk::gio::MenuItem::new(Some(name), None);
                item.set_action_and_target_value(Some("market.realm"), Some(&realm.to_variant()));
                menu.append_item(&item);
            }
        }

        let title = realms
            .iter()
            .find(|(realm, _)| *realm == browsing)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "Region-wide commodities".to_string());
        if let Some(label) = imp.realm_label.borrow().as_ref() {
            label.set_label(&title);
            label.set_tooltip_text(Some(&title));
        }
        // Nothing to choose between with no realm watched, and a menu of one is
        // a control that cannot do anything. The picker earns its place the
        // moment a second market exists.
        if let Some(button) = imp.realm_button.borrow().as_ref() {
            button.set_sensitive(!realms.is_empty());
        }
    }

    /// The whole file as it stands: a header, a table, and what it is showing.
    fn browse_body(&self) -> gtk::Widget {
        let column = almanac::column(0);
        column.set_margin_start(28);

        column.append(&Self::table_header());

        let model = gtk::gio::ListStore::new::<Row>();
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        let view = gtk::ListView::new(Some(selection.clone()), Some(Self::table_factory()));
        view.add_css_class("al-table");
        view.set_vexpand(true);

        // The rail follows the selection rather than an activation: a row is
        // read, not opened, and the book beside it is the reading.
        let page = self.clone();
        selection.connect_selected_item_notify(move |selection| {
            let listed = selection
                .selected_item()
                .and_downcast::<Row>()
                .map(|row| row.listed());
            *page.imp().selected.borrow_mut() = listed;
            page.draw_browse_rail();
        });

        let table = gtk::Stack::builder()
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        table.add_named(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&view)
                .vexpand(true)
                .build(),
            Some("rows"),
        );
        table.add_named(
            &adw::StatusPage::builder()
                .icon_name("network-server-symbolic")
                .title("No snapshot yet")
                .description(
                    "The region's commodity file is downloaded whole every sync and \
                     costs nothing extra — it is the response the auction house data \
                     already arrives in. Sign in, or add a realm, and this fills.",
                )
                .vexpand(true)
                .build(),
            Some("none"),
        );
        column.append(&table);

        let footer = almanac::mono("", &["al-footnote"]);
        footer.set_margin_top(9);
        footer.set_margin_bottom(12);
        footer.set_margin_start(12);
        column.append(&footer);

        let imp = self.imp();
        *imp.model.borrow_mut() = Some(model);
        *imp.selection.borrow_mut() = Some(selection);
        *imp.table.borrow_mut() = Some(table);
        *imp.footer.borrow_mut() = Some(footer);
        column.upcast()
    }

    /// The column headings, and which column the table is in the order of.
    ///
    /// Only one column is sorted and it is not clickable, because the ordering
    /// is written once — see [`MarketPage::ordered`] — and a sorter per column
    /// would be a second implementation of the same comparison.
    fn table_header() -> gtk::Box {
        let header = almanac::row(12);
        header.add_css_class("al-table-row");
        header.set_baseline_position(gtk::BaselinePosition::Bottom);

        let item = almanac::mono("ITEM", &["al-column-header"]);
        item.set_hexpand(true);
        header.append(&item);

        for (title, width, sorted) in [
            ("CHEAPEST", CHEAPEST, false),
            ("LISTED", LISTED, false),
            // The caret marks the one ordering there is: the median unit price
            // times the quantity listed, which is how much market is actually
            // here. The floor alone puts one fantasy listing at three million
            // gold at the top of the page.
            ("MARKET SIZE ▾", SIZE, true),
            ("SELLING", SELLING, false),
        ] {
            let label = almanac::mono(
                title,
                if sorted {
                    &["al-column-header", "al-gold"]
                } else {
                    &["al-column-header"]
                },
            );
            label.set_xalign(1.0);
            label.set_size_request(width, -1);
            header.append(&label);
        }
        header
    }

    /// One row of the table: five cells on one baseline.
    ///
    /// The mixed faces are the reason for the baseline — a 13px name beside a
    /// 12px monospaced price sit on different centres and reading down a column
    /// of them is what a table is for.
    fn table_factory() -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();

        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let line = almanac::row(12);
            line.add_css_class("al-table-row");
            line.set_baseline_position(gtk::BaselinePosition::Center);

            let name = almanac::label("", &[]);
            name.set_hexpand(true);
            name.set_valign(gtk::Align::Baseline);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            line.append(&name);

            for (width, classes) in [
                (CHEAPEST, "al-price"),
                (LISTED, "al-caption"),
                (SIZE, "al-price"),
                (SELLING, "al-caption"),
            ] {
                let cell = almanac::mono("", &[classes]);
                cell.set_xalign(1.0);
                cell.set_valign(gtk::Align::Baseline);
                cell.set_size_request(width, -1);
                line.append(&cell);
            }

            // The class rather than the stock selection state, so the row the
            // rail is reading is marked in the page's own accent.
            let row = line.clone();
            item.connect_selected_notify(move |item| {
                if item.is_selected() {
                    row.add_css_class("al-selected");
                } else {
                    row.remove_css_class("al-selected");
                }
            });
            item.set_child(Some(&line));
        });

        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(row), Some(line)) = (
                item.item().and_downcast::<Row>(),
                item.child().and_downcast::<gtk::Box>(),
            ) else {
                return;
            };
            let listed = row.listed();

            let mut cells = Vec::new();
            let mut child = line.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Ok(label) = widget.downcast::<gtk::Label>() {
                    cells.push(label);
                }
            }
            if cells.len() < 5 {
                return;
            }

            cells[0].set_label(&listed.title());
            // An item the auction house has only ever given an id for. Said in
            // the mono face rather than hidden: an id is what somebody pasting
            // from a wiki has, and it is searchable.
            if listed.name.is_none() {
                cells[0].add_css_class("al-mono");
                cells[0].add_css_class("al-unknown");
            } else {
                cells[0].remove_css_class("al-mono");
                cells[0].remove_css_class("al-unknown");
            }

            cells[1].set_label(&gold(listed.cheapest));
            cells[2].set_label(&format!(
                "{} in {}",
                almanac::thousands(u64::from(listed.quantity)),
                almanac::thousands(u64::from(listed.listings))
            ));
            cells[3].set_label(&gold(listed.depth()));
            cells[3].add_css_class("al-gold");

            // "not watched" is the honest state and the argument for watching:
            // only a watched item has the history this column reads.
            cells[4].set_label(&selling(&listed));
            if listed.span_hours == 0 {
                cells[4].add_css_class("al-unwatched");
                cells[4].remove_css_class("al-gold");
            } else {
                cells[4].remove_css_class("al-unwatched");
                cells[4].add_css_class("al-gold");
            }

            if item.is_selected() {
                line.add_css_class("al-selected");
            } else {
                line.remove_css_class("al-selected");
            }
        });

        factory
    }

    /// Hand the browser one market: a realm's own listings and the region-wide
    /// commodities with them.
    pub fn show_market(&self, realm: u32, market: &[Listed], watched: &HashSet<u32>) {
        let imp = self.imp();
        imp.browsing.set(realm);
        *imp.market.borrow_mut() = market.to_vec();
        *imp.watched.borrow_mut() = watched.clone();
        let realms = imp.realms.borrow().clone();
        self.draw_realm_picker(&realms);
        self.refill();
    }

    /// The market, filtered by what was typed and in the one order there is.
    ///
    /// The filter is `market::browse`. The order is here, written once and read
    /// by both the table and [`MarketPage::wants_names`], rather than by a
    /// sorter per column — two orderings of one list is how they come to
    /// disagree, and there is nothing for a person to click that could make
    /// them.
    fn ordered(&self) -> Vec<Listed> {
        let imp = self.imp();
        let market = imp.market.borrow();
        let mut rows = market::browse(&market, &imp.needle.borrow());
        rows.sort_by(|a, b| {
            b.median
                .saturating_mul(u64::from(b.quantity))
                .cmp(&a.median.saturating_mul(u64::from(a.quantity)))
                .then_with(|| a.title().to_lowercase().cmp(&b.title().to_lowercase()))
        });
        rows
    }

    /// Put the filtered, ordered market back into the list model.
    fn refill(&self) {
        let imp = self.imp();
        let Some(model) = imp.model.borrow().clone() else {
            return;
        };

        let total = imp.market.borrow().len();
        let rows = self.ordered();
        let shown = rows.len().min(BROWSE_SHOWN);
        let objects: Vec<Row> = rows.iter().take(BROWSE_SHOWN).map(Row::new).collect();
        model.splice(0, model.n_items(), &objects);

        if let Some(table) = imp.table.borrow().as_ref() {
            table.set_visible_child_name(if total == 0 { "none" } else { "rows" });
        }
        if let Some(entry) = imp.entry.borrow().as_ref() {
            entry.set_placeholder_text(Some(&format!(
                "Search {} items on the market",
                almanac::thousands(total as u64)
            )));
        }
        if let Some(footer) = imp.footer.borrow().as_ref() {
            let needle = imp.needle.borrow().trim().is_empty();
            footer.set_label(&match (needle, rows.len() > BROWSE_SHOWN) {
                (true, true) => format!(
                    "SHOWING {} OF {} — SEARCH TO REACH THE REST",
                    almanac::thousands(shown as u64),
                    almanac::thousands(total as u64)
                ),
                (true, false) => format!(
                    "SHOWING {} OF {}",
                    almanac::thousands(shown as u64),
                    almanac::thousands(total as u64)
                ),
                (false, true) => format!(
                    "{} OF {} MATCH — NARROW IT TO REACH THE REST",
                    almanac::thousands(rows.len() as u64),
                    almanac::thousands(total as u64)
                ),
                (false, false) => format!(
                    "{} OF {} MATCH",
                    almanac::thousands(rows.len() as u64),
                    almanac::thousands(total as u64)
                ),
            });
        }

        // Splicing does not re-announce the selection, and the rail has to be
        // about a row that is still on screen.
        let selected = imp
            .selection
            .borrow()
            .as_ref()
            .and_then(|selection| selection.selected_item())
            .and_downcast::<Row>()
            .map(|row| row.listed());
        *imp.selected.borrow_mut() = selected;
        self.draw_browse_rail();
    }

    /// Which items on the page still have no name.
    ///
    /// The order matters and is the whole reason this reads the *filtered,
    /// ordered* list rather than the store: the budget is a hundred and fifty
    /// names a sync against a market of tens of thousands, and spending it on
    /// whatever the database happened to return first scatters names through a
    /// page with no pattern and leaves the top of it blank for weeks. The same
    /// argument as `art_wanted`.
    ///
    /// The page's own order is market size descending, so what it asks for
    /// first is also what is being traded most — which is what somebody will
    /// type the name of.
    pub fn wants_names(&self, budget: usize) -> Vec<u32> {
        self.ordered()
            .into_iter()
            .filter(|listed| listed.name.is_none())
            .take(budget)
            .map(|listed| listed.item_id)
            .collect()
    }

    /// Ask what a typed name is, when the rows cannot answer it themselves.
    ///
    /// Only when the search finds nothing. A market that already holds a
    /// matching name needs no request, and the overwhelming majority of
    /// searches are for something the browser can already see.
    ///
    /// Debounced, because this hangs off every keystroke and "copper ore" is
    /// eleven of them. The timeout re-reads the field rather than capturing the
    /// text, so a person still typing never spends a request on a prefix.
    fn look_up(&self, text: &str) {
        let needle = text.trim().to_lowercase();
        // Below three characters the search endpoint's twenty-five results are
        // a random corner of the game rather than an answer.
        if needle.len() < 3 {
            return;
        }
        if !self.rows_for(&needle).is_empty() {
            return;
        }

        let page = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(450), move || {
            let imp = page.imp();
            let still = imp.needle.borrow().trim().to_lowercase();
            if still != needle || *imp.looked_up.borrow() == needle {
                return;
            }
            imp.looked_up.replace(needle.clone());
            if let Some(handler) = imp.on_look_up.borrow().as_ref() {
                handler(needle);
            }
        });
    }

    /// The rows a needle matches, out of the whole snapshot.
    fn rows_for(&self, needle: &str) -> Vec<Listed> {
        crate::model::market::browse(&self.imp().market.borrow(), needle)
    }

    /// Show one item's book in the rail.
    ///
    /// Filling the rail rather than pushing a page: browsing is a column of
    /// rows being compared, and navigating away from the comparison to read one
    /// of its rows is the wrong shape for the question.
    pub fn open_item(&self, listed: &Listed) {
        self.open_tab("browse");

        // Move the selection with it where the row is on screen, or the table
        // would be marking one item while the rail read another.
        let found = self
            .imp()
            .selection
            .borrow()
            .as_ref()
            .and_then(|selection| {
                (0..selection.n_items())
                    .find(|index| {
                        selection
                            .item(*index)
                            .and_downcast::<Row>()
                            .is_some_and(|row| row.listed().item_id == listed.item_id)
                    })
                    .map(|index| (selection.clone(), index))
            });

        match found {
            Some((selection, index)) => selection.set_selected(index),
            None => {
                *self.imp().selected.borrow_mut() = Some(listed.clone());
                self.draw_browse_rail();
            }
        }
    }

    /// The selected row's order book — the whole reason `Depth` keeps more than
    /// a floor.
    fn draw_browse_rail(&self) {
        let Some(rail) = self.imp().browse_rail.borrow().clone() else {
            return;
        };
        while let Some(child) = rail.first_child() {
            rail.remove(&child);
        }

        // Which market, first and whatever else the rail is showing. It is the
        // one thing here that is true of the whole page rather than of the row
        // somebody happens to have clicked, so it does not come and go with the
        // selection.
        rail.append(&almanac::section("MARKET"));
        if let Some(picker) = self.imp().realm_button.borrow().as_ref() {
            rail.append(picker);
        }
        rail.append(&almanac::hairline());

        let Some(listed) = self.imp().selected.borrow().clone() else {
            rail.append(&almanac::section("NOTHING CHOSEN"));
            rail.append(&almanac::caption(
                "Choose a row and its book — what one costs, what a tenth of the \
                 stock costs, and where the real middle is — is drawn here.",
            ));
            rail.append(&Self::rail_footer("WHOLE FILE REPLACED HOURLY"));
            return;
        };

        let head = almanac::column(9);
        head.append(&almanac::section(&listed.title().to_uppercase()));
        head.append(&almanac::caption(
            "The shape of the book, not just its first row.",
        ));
        rail.append(&head);

        let card = almanac::card(11);
        card.append(&Self::book_line(
            "One costs",
            &gold(listed.cheapest),
            &["al-figure"],
        ));
        card.append(&Self::book_line(
            "Through the cheap tenth",
            &gold(listed.tenth),
            &["al-price"],
        ));
        card.append(&Self::book_line(
            "The real middle",
            &gold(listed.median),
            &["al-price"],
        ));
        card.append(&almanac::hairline());
        card.append(&Self::book_line(
            "Units listed",
            &almanac::thousands(u64::from(listed.quantity)),
            &["al-price"],
        ));
        card.append(&Self::book_line(
            "Across auctions",
            &almanac::thousands(u64::from(listed.listings)),
            &["al-price"],
        ));
        rail.append(&card);

        // The bar is the median, cut where the floor and the cheap tenth fall.
        // Mostly gold is a book with no spread in it; mostly track is a floor
        // that one hopeful listing is holding down.
        let middle = listed.median.max(1) as f64;
        let floor = (listed.cheapest as f64 / middle).clamp(0.0, 1.0);
        let tenth = (listed.tenth as f64 / middle).clamp(floor, 1.0);
        rail.append(&almanac::depth([floor, tenth - floor, 1.0 - tenth]));

        let captions = almanac::row(6);
        for (text, align) in [
            ("ONE COSTS", gtk::Align::Start),
            ("CHEAP TENTH", gtk::Align::Center),
            ("THE MIDDLE", gtk::Align::End),
        ] {
            let label = almanac::mono(text, &["al-footnote"]);
            label.set_hexpand(true);
            label.set_halign(align);
            captions.append(&label);
        }
        rail.append(&captions);

        if self.imp().watched.borrow().contains(&listed.item_id) {
            rail.append(&almanac::mono(
                "WATCHED — ITS HISTORY IS BEING KEPT",
                &["al-meta", "al-gold"],
            ));
        } else {
            let watch = gtk::Button::builder().label("Watch this item").build();
            watch.add_css_class("al-gold-button");
            let page = self.clone();
            let id = listed.item_id;
            let name = listed.title();
            watch.connect_clicked(move |_| {
                if let Some(handler) = page.imp().on_watch.borrow().as_ref() {
                    handler(id, name.clone());
                }
            });
            rail.append(&watch);

            rail.append(&almanac::caption(
                "Nothing before you ask can be recovered. Blizzard publishes no history \
                 at all, so the first snapshot after you press this is where yours \
                 starts.",
            ));
        }

        rail.append(&Self::rail_footer("WHOLE FILE REPLACED HOURLY"));
    }

    /// A label and a figure on one line, at the size that line deserves.
    fn book_line(name: &str, value: &str, classes: &[&str]) -> gtk::Box {
        let line = almanac::row(8);
        line.set_baseline_position(gtk::BaselinePosition::Center);
        let label = almanac::label(name, &["al-caption"]);
        label.set_hexpand(true);
        label.set_valign(gtk::Align::Baseline);
        line.append(&label);
        let figure = almanac::mono(value, classes);
        figure.set_halign(gtk::Align::End);
        figure.set_valign(gtk::Align::Baseline);
        line.append(&figure);
        line
    }

    // -- watching -------------------------------------------------------------

    fn watching_controls(&self) -> gtk::Widget {
        let controls = almanac::row(9);
        controls.set_halign(gtk::Align::End);
        controls.set_hexpand(true);

        let item = gtk::Button::builder().label("Watch an Item…").build();
        let page = self.clone();
        item.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_add_item.borrow().as_ref() {
                handler();
            }
        });
        controls.append(&item);

        let realm = gtk::Button::builder().label("Add a Realm…").build();
        let page = self.clone();
        realm.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_add_realm.borrow().as_ref() {
                handler();
            }
        });
        controls.append(&realm);
        controls.upcast()
    }

    /// The watch list, grouped by the market each item is priced on.
    ///
    /// Region-wide commodities first because they are the market everybody has
    /// — the per-realm groups are the ones somebody opted into.
    fn draw_watching(&self, quotes: &[Quote], realms: &[(u32, String)]) {
        let Some(column) = self.imp().watching.borrow().clone() else {
            return;
        };
        while let Some(child) = column.first_child() {
            column.remove(&child);
        }

        if quotes.is_empty() {
            column.append(&self.nothing_watched(realms.is_empty()));
            return;
        }

        let mut groups: Vec<(u32, String)> = vec![(0, "REGION-WIDE COMMODITIES".to_string())];
        groups.extend(realms.iter().map(|(id, name)| (*id, name.to_uppercase())));
        // A quote on a realm that is no longer followed still has a history and
        // still has to be reachable, so it keeps a group of its own rather than
        // disappearing.
        for quote in quotes {
            if !groups.iter().any(|(id, _)| *id == quote.realm) {
                groups.push((quote.realm, quote.realm_name.to_uppercase()));
            }
        }

        for (realm, name) in groups {
            let mut rows: Vec<&Quote> = quotes.iter().filter(|q| q.realm == realm).collect();
            if rows.is_empty() {
                continue;
            }
            rows.sort_by_key(|quote| quote.name.to_lowercase());

            let cards = almanac::column(9);
            for quote in rows {
                cards.append(&self.watch_row(quote));
            }
            column.append(&almanac::titled(&name, &cards));
        }
    }

    /// One watched item: what it has been doing, and where it ended up.
    fn watch_row(&self, quote: &Quote) -> gtk::Box {
        let card = almanac::card(0);
        let line = almanac::row(12);
        line.set_valign(gtk::Align::Center);

        let text = almanac::column(4);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);

        let title = almanac::label(&quote.name, &["al-row-title"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&title);

        let days = quote.days();
        let since = quote
            .since()
            .map(|at| at.format("%-d %B").to_string().to_uppercase())
            .unwrap_or_else(|| "TODAY".to_string());
        let meta = almanac::meta(&if days >= TERM {
            // The ceiling is a term of the licence, not a cache policy, and a
            // series sitting on it is at the oldest reading there will ever be.
            format!("WATCHING SINCE {since} · {TERM} DAYS HELD — THE TERM'S CEILING")
        } else {
            format!(
                "WATCHING SINCE {since} · {} OBSERVED",
                almanac::plural(days as usize, "DAY", "DAYS").to_uppercase()
            )
        });
        if quote.history.len() < 2 {
            meta.add_css_class("al-gold");
        }
        text.append(&meta);

        if quote.history.len() < 2 {
            let note = almanac::caption(
                "Two readings is not a trend. Nothing before you asked can be recovered.",
            );
            note.set_margin_top(5);
            text.append(&note);
        } else {
            let figures = almanac::row(16);
            figures.set_margin_top(5);
            if let Some((_, _, quantity)) = quote.latest() {
                figures.append(&almanac::caption(&format!(
                    "{} listed",
                    almanac::thousands(u64::from(quantity))
                )));
            }
            figures.append(&almanac::caption(&moving(quote.moved(), quote.hours())));
            text.append(&figures);
        }
        line.append(&text);

        // A chart, or the reason there is not one. A flat line drawn from a
        // single reading is a lie a chart tells very convincingly.
        if quote.history.len() < 2 {
            let empty = almanac::card(0);
            empty.add_css_class("al-unmeasured");
            empty.set_size_request(SPARK.0, SPARK.1);
            let label = almanac::mono("NOT ENOUGH\nHISTORY YET", &["al-footnote"]);
            label.set_justify(gtk::Justification::Center);
            label.set_halign(gtk::Align::Center);
            label.set_valign(gtk::Align::Center);
            label.set_hexpand(true);
            empty.append(&label);
            line.append(&empty);
        } else {
            line.append(&almanac::spark(quote.prices(), SPARK.0, SPARK.1));
        }

        let right = almanac::column(5);
        right.set_size_request(86, -1);
        right.set_valign(gtk::Align::Center);
        let price = almanac::mono(
            &quote
                .latest()
                .map(|(_, price, _)| gold(price))
                .unwrap_or_else(|| "—".to_string()),
            &["al-figure"],
        );
        price.set_halign(gtk::Align::End);
        right.append(&price);

        // Coloured by direction, which is what the series did. It is not advice
        // — a price going up is good news for a seller and bad for a buyer, and
        // nothing here knows which one is reading.
        let change = match quote.change() {
            Some(change) => almanac::mono(
                &format!(
                    "{}{:.1}%",
                    if change > 0.0 { "+" } else { "−" },
                    change.abs() * 100.0
                ),
                &[
                    "al-caption",
                    if change > 0.0 {
                        "al-positive"
                    } else {
                        "al-negative"
                    },
                ],
            ),
            None => almanac::mono("—", &["al-caption"]),
        };
        change.set_halign(gtk::Align::End);
        right.append(&change);
        line.append(&right);

        line.append(&self.drop_item(quote.item_id));
        card.append(&line);
        card
    }

    /// The token, the auction houses being fetched, and the horizon.
    fn draw_watching_rail(&self, quotes: &[Quote], realms: &[(u32, String)], token: Option<u64>) {
        let Some(rail) = self.imp().watching_rail.borrow().clone() else {
            return;
        };
        while let Some(child) = rail.first_child() {
            rail.remove(&child);
        }

        if let Some(price) = token {
            let block = almanac::column(9);
            block.append(&almanac::section("WOW TOKEN"));
            block.append(&almanac::mono(&gold(price), &["al-figure-large"]));
            // No chart: the token arrives as one current price with no series
            // behind it, and the rule that governs every other row here governs
            // this one too.
            block.append(&almanac::caption(
                "Region-wide, and the one price Blizzard sets rather than players.",
            ));
            rail.append(&block);
            rail.append(&almanac::hairline());
        }

        let houses = almanac::column(7);
        let commodities = almanac::row(8);
        commodities.append(&almanac::label("Region-wide commodities", &[]));
        let always = almanac::mono("ALWAYS", &["al-footnote"]);
        always.set_hexpand(true);
        always.set_halign(gtk::Align::End);
        commodities.append(&always);
        houses.append(&commodities);

        for (id, name) in realms {
            let line = almanac::row(8);
            line.set_valign(gtk::Align::Center);
            line.append(&almanac::label(name, &[]));

            // The date the realm was added is not recorded anywhere, so this is
            // the oldest price still held for it — which is a floor on the
            // same fact and is said as what it is.
            let oldest = quotes
                .iter()
                .filter(|quote| quote.realm == *id)
                .filter_map(Quote::since)
                .min();
            let when = almanac::mono(
                &match oldest {
                    Some(at) => format!(
                        "PRICES SINCE {}",
                        at.format("%-d %b").to_string().to_uppercase()
                    ),
                    None => "NO PRICES YET".to_string(),
                },
                &["al-footnote"],
            );
            when.set_hexpand(true);
            when.set_halign(gtk::Align::End);
            line.append(&when);
            line.append(&self.drop_realm(*id));
            houses.append(&line);
        }
        rail.append(&almanac::titled("AUCTION HOUSES", &houses));

        rail.append(&almanac::hairline());
        rail.append(&almanac::caption(
            "History has a thirty-day horizon because the API terms require one. It is \
             a term of the licence rather than a cache policy — so the answer to \"we \
             need more\" is richer readings inside the window, never older ones.",
        ));
        rail.append(&Self::rail_footer(
            "SALES INFERRED FROM STOCK\nDISAPPEARING BETWEEN SNAPSHOTS",
        ));
    }

    // -- crafting -------------------------------------------------------------

    fn crafting_controls(&self) -> gtk::Widget {
        let note = almanac::mono("RANKED BY MARGIN × WHAT IS ACTUALLY SELLING", &["al-meta"]);
        note.set_halign(gtk::Align::End);
        note.set_valign(gtk::Align::Center);
        note.set_hexpand(true);
        note.upcast()
    }

    /// What to make, what to sell, and what to buy before it is gone.
    fn draw_crafting(&self, crafting: &Crafting, resale: &[Resale], offers: &[Offer]) {
        let Some(column) = self.imp().crafting.borrow().clone() else {
            return;
        };
        while let Some(child) = column.first_child() {
            column.remove(&child);
        }

        // First, because it is the only thing on this page that expires. A
        // price history keeps; a listing does not.
        if !offers.is_empty() {
            let realms = self.imp().realms.borrow().clone();
            let named: std::collections::HashMap<u32, &str> = realms
                .iter()
                .map(|(id, name)| (*id, name.as_str()))
                .collect();

            let cards: Vec<gtk::Box> = offers
                .iter()
                .take(SHOWN_OFFERS)
                .map(|offer| {
                    Self::compact(
                        match offer.kind {
                            Kind::Mount => "starred-symbolic",
                            Kind::Pet => "emblem-favorite-symbolic",
                            Kind::Toy => "applications-games-symbolic",
                            Kind::Decor => "user-home-symbolic",
                        },
                        &offer.name,
                        &format!(
                            "{} on {}",
                            almanac::plural(offer.quantity as usize, "listed", "listed"),
                            named.get(&offer.realm).copied().unwrap_or("the region")
                        ),
                        &gold(offer.unit_price),
                    )
                })
                .collect();

            let block = almanac::column(9);
            block.append(&Self::pairs(cards));
            if offers.len() > SHOWN_OFFERS {
                block.append(&almanac::caption(&format!(
                    "and {} more — narrow it down in the collection pages",
                    offers.len() - SHOWN_OFFERS
                )));
            }
            column.append(&almanac::titled("MISSING, AND FOR SALE", &block));
        }

        let Crafting { worth, unmeasured } = crafting;

        // The one recipe with the fattest paper margin, when the ranking has
        // put something else above it. That disagreement is the feature, so it
        // is said on the card rather than left to be noticed.
        let fattest = worth
            .iter()
            .take(RANKED_SHOWN)
            .enumerate()
            .max_by_key(|(_, entry)| entry.margin)
            .map(|(index, _)| index);

        if !worth.is_empty() {
            let cards = almanac::column(9);
            for (index, entry) in worth.iter().take(RANKED_SHOWN).enumerate() {
                cards.append(&Self::making_card(
                    entry,
                    index == 0,
                    if fattest == Some(index) && index > 0 {
                        Some(index)
                    } else {
                        None
                    },
                ));
            }
            if worth.len() > RANKED_SHOWN {
                cards.append(&almanac::caption(&format!(
                    "and {} more",
                    worth.len() - RANKED_SHOWN
                )));
            }
            column.append(&cards);
        }

        // Said out loud rather than left as a shorter list. A recipe whose
        // reagents have no price is not a bad flip, it is one nobody has
        // priced, and quietly dropping it presents a subset as the whole.
        let short = unmeasured.missing_reagent + unmeasured.missing_output;
        if short > 0 {
            let card = almanac::card(4);
            card.add_css_class("al-unmeasured");
            card.append(&almanac::label(
                &format!(
                    "{} could not be priced",
                    almanac::plural(short, "recipe", "recipes")
                ),
                &["al-row-title"],
            ));
            card.append(&almanac::caption(
                "Something they need, or the thing they make, has not been seen on a \
                 watched realm. They are counted here rather than quietly dropped — \
                 watch the realm you craft on and they fill in.",
            ));
            column.append(&card);
        }

        if worth.is_empty() && *unmeasured == Unmeasured::default() {
            let card = almanac::card(4);
            card.add_css_class("al-unmeasured");
            card.append(&almanac::label("No recipe books yet", &["al-row-title"]));
            card.append(&almanac::caption(
                "Open each character's profession window once. The game will not tell \
                 an addon what somebody can make until they do.",
            ));
            column.append(&card);
        }

        if !resale.is_empty() {
            let cards: Vec<gtk::Box> = resale
                .iter()
                .take(SHOWN_OFFERS)
                .map(|entry| {
                    let card = Self::compact(
                        "emblem-favorite-symbolic",
                        &entry.name,
                        &format!(
                            "{} · {}",
                            almanac::plural(entry.spare as usize, "spare", "spares"),
                            moving(entry.sold, entry.span_hours)
                        ),
                        // The cheapest quality's price, deliberately. Armory
                        // knows the quality of every pet listed and of none in
                        // your own journal, so this is the figure that holds
                        // whichever the spare turns out to be.
                        &gold(entry.floor),
                    );
                    if entry.ceiling > entry.floor {
                        let spread = almanac::mono(
                            &format!("up to {}", gold(entry.ceiling)),
                            &["al-footnote"],
                        );
                        spread.set_halign(gtk::Align::End);
                        card.append(&spread);
                    }
                    card
                })
                .collect();

            let block = almanac::column(9);
            block.append(&Self::pairs(cards));
            if resale.len() > SHOWN_OFFERS {
                block.append(&almanac::caption(&format!(
                    "and {} more",
                    resale.len() - SHOWN_OFFERS
                )));
            }
            column.append(&almanac::titled("SPARES WORTH SELLING", &block));
        }
    }

    /// One craft worth making, and who should make it.
    fn making_card(entry: &Making, top: bool, paper: Option<usize>) -> gtk::Box {
        let card = if top {
            almanac::earned_card(0)
        } else {
            almanac::card(0)
        };
        // The ranking's own argument, at the weight it deserves: a fat margin
        // nobody buys is still on the page, and still fourth.
        if paper.is_some() {
            card.set_opacity(0.82);
        }

        let line = almanac::row(15);
        line.set_valign(gtk::Align::Center);

        let text = almanac::column(4);
        text.set_hexpand(true);

        let heading = almanac::row(8);
        heading.set_baseline_position(gtk::BaselinePosition::Center);
        let name = almanac::label(&entry.name, &["al-row-title"]);
        name.set_valign(gtk::Align::Baseline);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        heading.append(&name);
        if entry.makes > 1 {
            let makes = almanac::mono(&format!("×{}", entry.makes), &["al-caption"]);
            makes.set_valign(gtk::Align::Baseline);
            heading.append(&makes);
        }
        text.append(&heading);

        text.append(&almanac::caption(&format!(
            "{} · {} · {} in reagents · {}",
            entry.by_name,
            entry.realm_name,
            gold(entry.cost),
            moving(entry.sold, entry.span_hours)
        )));

        // Shown, and deliberately not taken off the cost — the addon's Warband
        // bag indices have never been confirmed against a stocked bank, and a
        // wrong index has to look wrong rather than inflate a margin.
        if !entry.held.is_empty() {
            let held = almanac::mono(
                &format!(
                    "{} IN THE WARBAND BANK — NOT TAKEN OFF THE COST",
                    almanac::plural(entry.held.len(), "REAGENT", "REAGENTS").to_uppercase()
                ),
                &["al-price", "al-gold"],
            );
            held.set_wrap(true);
            text.append(&held);
        }

        if let Some(index) = paper {
            text.append(&almanac::mono(
                &format!(
                    "BIG MARGIN, ALMOST NO BUYERS — WHICH IS WHY IT IS {}",
                    ordinal(index)
                ),
                &["al-meta"],
            ));
        }
        line.append(&text);

        let right = almanac::column(5);
        right.set_size_request(120, -1);
        right.set_valign(gtk::Align::Center);
        let margin = almanac::mono(
            &format!("+{}", gold(entry.margin.max(0) as u64)),
            &["al-figure"],
        );
        margin.set_halign(gtk::Align::End);
        right.append(&margin);
        let note = almanac::label("each, after the cut", &["al-caption"]);
        note.set_halign(gtk::Align::End);
        right.append(&note);
        line.append(&right);

        card.append(&line);
        card
    }

    /// How the ranking works, whose books it read, and what it assumed.
    fn draw_crafting_rail(&self, crafting: &Crafting) {
        let Some(rail) = self.imp().crafting_rail.borrow().clone() else {
            return;
        };
        while let Some(child) = rail.first_child() {
            rail.remove(&child);
        }

        let ranked = almanac::column(8);
        let sentence = almanac::caption("");
        // The multiplication sign is the whole argument, so it carries the
        // accent. Pango markup rather than a class because it is one glyph
        // inside a sentence; the colour is read when the rail is drawn.
        sentence.set_markup(&format!(
            "Margin <span foreground=\"{}\">×</span> what has actually been selling, \
             not margin. A four-hundred-gold profit on something nobody buys is forty \
             unsold flasks.",
            ink(almanac::Palette::current().gold_text)
        ));
        ranked.append(&sentence);
        ranked.append(&almanac::caption(
            "Sale volume is inferred from stock disappearing between hourly snapshots. \
             Blizzard records no sale anywhere.",
        ));
        rail.append(&almanac::titled("HOW THIS IS RANKED", &ranked));

        rail.append(&almanac::hairline());

        // Whose books answered. The addon reads a recipe book one profession
        // window at a time, so a character with nothing here is silence rather
        // than a character who can make nothing.
        let books = almanac::column(8);
        let mut names: Vec<(String, usize)> = Vec::new();
        for entry in &crafting.worth {
            match names.iter_mut().find(|(name, _)| *name == entry.by_name) {
                Some((_, count)) => *count += 1,
                None => names.push((entry.by_name.clone(), 1)),
            }
        }
        names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        if names.is_empty() {
            books.append(&almanac::caption("No book has been read yet."));
        }
        for (name, count) in &names {
            let line = almanac::row(9);
            line.set_valign(gtk::Align::Center);
            line.append(&read_dot());
            let label = almanac::label(name, &[]);
            label.set_hexpand(true);
            line.append(&label);
            let count = almanac::mono(&format!("{count} WORTH MAKING"), &["al-footnote"]);
            count.set_halign(gtk::Align::End);
            line.append(&count);
            books.append(&line);
        }
        books.append(&almanac::caption(
            "Open each character's profession window once. The game will not tell an \
             addon what somebody can make until you do, and there is no way around it.",
        ));
        rail.append(&almanac::titled("RECIPE BOOKS", &books));

        rail.append(&almanac::hairline());

        let assumes = almanac::column(8);
        for line in [
            "A one-star craft, every time. Quality depends on skill, specialisation and \
             luck, and Armory reads none of them.",
            "Reagents at the cheapest quality that has a price, minus the auction \
             house's five percent.",
            "Warband stock is shown beside a row and never subtracted — the bag indices \
             are unconfirmed, and a wrong number you can see beats one folded into a \
             margin.",
        ] {
            assumes.append(&almanac::caption(line));
        }
        rail.append(&almanac::titled("WHAT THESE FIGURES ASSUME", &assumes));
    }

    // -- the page -------------------------------------------------------------

    pub fn show(
        &self,
        quotes: &[Quote],
        realms: &[(u32, String)],
        token: Option<u64>,
        offers: &[Offer],
        resale: &[Resale],
        crafting: &Crafting,
    ) {
        *self.imp().realms.borrow_mut() = realms.to_vec();

        if let Some(tally) = self.imp().tally.borrow().as_ref() {
            let mut items: Vec<u32> = quotes.iter().map(|quote| quote.item_id).collect();
            items.sort_unstable();
            items.dedup();
            tally.set_label(&items.len().to_string());
        }

        self.draw_realm_picker(realms);

        self.draw_watching(quotes, realms);
        self.draw_watching_rail(quotes, realms, token);
        self.draw_crafting(crafting, resale, offers);
        self.draw_crafting_rail(crafting);
    }

    /// One tab's scrolling body, held so it can be redrawn.
    fn scrolling(&self, tab: &str) -> gtk::Widget {
        let column = almanac::column(14);
        column.set_valign(gtk::Align::Start);
        column.set_margin_bottom(24);
        column.set_margin_start(28);
        column.set_margin_end(24);

        let imp = self.imp();
        match tab {
            "watching" => *imp.watching.borrow_mut() = Some(column.clone()),
            _ => *imp.crafting.borrow_mut() = Some(column.clone()),
        }

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build();
        scroller.add_css_class("al-main");
        scroller.upcast()
    }

    /// One tab's rail, held so it can be redrawn.
    fn rail(&self, tab: &str) -> gtk::Widget {
        let column = almanac::rail_column();
        let imp = self.imp();
        match tab {
            "browse" => *imp.browse_rail.borrow_mut() = Some(column.clone()),
            "watching" => *imp.watching_rail.borrow_mut() = Some(column.clone()),
            _ => *imp.crafting_rail.borrow_mut() = Some(column.clone()),
        }
        almanac::rail_pane(&column).upcast()
    }

    /// The last line of a rail: what the page is standing on.
    fn rail_footer(text: &str) -> gtk::Label {
        let label = almanac::mono(text, &["al-footnote"]);
        label.set_margin_top(6);
        label.set_justify(gtk::Justification::Left);
        label
    }

    /// A small card: an icon, a name, a note, and a price.
    fn compact(icon: &str, title: &str, subtitle: &str, price: &str) -> gtk::Box {
        let card = almanac::card(4);
        let line = almanac::row(11);
        line.set_valign(gtk::Align::Center);

        let image = gtk::Image::from_icon_name(icon);
        image.set_valign(gtk::Align::Center);
        line.append(&image);

        let text = almanac::column(2);
        text.set_hexpand(true);
        let name = almanac::label(title, &[]);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name);
        text.append(&almanac::caption(subtitle));
        line.append(&text);

        let figure = almanac::mono(price, &["al-price", "al-gold"]);
        figure.set_halign(gtk::Align::End);
        figure.set_valign(gtk::Align::Center);
        line.append(&figure);

        card.append(&line);
        card
    }

    /// Compact cards, two to a line.
    fn pairs(cards: Vec<gtk::Box>) -> gtk::Grid {
        let grid = gtk::Grid::builder()
            .column_spacing(9)
            .row_spacing(9)
            .column_homogeneous(true)
            .build();
        for (index, card) in cards.into_iter().enumerate() {
            grid.attach(&card, index as i32 % 2, index as i32 / 2, 1, 1);
        }
        grid
    }

    fn nothing_watched(&self, no_realms: bool) -> gtk::Widget {
        let status = adw::StatusPage::builder()
            .icon_name("network-server-symbolic")
            .title("Nothing watched yet")
            .description(if no_realms {
                "Browse shows the whole market as it stands right now, and costs \
                 nothing — but only a watched item gets a *history*. Blizzard publishes \
                 none at all, so the first snapshot after you ask is where yours starts, \
                 and the days before it cannot be recovered. Add a realm to follow gear, \
                 pets and recipes there too."
            } else {
                "Browse shows the whole market as it stands right now. Watching an item \
                 is what starts recording its price — Blizzard publishes no history, so \
                 the first snapshot after you ask is where yours starts, and a trend \
                 needs a few hours before it means anything."
            })
            .vexpand(true)
            .build();

        let add = gtk::Button::builder()
            .label("Watch an Item")
            .halign(gtk::Align::Center)
            .build();
        add.add_css_class("pill");
        add.add_css_class("suggested-action");

        let page = self.clone();
        add.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_add_item.borrow().as_ref() {
                handler();
            }
        });
        status.set_child(Some(&add));
        status.upcast()
    }

    /// Stop watching an item.
    fn drop_item(&self, item_id: u32) -> gtk::Button {
        let drop = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text("Stop watching this item")
            .valign(gtk::Align::Center)
            .build();
        drop.add_css_class("flat");

        let page = self.clone();
        drop.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_unwatch.borrow().as_ref() {
                handler(item_id);
            }
        });
        drop
    }

    /// Stop fetching a realm's auction house.
    fn drop_realm(&self, realm: u32) -> gtk::Button {
        let drop = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text("Stop fetching this realm")
            .valign(gtk::Align::Center)
            .build();
        drop.add_css_class("flat");

        let page = self.clone();
        drop.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_unwatch_realm.borrow().as_ref() {
                handler(realm);
            }
        });
        drop
    }
}

/// A character whose profession window has been opened, so their book was read.
///
/// Drawn rather than styled: the mark is gold because the book answered, and
/// gold is not something the class-coloured dot beside a name can be.
fn read_dot() -> gtk::DrawingArea {
    let dot = gtk::DrawingArea::builder()
        .content_width(7)
        .content_height(7)
        .valign(gtk::Align::Center)
        .build();
    dot.set_draw_func(|_, context, width, height| {
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        almanac::Palette::current().gold.apply(context);
        context.arc(cx, cy, cx.min(cy), 0.0, std::f64::consts::TAU);
        let _ = context.fill();
    });
    dot
}

/// A colour as Pango wants it, which is not how CSS wants it.
fn ink(colour: almanac::Ink) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (colour.0 * 255.0).round() as u8,
        (colour.1 * 255.0).round() as u8,
        (colour.2 * 255.0).round() as u8
    )
}

/// Where a recipe landed in the ranking, in words.
///
/// "Which is why it is fourth" is a sentence; "which is why it is 4th" is a
/// readout, and this line exists to be read as an argument.
fn ordinal(index: usize) -> String {
    const WORDS: [&str; 12] = [
        "FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH", "SIXTH", "SEVENTH", "EIGHTH", "NINTH",
        "TENTH", "ELEVENTH", "TWELFTH",
    ];
    WORDS
        .get(index)
        .map(|word| (*word).to_string())
        .unwrap_or_else(|| format!("{}TH", index + 1))
}

/// What the browser's last column says.
///
/// Only a watched item can answer at all, and "not watched" is the honest state
/// rather than a blank — it is also the whole argument for watching one.
fn selling(listed: &Listed) -> String {
    if listed.span_hours == 0 {
        return "not watched".to_string();
    }
    let per_day = market::per_day(listed.sold, listed.span_hours);
    if per_day >= 1.0 {
        format!("{}/day", almanac::thousands(per_day.round() as u64))
    } else if listed.sold > 0 {
        format!(
            "{} in {}d",
            almanac::thousands(u64::from(listed.sold)),
            (f64::from(listed.span_hours) / 24.0).round().max(1.0) as u64
        )
    } else {
        "none moved".to_string()
    }
}

/// How fast something has been moving, said as a rate.
///
/// A rate rather than a count, because a count is meaningless without the span
/// it covers — eighteen sold is a busy market over an hour and a dead one over
/// a month. Below one a day the count is given instead: "0.3 a day" is a
/// precision the evidence does not support, and "4 sold in 9 days" is what was
/// actually seen.
///
/// Approximate throughout, and said so elsewhere on the page. Blizzard records
/// no sale anywhere, so this is stock that stopped being listed and some of it
/// was cancelled.
fn moving(sold: u32, span_hours: u32) -> String {
    if sold == 0 {
        return "nothing seen to move".to_string();
    }
    let per_day = market::per_day(sold, span_hours);
    if per_day >= 1.0 {
        format!(
            "{} a day moving",
            almanac::thousands(per_day.round() as u64)
        )
    } else {
        let days = (f64::from(span_hours.max(1)) / 24.0).round().max(1.0) as usize;
        format!(
            "{} sold in {}",
            almanac::thousands(u64::from(sold)),
            almanac::plural(days, "day", "days")
        )
    }
}

/// Copper as gold, the way the game writes it.
pub fn gold(copper: u64) -> String {
    let gold = copper / 10_000;
    let silver = (copper % 10_000) / 100;
    if gold > 0 {
        format!("{}g {silver}s", almanac::thousands(gold))
    } else {
        format!("{silver}s {}c", copper % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copper_reads_as_gold_and_silver() {
        assert_eq!(gold(1_234_500), "123g 45s");
        assert_eq!(gold(4_500), "45s 0c");
    }

    fn quote(prices: &[u64]) -> Quote {
        let at = Utc::now();
        Quote {
            item_id: 1,
            name: "Mycobloom".into(),
            realm: 0,
            realm_name: "Region-wide".into(),
            history: prices
                .iter()
                .enumerate()
                .map(|(index, price)| (at + chrono::Duration::hours(index as i64), *price, 10))
                .collect(),
        }
    }

    #[test]
    fn one_observation_is_not_a_trend() {
        // Showing 0% would claim a stable market we have not watched long
        // enough to have seen.
        assert_eq!(quote(&[100]).change(), None);
        assert_eq!(quote(&[]).change(), None);
    }

    #[test]
    fn a_change_is_measured_across_what_is_held() {
        let change = quote(&[100, 150]).change().expect("a change");
        assert!((change - 0.5).abs() < 1e-9);

        let change = quote(&[200, 100]).change().expect("a change");
        assert!((change + 0.5).abs() < 1e-9);
    }

    #[test]
    fn only_stock_that_disappeared_counts_as_having_moved() {
        // The same inference `record_prices` makes, and the only one available:
        // a quantity going up is somebody listing more and says nothing about
        // demand. Counting it would make a stagnant market look busy.
        let at = Utc::now();
        let held = |quantities: &[u32]| Quote {
            item_id: 1,
            name: "Mycobloom".into(),
            realm: 0,
            realm_name: "Region-wide".into(),
            history: quantities
                .iter()
                .enumerate()
                .map(|(index, quantity)| {
                    (at + chrono::Duration::hours(index as i64), 100, *quantity)
                })
                .collect(),
        };

        assert_eq!(held(&[10, 6, 9, 7]).moved(), 6);
        assert_eq!(held(&[1, 5]).moved(), 0);
        assert_eq!(held(&[4]).moved(), 0, "one reading is no evidence at all");
    }

    #[test]
    fn a_span_is_what_was_watched_and_never_the_term() {
        // Thirty days is the most the store may keep. A series four hours long
        // is four hours of evidence, and dividing by thirty days quotes a
        // number nobody measured.
        assert_eq!(quote(&[100, 110, 120, 130]).days(), 1);
        assert_eq!(quote(&[]).days(), 1);

        // Four hourly readings are three hours of evidence, and a rate has to
        // be over those rather than over the day they are rounded up to.
        assert_eq!(quote(&[100, 110, 120, 130]).hours(), 3);
        assert_eq!(
            quote(&[100]).hours(),
            1,
            "dividing by nothing is not an option"
        );
    }

    #[test]
    fn a_rate_below_one_a_day_is_given_as_the_count_that_was_seen() {
        assert_eq!(moving(0, 240), "nothing seen to move");
        assert_eq!(moving(4, 216), "4 sold in 9 days");
        assert_eq!(moving(240, 240), "24 a day moving");
    }
}
