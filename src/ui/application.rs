//! `ArmoryApplication`: the only object that owns state or asks anything.
//!
//! Every widget in `ui/` reports what a person did and waits. This file is where
//! that becomes a request, a roster, and a page. Having one such place is what
//! keeps the widget tree free of `RefCell`s pointing at each other.
//!
//! The shape of a sync: fetch the account index, then fan out across the
//! enrolled characters. Nothing waits for the slowest. Every per-character call
//! is conditional on the `Last-Modified` we already hold, so a character who has
//! not logged out since last time answers `304` with no body and costs one round
//! trip — which is what makes an account this size affordable to sync at all.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use chrono::{DateTime, Utc};
use gtk::gio;
use gtk::glib;

use super::character_page;
use super::collector::{Dump, Watch};
use super::http::Http;
use super::images::Images;
use super::journal_dialog::JournalDialog;
use super::keyring::{self, Keyring, ACCESS_TOKEN, CLIENT_SECRET};
use super::load_stylesheet;
use super::redirect::Redirect;
use super::sync::Service;
use super::sync_dialog::{self, SyncDialog};
use super::watch_dialog::WatchDialog;
use super::window::ArmoryWindow;
use super::{Quote, Warband};
use crate::model::addon::collector::ReadError;
use crate::model::character::{CharacterKey, Detail, Roster};
use crate::model::chronicle::{Entry, SessionId};
use crate::model::cohort::Cohort;
use crate::model::market;
use crate::model::plan::{self, Inputs};
use crate::model::provenance::Origin;
use crate::model::replica;
use crate::model::run::{Attestation, Bucket, Exclusion, Run};
use crate::model::settings::Settings;
use crate::model::source::blizzard::auctions;
use crate::model::source::blizzard::collections::{self, Kind, Source};
use crate::model::source::blizzard::gamedata::{self, Achievement};
use crate::model::source::blizzard::media;
use crate::model::source::blizzard::oauth::{self, ClientCredentials, Token};
use crate::model::source::blizzard::profile;
use crate::model::source::journal;
use crate::model::source::{Outcome, Reason};
use crate::model::store::Store;
use crate::model::sync::{Remote, SyncError};
use crate::APP_ID;

/// How many render URLs to ask for in one sync.
///
/// One request each, and the catalogue has thousands of entries with no art of
/// their own. Enough to fill the visible part of a grid several times over,
/// small enough that it is a rounding error against the hourly quota. "Fetch
/// Missing Artwork" in the menu is the way to have the lot.
const ART_PER_SYNC: usize = 120;

/// How many item names to fetch in one sync.
///
/// The same shape of problem as the artwork and the same answer. A realm's
/// commodity market is tens of thousands of items and every name is one call,
/// so the browser fills in over successive syncs rather than in one — and the
/// budget is spent on what somebody is actually looking at, which is why
/// `MarketPage::wants_names` reads the visible rows rather than the store's
/// order.
const NAMES_PER_SYNC: usize = 150;

/// How much of the Adventure Guide to fetch in one sync.
///
/// A hundred and fifty instances and about a thousand encounters, one call
/// each, and none of it ever changes once fetched — so this converges and then
/// costs one call for the index, for ever.
///
/// Higher than the artwork's budget on purpose. Art fills in a page somebody is
/// already looking at, so spreading it out costs nothing; the guide is a
/// one-off backfill of static data that several features are waiting on, and
/// eleven hundred calls against a ceiling of thirty-six thousand an hour is not
/// a cost worth spreading over a day. At this rate it is done in six syncs.
const GUIDE_PER_SYNC: usize = 200;

/// How many evenings the journal page draws.
///
/// A card each, with an expander of facts inside it, so this is bounded by what
/// somebody will scroll rather than by what has been recorded. Nothing is
/// deleted at this number — the store keeps every session forever, because none
/// of it came through Blizzard's API — it is only what one page shows.
const SESSIONS_SHOWN: usize = 60;

