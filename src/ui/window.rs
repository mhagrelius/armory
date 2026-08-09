//! The window: a sidebar of places, and whichever one is open.
//!
//! This was four tabs in a view switcher. Four is already the ceiling for a
//! switcher and the collection was one of them — which meant mounts, pets and
//! toys shared a single scrolling page and none of them could have a search
//! box, a filter bar or a count of its own. A sidebar has no such ceiling, and
//! it lets each collection carry its own progress on the row that opens it,
//! which is the number a collector came to see.
//!
//! Onboarding is not in the sidebar. Until there is an account there is nothing
//! for any of these to show, and a list of six empty places invites someone to
//! press them and find out. So setup is a separate face of the window
//! altogether, and the sidebar appears with the data it navigates.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::{
    ChroniclePage, CollectionPage, Images, MarketPage, Onboarding, ReputationsPage, RosterPage,
    RunPage, ZonePage,
};
use crate::model::source::blizzard::collections::Kind;

/// The sidebar's rows, in order, paired with the stack page each one opens.
///
/// One list rather than two parallel ones: the sidebar reports a flattened
/// index across its sections, and any drift between that order and the stack's
/// names is a row that opens the wrong page.
///
/// Chronicle sits beside Run in the first section rather than under Account,
/// because those two are the pages about what a person did; everything between
/// them is about what the account has.
const PLACES: [(&str, &str, &str); 10] = [
    ("run", "Run", "media-playlist-repeat-symbolic"),
    ("chronicle", "Chronicle", "document-edit-symbolic"),
    ("zones", "Zones", "map-symbolic"),
    ("mounts", "Mounts", "starred-symbolic"),
    ("pets", "Pets", "emblem-favorite-symbolic"),
    ("toys", "Toys", "applications-games-symbolic"),
    ("decor", "Decor", "user-home-symbolic"),
    ("roster", "Roster", "system-users-symbolic"),
    ("reputations", "Reputations", "emblem-shared-symbolic"),
    ("market", "Market", "network-server-symbolic"),
];