/// How long to let a journal entry take, in seconds.
///
/// Writing four hundred words is tens of seconds of work for a language model,
/// and the twenty the rest of the application allows an API is not a timeout so
/// much as a guarantee the feature never finishes.
const JOURNAL_TIMEOUT: u32 = 180;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ArmoryApplication {
        pub settings: RefCell<Settings>,
        pub store: OnceCell<Rc<RefCell<Store>>>,
        pub http: OnceCell<Http>,
        /// Blizzard's art. Separate from `http` and deliberately so: the render
        /// service is not an API host, spends no quota and takes no token, and
        /// putting image traffic through the same rate gate as the API would
        /// make a scrolling grid slow down a sync.
        pub images: OnceCell<Images>,
        pub window: RefCell<Option<ArmoryWindow>>,
        pub roster: RefCell<Roster>,
        pub cohort: RefCell<Cohort>,
        /// The expensive half, for the enrolled characters only.
        pub details: RefCell<HashMap<CharacterKey, Detail>>,
        pub token: RefCell<Option<Token>>,
        /// Held for the length of a sign-in. Dropping it unbinds the port, and
        /// the port is fixed and registered, so it must not leak.
        pub redirect: RefCell<Option<Redirect>>,
        /// Bumped whenever a sync starts, so a callback from a sync the person
        /// has already replaced drops its result rather than writing it into
        /// the page that superseded it.
        pub generation: Cell<u64>,
        pub syncing: Cell<bool>,
        /// Whether a redraw is already scheduled. See `refresh_views`.
        pub redraw_queued: Cell<bool>,
        /// Whether the catalogues have changed since they were last drawn.
        ///
        /// Rebuilding them is the expensive half of a redraw — six thousand
        /// rows out of SQLite and six thousand objects into list models — and
        /// almost every redraw during a sync is prompted by something else
        /// entirely, like one character's item level arriving.
        pub collections_dirty: Cell<bool>,
        /// How many per-character calls are still in flight, so the spinner
        /// stops when the last one lands rather than when the first does.
        pub outstanding: Cell<usize>,
        /// Drop chances out of an installed Rarity, or empty when there is
        /// none. Read once at startup — see `start_watching`.
        pub chances: RefCell<crate::model::rarity::Chances>,
        /// Which market the browser is reading, once somebody has picked one.
        ///
        /// `None` is "not chosen", which is not the same as region-wide: it
        /// falls back to the first watched realm, and region-wide is realm 0
        /// picked deliberately. In memory rather than in `Settings`, because
        /// this is where somebody is looking rather than how the application is
        /// set up — the same standing as which tab is open.
        pub browsing: Cell<Option<u32>>,

        /// The current run, and its row id.
        pub run: RefCell<Option<(i64, Run)>>,
        /// Everything the planner needs, accumulated across a sync.
        pub inputs: RefCell<Inputs>,
        /// The monitor on the addon's SavedVariables file.
        pub watch: RefCell<Option<Watch>>,
        /// Render URLs that had to be asked for, keyed by what they illustrate.
        ///
        /// Mounts and pets are absent from all three: their pictures are
        /// addressed by a creature display id the addon already recorded, so
        /// nothing has to be looked up. These are the ones Blizzard is the only
        /// source for.
        ///
        /// Held rather than stored, on the same reasoning as `reputations`: the
        /// media bodies these were read out of are already in the response
        /// cache under the ordinary thirty-day term, so a relaunch re-reads
        /// them from there rather than keeping a second copy of the same URLs
        /// under a schema of their own. `restore_art` is what does that, and
        /// without it a launch starts with no artwork and spends its whole
        /// per-sync budget re-earning what it already had.
        pub portraits: RefCell<HashMap<CharacterKey, String>>,
        /// Icon URLs keyed by item id. Toys and decor share the map because
        /// both are items and an item id means the same thing to both.
        pub toy_art: RefCell<HashMap<u32, String>>,
        pub achievement_art: RefCell<HashMap<u32, String>>,
        /// The WoW Token's last known price, in copper.
        pub token_price: Cell<Option<u64>>,
        /// Reputations per character, with their names and tiers.
        ///
        /// Held rather than stored: the raw bodies are already in the response
        /// cache, so a relaunch re-reads them from there instead of keeping a
        /// second copy under a different schema.
        pub reputations: RefCell<HashMap<CharacterKey, Vec<profile::FactionStanding>>>,
        /// Missing collectibles seen in the last snapshot of each realm.
        ///
        /// Not stored. A listing is gone within the hour and a bargain from
        /// last Tuesday is worse than no bargain at all.
        pub offers: RefCell<Vec<crate::model::market::Offer>>,
        /// When the addon last wrote, as it saw the clock.
        pub collected_at: Cell<Option<chrono::DateTime<Utc>>>,
        /// Set when a name landed, so a redraw knows the market page has
        /// something new to say without asking the store on every callback.
        pub names_arrived: Cell<bool>,

        /// The client the journal writes through. Separate from `http` for the
        /// same reason `images` is: a different host with different limits and,
        /// here, a wildly different idea of how long an answer takes.
        pub journal_http: OnceCell<Http>,
        /// Evenings with a request in flight, so a second press does not buy a
        /// second entry for the same one.
        pub writing: RefCell<Vec<SessionId>>,
        /// Evenings queued behind the one being written, for "Write All".
        pub queued: RefCell<Vec<SessionId>>,
        /// Whether a model answered the last time one was asked.
        ///
        /// Cached rather than asked on demand: the page needs it on every
        /// redraw, a redraw happens after every burst of a sync, and asking is
        /// a round trip to a server that may not be there. Kept current at
        /// startup, on a save, and on a test.
        pub journal_ready: Cell<bool>,
        /// What that model is called, as `/props` named it.
        ///
        /// llama-server serves whatever it was launched with and ignores the
        /// `model` field of a request, so this is the only place an entry's
        /// attribution can come from.
        pub journal_model: RefCell<Option<String>>,

        // -- sharing this account with the other machines --------------------
        /// Whether a pass is in flight. One at a time: two would both read the
        /// outbox, both push it, and both drain it.
        pub passing: Cell<bool>,
        /// Whether a `/wait` is parked. At most one, or a quiet evening ends
        /// with a thread per tick.
        pub parked: Cell<bool>,
        /// Passes that have failed in a row.
        ///
        /// A NAS asleep, a machine between networks and a suspended laptop all
        /// produce one failed pass, and saying so each time trains somebody to
        /// stop reading it.
        pub failures: Cell<usize>,
        /// What the last pass did, for the sync page.
        pub last_pass: RefCell<Option<Pass>>,
        /// A pending debounced pass, cancelled and re-armed by each new write.
        pub pass_due: RefCell<Option<glib::SourceId>>,
        /// The sync dialog, while it is open, so a pass can redraw it.
        pub sync_dialog: RefCell<Option<super::SyncDialog>>,
    }

    /// What one pass did, as the sync page reports it.
    #[derive(Debug, Clone)]
    pub struct Pass {
        pub at: DateTime<Utc>,
        pub sent: usize,
        pub landed: usize,
        pub removed: usize,
        /// Rows the other end could not read. A number climbing here is the
        /// shape of one machine running an older build.
        pub unreadable: usize,
        /// `None` when the pass finished.
        pub failed: Option<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ArmoryApplication {
        const NAME: &'static str = "ArmoryApplication";
        type Type = super::ArmoryApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for ArmoryApplication {}

    impl ApplicationImpl for ArmoryApplication {
        fn startup(&self) {
            // Chain up first: the toolkit initialises in the parent handler, and
            // anything touching GTK before it is undefined.
            self.parent_startup();
            let app = self.obj();

            if let Some(display) = gtk::gdk::Display::default() {
                load_stylesheet(&display);
            }
            app.load_settings();
            app.open_store();
            app.install_actions();
            app.start_sharing();
        }

        fn activate(&self) {
            let app = self.obj();
            app.window().present();
            app.restore();
        }

        fn shutdown(&self) {
            // The 30-day expiry Blizzard's terms require, on the way out rather
            // than on the way in: a first paint should not wait on a sweep.
            if let Some(store) = self.obj().imp().store.get() {
                let _ = store.borrow().purge();
            }
            // Cached art gets the same sweep. It is not covered by that term —
            // it is hotlinked from a public CDN rather than obtained through the
            // API — but a cache directory that only grows is its own problem.
            if let Some(images) = self.obj().imp().images.get() {
                images.purge();
            }
            self.parent_shutdown();
        }
    }

    impl GtkApplicationImpl for ArmoryApplication {}
    impl AdwApplicationImpl for ArmoryApplication {}
}

glib::wrapper! {
    pub struct ArmoryApplication(ObjectSubclass<imp::ArmoryApplication>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for ArmoryApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl ArmoryApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("flags", gio::ApplicationFlags::default())
            .build()
    }

    // -- directories and startup ---------------------------------------------

    fn config_dir(&self) -> PathBuf {
        glib::user_config_dir().join("armory")
    }

    fn data_dir(&self) -> PathBuf {
        glib::user_data_dir().join("armory")
    }

    fn settings_path(&self) -> PathBuf {
        self.config_dir().join("settings.json")
    }

    fn load_settings(&self) {
        *self.imp().settings.borrow_mut() = Settings::load(&self.settings_path());
    }

    fn save_settings(&self) {
        let settings = self.imp().settings.borrow().clone();
        if let Err(error) = settings.save(&self.settings_path()) {
            eprintln!("armory: could not write settings: {error}");
        }
    }

    fn open_store(&self) {
        let directory = self.data_dir();
        let _ = std::fs::create_dir_all(&directory);

        // An in-memory fallback means a machine with an unwritable data
        // directory still runs; it just forgets. Refusing to start would be a
        // worse answer to a full disk.
        let store = Store::open(&directory.join("armory.db"))
            .or_else(|_| Store::in_memory())
            .expect("a store");

        let _ = self.imp().store.set(Rc::new(RefCell::new(store)));
        let _ = self.imp().http.set(Http::new());
        let _ = self.imp().images.set(Images::new());
    }

    fn store(&self) -> Rc<RefCell<Store>> {
        Rc::clone(self.imp().store.get().expect("a store"))
    }

    fn http(&self) -> Http {
        self.imp().http.get().expect("an http client").clone()
    }

    fn images(&self) -> Images {
        self.imp().images.get().expect("an image cache").clone()
    }

    fn window(&self) -> ArmoryWindow {
        if let Some(window) = self.imp().window.borrow().clone() {
            return window;
        }
        let window = ArmoryWindow::new(self, &self.images());
        self.connect_window(&window);
        *self.imp().window.borrow_mut() = Some(window.clone());
        window
    }

    fn connect_window(&self, window: &ArmoryWindow) {
        let app = self.clone();
        window.onboarding().connect_sign_in(move || app.sign_in());

        let app = self.clone();
        window.onboarding().connect_skipped(move || app.skip_api());

        let app = self.clone();
        window.run_page().connect_start(move || app.start_run());

        let app = self.clone();
        window.run_page().connect_attested(move |id, done| {
            // The cohort's first member stands in for "me" until goals carry a
            // character picker. Attestation is a person saying it happened; who
            // it happened on matters less than that it did.
            let who = app.imp().cohort.borrow().keys().next().cloned();
            app.attest(id, done.then_some(who).flatten());
        });

        let app = self.clone();
        window
            .run_page()
            .connect_excluded(move |id, excluded| app.set_excluded(id, excluded));

        // Pressing one of last night's numbers is asking to read about that
        // evening, and the journal is where evenings are read.
        let app = self.clone();
        window
            .run_page()
            .connect_evening(move || app.window().open("chronicle"));

        // Opening a character fills its page from the store. Read then rather
        // than held, because everything on it — the evenings, the counters, the
        // run's credit — is already read for other pages and a second copy of
        // all of it would be one more thing to keep in step.
        let app = self.clone();
        window
            .roster_page()
            .connect_open_character(move |key| app.show_character(&key));

        let app = self.clone();
        window.market_page().connect_unwatched(move |item_id| {
            let _ = app.store().borrow().unwatch_item(item_id);
            app.refresh_views();
        });

        let app = self.clone();
        window.market_page().connect_realm_unwatched(move |realm| {
            let _ = app.store().borrow().unwatch_realm(realm);
            app.refresh_views();
        });

        let app = self.clone();
        window
            .market_page()
            .connect_add_realm(move || app.pick_realm());

        let app = self.clone();
        window.market_page().connect_browse_realm(move |realm| {
            app.imp().browsing.set(Some(realm));
            app.refresh_views();
        });

        let app = self.clone();
        window
            .market_page()
            .connect_add_item(move || app.pick_item());

        // Watching an item found by browsing. The same action as the picker's,
        // reached from the other end: the picker starts from a name somebody
        // typed and this starts from a row they were already looking at.
        let app = self.clone();
        window.market_page().connect_watch(move |id, name| {
            let _ = app.store().borrow().watch_item(id, &name);
            app.window()
                .toast(&format!("Watching {name}. Its history starts now."));
            app.refresh_views();
        });

        // Somebody typed a name the browser could not match. See `look_up`.
        let app = self.clone();
        window
            .market_page()
            .connect_look_up(move |name| app.look_up_item(&name));

        let app = self.clone();
        window
            .roster_page()
            .connect_toggled(move |key| app.toggle_enrolment(&key));

        let app = self.clone();
        window
            .chronicle_page()
            .connect_write(move |id| app.write_entry(id));

        let app = self.clone();
        window.chronicle_page().connect_forget(move |id| {
            let _ = app.store().borrow().forget_session(&id);
            app.window().toast("Evening forgotten");
            app.refresh_views();
        });

        let app = self.clone();
        window
            .chronicle_page()
            .connect_setup(move || app.show_journal_setup());
    }

    /// Enrol a character, or withdraw them.
    ///
    /// Every borrow here is taken, used and released before the next line. That
    /// is not fussiness: [`Self::refresh_views`] reads the cohort, the roster,
    /// the details, the run and the inputs, so *any* borrow still alive when it
    /// is called panics at runtime rather than failing to compile.
    ///
    /// The version this replaced took a `RefMut` and then shadowed it with a
    /// clone, so the `drop` that looked like it released the guard dropped the
    /// clone instead — and enrolling anybody crashed the application every
    /// time.
    fn toggle_enrolment(&self, key: &CharacterKey) {
        let cohort = {
            let mut cohort = self.imp().cohort.borrow_mut();
            cohort.toggle(key);
            cohort.clone()
        };

        let _ = self.store().borrow_mut().save_cohort(&cohort);
        self.refresh_views();
    }

    fn install_actions(&self) {
        let sync = gio::SimpleAction::new("sync", None);
        let app = self.clone();
        sync.connect_activate(move |_, _| app.sync());
        self.add_action(&sync);

        // Going back to setup, which is otherwise unreachable once someone has
        // chosen the addon-only path. That choice is recorded so onboarding
        // does not reappear every launch — and without a way back it becomes a
        // one-way door, which is what it was until somebody registered a client
        // and found nowhere to put it.
        let setup = gio::SimpleAction::new("setup", None);
        let app = self.clone();
        setup.connect_activate(move |_, _| app.show_setup());
        self.add_action(&setup);

        let fetch_art = gio::SimpleAction::new("fetch-art", None);
        let app = self.clone();
        fetch_art.connect_activate(move |_, _| app.fetch_all_art());
        self.add_action(&fetch_art);

        let journal = gio::SimpleAction::new("journal-setup", None);
        let app = self.clone();
        journal.connect_activate(move |_, _| app.show_journal_setup());
        self.add_action(&journal);

        // In the menu rather than beside the per-card button: writing one
        // evening and writing every evening are different acts, and the
        // expensive one should not be the easier press.
        let write_all = gio::SimpleAction::new("write-journal", None);
        let app = self.clone();
        write_all.connect_activate(move |_, _| app.write_all());
        self.add_action(&write_all);

        // Starting over. The Run page's own "Start a run" button lives in its
        // empty state, so with a run already in place there is otherwise no
        // way to begin a different one — and a run's cohort is frozen at its
        // baseline, so re-aiming one at another character is not a thing that
        // can be done to it.
        let new_run = gio::SimpleAction::new("new-run", None);
        let app = self.clone();
        new_run.connect_activate(move |_, _| app.confirm_new_run());
        self.add_action(&new_run);

        // A pass is silent when it works, so there has to be somewhere to ask.
        let share = gio::SimpleAction::new("sync-status", None);
        let app = self.clone();
        share.connect_activate(move |_, _| app.show_sync_status());
        self.add_action(&share);

        let sign_out = gio::SimpleAction::new("sign-out", None);
        let app = self.clone();
        sign_out.connect_activate(move |_, _| app.sign_out());
        self.add_action(&sign_out);

        let about = gio::SimpleAction::new("about", None);
        let app = self.clone();
        about.connect_activate(move |_, _| app.show_about());
        self.add_action(&about);

        let quit = gio::SimpleAction::new("quit", None);
        let app = self.clone();
        quit.connect_activate(move |_, _| app.quit());
        self.add_action(&quit);
        self.set_accels_for_action("app.quit", &["<Primary>q"]);
        self.set_accels_for_action("app.sync", &["<Primary>r"]);
    }

    // -- restoring what was already known ------------------------------------

    /// Show whatever is already on disk, before anything is fetched.
    ///
    /// Profile data is a logout snapshot, so what was stored last time is what
    /// Blizzard would answer with anyway until someone plays. Waiting for the
    /// network to draw a roster that has not changed would be a blank window
    /// for no reason.
    fn restore(&self) {
        let store = self.store();
        let roster = store.borrow().roster().unwrap_or_default();
        let mut cohort = store.borrow().cohort().unwrap_or_default();
        cohort.prune(&roster);
        let details = store.borrow().details().unwrap_or_default();

        *self.imp().roster.borrow_mut() = roster;
        *self.imp().cohort.borrow_mut() = cohort;
        *self.imp().details.borrow_mut() = details;

        // What is already known, before anything is fetched. Profile data is a
        // logout snapshot, so last time's answer is still Blizzard's answer
        // until somebody plays.
        {
            let mut inputs = self.imp().inputs.borrow_mut();
            inputs.attributions = store.borrow().attributions().unwrap_or_default();
            inputs.catalogue = store.borrow().achievements().unwrap_or_default();
            inputs.criteria = store.borrow().criteria().unwrap_or_default();
            // Who actually earned the account's account-wide progress. Read
            // back like everything else here, because it is cumulative across
            // months and the addon may not have written since.
            inputs.provenance = store.borrow().provenance().unwrap_or_default();
        }
        *self.imp().run.borrow_mut() = store.borrow().current_run().unwrap_or(None);
        self.restore_reputations();
        self.restore_art();
        self.restore_token();
        self.start_watching();

        let window = self.window();
        let settings = self.imp().settings.borrow().clone();
        // Onboarding is skipped once there is a client *or* once someone has
        // said they do not want one. Showing it again every launch would be
        // asking a question already answered.
        window.show_onboarding(!settings.is_registered() && !settings.addon_only);

        if settings.is_registered() {
            window.onboarding().set_client_id(&settings.client_id);
            window.onboarding().set_region(settings.region);
        }
        window
            .onboarding()
            .set_secret_held(self.stored_secret().is_some());
        // One call at startup rather than one per redraw, and it doubles as
        // the readiness check; see the note on `journal_ready`.
        self.identify_journal(None);
        // Straight to the draw at startup: there is no burst to coalesce yet,
        // and a first paint should not wait on a timer. The catalogues have
        // not been drawn at all yet, so they count as changed.
        self.imp().collections_dirty.set(true);
        self.redraw_now();
    }

    /// Re-read reputations out of the response cache.
    ///
    /// Profile data is a logout snapshot, so the body from the last sync is
    /// still Blizzard's answer until somebody plays. Parsing it back is cheaper
    /// than a second schema, and it means the page has something to show before
    /// the first sync of the session lands.
    fn restore_reputations(&self) {
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow().clone();
        let cohort = self.imp().cohort.borrow().clone();
        let store = self.store();

        for character in cohort.members(&roster) {
            let url = profile::reputations(region, &character.key).url;
            let Ok(Some(body)) = store.borrow().response(&url, chrono::Duration::days(30)) else {
                continue;
            };
            if let Outcome::Found(reputations) = profile::parse_reputations(&body, character.level)
            {
                self.imp()
                    .reputations
                    .borrow_mut()
                    .insert(character.key.clone(), reputations.detail);
            }
        }
    }

    /// Re-read the artwork URLs out of the response cache.
    ///
    /// The three art maps are held in memory and nothing wrote them anywhere,
    /// so every launch used to start with no pictures at all and spend its
    /// whole per-sync budget — a hundred and twenty calls — re-earning URLs it
    /// already had the bodies for. Two thousand toys at a hundred and twenty a
    /// sync never converges, and quitting threw away whatever had converged.
    ///
    /// The bodies themselves were always there: `fetch_bare` stores every one
    /// under its URL, on the same thirty-day term as the rest. This turns them
    /// back into URLs. Same reasoning as [`Application::restore_reputations`] —
    /// re-parse what is on disk rather than keep a second copy of it under a
    /// schema of its own.
    fn restore_art(&self) {
        let region = self.imp().settings.borrow().region;
        let store = self.store();
        let ttl = chrono::Duration::days(30);

        // Items and achievements in bulk. Which id a media body describes is
        // not in the body, so it is read back off the URL it is filed under.
        for (needle, art) in [
            (media::ITEM_MEDIA, &self.imp().toy_art),
            (media::ACHIEVEMENT_MEDIA, &self.imp().achievement_art),
        ] {
            let Ok(bodies) = store.borrow().responses_matching(needle, ttl) else {
                continue;
            };
            let mut art = art.borrow_mut();
            for (url, body) in bodies {
                let Some(id) = media::media_id(&url, needle) else {
                    continue;
                };
                if let Outcome::Found(url) = media::parse_icon(&body) {
                    art.insert(id, url);
                }
            }
        }

        // Portraits are one per enrolled character rather than one per
        // catalogue entry, so they are cheaper asked for by name.
        let roster = self.imp().roster.borrow().clone();
        let cohort = self.imp().cohort.borrow().clone();
        for character in cohort.members(&roster) {
            let url = media::character(region, &character.key).url;
            let Ok(Some(body)) = store.borrow().response(&url, ttl) else {
                continue;
            };
            if let Outcome::Found(url) = media::parse_portrait(&body, media::Portrait::Avatar) {
                self.imp()
                    .portraits
                    .borrow_mut()
                    .insert(character.key.clone(), url);
            }
        }
    }

    /// Redraw the pages, once, after the current burst of answers.
    ///
    /// A sync is a thousand responses and every one of them used to redraw
    /// everything — and "everything" is now four collection catalogues read out
    /// of SQLite and turned into six thousand objects, plus a run page of
    /// several hundred rows. That is a couple of hundred milliseconds a time,
    /// on the main loop, and a thousand of them is the window going grey and
    /// GNOME offering to kill it.
    ///
    /// So a redraw is *requested* rather than performed. The requests inside
    /// one burst collapse into a single draw a moment later, which is both
    /// faster and better behaved: the page settles once with the finished
    /// answer instead of flickering through nine hundred intermediate ones.
    fn refresh_views(&self) {
        // The one place a pass gets scheduled from.
        //
        // Every mutation in this file ends up here, so hooking it here means a
        // write added next year is shared without anybody remembering to say
        // so — the same argument the change log's triggers are built on, one
        // level up. `share_soon` costs a single query against the log when
        // there is nothing waiting, which is the overwhelmingly common case.
        self.share_soon();

        if self.imp().redraw_queued.replace(true) {
            return;
        }
        let app = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            app.imp().redraw_queued.set(false);
            app.redraw_now();
        });
    }

    /// Fill the character page for whoever was just opened.
    ///
    /// Everything here is read out of the store rather than held: the evenings
    /// and the counters are already read for the chronicle and the zones, and a
    /// third copy kept in memory for a page somebody visits occasionally is
    /// three things to keep in step instead of one.
    fn show_character(&self, key: &CharacterKey) {
        let window = self.window();
        let Some(page) = window.roster_page().character_page() else {
            return;
        };
        let Some(character) = self
            .imp()
            .roster
            .borrow()
            .characters
            .iter()
            .find(|character| &character.key == key)
            .cloned()
        else {
            return;
        };

        let store = self.store();
        // This character's evenings, newest first — the order `sessions`
        // already returns them in.
        let evenings: Vec<crate::model::chronicle::Digest> = store
            .borrow()
            .sessions(SESSIONS_SHOWN)
            .unwrap_or_default()
            .iter()
            .filter(|session| &session.character == key)
            .map(|session| session.digest())
            .collect();

        let tallies = store
            .borrow()
            .tallies()
            .unwrap_or_default()
            .remove(key)
            .unwrap_or_default();

        page.show(character_page::Held {
            character: Some(character),
            detail: self
                .imp()
                .details
                .borrow()
                .get(key)
                .cloned()
                .unwrap_or_default(),
            portrait: self.imp().portraits.borrow().get(key).cloned(),
            evenings,
            tallies,
            share: self.run_share(key),
            region: self.imp().settings.borrow().region,
        });
    }

    /// How much of the run this character can be credited with.
    ///
    /// A floor, and the page says so. Most of a run is account-wide work that
    /// nothing can pin on one character; `Run::credited` counts only the goals
    /// somebody attested to or that were measured against one character, and
    /// sharing the rest out evenly would invent an answer nobody measured.
    fn run_share(&self, key: &CharacterKey) -> character_page::Share {
        let held = self.imp().run.borrow();
        let Some((_, run)) = held.as_ref() else {
            return character_page::Share::default();
        };
        let credit = run.credited();
        let roster = self.imp().roster.borrow();
        let name_of = |key: &CharacterKey| {
            roster
                .characters
                .iter()
                .find(|character| &character.key == key)
                .map(|character| character.display_name.clone())
                .unwrap_or_else(|| key.name.clone())
        };

        character_page::Share {
            credited: credit.get(key).copied().unwrap_or(0),
            closed: run.progress().done,
            runner_up: credit
                .iter()
                .filter(|(other, _)| *other != key)
                .max_by_key(|(_, count)| **count)
                .map(|(other, count)| (name_of(other), *count)),
        }
    }

    /// Redraw the pages now, whatever else is in flight.
    fn redraw_now(&self) {
        let window = self.window();
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow();
        let cohort = self.imp().cohort.borrow();
        let details = self.imp().details.borrow();

        // Read back out of storage rather than held in memory: a journal is
        // bounded by what somebody will scroll rather than by what has been
        // recorded, and holding a decade of evenings in memory to save one
        // query would buy nothing. Read here rather than beside the chronicle
        // because three pages want the same rows — the run's last fortnight,
        // the zones' visits, and the journal itself.
        let store = self.store();
        let sessions = store.borrow().sessions(SESSIONS_SHOWN).unwrap_or_default();

        window.roster_page().show(
            &roster,
            &cohort,
            &details,
            &self.imp().portraits.borrow(),
            &self.warband(),
            region,
        );

        window.reputations_page().show(
            &roster,
            &self.imp().reputations.borrow(),
            &self.imp().inputs.borrow().provenance,
        );

        // The run page draws the account's work as something that happened over
        // time, so it needs the fortnight of evenings and the names behind the
        // goals — neither of which is in the run itself.
        window.run_page().set_context(crate::ui::run_page::Context {
            roster: roster.clone(),
            cohort: cohort.clone(),
            sessions: sessions.clone(),
        });

        match self.imp().run.borrow().as_ref() {
            Some((_, run)) => {
                let catalogue = self.imp().inputs.borrow().catalogue.clone();
                window.run_page().show(run, &catalogue);
                window
                    .run_page()
                    .set_art(&self.imp().achievement_art.borrow());
            }
            None => window.run_page().show_no_run(cohort.len()),
        }

        // Whichever side the cohort plays. Faction-locked entries the account
        // can never have are not a gap in its collection.
        let faction = cohort
            .members(&roster)
            .first()
            .map(|character| character.faction)
            .or_else(|| roster.characters.first().map(|character| character.faction))
            .unwrap_or(crate::model::character::Faction::Neutral);

        // Read back out of storage rather than held in memory: the catalogue is
        // thousands of entries per kind and keeping four copies of it live to
        // save one query would be trading memory for nothing.
        //
        // Only when something wrote to it, though. This is the expensive part
        // of a redraw by a wide margin, and a redraw prompted by one
        // character's gold arriving has no business rebuilding six thousand
        // collection entries.
        if self.imp().collections_dirty.replace(false) {
            // Every pull this account has ever made, merged across characters:
            // a mount is account-wide, so an alt's raiding is rolls at it too.
            // This is what puts "31 TRIES" on a card, and it is the only place
            // that number exists — Blizzard forgets an attempt the moment the
            // encounter ends.
            let attempts = crate::model::tally::account_attempts(
                &store.borrow().tallies().unwrap_or_default(),
            );
            let chances = self.imp().chances.borrow();
            for kind in Kind::ALL {
                let (catalogue, owned) = store.borrow().collectibles(kind).unwrap_or_default();
                let page = window.collection_page(kind);
                page.show(&catalogue, &owned, faction, region, &attempts, &chances);
                window.set_tally(kind, owned.len(), catalogue.len());
            }
        }
        // Artwork arrives separately from the catalogue and is cheap to apply,
        // so it is not behind the same flag.
        for kind in [Kind::Toy, Kind::Decor] {
            window
                .collection_page(kind)
                .set_art(&self.imp().toy_art.borrow());
        }

        let realms = store.borrow().watched_realms().unwrap_or_default();
        let quotes: Vec<Quote> = store
            .borrow()
            .watched()
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(item_id, name)| {
                // Region-wide commodities plus each watched realm. An item that
                // is a commodity has no realm history and vice versa, so the
                // empty ones are dropped rather than shown as blank rows.
                std::iter::once((0u32, "Region-wide".to_string()))
                    .chain(realms.iter().cloned())
                    .filter_map({
                        let store = Rc::clone(&store);
                        let name = name.clone();
                        move |(realm, realm_name)| {
                            let history = store.borrow().price_history(realm, item_id).ok()?;
                            (!history.is_empty()).then(|| Quote {
                                item_id,
                                name: name.clone(),
                                realm,
                                realm_name,
                                history,
                            })
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // The browser's realm: whichever one was picked, falling back to the
        // first watched one — or to the region-wide commodity market when
        // nothing is watched, which is the usual case and the one where there
        // is most to look at.
        //
        // Checked against the watch list on every redraw rather than trusted,
        // because a realm can stop being watched while its market is the one on
        // screen, and browsing a realm nothing is recorded for is an empty page
        // with no explanation.
        let browsing = match self.imp().browsing.get() {
            Some(realm) if realm == 0 || realms.iter().any(|(id, _)| *id == realm) => realm,
            _ => realms.first().map(|(realm, _)| *realm).unwrap_or(0),
        };
        let market = store.borrow().snapshot(browsing).unwrap_or_default();
        let watching: HashSet<u32> = store
            .borrow()
            .watched()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        window
            .market_page()
            .show_market(browsing, &market, &watching);

        let crafting = self.making(&realms);
        window.market_page().show(
            &quotes,
            &realms,
            self.imp().token_price.get(),
            &self.imp().offers.borrow(),
            &self.resale(&realms),
            &crafting,
        );

        let entries = store.borrow().entries().unwrap_or_default();
        // Screenshots taken since the oldest evening on the page, and no
        // earlier: a folder that has been filling up for a decade is thousands
        // of files, and only these evenings' can possibly match.
        let shots = match (
            self.imp().settings.borrow().wow_path.as_deref(),
            sessions.last(),
        ) {
            (Some(wow), Some(oldest)) => {
                super::collector::screenshots_since(wow, oldest.started_at)
            }
            _ => Vec::new(),
        };
        // Read out of storage rather than off the last addon dump: the write
        // merges by taking the larger count, so the dump alone is the wrong
        // number for a character whose addon folder has been cleared.
        let tallies = store.borrow().tallies().unwrap_or_default();
        // Zones: the corpus is compiled in, so this only has to hand over what
        // the account itself contributes — the evenings, the hours, and
        // whatever of the Adventure Guide has arrived so far.
        window.zone_page().show(&crate::ui::zone_page::Held {
            sessions: sessions.clone(),
            tallies: tallies.clone(),
            guide: store.borrow().guide().unwrap_or_default(),
            items: store.borrow().items().unwrap_or_default(),
            // The whole snapshot, keyed by item. Gear carries bonus-id
            // variants and one item id can be several actual items, so this
            // takes the cheapest of them — a floor, which is the honest figure
            // when Blizzard publishes no dictionary for what the variants mean.
            market: {
                let mut cheapest: HashMap<u32, (u64, u32)> = HashMap::new();
                for realm in std::iter::once(0).chain(realms.iter().map(|(id, _)| *id)) {
                    for listed in store.borrow().snapshot_all(realm).unwrap_or_default() {
                        let entry = cheapest.entry(listed.0).or_insert((listed.1, 0));
                        entry.0 = entry.0.min(listed.1);
                        entry.1 = entry.1.saturating_add(listed.2);
                    }
                }
                cheapest
            },
        });
        window.chronicle_page().show(
            &sessions,
            &entries,
            &shots,
            self.imp().journal_ready.get(),
            &tallies,
        );
    }

    /// Which spare pets are worth selling, and where.
    ///
    /// The join lives in `model::market`; this supplies the three halves it
    /// needs and nothing else. Read out of storage on every draw rather than
    /// held: the price series are the same rows the sparklines already come
    /// from, and a second copy of a realm's pet market in memory buys nothing.
    /// What is worth crafting, and which character should craft it.
    ///
    /// Region-wide commodities *and* each watched realm, because a reagent is a
    /// commodity and lives under realm 0 while a crafted piece of gear is a
    /// realm auction and does not. Reading both is what lets one recipe be
    /// costed at all.
    fn making(&self, realms: &[(u32, String)]) -> crate::model::market::Crafting {
        let store = self.store();
        let books = store.borrow().recipes().unwrap_or_default();
        if books.is_empty() {
            // Nobody has opened a profession window. The page says so itself;
            // there is no point querying a market to find out.
            return crate::model::market::Crafting::default();
        }

        let items = store.borrow().recipe_items().unwrap_or_default();
        let markets: Vec<crate::model::market::Market> =
            std::iter::once((0u32, "Region-wide".to_string()))
                .chain(realms.iter().cloned())
                .map(|(realm, name)| {
                    let series = store
                        .borrow()
                        .commodity_series(realm, &items)
                        .unwrap_or_default();
                    (realm, name, series)
                })
                .filter(|(_, _, series)| !series.is_empty())
                .collect();

        let names: HashMap<CharacterKey, String> = self
            .imp()
            .roster
            .borrow()
            .characters
            .iter()
            .map(|character| (character.key.clone(), character.display_name.clone()))
            .collect();
        let bank = store.borrow().warband_bank().unwrap_or_default();

        crate::model::market::worth_making(&books, &names, &markets, &bank)
    }

    fn resale(&self, realms: &[(u32, String)]) -> Vec<crate::model::market::Resale> {
        let store = self.store();
        let held = store.borrow().pets_held().unwrap_or_default();
        if held.is_empty() {
            // Nothing has told us what is a spare. Every pet would look like
            // one copy, and the answer would be an empty list either way — but
            // this way it costs no query.
            return Vec::new();
        }

        let (catalogue, _) = store.borrow().collectibles(Kind::Pet).unwrap_or_default();
        let markets: Vec<crate::model::market::Market> = realms
            .iter()
            .map(|(realm, name)| {
                let series = store
                    .borrow()
                    .price_series(*realm, auctions::CAGED_PET)
                    .unwrap_or_default();
                (*realm, name.clone(), series)
            })
            .collect();

        crate::model::market::worth_selling(&catalogue, &held, &markets)
    }

    /// What the addon has reported, for the Warband group.
    fn warband(&self) -> Warband {
        let store = self.store();
        let bank_items = store.borrow().warband_bank().map(|b| b.len()).unwrap_or(0);
        let currencies = store
            .borrow()
            .currencies()
            .map(|c| c.values().map(|amounts| amounts.len()).sum())
            .unwrap_or(0);

        // Counted across every character the addon has watched. A currency that
        // arrived three different ways on three characters is three answers,
        // which is what the page shows.
        let mut earned_currencies = 0;
        let mut transferred_currencies = 0;
        let mut unclear_currencies = 0;
        for held in self.imp().inputs.borrow().provenance.values() {
            for currency in held.currency.values() {
                match currency.origin() {
                    Origin::Earned => earned_currencies += 1,
                    Origin::Transferred => transferred_currencies += 1,
                    Origin::Unclear => unclear_currencies += 1,
                    Origin::Existing => {}
                }
            }
        }

        Warband {
            installed: self.imp().watch.borrow().is_some(),
            bank_items,
            currencies,
            earned_currencies,
            transferred_currencies,
            unclear_currencies,
            written_at: self.imp().collected_at.get(),
        }
    }

    // -- the collector addon --------------------------------------------------

    /// Find the WoW install and start watching the addon's file.
    ///
    /// Failing to find one is not an error worth saying anything about: the
    /// addon is optional, and most of the application works without it. What it
    /// costs is that every already-earned achievement has to be assumed
    /// poisoned, and the run page says so where somebody can see it.
    fn start_watching(&self) {
        let wow = self
            .imp()
            .settings
            .borrow()
            .wow_path
            .clone()
            .or_else(|| crate::model::settings::find_wow(&glib::home_dir()));

        let Some(wow) = wow else { return };
        if self.imp().settings.borrow().wow_path.is_none() {
            self.imp().settings.borrow_mut().wow_path = Some(wow.clone());
            self.save_settings();
        }

        // Drop chances, if Rarity is installed beside our own addon. Read once
        // here rather than on every redraw: it is sixty-odd files off a disk
        // and it changes only when somebody updates an addon, which is not
        // something that happens while the window is open.
        let chances = crate::model::rarity::read(&wow);
        if !chances.is_empty() {
            *self.imp().chances.borrow_mut() = chances;
            self.imp().collections_dirty.set(true);
        }

        // One account is the normal case. More than one means several
        // Battle.net logins have used this install, and the first is as good a
        // guess as any — the file itself names the characters, so a wrong guess
        // shows up as attributions for characters not on this roster rather
        // than as silently wrong data.
        let Some(account) = crate::model::addon::accounts(&wow).into_iter().next() else {
            return;
        };

        let app = self.clone();
        match Watch::new(&wow, &account, move |result| app.collected(result)) {
            Ok(watch) => *self.imp().watch.borrow_mut() = Some(watch),
            Err(error) => eprintln!("armory: could not watch the addon file: {error}"),
        }
    }

    /// The addon wrote. Store what it said and re-plan.
    ///
    /// This is the whole application when there is no API client — roster,
    /// collections, achievements and all. Blizzard's developer portal has been
    /// refusing to create clients since late 2025, and an application that
    /// cannot be used without it is an application that cannot be used.
    fn collected(&self, result: Result<Dump, ReadError>) {
        let dump = match result {
            Ok(dump) => dump,
            // The folder holds every addon's file and a watcher will see them
            // all. "Not ours" is not a fault.
            Err(ReadError::NotCollectorData) => return,
            Err(error) => {
                self.window()
                    .toast(&format!("The collector addon's file: {error}"));
                return;
            }
        };

        let Dump {
            collected,
            characters,
            sessions,
        } = dump;
        self.imp().collected_at.set(collected.written_at);

        // The evenings, before anything else: they are the only thing here the
        // addon is the sole record of. The collector's tables can be rebuilt
        // from a later logout, but the addon keeps its last forty sessions and
        // drops the rest, so a session not filed now can be gone for good.
        if !sessions.is_empty() {
            match self.store().borrow_mut().save_sessions(&sessions) {
                Ok(0) => {}
                Ok(added) => {
                    self.window().toast(&match added {
                        1 => "One new evening in the Chronicle".to_string(),
                        added => format!("{added} new evenings in the Chronicle"),
                    });
                    // Only if somebody asked for that. Spending their credit as
                    // a side effect of the game writing a file is not something
                    // to do because it would be convenient.
                    if self.imp().settings.borrow().journal_automatic {
                        self.write_all();
                    }
                }
                Err(error) => eprintln!("armory: could not keep the sessions: {error}"),
            }
        }

        // The roster, with no web API involved. Merged rather than replacing:
        // a sync may have found characters this player has never logged in on,
        // and the addon knowing nothing about them is not evidence they are
        // gone.
        if !characters.is_empty() {
            let mut roster = self.imp().roster.borrow().clone();
            let mut details = self.imp().details.borrow().clone();

            for read in &characters {
                roster
                    .characters
                    .retain(|existing| existing.key != read.character.key);
                roster.characters.push(read.character.clone());
                // Absorbed rather than assigned. The addon cannot see the
                // Mythic+ rating, the renown, the account's achievement points
                // or a lifetime of raiding, and writing its answer over the
                // whole struct blanked all four on every logout.
                details
                    .entry(read.character.key.clone())
                    .or_default()
                    .absorb(read.detail.clone());
            }

            let roster = Roster::new(roster.characters);
            let _ = self.store().borrow_mut().save_roster(&roster);
            for (key, detail) in &details {
                let _ = self.store().borrow().save_detail(key, detail);
            }
            *self.imp().roster.borrow_mut() = roster;
            *self.imp().details.borrow_mut() = details;
        }

        // What the collections page shows, with sources the web API cannot
        // express: "Drop: Attumen the Huntsman, Karazhan" rather than "DROP".
        if !collected.collectibles.is_empty() {
            self.imp().collections_dirty.set(true);
            let _ = self
                .store()
                .borrow_mut()
                .save_collectibles(&collected.collectibles);
            for kind in Kind::ALL {
                let owned: HashSet<u32> = collected
                    .owned
                    .iter()
                    .filter(|(owned_kind, _)| *owned_kind == kind)
                    .map(|(_, id)| *id)
                    .collect();
                let _ = self.store().borrow_mut().save_owned(kind, &owned);
            }
        }

        {
            let mut inputs = self.imp().inputs.borrow_mut();
            inputs.attributions = collected.earned_by.clone();
            inputs.criteria = collected.criteria.clone();

            // Names, so the interface reads "Loremaster of Kalimdor" rather
            // than "Achievement 4956". The API fills this in 200 at a time and
            // the addon does the lot in one go, so the addon wins where both
            // are present.
            for (id, achievement) in &collected.catalogue {
                inputs.catalogue.insert(*id, achievement.clone());
            }

            // Only when the API has not already supplied a richer list: its
            // criteria trees are properly nested where the addon's are one
            // level deep, and a nested tree measures a meta-achievement better.
            if inputs.progress.is_empty() {
                inputs.progress = collected.progress();
            }
            for read in &characters {
                inputs
                    .primary
                    .entry(read.character.key.clone())
                    .or_default()
                    .quests = read.quests.clone();
            }
        }

        let catalogue: Vec<Achievement> = collected.catalogue.values().cloned().collect();
        if !catalogue.is_empty() {
            let _ = self.store().borrow_mut().save_achievements(&catalogue);
        }
        let _ = self.store().borrow_mut().save_collected(&collected);

        // Read back rather than taken from the dump, and after the write. The
        // store merges by taking the larger of the two, so a reinstalled addon
        // that has started counting from zero does not erase a year of a
        // character's work — and planning against the dump directly would use
        // exactly the numbers that merge exists to protect against.
        if !collected.earned.is_empty() {
            let merged = self.store().borrow().provenance().unwrap_or_default();
            self.imp().inputs.borrow_mut().provenance = merged;
        }

        // Attribution is what decides poisoning, so a fresh set changes the
        // standing of every already-earned goal. That is a re-plan, not a
        // re-measure.
        self.replan();
    }

    // -- the journal ---------------------------------------------------------

    /// Where the `llama-server` writing the entries is.
    fn journal_server(&self) -> String {
        self.imp().settings.borrow().journal_server.clone()
    }

    /// A client with a patience appropriate to a language model.
    ///
    /// Its own, not the one the rest of the application syncs through. Twenty
    /// seconds is the right ceiling for a database lookup and a guarantee this
    /// feature never completes: a model on a busy machine takes a minute to
    /// write four hundred words, and it is generating the whole time. The rate
    /// gate wants separating for the same reason — a local server has no quota
    /// and one journal entry has no business spending a sync's budget.
    fn journal_http(&self) -> Http {
        self.imp()
            .journal_http
            .get_or_init(|| Http::with_timeout(JOURNAL_TIMEOUT))
            .clone()
    }

    /// Ask the server what it is running, and remember the answer.
    ///
    /// One call, at startup and whenever the address changes. It is what puts a
    /// model's name on an entry: llama-server serves whatever it was launched
    /// with and ignores the `model` field of a request entirely, so the request
    /// cannot say and only `/props` can.
    ///
    /// Also the readiness check. Nothing else here can tell "no server running"
    /// from "server running and slow", and the difference decides whether the
    /// page offers to write or offers to fix the address.
    fn identify_journal(&self, then: Option<JournalDialog>) {
        let server = self.journal_server();
        let app = self.clone();
        self.journal_http()
            .fetch(journal::identify(&server), move |outcome| {
                let named = outcome
                    .found()
                    .and_then(|response| journal::parse_identity(&response.body));

                app.imp().journal_ready.set(named.is_some());
                *app.imp().journal_model.borrow_mut() = named.clone();

                if let Some(dialog) = then {
                    match &named {
                        Some(model) => dialog.set_status("Answering", model),
                        None => dialog.set_status(
                            "Nothing answered",
                            "Check the address, and that llama-server is running.",
                        ),
                    }
                }
                app.refresh_views();
            });
    }

    fn show_journal_setup(&self) {
        let settings = self.imp().settings.borrow().clone();
        let dialog = JournalDialog::new(&settings.journal_server, settings.journal_automatic);

        if let Some(model) = self.imp().journal_model.borrow().as_ref() {
            dialog.set_status("Answering", model);
        }

        let app = self.clone();
        dialog.connect_save(move |server, automatic| {
            {
                let mut settings = app.imp().settings.borrow_mut();
                settings.journal_automatic = automatic;
                settings.journal_server = server;
            }
            app.save_settings();
            // The address may have changed, so what is running there may have
            // too — and the flag the page draws from is the answer to that.
            app.identify_journal(None);
        });

        let app = self.clone();
        let held = dialog.clone();
        dialog.connect_test(move |server| {
            // Tested against what is in the field rather than what is saved, so
            // somebody can check an address before committing to it.
            app.imp().settings.borrow_mut().journal_server = server;
            app.identify_journal(Some(held.clone()));
        });

        dialog.present(Some(&self.window()));
    }

    /// Write up one evening.
    ///
    /// Guarded against a second press while one is in flight: two entries for
    /// the same evening is one of them wasted, and a local model is busy for
    /// long enough that somebody will press again.
    fn write_entry(&self, id: SessionId) {
        // Already in flight, usually because somebody pressed a card's own
        // button and then asked for everything. Both of these keep the queue
        // moving rather than stalling it silently with entries left in it —
        // each id appears once, so this cannot loop.
        if self.imp().writing.borrow().contains(&id) {
            self.write_next();
            return;
        }

        let store = self.store();
        let Some(session) = store
            .borrow()
            .sessions(SESSIONS_SHOWN)
            .unwrap_or_default()
            .into_iter()
            .find(|session| session.id() == id)
        else {
            self.write_next();
            return;
        };

        self.imp().writing.borrow_mut().push(id.clone());
        self.window().chronicle_page().set_writing(&id, true);

        let app = self.clone();
        let request = journal::write(&self.journal_server(), &session.digest());
        self.journal_http().fetch(request, move |outcome| {
            app.imp().writing.borrow_mut().retain(|held| held != &id);
            app.window().chronicle_page().set_writing(&id, false);

            let written = match outcome {
                Outcome::Found(response) => journal::parse_written(&response.body),
                Outcome::Empty => Outcome::Empty,
                Outcome::Unchanged => Outcome::Unchanged,
                Outcome::Unusable(reason) => Outcome::Unusable(reason),
                Outcome::Stale(reason) => Outcome::Stale(reason),
            };

            match written {
                Outcome::Found(written) => {
                    let entry = Entry {
                        session: id.clone(),
                        title: written.title,
                        body: written.body,
                        // What `/props` said, where it said anything: the
                        // completion response names whatever the request asked
                        // for, which llama-server ignores.
                        model: app
                            .imp()
                            .journal_model
                            .borrow()
                            .clone()
                            .unwrap_or(written.model),
                        written_at: Utc::now(),
                    };
                    if let Err(error) = app.store().borrow().save_entry(&entry) {
                        app.window()
                            .toast(&format!("Could not keep the entry: {error}"));
                    }
                    app.refresh_views();
                    app.write_next();
                }
                // A model that answered with nothing is not a failure to
                // report as one, but it is also not an entry.
                Outcome::Empty | Outcome::Unchanged => {
                    app.window().toast("The entry came back empty");
                    app.abandon_queue();
                }
                Outcome::Unusable(reason) | Outcome::Stale(reason) => {
                    app.window().toast(&format!("No entry written: {reason}"));
                    app.abandon_queue();
                }
            }
        });
    }

    /// Stop a run of entries after one of them failed.
    ///
    /// A bad key or an empty balance fails identically thirty times in a row,
    /// and thirty toasts saying so is worse than one. It also stops a queue
    /// billing somebody for calls that are not going to work.
    fn abandon_queue(&self) {
        let abandoned = self.imp().queued.borrow_mut().drain(..).len();
        if abandoned > 0 {
            self.window()
                .toast(&format!("Stopped — {abandoned} evenings left unwritten"));
        }
    }

    /// Write up everything that has not been written yet.
    ///
    /// Deliberately sequential rather than a fan-out: this is the one action
    /// here that bills somebody per call, and a burst of thirty is a bill
    /// nobody agreed to before seeing the first one. Each entry starts the
    /// next, so pressing it once on a fresh install writes the oldest, then the
    /// next, and any failure stops the queue rather than repeating itself
    /// thirty times.
    fn write_all(&self) {
        if !self.imp().journal_ready.get() {
            self.window()
                .toast("No model is answering — check the address in Journal Setup");
            self.show_journal_setup();
            return;
        }
        let store = self.store();
        let entries = store.borrow().entries().unwrap_or_default();
        let pending: Vec<SessionId> = store
            .borrow()
            .sessions(SESSIONS_SHOWN)
            .unwrap_or_default()
            .into_iter()
            .filter(|session| session.digest().is_worth_writing())
            .map(|session| session.id())
            .filter(|id| !entries.contains_key(id))
            .collect();

        match pending.len() {
            0 => self.window().toast("Every evening is already written up"),
            count => {
                self.window()
                    .toast(&format!("Writing {count} entries, one at a time"));
                *self.imp().queued.borrow_mut() = pending;
                self.write_next();
            }
        }
    }

    fn write_next(&self) {
        let Some(id) = self.imp().queued.borrow_mut().pop() else {
            return;
        };
        self.write_entry(id);
    }

    // -- signing in ----------------------------------------------------------

    fn credentials(&self) -> Option<ClientCredentials> {
        let id = self.imp().settings.borrow().client_id.clone();
        if id.trim().is_empty() {
            return None;
        }
        let secret = self.stored_secret()?;
        Some(ClientCredentials { id, secret })
    }

    /// The client secret, if the keyring is holding one.
    fn stored_secret(&self) -> Option<String> {
        Keyring::open().ok()?.lookup(CLIENT_SECRET).ok().flatten()
    }

    /// Keep the access token, so a relaunch inside the day does not need a
    /// browser.
    ///
    /// Battle.net issues no refresh token — its own staff have said so — which
    /// means the only way back from an expired session is the whole
    /// authorization flow. The token itself lasts twenty-four hours, so storing
    /// it turns "sign in again every launch" into "sign in again tomorrow".
    ///
    /// The keyring rather than the settings file, because a bearer token is a
    /// credential: anything holding it can read this account's profile until it
    /// expires.
    fn remember_token(&self, token: &Token) {
        let until = Utc::now() + chrono::Duration::seconds(token.expires_in);
        let held = serde_json::json!({ "access": token.access, "until": until.to_rfc3339() });

        if let Ok(keyring) = Keyring::open() {
            let _ = keyring.store(
                ACCESS_TOKEN,
                &held.to_string(),
                "Armory — Battle.net session",
            );
        }
    }

    /// The stored token, if there is one and it has not expired.
    fn restore_token(&self) {
        let Some(held) = Keyring::open()
            .ok()
            .and_then(|keyring| keyring.lookup(ACCESS_TOKEN).ok().flatten())
        else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&held) else {
            return;
        };
        let (Some(access), Some(until)) = (
            value.get("access").and_then(|access| access.as_str()),
            value
                .get("until")
                .and_then(|until| until.as_str())
                .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok()),
        ) else {
            return;
        };

        // A minute of margin. A token that expires mid-sync fails every call
        // after it does, and the failures read as Blizzard being down.
        let remaining = until.to_utc() - Utc::now();
        if remaining < chrono::Duration::minutes(1) {
            let _ = Keyring::open().map(|keyring| keyring.clear(ACCESS_TOKEN));
            return;
        }

        *self.imp().token.borrow_mut() = Some(Token {
            access: access.to_string(),
            expires_in: remaining.num_seconds(),
            refresh: None,
        });
    }

    /// Forget the session, on the way out of one.
    fn forget_token(&self) {
        *self.imp().token.borrow_mut() = None;
        if let Ok(keyring) = Keyring::open() {
            let _ = keyring.clear(ACCESS_TOKEN);
        }
    }

    /// Begin the authorization code flow.
    fn sign_in(&self) {
        let window = self.window();
        let onboarding = window.onboarding();

        // A blank secret field means "use the one you already have", not "I
        // have no secret". The field cannot be pre-filled — putting the stored
        // secret into a widget means reading it out of the keyring to display
        // it, and a field of dots from storage looks exactly like a field of
        // dots somebody typed — so an empty one falls back to the keyring
        // rather than refusing. Without this, a Battle.net session lapsing
        // after its day sends somebody hunting for a secret Armory is already
        // holding.
        let typed = onboarding.client_secret();
        let secret = if typed.is_empty() {
            self.stored_secret().unwrap_or_default()
        } else {
            typed
        };

        let client = ClientCredentials {
            id: onboarding.client_id(),
            secret,
        };
        if client.id.is_empty() {
            onboarding.report(Some("Fill in the client ID first."));
            return;
        }
        if client.secret.is_empty() {
            onboarding.report(Some(
                "Fill in the client secret. Armory has none saved for this client.",
            ));
            return;
        }

        // Save the id and region now, so a failed sign-in does not make someone
        // paste them again. The secret goes to the keyring only once Battle.net
        // has confirmed it works.
        {
            let mut settings = self.imp().settings.borrow_mut();
            settings.client_id = client.id.clone();
            settings.region = onboarding.region();
        }
        self.save_settings();

        // Not a cryptographic nonce and not pretending to be one: with no PKCE
        // available this only has to be unpredictable enough that a redirect
        // Armory did not start does not match one it did.
        let state = format!(
            "{:x}{:x}",
            glib::real_time(),
            glib::random_int() as u64 ^ glib::monotonic_time() as u64
        );

        let app = self.clone();
        let for_exchange = client.clone();
        let redirect = Redirect::listen(&state, move |result| {
            let onboarding = app.window().onboarding();
            match result {
                Ok(code) => app.exchange(&for_exchange, &code),
                Err(reason) => {
                    onboarding.set_busy(false);
                    onboarding.report(Some(&format!("Sign-in did not finish: {reason}")));
                    *app.imp().redirect.borrow_mut() = None;
                }
            }
        });

        let redirect = match redirect {
            Ok(redirect) => redirect,
            Err(error) => {
                onboarding.report(Some(&format!(
                    "Armory could not listen on port {} for Battle.net to send you \
                     back ({error}). Close whatever is using that port and try again.",
                    oauth::REDIRECT_PORT
                )));
                return;
            }
        };
        *self.imp().redirect.borrow_mut() = Some(redirect);

        onboarding.set_busy(true);
        onboarding.report(Some(
            "Armory opened Battle.net in your browser. Sign in there, and this \
             window will carry on by itself.",
        ));

        let launcher = gtk::UriLauncher::new(&oauth::authorize_url(&client, &state));
        launcher.launch(Some(&window), gio::Cancellable::NONE, |_| {});
    }

    /// Show the setup page again.
    ///
    /// Nothing is cleared. Somebody coming back to add a client has not asked
    /// to lose their run, their enrolments or the addon data they already have.
    fn show_setup(&self) {
        let window = self.window();
        let settings = self.imp().settings.borrow().clone();
        window.onboarding().set_client_id(&settings.client_id);
        window.onboarding().set_region(settings.region);
        window
            .onboarding()
            .set_secret_held(self.stored_secret().is_some());
        window.onboarding().report(None);
        window.show_onboarding(true);
    }

    /// Go on without a Battle.net client.
    ///
    /// Recorded in settings so the choice survives a restart, and so onboarding
    /// does not reappear every launch asking for something the person has
    /// already declined.
    fn skip_api(&self) {
        self.imp().settings.borrow_mut().addon_only = true;
        self.save_settings();

        let window = self.window();
        window.show_onboarding(false);

        if self.imp().watch.borrow().is_none() {
            window.toast(
                "Install the collector addon and log in once — Armory has nothing to \
                 read yet.",
            );
        }
        self.refresh_views();
    }

    /// Trade the code for a token, and remember the secret if it worked.
    fn exchange(&self, client: &ClientCredentials, code: &str) {
        let app = self.clone();
        let client = client.clone();
        self.http()
            .fetch(oauth::exchange(&client, code), move |outcome| {
                let onboarding = app.window().onboarding();
                onboarding.set_busy(false);
                *app.imp().redirect.borrow_mut() = None;

                let response = match outcome {
                    Outcome::Found(response) => response,
                    other => {
                        let reason = other
                            .gap()
                            .unwrap_or(Reason::Malformed("no answer from Battle.net".into()));
                        onboarding.report(Some(&format!("Battle.net refused: {reason}")));
                        return;
                    }
                };

                match oauth::parse_token(&response.body) {
                    Outcome::Found(token) => {
                        // Only now is the secret known to work, which is the
                        // right moment to keep it.
                        match Keyring::open().and_then(|keyring| {
                            keyring.store(
                                CLIENT_SECRET,
                                &client.secret,
                                "Armory — Battle.net API client secret",
                            )
                        }) {
                            Ok(()) => onboarding.set_secret_held(true),
                            Err(error) => {
                                // Signed in, but it will need doing again next
                                // launch. Better said out loud than discovered.
                                app.window().toast(&format!(
                                    "Signed in, but the secret could not be saved: {error}"
                                ));
                            }
                        }

                        app.remember_token(&token);
                        *app.imp().token.borrow_mut() = Some(token);
                        app.window().show_onboarding(false);
                        app.sync();
                    }
                    other => {
                        let reason = other
                            .gap()
                            .unwrap_or(Reason::Malformed("an empty token".into()));
                        onboarding.report(Some(&format!("Battle.net refused: {reason}")));
                    }
                }
            });
    }

    fn sign_out(&self) {
        self.forget_token();
        if let Ok(keyring) = Keyring::open() {
            let _ = keyring.clear(CLIENT_SECRET);
        }
        {
            let mut settings = self.imp().settings.borrow_mut();
            settings.client_id.clear();
            // Otherwise the next launch hides setup and there is no way back to
            // it — signing out would strand somebody on a page they cannot
            // leave.
            settings.addon_only = false;
        }
        self.save_settings();

        let window = self.window();
        window.onboarding().set_client_id("");
        window.onboarding().set_secret_held(false);
        window.onboarding().report(None);
        window.show_onboarding(true);
        window.toast("Signed out. Your characters and enrolments are still here.");
    }

    // -- syncing -------------------------------------------------------------

    fn sync(&self) {
        if self.imp().syncing.get() {
            return;
        }
        let Some(client) = self.credentials() else {
            self.window()
                .toast("Set up a Battle.net client first, under Setup.");
            return;
        };

        // A token lasts a day and is not persisted — Blizzard's own staff say
        // refresh tokens are not issued, and the discovery document disagrees,
        // so nothing here depends on either. A missing token means the sign-in
        // has to be done again, and saying so beats a silent failure.
        let Some(token) = self.imp().token.borrow().clone() else {
            let _ = client;
            self.window().toast(
                "Sign in again to sync — Battle.net sessions last a day. Your secret is \
                 still saved, so the sign-in button is all it takes.",
            );
            // Through `show_setup` rather than straight to the page, so the
            // secret field says the secret is still held. Arriving at a blank
            // password box after a session lapses is what makes somebody go
            // looking for a value Armory already has.
            self.show_setup();
            return;
        };

        self.imp().syncing.set(true);
        let generation = self.imp().generation.get() + 1;
        self.imp().generation.set(generation);

        let window = self.window();
        window.set_busy(true);

        let region = self.imp().settings.borrow().region;
        let request = profile::account(region).bearer(&token.access);
        let url = request.url.clone();

        // Conditional on what we already hold. A character who has not played
        // since the last sync costs one round trip and no body.
        let request = match self.store().borrow().last_modified(&url) {
            Ok(Some(stamp)) => request.if_modified_since(&stamp),
            _ => request,
        };

        let app = self.clone();
        self.http().fetch(request, move |outcome| {
            if app.imp().generation.get() != generation {
                return;
            }
            app.imp().syncing.set(false);
            app.finish_account_sync(&url, outcome);

            // The roster is what says who is enrolled, so the fan-out can only
            // start once it has landed. The spinner stays up until the last of
            // those calls returns.
            match app.imp().token.borrow().clone() {
                Some(token) => {
                    app.sync_cohort(&token, generation);
                    app.sync_run_inputs(&token, generation);
                    app.sync_catalogue(&token, generation);
                    app.sync_collections(&token, generation);
                    app.sync_market(&token, generation);
                    // Artwork last and capped: it is the part of a sync nothing
                    // depends on, and a page that draws with placeholders is
                    // still a page that works.
                    app.sync_media(&token, generation, ART_PER_SYNC);
                    app.sync_item_names(&token, generation, NAMES_PER_SYNC);
                    app.sync_guide(&token, generation, GUIDE_PER_SYNC);
                }
                None => app.window().set_busy(false),
            }
            if app.imp().outstanding.get() == 0 {
                app.window().set_busy(false);
            }
        });
    }

    fn finish_account_sync(&self, url: &str, outcome: Outcome<super::http::Response>) {
        let window = self.window();
        let store = self.store();

        match outcome {
            Outcome::Unchanged => {
                let _ = store.borrow().touch_response(url);
                window.toast("Nothing has changed since the last sync.");
            }
            Outcome::Found(response) => match profile::parse_account(&response.body) {
                Outcome::Found(roster) => {
                    let _ = store.borrow().store_response(
                        url,
                        &response.body,
                        response.last_modified.as_deref(),
                    );
                    let _ = store.borrow_mut().save_roster(&roster);

                    let mut cohort = self.imp().cohort.borrow().clone();
                    cohort.prune(&roster);
                    let _ = store.borrow_mut().save_cohort(&cohort);

                    let count = roster.len();
                    *self.imp().roster.borrow_mut() = roster;
                    *self.imp().cohort.borrow_mut() = cohort;
                    self.refresh_views();
                    window.toast(&format!("Synced {count} characters."));
                }
                other => {
                    let reason = other.gap().unwrap_or(Reason::Malformed(
                        "Blizzard sent an account with no characters".into(),
                    ));
                    window.toast(&format!("Could not read the account: {reason}"));
                }
            },
            Outcome::Empty => {
                window.toast("Battle.net has no characters for this account yet.");
            }
            other => {
                if let Some(reason) = other.gap() {
                    window.toast(&format!("Sync failed: {reason}"));
                    if matches!(reason, Reason::Unauthorised(_)) {
                        // The stored copy is the same dead token. Leaving it
                        // there would restore it at the next launch and fail
                        // again in the same way.
                        self.forget_token();
                    }
                }
            }
        }
    }

    /// Fetch the expensive half for every enrolled character.
    ///
    /// One fan-out, no waiting: each call's callback lands when it lands and
    /// redraws the row it belongs to. A character whose professions time out
    /// still gets an item level, which is why every field on [`Detail`] is
    /// optional and why these are merged rather than assigned.
    ///
    /// Every request is conditional on the `Last-Modified` already held. A
    /// character who has not logged out since the last sync answers `304` with
    /// no body, so most of a sync of twenty-three characters is round trips and
    /// almost no bytes.
    fn sync_cohort(&self, token: &Token, generation: u64) {
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow().clone();
        let cohort = self.imp().cohort.borrow().clone();

        for character in cohort.members(&roster) {
            let key = character.key.clone();

            self.fetch_detail(token, generation, &key, profile::summary(region, &key), {
                move |detail, body| {
                    if let Outcome::Found(summary) = profile::parse_summary(&body) {
                        detail.item_level = summary.item_level;
                        detail.equipped_item_level = summary.equipped_item_level;
                        detail.spec = summary.spec;
                        detail.guild = summary.guild;
                        detail.achievement_points = summary.achievement_points;
                        detail.last_login = summary.last_login;
                    }
                }
            });

            self.fetch_detail(
                token,
                generation,
                &key,
                profile::professions(region, &key),
                move |detail, body| {
                    if let Outcome::Found(professions) = profile::parse_professions(&body) {
                        // Merged, not assigned, for the same reason
                        // `save_collectibles` is. The API has the expansion
                        // tier and the addon has the specialisation trees and
                        // the knowledge spent, and neither knows the other's
                        // half — so a profession sync after a logout must not
                        // take the trees off every character.
                        detail.professions = professions
                            .into_iter()
                            .map(|mut fetched| {
                                if let Some(known) = detail
                                    .professions
                                    .iter()
                                    .find(|held| held.name == fetched.name)
                                {
                                    fetched.specialisations.clone_from(&known.specialisations);
                                    fetched.knowledge = known.knowledge;
                                }
                                fetched
                            })
                            .collect();
                    }
                },
            );

            self.fetch_detail(
                token,
                generation,
                &key,
                profile::mythic_keystone(region, &key),
                move |detail, body| {
                    detail.mythic_rating = profile::parse_mythic_keystone(&body).found();
                },
            );

            // Assigned rather than merged, unlike the professions above: the
            // addon and the API answer this one in the same shape and neither
            // holds half of it, so whichever looked last is simply the more
            // recent look. `Outcome::Empty` is left alone on purpose — a
            // character who came back naked is a real answer and a parser that
            // has stopped understanding the response is not, and only `Found`
            // is evidence of either.
            self.fetch_detail(
                token,
                generation,
                &key,
                profile::equipment(region, &key),
                move |detail, body| {
                    if let Outcome::Found(worn) = profile::parse_equipment(&body) {
                        detail.equipment = Some(worn);
                    }
                },
            );

            // Lifetime raid progress. The addon cannot answer this one — the
            // client knows only the current lockout — so nothing here has to
            // be careful about overwriting its half.
            //
            // The same URL `sync_run_inputs` reads for encounter *ids*, asked
            // again for what a person would say about it. Two accumulators and
            // one body: the second ask carries `If-Modified-Since` like every
            // other, so it costs a round trip and no bytes, which is cheaper
            // than threading a second output through `fetch_input`.
            self.fetch_detail(
                token,
                generation,
                &key,
                profile::raid_encounters(region, &key),
                move |detail, body| {
                    if let Outcome::Found(raids) = profile::parse_raids(&body) {
                        detail.raids = Some(raids);
                    }
                },
            );

            self.fetch_detail(
                token,
                generation,
                &key,
                profile::reputations(region, &key),
                move |detail, body| {
                    if let Ok(value) = serde_json::from_slice(&body) {
                        detail.renown = profile::highest_renown(&value);
                    }
                },
            );

            // Gold, from the one endpoint that has it. Addressed by numeric ids
            // rather than by slug and name, so it needs the whole character.
            self.fetch_detail(
                token,
                generation,
                &key,
                profile::protected_character(region, character),
                move |detail, body| {
                    detail.money = profile::parse_protected(&body).found();
                },
            );
        }
    }

    /// Fetch the achievement progress and primary data a run is measured from.
    ///
    /// Achievements come from one enrolled character rather than from all of
    /// them: the response is account-wide, so asking every character would be
    /// twenty-three copies of the same answer. Primary data — quests,
    /// statistics, encounters — is genuinely per character and is asked for
    /// each one.
    fn sync_run_inputs(&self, token: &Token, generation: u64) {
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow().clone();
        let cohort = self.imp().cohort.borrow().clone();

        let Some(first) = cohort.members(&roster).first().map(|c| c.key.clone()) else {
            return;
        };

        self.fetch_input(
            token,
            generation,
            profile::achievements(region, &first),
            move |inputs, body| {
                if let Outcome::Found(progress) = profile::parse_achievements(&body) {
                    inputs.progress = progress;
                }
            },
        );

        for character in cohort.members(&roster) {
            let key = character.key.clone();
            let level = character.level;

            let for_quests = key.clone();
            self.fetch_input(
                token,
                generation,
                profile::completed_quests(region, &key),
                move |inputs, body| {
                    if let Outcome::Found(quests) = profile::parse_completed_quests(&body) {
                        inputs.primary.entry(for_quests).or_default().quests = quests;
                    }
                },
            );

            let for_statistics = key.clone();
            self.fetch_input(
                token,
                generation,
                profile::statistics(region, &key),
                move |inputs, body| {
                    if let Outcome::Found(statistics) = profile::parse_statistics(&body) {
                        inputs.primary.entry(for_statistics).or_default().statistics = statistics;
                    }
                },
            );

            let for_reputations = key.clone();
            let app = self.clone();
            self.fetch_input(
                token,
                generation,
                profile::reputations(region, &key),
                move |inputs, body| {
                    if let Outcome::Found(reputations) = profile::parse_reputations(&body, level) {
                        app.imp()
                            .reputations
                            .borrow_mut()
                            .insert(for_reputations.clone(), reputations.detail);
                        let entry = inputs.primary.entry(for_reputations).or_default();
                        entry.reputations = reputations.standings;
                        entry.inherited_reputations = reputations.inherited;
                    }
                },
            );

            // Dungeons and raids land in the same set: a criterion refers to an
            // encounter by id and does not care which endpoint it came from.
            for request in [
                profile::dungeon_encounters(region, &key),
                profile::raid_encounters(region, &key),
            ] {
                let for_encounters = key.clone();
                self.fetch_input(token, generation, request, move |inputs, body| {
                    if let Outcome::Found(encounters) = profile::parse_encounters(&body) {
                        inputs
                            .primary
                            .entry(for_encounters)
                            .or_default()
                            .encounters
                            .extend(encounters);
                    }
                });
            }
        }
    }

    /// Sync mounts, pets and toys: what exists, and what the account has.
    ///
    /// The owned lists are account-wide, so one call each rather than one per
    /// character. The catalogue index is one call each too, and comes back with
    /// names and no sources — sources are one call per entry, so they fill in a
    /// batch at a time the same way achievement names do.
    fn sync_collections(&self, token: &Token, generation: u64) {
        const DETAILS_PER_SYNC: usize = 150;
        let region = self.imp().settings.borrow().region;

        for kind in Kind::ALL {
            let app = self.clone();
            self.fetch_bare(
                token,
                generation,
                collections::collected(region, kind),
                move |body| {
                    if let Outcome::Found(owned) = collections::parse_collected(&body, kind) {
                        let _ = app.store().borrow_mut().save_owned(kind, &owned);
                        app.imp().collections_dirty.set(true);
                        // The baseline's idea of what is already spent. A goal
                        // for a mount the account owns is excluded, and this is
                        // where that set comes from.
                        app.imp().inputs.borrow_mut().owned.extend(owned);
                    }
                },
            );

            let app = self.clone();
            let token = token.clone();
            self.fetch_bare(
                token.clone(),
                generation,
                collections::index(region, kind),
                move |body| {
                    let Outcome::Found(catalogue) = collections::parse_index(&body, kind) else {
                        return;
                    };
                    let _ = app.store().borrow_mut().save_collectibles(&catalogue);
                    app.imp().collections_dirty.set(true);

                    // Sources, for the entries that do not have one yet.
                    let known = app
                        .store()
                        .borrow()
                        .collectibles(kind)
                        .map(|(entries, _)| entries)
                        .unwrap_or_default();
                    let wanted: Vec<u32> = known
                        .iter()
                        .filter(|entry| {
                            entry.source == Source::Unknown && entry.link_id == entry.id
                        })
                        .map(|entry| entry.id)
                        .take(DETAILS_PER_SYNC)
                        .collect();

                    for id in wanted {
                        let app = app.clone();
                        app.clone().fetch_bare(
                            token.clone(),
                            generation,
                            collections::detail(region, kind, id),
                            move |body| {
                                if let Outcome::Found(entry) =
                                    collections::parse_detail(&body, kind)
                                {
                                    let _ = app
                                        .store()
                                        .borrow_mut()
                                        .save_collectibles(std::slice::from_ref(&entry));
                                    app.imp().collections_dirty.set(true);
                                }
                            },
                        );
                    }
                },
            );
        }
    }

    /// Fetch the render URLs that cannot be worked out locally.
    ///
    /// Most of the application's artwork costs nothing: a mount or pet is drawn
    /// from a creature display id the addon recorded, and a class crest from
    /// the class's own name. What is left needs Blizzard to be asked, one
    /// request each, and each of those is a small JSON body carrying a URL:
    ///
    /// - a **character portrait**, which is keyed by a hash only Blizzard holds
    /// - a **toy's icon**, because a toy is an item and an item's art is
    ///   addressed by a texture name nothing local knows
    /// - an **achievement's icon**, same reason
    ///
    /// Capped per sync for the same reason achievement names are: two thousand
    /// toys is two thousand requests, and nobody is looking at two thousand
    /// toys. The ones on screen come first and the rest arrive over following
    /// syncs, or all at once from the menu.
    /// Put names to the ids in the market snapshot, a budget at a time.
    ///
    /// A listing carries an item id and nothing at all else, and there is no
    /// endpoint that turns a list of ids into a list of names — the search
    /// endpoint goes the other way, from a name somebody typed. So this is one
    /// call per item, and the browser is honest about it: an item with no name
    /// yet is shown as its id rather than hidden.
    /// Turn a name somebody typed in the browser into item ids, and keep them.
    ///
    /// The auction house names nothing. A listing is an id, and the names in
    /// the browser arrive one `/data/wow/item/{id}` call at a time, a hundred
    /// and fifty a sync, spent on whatever is at the top of the page. Against a
    /// market of thirty thousand ids that converges slowly and in the wrong
    /// order for somebody hunting one thing: typing "copper ore" matched
    /// nothing, which reads as an empty market rather than as an unnamed one.
    ///
    /// `/data/wow/search/item` goes the other way — a name to its ids — and it
    /// is the same call the watch picker already makes. One small request per
    /// search that found nothing, and the names it answers are written down, so
    /// the same search is free ever after.
    ///
    /// **The names are written and the bindings are not.** A search result
    /// carries no `preview_item`, so `sellable` stays at its default and the
    /// item is treated as unknown by `Place::spoils` — which is what an item
    /// absent from the table already means there, so nothing is claimed that
    /// was not measured. `sync_item_names` still fetches the full record for
    /// anything on screen and fills the binding in properly.
    fn look_up_item(&self, name: &str) {
        let Some(token) = self.imp().token.borrow().clone() else {
            self.window()
                .toast("Sign in to search by name — the catalogue is Blizzard's.");
            return;
        };
        let region = self.imp().settings.borrow().region;
        let generation = self.imp().generation.get();
        let locale = region.default_locale();
        let app = self.clone();

        self.clone().fetch_bare(
            &token,
            generation,
            gamedata::item_search(region, name),
            move |body| {
                let Outcome::Found(found) = gamedata::parse_item_search(&body, locale) else {
                    return;
                };
                if found.is_empty() {
                    return;
                }
                {
                    let store = app.store();
                    let store = store.borrow();
                    for (id, name) in &found {
                        let _ = store.name_found_item(*id, name);
                    }
                }
                app.refresh_views();
            },
        );
    }

    fn sync_item_names(&self, token: &Token, generation: u64, budget: usize) {
        let region = self.imp().settings.borrow().region;
        let wanted = self.window().market_page().wants_names(budget);

        for id in wanted {
            let app = self.clone();
            self.fetch_bare(token, generation, gamedata::item(region, id), move |body| {
                // The one call answers the name *and* whether the thing can be
                // sold, which is what decides if it is worth looking for.
                if let Outcome::Found(item) = gamedata::parse_item(&body) {
                    let _ = app.store().borrow().name_item(id, &item);
                    app.imp().names_arrived.set(true);
                }
            });
        }
    }

    /// Fill in the Adventure Guide, a few at a time.
    ///
    /// The guide is what says *what a dungeon was* — a paragraph per instance
    /// and a paragraph per boss, written as the premise rather than as a plot
    /// summary — and it is the only place that says it. It also carries each
    /// encounter's loot, which is how an item is traced back to a boss, a boss
    /// to an instance, and an instance to the zone somebody was standing in.
    ///
    /// Nothing here ever changes once fetched, so this converges and then
    /// costs one call for the index.
    fn sync_guide(&self, token: &Token, generation: u64, budget: usize) {
        let region = self.imp().settings.borrow().region;
        let app = self.clone();
        self.fetch_bare(
            token,
            generation,
            gamedata::instance_index(region),
            move |body| {
                let Outcome::Found(known) = gamedata::parse_instance_index(&body) else {
                    return;
                };
                let Ok((instances, encounters)) = app.store().borrow().guide_gaps(&known) else {
                    return;
                };
                app.fill_guide(region, instances, encounters, budget);
            },
        );
    }

    /// Spend the guide's budget: instances first, then their encounters.
    ///
    /// Instances first on purpose. An encounter id is only known *because* an
    /// instance listed it, so a sync that fetched encounters first would have
    /// nothing to fetch — and an instance with no encounters yet still says
    /// what the place was, which is most of the value.
    fn fill_guide(
        &self,
        region: crate::model::source::blizzard::Region,
        instances: Vec<u32>,
        encounters: Vec<u32>,
        budget: usize,
    ) {
        let Some(token) = self.imp().token.borrow().clone() else {
            return;
        };
        let generation = self.imp().generation.get();
        let mut left = budget;

        for id in instances.into_iter().take(left) {
            left = left.saturating_sub(1);
            let app = self.clone();
            self.fetch_bare(
                &token,
                generation,
                gamedata::instance(region, id),
                move |body| {
                    if let Outcome::Found(instance) = gamedata::parse_instance(&body) {
                        let _ = app.store().borrow().save_instance(&instance);
                    }
                },
            );
        }

        for id in encounters.into_iter().take(left) {
            let app = self.clone();
            self.fetch_bare(
                &token,
                generation,
                gamedata::encounter(region, id),
                move |body| {
                    if let Outcome::Found(encounter) = gamedata::parse_encounter(&body) {
                        let _ = app.store().borrow().save_encounter(&encounter);
                    }
                },
            );
        }
    }

    fn sync_media(&self, token: &Token, generation: u64, budget: usize) {
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow().clone();
        let cohort = self.imp().cohort.borrow().clone();

        // Portraits, for the enrolled characters only. The others are drawn
        // from their class crest, which is a true picture of a real thing
        // rather than a placeholder, so there is nothing to fix by spending a
        // request on them.
        for character in cohort.members(&roster) {
            let key = character.key.clone();
            if self.imp().portraits.borrow().contains_key(&key) {
                continue;
            }
            let app = self.clone();
            self.fetch_bare(token, generation, media::character(region, &key), {
                let key = key.clone();
                move |body| {
                    if let Outcome::Found(url) =
                        media::parse_portrait(&body, media::Portrait::Avatar)
                    {
                        app.imp().portraits.borrow_mut().insert(key, url);
                    }
                }
            });
        }

        let window = self.window();

        // Toys and decor. Both are items, and neither has a creature display to
        // be drawn from for nothing. `art_wanted` asks the page rather than the
        // store, because the page knows what is actually in front of somebody.
        for kind in [Kind::Toy, Kind::Decor] {
            for id in window.collection_page(kind).art_wanted(budget) {
                let app = self.clone();
                self.fetch_bare(token, generation, media::item(region, id), move |body| {
                    if let Outcome::Found(url) = media::parse_icon(&body) {
                        app.imp().toy_art.borrow_mut().insert(id, url);
                    }
                });
            }
        }

        for id in window.run_page().art_wanted(budget) {
            let app = self.clone();
            self.fetch_bare(
                token,
                generation,
                media::achievement(region, id),
                move |body| {
                    if let Outcome::Found(url) = media::parse_icon(&body) {
                        app.imp().achievement_art.borrow_mut().insert(id, url);
                    }
                },
            );
        }
    }

    /// Fetch every missing picture, rather than a sync's worth.
    ///
    /// Behind a menu item because it is thousands of requests. They are small
    /// and cached forever after, so it is a minute once rather than a trickle
    /// over a fortnight — but it is the person's quota, and spending it is
    /// their call to make.
    fn fetch_all_art(&self) {
        let Some(token) = self.imp().token.borrow().clone() else {
            self.window()
                .toast("Sign in to fetch artwork — the render URLs come from Blizzard.");
            return;
        };
        let generation = self.imp().generation.get();
        self.window().set_busy(true);
        self.window()
            .toast("Fetching artwork. It is cached once it lands, so this happens once.");
        self.sync_media(&token, generation, usize::MAX);
    }

    /// Fetch prices for the realms and items that were opted into.
    ///
    /// Nothing here happens by default. Commodities are one region-wide call
    /// that costs 25x quota, and each watched realm is a document that has run
    /// 26–28 MB since the cross-faction merge, so this only runs for what
    /// somebody asked for.
    ///
    /// Only the watched items are kept out of each snapshot. Recording every
    /// item on five realms would be the 3 GB a day that makes this shape
    /// impossible.
    ///
    /// The two halves are opted into separately, which is why the item list
    /// being empty no longer skips the realms. A realm snapshot is worth
    /// fetching for the collection join alone — that is what answers "is any of
    /// what I am missing on sale" — and somebody who has added a realm and no
    /// items has asked for exactly that.
    fn sync_market(&self, token: &Token, generation: u64) {
        let region = self.imp().settings.borrow().region;
        let store = self.store();

        let watched: HashSet<u32> = store
            .borrow()
            .watched()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let realms = store.borrow().watched_realms().unwrap_or_default();

        // No early return on an empty watch list any more, and that is the
        // point of the browser: the region-wide commodity market is what it
        // shows, and it has to arrive before anybody has asked for anything.
        // The per-realm loop below does nothing on its own when no realm has
        // been added, which is the only part that was ever realm-specific.

        // The token price is one small call and the only price in the game
        // Blizzard sets rather than players, so it is always worth having.
        let app = self.clone();
        self.fetch_bare(token, generation, auctions::token(region), move |body| {
            if let Outcome::Found(price) = auctions::parse_token(&body) {
                app.imp().token_price.set(Some(price));
            }
        });

        // Commodities: region-wide, realm zero. The expensive call — 25x quota,
        // charged even for a 304 — and made every sync rather than only for a
        // watch list, because browsing the market is a question about the whole
        // of it. No collectible is a commodity: a caged pet is realm-locked and
        // so is everything else with a species or a bonus list.
        {
            let app = self.clone();
            let wanted = watched.clone();
            self.fetch_bare(
                token,
                generation,
                auctions::commodities(region),
                move |body| {
                    if let Outcome::Found(listings) = auctions::parse_commodities(&body) {
                        app.record(0, &listings, &wanted);
                    }
                },
            );
        }

        for (realm_id, _) in realms {
            let app = self.clone();
            let wanted = watched.clone();
            self.fetch_bare(
                token,
                generation,
                auctions::auctions(region, realm_id),
                move |body| {
                    if let Outcome::Found(listings) = auctions::parse_auctions(&body) {
                        app.record(realm_id, &listings, &wanted);
                        app.match_collection(realm_id, &listings);
                    }
                },
            );
        }
    }

    /// Find what the account is missing in one realm's snapshot.
    ///
    /// The join lives in `model::market`; this only supplies the two halves and
    /// keeps the answer. Kept in memory rather than stored: a listing is gone
    /// within the hour, and a bargain from last Tuesday is worse than no
    /// bargain at all.
    fn match_collection(&self, realm: u32, listings: &[auctions::Listing]) {
        let store = self.store();
        let mut found = Vec::new();

        for kind in Kind::ALL {
            let (catalogue, owned) = store.borrow().collectibles(kind).unwrap_or_default();
            if catalogue.is_empty() {
                continue;
            }
            found.extend(market::on_sale(&catalogue, &owned, listings, realm));
        }

        // This realm's answer replaces this realm's previous answer and leaves
        // the others alone: each snapshot speaks for one auction house.
        let mut offers = self.imp().offers.borrow_mut();
        offers.retain(|offer| offer.realm != realm);
        offers.extend(found);
        offers.sort_by(|a, b| {
            a.unit_price
                .cmp(&b.unit_price)
                .then_with(|| a.name.cmp(&b.name))
        });
    }

    // -- choosing what to watch ------------------------------------------------

    /// Offer the region's realms, and fetch the list if it is not held yet.
    ///
    /// The realm index is one call and its answer changes only when Blizzard
    /// opens or merges a realm, so it goes through the ordinary response cache
    /// and is almost always already there.
    fn pick_realm(&self) {
        let region = self.imp().settings.borrow().region;
        let roster = self.imp().roster.borrow().clone();
        // The realms this account plays on, offered first. With thirty-one
        // characters across nine realms, the one somebody wants is nearly
        // always one of theirs.
        let mine: Vec<String> = roster.realms().into_iter().map(|(slug, _)| slug).collect();

        let request = auctions::realm_index(region);
        let held = self
            .store()
            .borrow()
            .response(&request.url, chrono::Duration::days(30))
            .ok()
            .flatten();

        match held.as_deref().map(auctions::parse_realm_index) {
            Some(Outcome::Found(realms)) => self.show_realms(&realms, &mine),
            _ => {
                let Some(token) = self.imp().token.borrow().clone() else {
                    self.window().toast(
                        "Sign in to fetch the realm list — it is the only way to find a \
                         realm's auction house.",
                    );
                    return;
                };
                let app = self.clone();
                let generation = self.imp().generation.get();
                self.fetch_bare(&token, generation, request, move |body| {
                    if let Outcome::Found(realms) = auctions::parse_realm_index(&body) {
                        app.show_realms(&realms, &mine);
                    }
                });
            }
        }
    }

    fn show_realms(&self, realms: &[auctions::Realm], mine: &[String]) {
        let app = self.clone();
        let dialog = WatchDialog::realms(realms, mine, move |realm| app.watch_realm(&realm));
        dialog.present(Some(&self.window()));
    }

    /// Turn a realm into the auction house it trades in, and watch that.
    ///
    /// A realm and its market are different numbers — Terenas is realm 1567 and
    /// trades in connected realm 61 alongside Emerald Dream — so the id chosen
    /// in the list is never the id to fetch with.
    fn watch_realm(&self, realm: &auctions::Realm) {
        let region = self.imp().settings.borrow().region;
        let Some(token) = self.imp().token.borrow().clone() else {
            return;
        };
        let generation = self.imp().generation.get();
        let app = self.clone();
        let name = realm.name.clone();

        self.fetch_bare(
            &token,
            generation,
            auctions::realm(region, &realm.slug),
            move |body| match auctions::parse_realm_connection(&body) {
                Outcome::Found(connected) => {
                    let _ = app.store().borrow().watch_realm(connected, &name);
                    app.window().toast(&format!(
                        "Watching {name}. Prices start accumulating from the next sync."
                    ));
                    app.refresh_views();
                }
                other => {
                    let reason = other.gap().unwrap_or(Reason::Malformed(
                        "Blizzard did not say which auction house that realm uses".into(),
                    ));
                    app.window()
                        .toast(&format!("Could not add {name}: {reason}"));
                }
            },
        );
    }

    /// Search Blizzard's catalogue for an item to watch.
    fn pick_item(&self) {
        let region = self.imp().settings.borrow().region;

        // Built first and captured by the search closure, so results can be put
        // back into the dialog that asked for them.
        let dialog = std::rc::Rc::new(std::cell::RefCell::new(None::<WatchDialog>));

        let app = self.clone();
        let searching = std::rc::Rc::clone(&dialog);
        let app_for_choice = self.clone();

        let built = WatchDialog::items(
            move |text| {
                let Some(token) = app.imp().token.borrow().clone() else {
                    app.window()
                        .toast("Sign in to search — the catalogue is Blizzard's.");
                    return;
                };
                let generation = app.imp().generation.get();
                let searching = std::rc::Rc::clone(&searching);
                let locale = region.default_locale();

                app.clone().fetch_bare(
                    &token,
                    generation,
                    gamedata::item_search(region, &text),
                    move |body| {
                        let found = gamedata::parse_item_search(&body, locale)
                            .found()
                            .unwrap_or_default();
                        if let Some(dialog) = searching.borrow().as_ref() {
                            dialog.set_items(&found);
                        }
                    },
                );
            },
            move |id, name| {
                let _ = app_for_choice.store().borrow().watch_item(id, &name);
                app_for_choice.window().toast(&format!(
                    "Watching {name}. Blizzard publishes no history, so the first \
                     snapshot is where yours starts."
                ));
                app_for_choice.refresh_views();
            },
        );

        *dialog.borrow_mut() = Some(built.clone());
        built.present(Some(&self.window()));
    }

    /// Keep the cheapest price for each item worth a price out of one snapshot.
    ///
    /// Caged pets and recipe items are kept whether or not anybody asked for
    /// them, and they are the two exceptions to the opt-in rule. Watching a pet by hand is not
    /// possible in any useful sense — every pet in the game is item 82800, so
    /// the watch list cannot name one — and the question this feeds is "which
    /// of my spares is worth selling", which nobody can ask about a pet they
    /// have not thought of. A realm's pets are a few thousand series, and
    /// `record_prices` writes only what moved, so a quiet market costs almost
    /// nothing after the first snapshot.
    ///
    /// Reagents and crafted outputs are the second exception, and the same
    /// argument: nobody watches a reagent by hand, and `market::worth_making`
    /// cannot cost a craft against a price that was thrown away. Bounded by the
    /// account's own recipe books rather than by the market — an account whose
    /// professions have never been opened records nothing extra at all.
    fn record(&self, realm: u32, listings: &[auctions::Listing], watched: &HashSet<u32>) {
        // The whole market first, and the whole market only ever for *now*.
        // This is the half that costs nothing: the response was downloaded in
        // full and used to be discarded, and one table replaced hourly is what
        // turns it into something somebody can browse. History stays opt-in
        // below, because history is the expensive half and the one the
        // thirty-day term is about.
        let whole = auctions::depth(listings);
        let _ = self
            .store()
            .borrow_mut()
            .record_snapshot(realm, &whole, Utc::now());

        let recipe_items = self.store().borrow().recipe_items().unwrap_or_default();
        let mine: Vec<auctions::Listing> = listings
            .iter()
            .filter(|listing| {
                watched.contains(&listing.item_id)
                    || recipe_items.contains(&listing.item_id)
                    || listing.pet_species.is_some()
            })
            .cloned()
            .collect();
        if mine.is_empty() {
            return;
        }
        let book = auctions::depth(&mine);
        let _ = self
            .store()
            .borrow_mut()
            .record_prices(realm, &book, Utc::now());
    }

    /// A call whose answer goes straight to storage rather than into a field.
    ///
    /// Same conditional-request and generation handling as the others; the
    /// difference is only that the caller wants the body and not a place to put
    /// it.
    fn fetch_bare<F>(
        &self,
        token: impl std::borrow::Borrow<Token>,
        generation: u64,
        request: crate::model::source::Request,
        handle: F,
    ) where
        F: FnOnce(Vec<u8>) + 'static,
    {
        let url = request.url.clone();
        let request = request.bearer(&token.borrow().access);
        let request = match self.store().borrow().last_modified(&url) {
            Ok(Some(stamp)) => request.if_modified_since(&stamp),
            _ => request,
        };

        self.imp().outstanding.set(self.imp().outstanding.get() + 1);

        let app = self.clone();
        self.http().fetch(request, move |outcome| {
            if app.imp().generation.get() != generation {
                return;
            }
            app.imp()
                .outstanding
                .set(app.imp().outstanding.get().saturating_sub(1));

            let body = match outcome {
                Outcome::Found(response) => {
                    let _ = app.store().borrow().store_response(
                        &url,
                        &response.body,
                        response.last_modified.as_deref(),
                    );
                    Some(response.body)
                }
                Outcome::Unchanged => {
                    let _ = app.store().borrow().touch_response(&url);
                    app.store()
                        .borrow()
                        .response(&url, chrono::Duration::days(30))
                        .ok()
                        .flatten()
                }
                _ => None,
            };

            if let Some(body) = body {
                handle(body);
            }

            if app.imp().outstanding.get() == 0 {
                app.window().set_busy(false);
                app.refresh_views();
            }
        });
    }

    /// Fill in achievement names, a batch at a time.
    ///
    /// The catalogue is one call per achievement and there are several thousand
    /// of them, so fetching the lot would be most of an hour's quota spent on
    /// names for rows nobody will scroll to. Instead this asks about the ones
    /// the run is actually going to show, capped per sync — the gaps read as
    /// "Achievement 4956" until they fill in, which is ugly and honest.
    ///
    /// Names never change, so what lands is kept and never asked for again.
    fn sync_catalogue(&self, token: &Token, generation: u64) {
        const PER_SYNC: usize = 200;

        let region = self.imp().settings.borrow().region;
        let known: HashSet<u32> = self
            .imp()
            .inputs
            .borrow()
            .catalogue
            .keys()
            .copied()
            .collect();

        // The run's own goals first, because those are the rows on screen. A
        // run that has not started yet falls back to whatever the account has
        // progress on, which is the same list a run would be built from.
        let wanted: Vec<u32> = match self.imp().run.borrow().as_ref() {
            Some((_, run)) => run.goals.iter().map(|goal| goal.achievement_id).collect(),
            None => self
                .imp()
                .inputs
                .borrow()
                .progress
                .iter()
                .map(|progress| progress.id)
                .collect(),
        };

        let missing: Vec<u32> = wanted
            .into_iter()
            .filter(|id| !known.contains(id))
            .take(PER_SYNC)
            .collect();

        if missing.is_empty() {
            return;
        }

        for id in missing {
            let request = gamedata::achievement(region, id);
            let url = request.url.clone();
            let request = request.bearer(&token.access);

            self.imp().outstanding.set(self.imp().outstanding.get() + 1);

            let app = self.clone();
            self.http().fetch(request, move |outcome| {
                if app.imp().generation.get() != generation {
                    return;
                }
                app.imp()
                    .outstanding
                    .set(app.imp().outstanding.get().saturating_sub(1));

                if let Outcome::Found(response) = outcome {
                    let _ = app.store().borrow().store_response(
                        &url,
                        &response.body,
                        response.last_modified.as_deref(),
                    );
                    if let Outcome::Found(achievement) = gamedata::parse_achievement(&response.body)
                    {
                        let _ = app
                            .store()
                            .borrow_mut()
                            .save_achievements(std::slice::from_ref(&achievement));
                        app.imp()
                            .inputs
                            .borrow_mut()
                            .catalogue
                            .insert(achievement.id, achievement);
                    }
                }

                if app.imp().outstanding.get() == 0 {
                    app.window().set_busy(false);
                    // A name arriving can turn a goal from unrepeatable to
                    // ordinary or the reverse, which is a classification and not
                    // a measurement — so this is a re-plan.
                    app.replan();
                }
            });
        }
    }

    /// One call whose answer feeds the planner rather than a roster row.
    fn fetch_input<F>(
        &self,
        token: &Token,
        generation: u64,
        request: crate::model::source::Request,
        merge: F,
    ) where
        F: FnOnce(&mut Inputs, Vec<u8>) + 'static,
    {
        let url = request.url.clone();
        let request = request.bearer(&token.access);
        let request = match self.store().borrow().last_modified(&url) {
            Ok(Some(stamp)) => request.if_modified_since(&stamp),
            _ => request,
        };

        self.imp().outstanding.set(self.imp().outstanding.get() + 1);

        let app = self.clone();
        self.http().fetch(request, move |outcome| {
            if app.imp().generation.get() != generation {
                return;
            }
            app.imp()
                .outstanding
                .set(app.imp().outstanding.get().saturating_sub(1));

            let body = match outcome {
                Outcome::Found(response) => {
                    let _ = app.store().borrow().store_response(
                        &url,
                        &response.body,
                        response.last_modified.as_deref(),
                    );
                    Some(response.body)
                }
                Outcome::Unchanged => {
                    let _ = app.store().borrow().touch_response(&url);
                    app.store()
                        .borrow()
                        .response(&url, chrono::Duration::days(30))
                        .ok()
                        .flatten()
                }
                _ => None,
            };

            if let Some(body) = body {
                merge(&mut app.imp().inputs.borrow_mut(), body);
            }

            // Re-measure only when everything has landed. Doing it per response
            // would redraw the page a hundred times during one sync and show
            // half-computed progress on the way.
            if app.imp().outstanding.get() == 0 {
                app.window().set_busy(false);
                app.remeasure();
            }
        });
    }

    /// One per-character call, merged into that character's detail on arrival.
    fn fetch_detail<F>(
        &self,
        token: &Token,
        generation: u64,
        key: &CharacterKey,
        request: crate::model::source::Request,
        merge: F,
    ) where
        F: FnOnce(&mut Detail, Vec<u8>) + 'static,
    {
        let url = request.url.clone();
        let request = request.bearer(&token.access);
        let request = match self.store().borrow().last_modified(&url) {
            Ok(Some(stamp)) => request.if_modified_since(&stamp),
            _ => request,
        };

        self.imp().outstanding.set(self.imp().outstanding.get() + 1);

        let app = self.clone();
        let key = key.clone();
        self.http().fetch(request, move |outcome| {
            // A callback from a sync that has already been replaced writes into
            // nothing. Decrementing first would let the counter go backwards
            // across generations, so the guard comes before the bookkeeping.
            if app.imp().generation.get() != generation {
                return;
            }
            app.imp()
                .outstanding
                .set(app.imp().outstanding.get().saturating_sub(1));

            let body = match outcome {
                Outcome::Found(response) => {
                    let _ = app.store().borrow().store_response(
                        &url,
                        &response.body,
                        response.last_modified.as_deref(),
                    );
                    Some(response.body)
                }
                Outcome::Unchanged => {
                    let _ = app.store().borrow().touch_response(&url);
                    // Nothing has changed, so what is already held is current
                    // and the stored body is what to merge from.
                    app.store()
                        .borrow()
                        .response(&url, chrono::Duration::days(30))
                        .ok()
                        .flatten()
                }
                // A character with no keystone rating, or a profession call that
                // failed, leaves that field as it was. It is not an error worth
                // a toast per character across a roster this size.
                _ => None,
            };

            if let Some(body) = body {
                let mut details = app.imp().details.borrow_mut();
                let detail = details.entry(key.clone()).or_default();
                merge(detail, body);
                let saved = detail.clone();
                drop(details);
                let _ = app.store().borrow().save_detail(&key, &saved);
            }

            if app.imp().outstanding.get() == 0 {
                app.window().set_busy(false);
            }
            app.refresh_views();
        });
    }

    // -- runs -----------------------------------------------------------------

    /// Ask before throwing a run away, and say what that costs.
    ///
    /// Destructive and unrecoverable — the attestations especially, which are
    /// somebody answering a question nothing else could measure. Worth a
    /// sentence naming them rather than a generic "are you sure".
    fn confirm_new_run(&self) {
        let held = self.imp().run.borrow().clone();
        let enrolled = self.imp().cohort.borrow().len();

        let Some((id, run)) = held else {
            // No run to replace; the Run page's own button is the way in.
            self.start_run();
            return;
        };

        let attested = run
            .goals
            .iter()
            .filter(|goal| goal.attestation.is_some())
            .count();

        let mut detail = format!(
            "“{}” was measured from {}. Starting over throws away its baseline \
             and everything planned against it",
            run.name,
            run.baseline.taken_at.format("%-d %B %Y")
        );
        if attested > 0 {
            detail.push_str(&format!(
                ", including {attested} goal{} you attested to by hand",
                if attested == 1 { "" } else { "s" }
            ));
        }
        detail.push_str(&format!(
            ". The new run will be about the {} character{} enrolled on Roster, \
             and measured from now.",
            enrolled,
            if enrolled == 1 { "" } else { "s" }
        ));

        let dialog = adw::AlertDialog::new(Some("Start a new run?"), Some(&detail));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("start", "Start Over");
        dialog.set_response_appearance("start", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let app = self.clone();
        dialog.connect_response(None, move |dialog, response| {
            dialog.close();
            if response != "start" {
                return;
            }
            if let Err(error) = app.store().borrow_mut().forget_run(id) {
                eprintln!("armory: could not forget the run: {error}");
                return;
            }
            *app.imp().run.borrow_mut() = None;
            app.start_run();
            app.refresh_views();
        });

        dialog.present(Some(&self.window()));
    }

    /// Start a run: freeze what the account has now, and plan from it.
    fn start_run(&self) {
        let cohort = self.imp().cohort.borrow().clone();
        if cohort.is_empty() {
            self.window()
                .toast("Enrol at least one character on Roster first.");
            return;
        }

        let inputs = self.imp().inputs.borrow();
        if inputs.progress.is_empty() {
            self.window()
                .toast("Sync first — a baseline needs to know what the account has.");
            return;
        }

        let baseline = plan::take_baseline(&inputs.progress, &inputs.owned, Utc::now());
        let goals = plan::plan(&baseline, &cohort, &inputs);
        drop(inputs);

        let run = Run {
            name: "Fresh start".into(),
            baseline,
            cohort,
            goals,
        };

        match self.store().borrow_mut().save_run(None, &run) {
            Ok(id) => {
                *self.imp().run.borrow_mut() = Some((id, run));
                self.window()
                    .toast("Run started. Everything is measured from now.");
            }
            Err(error) => {
                self.window()
                    .toast(&format!("Could not save the run: {error}"));
                return;
            }
        }
        self.refresh_views();
    }

    /// Rebuild the current run's goals from scratch.
    ///
    /// Used when attribution changes, because that changes standing — which
    /// [`plan::remeasure`] deliberately will not touch. Decisions the person
    /// made are carried across: an attestation is their word, and a hand
    /// exclusion is their call.
    fn replan(&self) {
        let Some((id, existing)) = self.imp().run.borrow().clone() else {
            // No run yet, so there is nothing to plan. The fresh attribution is
            // still stored and will be used when one is started.
            self.refresh_views();
            return;
        };

        let attestations: HashMap<u32, _> = existing
            .goals
            .iter()
            .filter_map(|goal| Some((goal.achievement_id, goal.attestation.clone()?)))
            .collect();

        {
            let mut inputs = self.imp().inputs.borrow_mut();
            inputs.excluded_by_hand = existing
                .goals
                .iter()
                .filter(|goal| goal.bucket == Bucket::Excluded(Exclusion::ByHand))
                .map(|goal| goal.achievement_id)
                .collect();
        }

        let inputs = self.imp().inputs.borrow();
        let mut goals = plan::plan(&existing.baseline, &existing.cohort, &inputs);
        drop(inputs);

        for goal in &mut goals {
            if let Some(attestation) = attestations.get(&goal.achievement_id) {
                goal.attestation = Some(attestation.clone());
            }
        }

        let run = Run { goals, ..existing };
        let _ = self.store().borrow_mut().save_run(Some(id), &run);
        *self.imp().run.borrow_mut() = Some((id, run));
        self.refresh_views();
    }

    /// Re-measure the observable goals after fresh primary data lands.
    fn remeasure(&self) {
        let Some((id, mut run)) = self.imp().run.borrow().clone() else {
            return;
        };
        let inputs = self.imp().inputs.borrow();
        plan::remeasure(&mut run, &inputs);
        drop(inputs);

        let _ = self.store().borrow_mut().save_run(Some(id), &run);
        *self.imp().run.borrow_mut() = Some((id, run));
        self.refresh_views();
    }

    /// Mark a goal done by hand, or take the mark back.
    fn attest(&self, achievement_id: u32, character: Option<CharacterKey>) {
        let Some((id, mut run)) = self.imp().run.borrow().clone() else {
            return;
        };
        if let Some(goal) = run
            .goals
            .iter_mut()
            .find(|goal| goal.achievement_id == achievement_id)
        {
            goal.attestation = character.map(|character| Attestation {
                character,
                at: Utc::now(),
            });
        }

        let _ = self.store().borrow_mut().save_run(Some(id), &run);
        *self.imp().run.borrow_mut() = Some((id, run));
        self.refresh_views();
    }

    /// Take a goal out of the run, or put it back.
    fn set_excluded(&self, achievement_id: u32, excluded: bool) {
        let Some((id, mut run)) = self.imp().run.borrow().clone() else {
            return;
        };
        if let Some(goal) = run
            .goals
            .iter_mut()
            .find(|goal| goal.achievement_id == achievement_id)
        {
            if excluded {
                goal.bucket = Bucket::Excluded(Exclusion::ByHand);
            } else {
                // Put it back where the classifier would have placed it, rather
                // than assuming observable — the criteria may well be unknown.
                let inputs = self.imp().inputs.borrow();
                if let Some(progress) = inputs
                    .progress
                    .iter()
                    .find(|progress| progress.id == achievement_id)
                {
                    let mut inputs_without = Inputs {
                        excluded_by_hand: HashSet::new(),
                        ..Inputs::default()
                    };
                    inputs_without.catalogue = inputs.catalogue.clone();
                    inputs_without.criteria = inputs.criteria.clone();
                    inputs_without.owned = inputs.owned.clone();
                    goal.bucket = plan::classify(progress, &inputs_without);
                }
            }
        }

        let _ = self.store().borrow_mut().save_run(Some(id), &run);
        *self.imp().run.borrow_mut() = Some((id, run));
        self.refresh_views();
    }

    // -- sharing this account with the other machines ------------------------

    /// How often a pass runs even when nothing has said to.
    ///
    /// A backstop rather than the mechanism. The pass that matters is the one
    /// a `/wait` wakes, or the one a finished addon read schedules; this is
    /// what catches a machine whose parked wait died with a network it did not
    /// notice leaving.
    const PASS_EVERY: Duration = Duration::from_secs(300);

    /// How long to wait after a write before pushing.
    ///
    /// An addon read writes to a dozen tables in one go and a Blizzard sync
    /// finishes in bursts, so this restarts on each one: a burst is one push at
    /// the end of it rather than a push per table.
    const PASS_AFTER_WRITE: Duration = Duration::from_secs(3);

    /// How many rows go in one batch. The server clamps to its own ceiling;
    /// this is the client asking for a whole one.
    const PASS_BATCH: usize = 2_000;

    /// How many passes must fail in a row before saying so.
    const FAILURES_BEFORE_SAYING_SO: usize = 3;

    /// This installation's name in the change log.
    ///
    /// Made once and kept in the database beside the cursor, not in the
    /// settings file — copying `settings.json` between two machines to set them
    /// both up is a thing somebody will reasonably do, and two installations
    /// sharing an id means each is handed the other's rows as its own and
    /// neither ever pulls anything.
    fn machine(&self) -> String {
        let store = self.store();
        let held = store.borrow().machine();
        if !held.is_empty() {
            return held;
        }
        let made = glib::uuid_string_random().to_string();
        let _ = store.borrow().set_machine(&made);
        made
    }

    /// The server, if this machine has been given one.
    ///
    /// **Both or neither.** An address with no token cannot authenticate and a
    /// token with no address has nowhere to go.
    fn sync_target(&self) -> Option<Service> {
        let url = self.imp().settings.borrow().sync_url.trim().to_string();
        if url.is_empty() {
            return None;
        }
        let token = self.stored_sync_token()?;
        if token.trim().is_empty() {
            return None;
        }
        match Service::new(&url, &token, &self.machine()) {
            Ok(service) => Some(service),
            Err(error) => {
                eprintln!("armory: {error}");
                None
            }
        }
    }

    fn stored_sync_token(&self) -> Option<String> {
        Keyring::open()
            .ok()?
            .lookup(keyring::SYNC_TOKEN)
            .ok()
            .flatten()
    }

    /// Begin sharing, if there is anywhere to share to.
    fn start_sharing(&self) {
        if self.sync_target().is_none() {
            return;
        }
        self.share_now();

        let app = self.clone();
        glib::timeout_add_local(Self::PASS_EVERY, move || {
            app.share_now();
            glib::ControlFlow::Continue
        });
    }

    /// Push and pull after the next quiet moment.
    ///
    /// Called from everything that writes. Restarting the clock is the point:
    /// an addon read touches ten tables and a sync finishes in bursts, and a
    /// push per burst is ten round trips to say one thing.
    fn share_soon(&self) {
        if self.sync_target().is_none() {
            return;
        }
        // Nothing waiting, nothing to arm. Without this the reload at the end
        // of a productive pass would schedule another pass, which would find
        // nothing, land nothing, and not reload — so it terminates either way,
        // but one wasted round trip after every real one is a cost with no
        // buyer.
        //
        // `try_borrow`, because this is reached from every redraw in the
        // application and one of them may one day be inside a write. Skipping
        // is the right answer to that rather than a crash: the next redraw
        // arms it, and the backstop timer is behind that.
        let held = self.store();
        let Ok(store) = held.try_borrow() else {
            return;
        };
        // An account that has never been offered up has work to do even when
        // the log is empty — that is exactly the state `seed_log` exists for.
        let idle = store.seeded() && store.queued().is_ok_and(|waiting| waiting.is_empty());
        drop(store);
        if idle {
            return;
        }
        if let Some(pending) = self.imp().pass_due.borrow_mut().take() {
            pending.remove();
        }
        let app = self.clone();
        let handle = glib::timeout_add_local_once(Self::PASS_AFTER_WRITE, move || {
            *app.imp().pass_due.borrow_mut() = None;
            app.share_now();
        });
        *self.imp().pass_due.borrow_mut() = Some(handle);
    }

    /// One pass: everything waiting up, everything new down.
    fn share_now(&self) {
        if self.imp().passing.get() {
            return;
        }
        let Some(service) = self.sync_target() else {
            return;
        };

        // Everything this machine held before sharing was set up, offered up
        // once. The triggers only record writes, so without this a store with
        // a decade in it starts with an empty log and pushes nothing — and
        // says "nothing waiting" while it does so.
        if let Err(error) = self.store().borrow().seed_log() {
            eprintln!("armory: could not offer up the account: {error}");
        }

        self.imp().passing.set(true);

        let app = self.clone();
        glib::spawn_future_local(async move {
            let outcome = app.pass(service.clone()).await;
            app.finish_pass(outcome);
            app.park(service);
        });
    }

    /// The pass itself: `replica::pass` with an await between the steps.
    ///
    /// The decisions are all on the core's side — what to send, what to keep,
    /// when to stop — and this is the same loop `replica::pass` runs, with the
    /// network on a worker so the window keeps drawing through a first sync.
    /// Anything that has to be decided goes there, not here, or the blocking
    /// version and this one come to mean different things.
    ///
    /// **Every read and every write of the store happens on this thread.**
    /// Only the network goes to a worker. That is not tidiness: the change log
    /// is switched off by a flag in the database rather than by anything
    /// belonging to a connection, so a second thread applying a pull would
    /// silence this thread's writes for as long as it took, and neither of
    /// them would ever know.
    async fn pass(&self, service: Service) -> Result<imp::Pass, SyncError> {
        let mut report = replica::Report::default();

        loop {
            let step = self
                .store()
                .borrow()
                .next_step(Self::PASS_BATCH)
                .map_err(|error| SyncError(error.to_string()))?;

            match step {
                replica::Step::Push { parcel, through } => {
                    let sent = parcel.rows.len();
                    let carrier = service.clone();
                    let applied = gio::spawn_blocking(move || carrier.push(&parcel))
                        .await
                        .map_err(|_| SyncError("the worker went away".into()))??;
                    self.store()
                        .borrow()
                        .absorb_push(through, sent, &applied, &mut report)
                        .map_err(|error| SyncError(error.to_string()))?;
                }
                replica::Step::Drain(through) => {
                    let _ = self.store().borrow().drain(through);
                }
                replica::Step::Pull(since) => {
                    let carrier = service.clone();
                    let pulled = gio::spawn_blocking(move || carrier.pull(since, Self::PASS_BATCH))
                        .await
                        .map_err(|_| SyncError("the worker went away".into()))??;
                    let more = self
                        .store()
                        .borrow_mut()
                        .absorb_pull(&pulled, &mut report)
                        .map_err(|error| SyncError(error.to_string()))?;
                    if !more {
                        break;
                    }
                }
            }
        }

        Ok(imp::Pass {
            at: Utc::now(),
            sent: report.sent,
            landed: report.landed,
            removed: report.removed,
            unreadable: report.unreadable,
            failed: None,
        })
    }

    fn finish_pass(&self, outcome: Result<imp::Pass, SyncError>) {
        self.imp().passing.set(false);

        let pass = match outcome {
            Ok(pass) => {
                if self.imp().failures.get() >= Self::FAILURES_BEFORE_SAYING_SO {
                    self.window().set_notice(None);
                }
                self.imp().failures.set(0);
                // Only when something actually arrived. Redrawing ten pages
                // after a pass that agreed with the server is work nobody
                // asked for, on a timer.
                if pass.landed + pass.removed > 0 {
                    self.reload();
                }
                pass
            }
            Err(error) => {
                let failures = self.imp().failures.get() + 1;
                self.imp().failures.set(failures);
                // A banner only once it has stopped being ordinary. A NAS
                // asleep, a machine between networks and a suspended laptop
                // each produce one failed pass, and a banner for every one of
                // them teaches somebody to stop reading banners. The account
                // is whole on this machine either way; nothing is waiting on
                // the answer.
                if failures >= Self::FAILURES_BEFORE_SAYING_SO {
                    self.window().set_notice(Some(&format!(
                        "Not sharing — {failures} passes in a row have failed. {error}"
                    )));
                }
                imp::Pass {
                    at: Utc::now(),
                    sent: 0,
                    landed: 0,
                    removed: 0,
                    unreadable: 0,
                    failed: Some(error.0),
                }
            }
        };

        *self.imp().last_pass.borrow_mut() = Some(pass);
        self.show_sync_state();
    }

    /// Park on the server until another machine writes something.
    ///
    /// This is what makes an evening recorded downstairs appear up here in
    /// about as long as the network takes, rather than on the next tick.
    fn park(&self, service: Service) {
        if self.imp().parked.get() || self.imp().failures.get() > 0 {
            // A server that is not answering should be asked on the timer
            // rather than parked against, or a machine off the tailnet spends
            // its evening opening sockets that fail immediately.
            return;
        }
        self.imp().parked.set(true);

        let since = self.store().borrow().cursor(replica::PULLED);
        let app = self.clone();
        glib::spawn_future_local(async move {
            let carrier = service.clone();
            let woken = gio::spawn_blocking(move || carrier.wait(since)).await;
            app.imp().parked.set(false);
            if let Ok(Ok(true)) = woken {
                app.share_now();
            }
        });
    }

    /// Everything a pass may have changed, read again.
    ///
    /// The same work `restore` does at startup, because a pass can land any
    /// row in the account — a roster from another machine, an evening, a run's
    /// goals — and there is no cheaper honest answer than reading it back.
    fn reload(&self) {
        self.restore();
        self.refresh_views();
    }

    /// The sync dialog, and the numbers it draws.
    ///
    /// Held rather than rebuilt on every pass: it is a dialog somebody leaves
    /// open while a first sync of fifty thousand rows runs past, and watching
    /// the queue drain is most of what it is for.
    fn show_sync_status(&self) {
        let dialog = SyncDialog::new();

        let app = self.clone();
        dialog.connect_save(move |address, token| app.save_sync_target(&address, token.as_deref()));

        let app = self.clone();
        dialog.connect_pass(move || app.share_now());

        dialog.show_state(&self.sync_state());
        *self.imp().sync_dialog.borrow_mut() = Some(dialog.clone());
        dialog.present(Some(&self.window()));
    }

    /// Redraw the sync dialog if it is open. Cheap when it is not.
    fn show_sync_state(&self) {
        let held = self.imp().sync_dialog.borrow().clone();
        if let Some(dialog) = held {
            // A dialog somebody closed is still held here; asking whether it
            // has a root is how a widget says it is still on screen.
            if dialog.is_visible() {
                dialog.show_state(&self.sync_state());
            } else {
                *self.imp().sync_dialog.borrow_mut() = None;
            }
        }
    }

    fn sync_state(&self) -> sync_dialog::State {
        let store = self.store();
        let store = store.borrow();
        sync_dialog::State {
            server: self.imp().settings.borrow().sync_url.trim().to_string(),
            token_held: self
                .stored_sync_token()
                .is_some_and(|token| !token.trim().is_empty()),
            machine: store.machine(),
            passing: self.imp().passing.get(),
            queued: store.queued().unwrap_or_default(),
            queued_since: store.queued_since().ok().flatten().map(|at| when(&at)),
            last: self
                .imp()
                .last_pass
                .borrow()
                .as_ref()
                .map(|pass| sync_dialog::Pass {
                    when: when(&pass.at.to_rfc3339()),
                    sent: pass.sent,
                    landed: pass.landed,
                    removed: pass.removed,
                    unreadable: pass.unreadable,
                    failed: pass.failed.clone(),
                }),
            failures: self.imp().failures.get(),
        }
    }

    /// Remember where to share to, and start or stop doing it.
    fn save_sync_target(&self, address: &str, token: Option<&str>) {
        self.imp().settings.borrow_mut().sync_url = address.trim().to_string();
        self.save_settings();

        if let Some(token) = token {
            if let Ok(keyring) = Keyring::open() {
                let _ = keyring.store(keyring::SYNC_TOKEN, token, "Armory sync token");
            }
        }

        // An emptied address is the deliberate way to stop sharing, and it
        // takes the token with it: leaving a secret in the keyring for a
        // server nobody is talking to is a surprise waiting to happen.
        if self.imp().settings.borrow().sync_url.is_empty() {
            if let Ok(keyring) = Keyring::open() {
                let _ = keyring.clear(keyring::SYNC_TOKEN);
            }
            return;
        }

        self.imp().failures.set(0);
        self.share_now();
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("Armory")
            .application_icon(APP_ID)
            .developer_name("Matthew Hagrelius")
            .version(env!("CARGO_PKG_VERSION"))
            .license_type(gtk::License::Gpl30)
            .comments(
                "A World of Warcraft companion, built around replaying content an \
                 account already remembers.\n\nProfile data comes from Blizzard and \
                 is a snapshot taken when a character logs out, never a live view.",
            )
            .build();
        about.present(Some(&self.window()));
    }
}

/// An RFC 3339 stamp, as somebody would say it.
///
/// The shell's wording, not the core's. `armory-core` reports instants; "three
/// minutes ago" is English, and a shell on another platform would say it in
/// its own.
fn when(stamp: &str) -> String {
    let Ok(at) = DateTime::parse_from_rfc3339(stamp) else {
        return stamp.to_string();
    };
    let minutes = (Utc::now() - at.to_utc()).num_minutes();
    match minutes {
        ..=0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "an hour ago".into(),
        120..=1439 => format!("{} hours ago", minutes / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", minutes / 1440),
    }
}