/// The stack page name for one collection.
fn place_of(kind: Kind) -> &'static str {
    match kind {
        Kind::Mount => "mounts",
        Kind::Pet => "pets",
        Kind::Toy => "toys",
        Kind::Decor => "decor",
    }
}

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct ArmoryWindow {
        pub toasts: RefCell<Option<adw::ToastOverlay>>,
        /// Setup, or the application proper.
        pub faces: RefCell<Option<gtk::Stack>>,
        pub split: RefCell<Option<adw::NavigationSplitView>>,
        pub sidebar: RefCell<Option<adw::Sidebar>>,
        pub stack: RefCell<Option<adw::ViewStack>>,
        pub title: RefCell<Option<adw::WindowTitle>>,

        pub sync: RefCell<Option<gtk::Button>>,
        pub spinner: RefCell<Option<adw::Spinner>>,
        pub find: RefCell<Option<gtk::ToggleButton>>,
        /// Opens the page's rail when the window is too narrow to keep it
        /// beside the content. Hidden at every other width, because there is
        /// nothing for it to do when the rail is already on screen.
        pub rail_button: RefCell<Option<gtk::ToggleButton>>,
        /// Back to the roster from a character. Hidden everywhere else.
        pub back: RefCell<Option<gtk::Button>>,
        pub banner: RefCell<Option<adw::Banner>>,

        pub onboarding: RefCell<Option<Onboarding>>,
        pub roster: RefCell<Option<RosterPage>>,
        pub run: RefCell<Option<RunPage>>,
        pub chronicle: RefCell<Option<ChroniclePage>>,
        pub zones: RefCell<Option<ZonePage>>,
        pub market: RefCell<Option<MarketPage>>,
        pub reputations: RefCell<Option<ReputationsPage>>,
        pub collections: RefCell<Vec<CollectionPage>>,

        /// Which search bar the header's toggle currently drives, if any.
        pub searching: RefCell<Option<gtk::SearchBar>>,
        pub handler: RefCell<Option<glib::SignalHandlerId>>,
        pub collapsed: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ArmoryWindow {
        const NAME: &'static str = "ArmoryWindow";
        type Type = super::ArmoryWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for ArmoryWindow {}
    impl WidgetImpl for ArmoryWindow {}
    impl WindowImpl for ArmoryWindow {}
    impl ApplicationWindowImpl for ArmoryWindow {}
    impl AdwApplicationWindowImpl for ArmoryWindow {}
}

glib::wrapper! {
    pub struct ArmoryWindow(ObjectSubclass<imp::ArmoryWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native,
                    gtk::Root, gtk::ShortcutManager, gtk::gio::ActionGroup, gtk::gio::ActionMap;
}

impl ArmoryWindow {
    pub fn new(application: &impl IsA<gtk::Application>, images: &Images) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.build(images);
        window
    }

    fn build(&self, images: &Images) {
        self.set_title(Some("Armory"));
        // Wide enough for the sidebar, a main column and the page's rail beside
        // it, tall enough for four rows of a mount grid. Sixty pixels wider than
        // it was, which is what the rail costs and what the design is drawn at.
        self.set_default_size(1180, 800);

        let run = RunPage::new(images);
        let chronicle = ChroniclePage::new();
        let zones = ZonePage::new();
        let roster = RosterPage::new(images);
        let market = MarketPage::new();
        let reputations = ReputationsPage::new();
        let onboarding = Onboarding::new();
        let collections: Vec<CollectionPage> = Kind::ALL
            .into_iter()
            .map(|kind| CollectionPage::new(kind, images))
            .collect();

        let stack = adw::ViewStack::new();
        stack.add_named(&run, Some("run"));
        stack.add_named(&chronicle, Some("chronicle"));
        stack.add_named(&zones, Some("zones"));
        for page in &collections {
            stack.add_named(page, Some(place_of(page.kind())));
        }
        stack.add_named(&roster, Some("roster"));
        stack.add_named(&reputations, Some("reputations"));
        stack.add_named(&market, Some("market"));
        stack.set_vexpand(true);
        // Size to the page that is open, not to the widest of the ten.
        //
        // A homogeneous stack measures every child and takes the maximum, so
        // the market's five-column table set the minimum width of the window
        // for the Run page as well — the rail was pushed off the right edge of
        // a window nothing on screen needed to be that wide. Each place now
        // asks for what it needs and no more.
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);

        let banner = adw::Banner::builder().revealed(false).build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        content.append(&banner);
        content.append(&stack);

        let imp = self.imp();
        *imp.stack.borrow_mut() = Some(stack);
        *imp.banner.borrow_mut() = Some(banner);
        *imp.onboarding.borrow_mut() = Some(onboarding.clone());
        *imp.roster.borrow_mut() = Some(roster);
        *imp.run.borrow_mut() = Some(run);
        *imp.chronicle.borrow_mut() = Some(chronicle);
        *imp.zones.borrow_mut() = Some(zones);
        *imp.market.borrow_mut() = Some(market);
        *imp.reputations.borrow_mut() = Some(reputations);
        *imp.collections.borrow_mut() = collections;

        let split = adw::NavigationSplitView::builder()
            .sidebar(&self.sidebar_pane())
            .content(&self.content_pane(&content))
            .min_sidebar_width(200.0)
            .max_sidebar_width(260.0)
            .build();

        let setup = adw::ToolbarView::builder().content(&onboarding).build();
        setup.add_top_bar(
            &adw::HeaderBar::builder()
                .title_widget(&adw::WindowTitle::new("Armory", "Setup"))
                .build(),
        );

        let faces = gtk::Stack::new();
        faces.add_named(&setup, Some("setup"));
        faces.add_named(&split, Some("main"));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&faces));
        self.set_content(Some(&toasts));

        *imp.toasts.borrow_mut() = Some(toasts);
        *imp.faces.borrow_mut() = Some(faces);
        *imp.split.borrow_mut() = Some(split);

        self.install_breakpoint();

        // The header follows the roster's own navigation, whichever way it was
        // driven — the card, the back button, Escape or a swipe.
        let window = self.clone();
        self.roster_page()
            .connect_navigated(move |showing| window.show_character_header(showing));
        self.show_onboarding(true);
        self.open("run");
    }

    /// The list of places, and the counts that make it worth glancing at.
    fn sidebar_pane(&self) -> adw::NavigationPage {
        let sidebar = adw::Sidebar::new();

        let first = adw::SidebarSection::new();
        let collection = adw::SidebarSection::new();
        collection.set_title(Some("Collection"));
        let account = adw::SidebarSection::new();
        account.set_title(Some("Account"));

        for (index, (name, title, icon)) in PLACES.iter().enumerate() {
            let item = adw::SidebarItem::builder()
                .title(*title)
                .icon_name(*icon)
                .build();

            // A count on the row is the difference between a list of places and
            // a collection's standing at a glance. Filled in by `set_tally`
            // once there is anything to count.
            if matches!(*name, "mounts" | "pets" | "toys" | "decor") {
                let tally = gtk::Label::new(None);
                tally.add_css_class("dimmed");
                tally.add_css_class("caption");
                tally.add_css_class("tabular");
                item.set_suffix(Some(&tally));
            }

            match index {
                0..=1 => first.append(item),
                2..=5 => collection.append(item),
                _ => account.append(item),
            }
        }

        sidebar.append(first);
        sidebar.append(collection);
        sidebar.append(account);

        let window = self.clone();
        sidebar.connect_activated(move |_, index| {
            if let Some((name, _, _)) = PLACES.get(index as usize) {
                window.open(name);
            }
        });

        *self.imp().sidebar.borrow_mut() = Some(sidebar.clone());

        let view = adw::ToolbarView::builder().content(&sidebar).build();
        view.add_top_bar(
            &adw::HeaderBar::builder()
                .title_widget(&adw::WindowTitle::new("Armory", ""))
                .build(),
        );

        adw::NavigationPage::builder()
            .title("Armory")
            .child(&view)
            .build()
    }

    /// Whichever place is open, with the controls that act on it.
    fn content_pane(&self, content: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
        let title = adw::WindowTitle::new("Run", "");

        let sync = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Sync with Battle.net")
            .action_name("app.sync")
            .build();

        let spinner = adw::Spinner::builder().visible(false).build();

        let find = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text("Search this page")
            .visible(false)
            .build();

        let rail_button = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-right-symbolic")
            .tooltip_text("Show this page's standing")
            .visible(false)
            .build();
        rail_button.connect_toggled(|button| super::almanac::show_rails(button.is_active()));

        let menu = gtk::gio::Menu::new();
        let syncing = gtk::gio::Menu::new();
        syncing.append(Some("Sync"), Some("app.sync"));
        syncing.append(Some("Fetch Missing Artwork"), Some("app.fetch-art"));
        menu.append_section(None, &syncing);

        let journal = gtk::gio::Menu::new();
        journal.append(Some("Write Every Entry"), Some("app.write-journal"));
        journal.append(Some("Journal Setup…"), Some("app.journal-setup"));
        menu.append_section(None, &journal);

        let run = gtk::gio::Menu::new();
        run.append(Some("Start a New Run…"), Some("app.new-run"));
        menu.append_section(None, &run);

        let account = gtk::gio::Menu::new();
        account.append(Some("Sharing…"), Some("app.sync-status"));
        account.append(Some("Connect Battle.net Account…"), Some("app.setup"));
        account.append(Some("Sign Out"), Some("app.sign-out"));
        menu.append_section(None, &account);

        let about = gtk::gio::Menu::new();
        about.append(Some("About Armory"), Some("app.about"));
        menu.append_section(None, &about);

        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main menu")
            .menu_model(&menu)
            .primary(true)
            .build();

        // Back to the roster, when a character is open on top of it. The
        // roster's own `AdwNavigationView` has no header bar of its own —
        // giving it one draws a second header under this one — so the one
        // header there is grows a back button instead.
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Back to the roster")
            .visible(false)
            .build();
        back.add_css_class("flat");
        let window = self.clone();
        back.connect_clicked(move |_| window.roster_page().show_roster());

        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&back);
        header.pack_start(&sync);
        header.pack_start(&spinner);
        header.pack_end(&menu_button);
        header.pack_end(&rail_button);
        header.pack_end(&find);

        let view = adw::ToolbarView::builder().content(content).build();
        view.add_top_bar(&header);

        let imp = self.imp();
        *imp.title.borrow_mut() = Some(title);
        *imp.sync.borrow_mut() = Some(sync);
        *imp.spinner.borrow_mut() = Some(spinner);
        *imp.find.borrow_mut() = Some(find);
        *imp.rail_button.borrow_mut() = Some(rail_button);
        *imp.back.borrow_mut() = Some(back);

        adw::NavigationPage::builder()
            .title("Run")
            .child(&view)
            .build()
    }

    /// Fold things away as the window runs out of room, in the order they can
    /// be given up.
    ///
    /// Two breakpoints, and the order between them is the whole point. Every
    /// page is a main column and a right rail; the rail carries the page's
    /// standing, filters and caveats and the main column carries the thing
    /// itself. So the **rail folds first**, at 1160sp — squeezing the goals,
    /// the artwork or the market to keep a legend on screen has it backwards.
    /// Only when there is not room for the places sidebar either, at 800sp,
    /// does that fold too.
    ///
    /// **The first number has to clear the widest page, not look tidy.** It was
    /// 1080sp, and the market page needs 1159sp with its rail out — so between
    /// 1081 and 1158 the rail was still shown on a window with no room for it,
    /// the content overflowed to the right, and what went off the edge was the
    /// window's own close button. The default size of 1180 sits inside that
    /// band, so it was the *normal* case rather than an edge one. A page whose
    /// main column grows past this budget moves this number, or gives the
    /// growth back.
    fn install_breakpoint(&self) {
        let rails = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 1160sp").expect("a breakpoint condition"),
        );
        let window = self.clone();
        rails.connect_apply(move |_| window.set_rails_collapsed(true));
        let window = self.clone();
        rails.connect_unapply(move |_| window.set_rails_collapsed(false));
        self.add_breakpoint(rails);

        let sidebar = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 800sp").expect("a breakpoint condition"),
        );
        let window = self.clone();
        sidebar.connect_apply(move |_| window.set_collapsed(true));
        let window = self.clone();
        sidebar.connect_unapply(move |_| window.set_collapsed(false));
        self.add_breakpoint(sidebar);
    }

    fn set_collapsed(&self, collapsed: bool) {
        self.imp().collapsed.set(collapsed);
        if let Some(split) = self.imp().split.borrow().as_ref() {
            split.set_collapsed(collapsed);
        }
    }

    /// Fold every page's rail into an overlay, and offer the button that opens
    /// it again.
    fn set_rails_collapsed(&self, collapsed: bool) {
        super::almanac::collapse_rails(collapsed);
        if let Some(button) = self.imp().rail_button.borrow().as_ref() {
            button.set_visible(collapsed);
            button.set_active(false);
        }
    }

    /// The search bar belonging to a place, if it has one.
    ///
    /// Every page with a list long enough to lose something in.
    fn searchable(&self, place: &str) -> Option<gtk::SearchBar> {
        let imp = self.imp();
        if place == "roster" {
            return imp.roster.borrow().as_ref().and_then(RosterPage::search);
        }
        if place == "run" {
            return imp.run.borrow().as_ref().and_then(RunPage::search);
        }
        if place == "chronicle" {
            return imp
                .chronicle
                .borrow()
                .as_ref()
                .and_then(ChroniclePage::search);
        }
        if place == "market" {
            return imp.market.borrow().as_ref().and_then(MarketPage::search);
        }
        if place == "zones" {
            return imp.zones.borrow().as_ref().and_then(ZonePage::search);
        }
        if place == "reputations" {
            return imp
                .reputations
                .borrow()
                .as_ref()
                .and_then(ReputationsPage::search);
        }
        imp.collections
            .borrow()
            .iter()
            .find(|page| place_of(page.kind()) == place)
            .and_then(CollectionPage::search)
    }

    // -- what the application drives ------------------------------------------

    pub fn onboarding(&self) -> Onboarding {
        self.imp()
            .onboarding
            .borrow()
            .clone()
            .expect("the onboarding page")
    }

    pub fn roster_page(&self) -> RosterPage {
        self.imp().roster.borrow().clone().expect("the roster page")
    }

    /// Put a character in the header, or take it back out.
    ///
    /// One header bar for the whole window, so the character's name replaces
    /// the place's rather than sitting under it. The subtitle is the realm,
    /// which is the half of a character's name that says which Somechar this is.
    fn show_character_header(&self, showing: bool) {
        let imp = self.imp();
        if let Some(back) = imp.back.borrow().as_ref() {
            back.set_visible(showing);
        }
        let Some(title) = imp.title.borrow().clone() else {
            return;
        };
        match showing
            .then(|| self.roster_page().open_character_name())
            .flatten()
        {
            Some((name, realm)) => {
                title.set_title(&name);
                title.set_subtitle(&realm);
            }
            None => {
                // Back to whichever place the sidebar is on, which `open` is
                // the one thing that knows.
                title.set_subtitle("");
                let open = imp
                    .stack
                    .borrow()
                    .as_ref()
                    .and_then(|stack| stack.visible_child_name());
                if let Some(open) = open {
                    self.open(&open);
                }
            }
        }
    }

    pub fn run_page(&self) -> RunPage {
        self.imp().run.borrow().clone().expect("the run page")
    }

    pub fn chronicle_page(&self) -> ChroniclePage {
        self.imp()
            .chronicle
            .borrow()
            .clone()
            .expect("the chronicle page")
    }

    pub fn zone_page(&self) -> ZonePage {
        self.imp().zones.borrow().clone().expect("the zones page")
    }

    pub fn market_page(&self) -> MarketPage {
        self.imp().market.borrow().clone().expect("the market page")
    }

    pub fn reputations_page(&self) -> ReputationsPage {
        self.imp()
            .reputations
            .borrow()
            .clone()
            .expect("the reputations page")
    }

    pub fn collection_page(&self, kind: Kind) -> CollectionPage {
        self.imp()
            .collections
            .borrow()
            .iter()
            .find(|page| page.kind() == kind)
            .cloned()
            .expect("a page per kind")
    }

    pub fn collection_pages(&self) -> Vec<CollectionPage> {
        self.imp().collections.borrow().clone()
    }

    /// Swap between setup and the application proper.
    pub fn show_onboarding(&self, showing: bool) {
        if let Some(faces) = self.imp().faces.borrow().as_ref() {
            faces.set_visible_child_name(if showing { "setup" } else { "main" });
        }
    }

    /// Open one of the places in the sidebar.
    pub fn open(&self, name: &str) {
        let imp = self.imp();
        let Some((index, (_, title, _))) = PLACES
            .iter()
            .enumerate()
            .find(|(_, (place, _, _))| *place == name)
        else {
            return;
        };

        if let Some(stack) = imp.stack.borrow().as_ref() {
            stack.set_visible_child_name(name);
        }
        if let Some(sidebar) = imp.sidebar.borrow().as_ref() {
            sidebar.set_selected(index as u32);
        }
        if let Some(window_title) = imp.title.borrow().as_ref() {
            window_title.set_title(title);
        }
        // On a narrow window the sidebar is an overlay, and choosing something
        // from it should get out of the way rather than leave the person to
        // dismiss it.
        if imp.collapsed.get() {
            if let Some(split) = imp.split.borrow().as_ref() {
                split.set_show_content(true);
            }
        }
        self.follow_search();
    }

    /// Hand the header's search toggle, and the keyboard, to the open page.
    ///
    /// Exactly one bar is wired up at a time, and both halves matter.
    ///
    /// The **toggle** has to be rewired, or pressing it would open a search on
    /// a page nobody is looking at.
    ///
    /// The **key capture** has to be exclusive. `set_key_capture_widget` puts a
    /// controller on the window, so four bars all pointed at it are four
    /// controllers watching every keystroke — and four pages that quietly enter
    /// search mode with the same half-typed word in them. Which one consumes
    /// the key is a question about the order GTK happens to run controllers in,
    /// and that is not a thing to build on.
    fn follow_search(&self) {
        let imp = self.imp();
        let Some(find) = imp.find.borrow().clone() else {
            return;
        };

        if let Some(handler) = imp.handler.borrow_mut().take() {
            find.disconnect(handler);
        }
        if let Some(previous) = imp.searching.borrow_mut().take() {
            previous.set_key_capture_widget(gtk::Widget::NONE);
            previous.set_search_mode(false);
        }

        let open = imp
            .stack
            .borrow()
            .as_ref()
            .and_then(|stack| stack.visible_child_name())
            .unwrap_or_default()
            .to_string();

        match self.searchable(&open) {
            Some(bar) => {
                bar.set_key_capture_widget(Some(self));
                find.set_visible(true);
                find.set_active(bar.is_search_mode());
                let handler = {
                    let bar = bar.clone();
                    find.connect_toggled(move |button| bar.set_search_mode(button.is_active()))
                };
                *imp.handler.borrow_mut() = Some(handler);
                *imp.searching.borrow_mut() = Some(bar);
            }
            None => {
                find.set_visible(false);
                find.set_active(false);
            }
        }
    }

    /// Put a collection's standing on the row that opens it.
    pub fn set_tally(&self, kind: Kind, collected: usize, total: usize) {
        let Some(sidebar) = self.imp().sidebar.borrow().clone() else {
            return;
        };
        let Some(index) = PLACES
            .iter()
            .position(|(place, _, _)| *place == place_of(kind))
        else {
            return;
        };
        let Some(label) = sidebar
            .item(index as u32)
            .and_then(|item| item.suffix())
            .and_downcast::<gtk::Label>()
        else {
            return;
        };

        // Nothing rather than "0" before the first sync: a zero is a claim
        // about the account, and an empty catalogue is a claim about us.
        label.set_label(&if total == 0 {
            String::new()
        } else {
            format!("{collected}/{total}")
        });
    }

    pub fn set_busy(&self, busy: bool) {
        let imp = self.imp();
        if let Some(spinner) = imp.spinner.borrow().as_ref() {
            spinner.set_visible(busy);
        }
        if let Some(sync) = imp.sync.borrow().as_ref() {
            sync.set_sensitive(!busy);
        }
    }

    /// A condition that will stay true until somebody acts on it.
    ///
    /// A toast is the wrong shape for these — it goes away while the person is
    /// still reading the page it was about. Passing `None` takes the banner
    /// down again.
    pub fn set_notice(&self, notice: Option<&str>) {
        let Some(banner) = self.imp().banner.borrow().clone() else {
            return;
        };
        match notice {
            Some(text) => {
                banner.set_title(text);
                banner.set_revealed(true);
            }
            None => banner.set_revealed(false),
        }
    }

    pub fn toast(&self, message: &str) {
        if let Some(toasts) = self.imp().toasts.borrow().as_ref() {
            toasts.add_toast(adw::Toast::new(message));
        }
    }
}
