//! The run: what the cohort has done, and what is in the way.
//!
//! The home page, because it is the reason the application exists.
//!
//! The number in the ring is deliberately not "achievement points" or "percent
//! of the game". It is the fraction of what this run is *measured against*,
//! which excludes everything the account has permanently spent. A denominator
//! that includes the impossible produces a bar that can never fill, and a bar
//! that can never fill is one nobody looks at twice.
//!
//! ## What the page is
//!
//! Two panes. The main column is the run as a thing that happened over time —
//! the standing, the last fourteen evenings, last night's three numbers, and a
//! road of what has actually occurred. The rail is what to do next: the three
//! goals nearest to closing, and the ones only a person can settle.
//!
//! Everything the page says about the account's *work* is gold. Nothing else
//! is: the count of goals already spent, the day number, the caption under an
//! absent evening are all plain, and that restraint is what makes the accent
//! mean anything.
//!
//! ## Why the buckets are a page of their own
//!
//! Sixty-three attestable goals do not fit in a rail, and a run over a
//! decade-old account has thousands. The rail shows three of each and pushes
//! the full four-bucket list — with its search, its counts and its exclusion
//! buttons — as a subpage. That is an addition to the handed-off design, which
//! draws only the summary; without it there is no way to work through a list of
//! sixty-three, which is most of what somebody starting a run has to do.
//! Searching from the header pushes it too, because typing is asking for the
//! list.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::almanac::{self, Bar, Momentum, Ring};
use super::images::{Art, Images};
use crate::model::character::{CharacterKey, Roster};
use crate::model::chronicle::{Digest, Session};
use crate::model::cohort::Cohort;
use crate::model::run::{Bucket, Goal, Progress, Run};
use crate::model::source::blizzard::gamedata::Achievement;

/// How big an achievement's icon is on a goal row, and on a rail card.
const ART: i32 = 32;
const RAIL_ART: i32 = 44;

/// How wide this page's rail is. Wider than the others' 288 because it carries
/// two lists rather than a standing and a legend.
const RAIL: f64 = 352.0;

/// How many evenings the momentum strip is.
const DAYS: usize = 14;

/// How many entries the road shows before it stops. It is a summary of what has
/// happened, not the journal — the journal is a place of its own.
const ROAD_SHOWN: usize = 14;

/// How many of each list the rail carries.
const RAIL_SHOWN: usize = 3;

/// Called when someone marks a goal done, or takes the mark back.
type AttestHandler = Box<dyn Fn(u32, bool)>;
/// Called when someone drops a goal from the run, or puts it back.
type ExcludeHandler = Box<dyn Fn(u32, bool)>;
/// Called when someone starts a run.
type StartHandler = Box<dyn Fn()>;
/// Called when someone presses one of last night's numbers.
type EveningHandler = Box<dyn Fn()>;

/// How many goals to list at once.
///
/// A run over a decade-old account has thousands of poisoned goals, and a
/// `GtkListBox` holding all of them is a page that takes a second to appear.
/// Anything past the cap is reachable by searching rather than by finishing
/// something first.
const SHOWN: usize = 150;

/// Which set of goals is being looked at.
///
/// Not [`crate::model::run::Bucket`], which is the classification a goal
/// carries. This is a view over those: `Done` spans two of them, and `Spent`
/// covers every kind of exclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// Poisoned, measurable, and not finished. The work.
    ToDo,
    /// Poisoned with nothing able to measure it.
    Attest,
    /// Settled, whether by being earned during the run or by being attested.
    Done,
    /// Outside the denominator entirely.
    Spent,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::ToDo, Tab::Attest, Tab::Done, Tab::Spent];

    fn name(self) -> &'static str {
        match self {
            Tab::ToDo => "todo",
            Tab::Attest => "attest",
            Tab::Done => "done",
            Tab::Spent => "spent",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::ToDo => "To do",
            Tab::Attest => "Your word",
            Tab::Done => "Done",
            Tab::Spent => "Spent",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tab::ToDo => "view-list-bullet-symbolic",
            Tab::Attest => "document-edit-symbolic",
            Tab::Done => "object-select-symbolic",
            Tab::Spent => "user-trash-symbolic",
        }
    }

    /// What the list is, said once, where somebody reading it can see it.
    fn description(self) -> &'static str {
        match self {
            Tab::ToDo => {
                "Measured from each enrolled character's own progress, nearest first — \
                 not from whether the account has the achievement."
            }
            Tab::Attest => {
                "Your account earned these before the run began, and the game keeps no \
                 per-character record of them, so nothing can measure whether you have \
                 done them again. Tick the ones you have."
            }
            Tab::Done => {
                "Finished by an enrolled character since the run began, or marked done \
                 by hand."
            }
            Tab::Spent => {
                "Already collected on this account and impossible to earn again — many \
                 bind-on-pickup mounts will not drop for an account that has them. \
                 These are left out of the count rather than sitting in it as zeroes."
            }
        }
    }

    /// Whether a goal belongs in this view.
    fn holds(self, goal: &Goal) -> bool {
        match self {
            Tab::ToDo => {
                goal.standing.is_poisoned()
                    && goal.bucket == Bucket::Observable
                    && !goal.is_done()
                    && goal
                        .evaluation
                        .as_ref()
                        .is_some_and(|e| e.observable && !e.inherited)
            }
            Tab::Attest => {
                goal.standing.is_poisoned()
                    && goal.bucket == Bucket::Attestable
                    && goal.attestation.is_none()
            }
            Tab::Done => goal.is_done() || !goal.standing.is_poisoned(),
            Tab::Spent => matches!(goal.bucket, Bucket::Excluded(_)),
        }
    }
}

/// What the page has been handed besides the run itself.
///
/// The run is the goals; this is everything the almanac needs to draw the run
/// as something that happened to somebody — who is nearest to what, and what
/// the last fortnight of evenings looked like.
#[derive(Default, Clone)]
pub struct Context {
    pub roster: Roster,
    pub cohort: Cohort,
    pub sessions: Vec<Session>,
}

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct RunPage {
        pub images: RefCell<Option<super::Images>>,
        pub column: RefCell<Option<gtk::Box>>,
        /// The whole run view, swapped out for the empty state when there is no
        /// run to show.
        pub body: RefCell<Option<gtk::Stack>>,
        /// The almanac, and the four buckets pushed on top of it.
        pub navigation: RefCell<Option<adw::NavigationView>>,

        pub ring: RefCell<Option<super::Ring>>,
        pub momentum: RefCell<Option<super::Momentum>>,
        pub headline: RefCell<Option<gtk::Label>>,
        pub subline: RefCell<Option<gtk::Label>>,
        pub last_night: RefCell<Option<gtk::Box>>,
        pub road: RefCell<Option<gtk::Box>>,
        pub rail: RefCell<Option<gtk::Box>>,

        pub buckets: RefCell<Option<adw::ViewStack>>,
        pub search: RefCell<Option<gtk::SearchBar>>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub needle: RefCell<String>,
        pub on_attest: RefCell<Option<super::AttestHandler>>,
        pub on_exclude: RefCell<Option<super::ExcludeHandler>>,
        pub on_start: RefCell<Option<super::StartHandler>>,
        pub on_evening: RefCell<Option<super::EveningHandler>>,
        /// Achievement id to icon URL, as the media calls land.
        pub art: RefCell<HashMap<u32, String>>,
        /// The last thing drawn, so fresh artwork can redraw it without the
        /// application being asked for a run it already handed over.
        pub held: RefCell<Option<(Run, HashMap<u32, Achievement>)>>,
        pub context: RefCell<super::Context>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RunPage {
        const NAME: &'static str = "ArmoryRunPage";
        type Type = super::RunPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for RunPage {}
    impl WidgetImpl for RunPage {}
    impl BinImpl for RunPage {}
}

glib::wrapper! {
    pub struct RunPage(ObjectSubclass<imp::RunPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl RunPage {
    pub fn new(images: &Images) -> Self {
        let page: Self = glib::Object::builder().build();
        *page.imp().images.borrow_mut() = Some(images.clone());
        page.build();
        page
    }

    fn build(&self) {
        // The empty state, for a run that has not been started. It is the whole
        // page when it applies, so it is a sibling of the run view rather than
        // something appended into it.
        let column = almanac::column(18);
        column.set_margin_top(18);
        column.set_margin_bottom(24);
        column.set_margin_start(12);
        column.set_margin_end(12);

        let body = gtk::Stack::new();
        body.add_named(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(
                    &adw::Clamp::builder()
                        .maximum_size(720)
                        .child(&column)
                        .build(),
                )
                .build(),
            Some("none"),
        );
        body.add_named(&self.run_view(), Some("run"));

        let imp = self.imp();
        *imp.column.borrow_mut() = Some(column);
        *imp.body.borrow_mut() = Some(body.clone());

        self.set_child(Some(&body));
    }

    /// The almanac, and the buckets it can push.
    fn run_view(&self) -> adw::NavigationView {
        let navigation = adw::NavigationView::new();
        navigation.add(
            &adw::NavigationPage::builder()
                .title("Run")
                .tag("almanac")
                .child(&self.almanac())
                .build(),
        );
        navigation.add(
            &adw::NavigationPage::builder()
                .title("Goals")
                .tag("goals")
                .child(&self.goals())
                .build(),
        );
        *self.imp().navigation.borrow_mut() = Some(navigation.clone());
        navigation
    }

    /// The page proper: the standing, the fortnight, last night, and the road.
    fn almanac(&self) -> adw::OverlaySplitView {
        let column = almanac::column(16);
        column.add_css_class("al-main-column");
        // Packed to the top. Without this the scroller hands the column the
        // whole viewport and the road's rows share the slack out between them,
        // which draws four evenings a hundred pixels apart.
        column.set_valign(gtk::Align::Start);

        // (a) The hero row: the standing, and what it has been like lately.
        let ring = Ring::new(118);
        let headline = almanac::serif("", "al-page-title");
        let subline = almanac::caption("");
        let momentum = Momentum::new();

        let captions = almanac::row(0);
        let two_weeks = almanac::label("Two weeks ago", &["al-footnote"]);
        two_weeks.set_hexpand(true);
        captions.append(&two_weeks);
        let last = almanac::label("Last night", &["al-footnote"]);
        last.set_halign(gtk::Align::End);
        captions.append(&last);

        let block = almanac::column(9);
        block.set_hexpand(true);
        block.set_valign(gtk::Align::Center);
        block.append(&headline);
        block.append(&subline);
        block.append(&momentum.widget);
        block.append(&captions);

        let hero = almanac::row(22);
        hero.append(&ring.widget);
        hero.append(&block);
        column.append(&hero);

        // (b) Last night's three numbers, hidden entirely when there was no
        // last night. An empty row of dashes is not a lighter version of this
        // block, it is a different and worse one.
        let last_night = almanac::column(9);
        last_night.set_visible(false);
        column.append(&last_night);

        // (c) The road: what has actually happened, newest first. No label —
        // a spine of dated things needs no caption saying it is one.
        let road = almanac::column(17);
        column.append(&road);

        let rail = almanac::rail_column();

        let imp = self.imp();
        *imp.ring.borrow_mut() = Some(ring);
        *imp.momentum.borrow_mut() = Some(momentum);
        *imp.headline.borrow_mut() = Some(headline);
        *imp.subline.borrow_mut() = Some(subline);
        *imp.last_night.borrow_mut() = Some(last_night);
        *imp.road.borrow_mut() = Some(road);
        *imp.rail.borrow_mut() = Some(rail.clone());

        almanac::split(
            &almanac::main_column(&column),
            &almanac::rail_pane(&rail),
            RAIL,
        )
    }

    /// The four buckets, as a pushed page.
    fn goals(&self) -> adw::ToolbarView {
        let buckets = adw::ViewStack::new();
        buckets.set_vexpand(true);

        // Switching to a bucket is what builds it.
        let page = self.clone();
        buckets.connect_visible_child_notify(move |_| page.fill_visible());

        let switcher = adw::InlineViewSwitcher::builder()
            .stack(&buckets)
            // Labels only. With an icon as well the badge is drawn over it,
            // and a count sitting on top of a symbol is neither readable.
            .display_mode(adw::InlineViewSwitcherDisplayMode::Labels)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let stack = almanac::column(0);
        stack.append(
            &adw::Clamp::builder()
                .maximum_size(760)
                .child(&switcher)
                .build(),
        );
        stack.append(&buckets);

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search goals")
            .hexpand(true)
            .build();
        let page = self.clone();
        entry.connect_search_changed(move |entry| {
            page.imp()
                .needle
                .replace(entry.text().trim().to_lowercase());
            page.redraw();
        });

        let search = gtk::SearchBar::builder()
            .child(
                &adw::Clamp::builder()
                    .maximum_size(560)
                    .child(&entry)
                    .build(),
            )
            .build();
        search.connect_entry(&entry);

        // Typing is asking for the list. Without this the header's search
        // toggle would open a bar over a page that is not on screen.
        let page = self.clone();
        search.connect_search_mode_enabled_notify(move |bar| {
            if bar.is_search_mode() {
                page.show_goals();
            }
        });

        let view = adw::ToolbarView::builder().content(&stack).build();
        view.add_top_bar(&adw::HeaderBar::new());
        view.add_top_bar(&search);

        let imp = self.imp();
        *imp.buckets.borrow_mut() = Some(buckets);
        *imp.search.borrow_mut() = Some(search);
        *imp.entry.borrow_mut() = Some(entry);

        view
    }

    /// The search bar, so the window's header toggle can drive it.
    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    /// Push the full list of goals.
    pub fn show_goals(&self) {
        if let Some(navigation) = self.imp().navigation.borrow().as_ref() {
            if navigation
                .visible_page()
                .and_then(|page| page.tag())
                .as_deref()
                != Some("goals")
            {
                navigation.push_by_tag("goals");
            }
        }
        self.fill_visible();
    }

    /// Open one bucket — `todo`, `attest`, `done` or `spent`.
    ///
    /// The switcher is how a person does this. This is how a screenshot does
    /// it, so the three buckets the page does not open on get looked at as
    /// often as the one it does.
    pub fn show_bucket(&self, name: &str) {
        self.show_goals();
        if let Some(buckets) = self.imp().buckets.borrow().as_ref() {
            buckets.set_visible_child_name(name);
        }
        self.fill_visible();
    }

    pub fn connect_attested<F: Fn(u32, bool) + 'static>(&self, handler: F) {
        *self.imp().on_attest.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_excluded<F: Fn(u32, bool) + 'static>(&self, handler: F) {
        *self.imp().on_exclude.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_start<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_start.borrow_mut() = Some(Box::new(handler));
    }

    /// Called when somebody presses one of last night's numbers, which is a
    /// request to go and read about that evening.
    pub fn connect_evening<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_evening.borrow_mut() = Some(Box::new(handler));
    }

    fn clear(&self) -> Option<gtk::Box> {
        let column = self.imp().column.borrow().clone()?;
        while let Some(child) = column.first_child() {
            column.remove(&child);
        }
        Some(column)
    }

    /// Nothing has been started yet.
    pub fn show_no_run(&self, enrolled: usize) {
        *self.imp().held.borrow_mut() = None;
        if let Some(body) = self.imp().body.borrow().as_ref() {
            body.set_visible_child_name("none");
        }
        let Some(column) = self.clear() else { return };

        let status = adw::StatusPage::builder()
            .icon_name("media-playlist-repeat-symbolic")
            .title("No run yet")
            .vexpand(true)
            .build();

        status.set_description(Some(if enrolled == 0 {
            "Enrol the characters you want this run to be about, over on Roster, \
             then start a run here. A run takes a snapshot of what your account \
             already has, and measures progress from that point rather than from \
             what Blizzard thinks you have finished."
        } else {
            "Starting a run takes a snapshot of what your account already has. \
             Everything after that is measured against the snapshot, so content \
             your account finished years ago can be worked through again."
        }));

        if enrolled > 0 {
            let start = gtk::Button::builder()
                .label("Start a run")
                .halign(gtk::Align::Center)
                .build();
            start.add_css_class("suggested-action");
            start.add_css_class("pill");

            let page = self.clone();
            start.connect_clicked(move |_| {
                if let Some(handler) = page.imp().on_start.borrow().as_ref() {
                    handler();
                }
            });
            status.set_child(Some(&start));
        }

        column.append(&status);
    }

    /// The roster, the cohort and the evenings behind the run.
    ///
    /// Set before [`RunPage::show`] rather than passed to it: the application
    /// reads the sessions out of the store once for three pages, and the run is
    /// drawn in the middle of that.
    pub fn set_context(&self, context: Context) {
        self.imp().context.replace(context);
    }

    /// Draw a run's standing.
    pub fn show(&self, run: &Run, catalogue: &HashMap<u32, Achievement>) {
        *self.imp().held.borrow_mut() = Some((run.clone(), catalogue.clone()));
        self.redraw();
    }

    fn redraw(&self) {
        let Some((run, catalogue)) = self.imp().held.borrow().clone() else {
            return;
        };
        let imp = self.imp();
        if let Some(body) = imp.body.borrow().as_ref() {
            body.set_visible_child_name("run");
        }

        let progress = run.progress();
        self.draw_hero(&run, &progress);
        self.draw_last_night();
        self.draw_road(&run, &catalogue);
        self.draw_rail(&run, &catalogue);
        self.draw_buckets(&run, &catalogue);
    }

    // -- the main column ------------------------------------------------------

    fn draw_hero(&self, run: &Run, progress: &Progress) {
        let imp = self.imp();
        let context = imp.context.borrow().clone();

        if let Some(ring) = imp.ring.borrow().as_ref() {
            ring.set(
                progress.fraction(),
                &format!("{:.0}%", progress.fraction() * 100.0),
                &format!("{} / {}", progress.done, progress.counted),
            );
        }

        // Goals closed in the last seven days. Only an attestation carries a
        // date — a goal earned by playing is a flag with no timestamp anywhere
        // in Blizzard's data — so this counts what can honestly be counted and
        // says nothing about the rest.
        let week = chrono::Utc::now() - chrono::Duration::days(7);
        let closed = run
            .goals
            .iter()
            .filter(|goal| {
                goal.attestation
                    .as_ref()
                    .is_some_and(|attestation| attestation.at >= week)
            })
            .count();

        if let Some(headline) = imp.headline.borrow().as_ref() {
            headline.set_label(&match closed {
                0 => "Nothing closed this week".to_string(),
                count => format!("{} closed this week", almanac::spelled(count)),
            });
            // The headline is gold when it is about work, and plain when it is
            // reporting that there was none.
            if closed == 0 {
                headline.remove_css_class("al-gold");
            } else {
                headline.add_css_class("al-gold");
            }
        }

        if let Some(subline) = imp.subline.borrow().as_ref() {
            let day = (chrono::Utc::now() - run.baseline.taken_at)
                .num_days()
                .max(0)
                + 1;
            subline.set_label(&format!(
                "Day {day} of {} · {} goals this account has already spent, left out of the count",
                run.name, progress.excluded
            ));
        }

        if let Some(momentum) = imp.momentum.borrow().as_ref() {
            momentum.set(Self::fortnight(&context.sessions));
        }
    }

    /// The last fourteen days, as a fraction of the longest evening in them.
    ///
    /// `None` is a day nobody played, and it is drawn as an absence rather than
    /// as a zero-height bar — somebody who did not play on Tuesday did not play
    /// a very little on Tuesday.
    fn fortnight(sessions: &[Session]) -> Vec<Option<f64>> {
        let today = chrono::Utc::now().date_naive();
        let mut minutes = vec![0i64; DAYS];
        for session in sessions {
            let day = (today - session.started_at.date_naive()).num_days();
            if (0..DAYS as i64).contains(&day) {
                let index = DAYS - 1 - day as usize;
                minutes[index] += (session.ended_at - session.started_at).num_minutes().max(0);
            }
        }
        let longest = minutes.iter().copied().max().unwrap_or(0);
        minutes
            .into_iter()
            .map(|played| match (played, longest) {
                (0, _) | (_, 0) => None,
                (played, longest) => Some(played as f64 / longest as f64),
            })
            .collect()
    }

    /// Last night's three numbers.
    ///
    /// These are the chronicle's, read by the run — the same three facts the
    /// journal card carries, at the top of the page rather than buried in the
    /// evening they came from. Activating one goes and reads about it.
    fn draw_last_night(&self) {
        let imp = self.imp();
        let Some(block) = imp.last_night.borrow().clone() else {
            return;
        };
        while let Some(child) = block.first_child() {
            block.remove(&child);
        }

        let context = imp.context.borrow().clone();
        let Some(session) = context
            .sessions
            .iter()
            .max_by_key(|session| session.started_at)
        else {
            block.set_visible(false);
            return;
        };
        let digest = session.digest();

        let hours = (digest.ended_at - digest.started_at).num_minutes().max(0);
        // Where the evening was *spent*, which is the stop it stayed longest
        // at rather than the last one — an evening in Nagrand that ended with
        // a hearthstone to Dornogal was an evening in Nagrand.
        let where_ = digest
            .route
            .iter()
            .max_by_key(|stop| stop.stayed)
            .map(|stop| stop.zone.to_uppercase())
            .unwrap_or_else(|| digest.display_name.to_uppercase());
        block.append(&almanac::section(&format!(
            "LAST NIGHT — {where_}, {}H {}M",
            hours / 60,
            hours % 60
        )));

        let cards = almanac::row(10);
        cards.set_homogeneous(true);
        for card in Self::three_numbers(&digest, false) {
            let page = self.clone();
            let click = gtk::GestureClick::new();
            click.connect_released(move |_, _, _, _| {
                if let Some(handler) = page.imp().on_evening.borrow().as_ref() {
                    handler();
                }
            });
            card.add_controller(click);
            card.add_css_class("al-activatable");
            cards.append(&card);
        }
        block.append(&cards);
        block.set_visible(true);
    }

    /// The longest fight, the hardest hit and the closest call.
    ///
    /// How long a fight took, without the reader having to guess at the units.
    ///
    /// `0:10` was the obvious formatting and it is genuinely ambiguous — a
    /// duration written that way reads as ten minutes about as easily as ten
    /// seconds, and the card carries no unit anywhere near it. So anything
    /// under a minute is said outright, and past that `m:ss` means what it
    /// looks like.
    fn fight_length(seconds: u32) -> String {
        if seconds < 60 {
            return format!("{seconds}s");
        }
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    /// Public to the module so the chronicle card draws the same three, at the
    /// smaller size. Nothing in the game names the *opponent* of the longest
    /// fight — only the hardest hit carries one — so where there is no name
    /// there is no footnote rather than a plausible guess at one.
    pub(super) fn three_numbers(digest: &Digest, small: bool) -> Vec<gtk::Box> {
        let mut cards = Vec::new();

        if digest.longest_fight > 0 {
            cards.push(almanac::stat(
                "Longest fight",
                &Self::fight_length(digest.longest_fight),
                "",
                small,
            ));
        }
        if digest.worst_hit > 0 {
            cards.push(almanac::stat(
                "Hardest hit taken",
                &almanac::thousands(digest.worst_hit),
                digest.worst_hit_by.as_deref().unwrap_or(""),
                small,
            ));
        }
        if digest.lowest_health < 100 {
            cards.push(almanac::stat(
                "Closest to dying",
                &format!("{}%", digest.lowest_health),
                // Only when the evening ended with the character alive and
                // something else dead. Otherwise it is a claim about a fight
                // nobody recorded the end of.
                if digest.deaths.is_empty() && !digest.felled.is_empty() {
                    "and then it died first"
                } else {
                    ""
                },
                small,
            ));
        }
        cards
    }

    /// What has actually happened, newest first.
    ///
    /// A merge of three sources with nothing in common but a clock: the
    /// evenings played, the goals settled by hand, and the run's own events.
    /// Sorted together rather than grouped, because "the night I enrolled
    /// Aeltor" is the same kind of fact as "the night I closed Loremaster".
    fn draw_road(&self, run: &Run, catalogue: &HashMap<u32, Achievement>) {
        let Some(road) = self.imp().road.borrow().clone() else {
            return;
        };
        while let Some(child) = road.first_child() {
            road.remove(&child);
        }

        let context = self.imp().context.borrow().clone();
        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, String, String)> = Vec::new();

        for session in &context.sessions {
            let digest = session.digest();
            let minutes = (digest.ended_at - digest.started_at).num_minutes().max(0);
            let where_ = digest
                .route
                .iter()
                .max_by_key(|stop| stop.stayed)
                .map(|stop| stop.zone.clone())
                .unwrap_or_else(|| "somewhere unrecorded".to_string());
            let mut parts = vec![format!("{}h {}m", minutes / 60, minutes % 60)];
            if !digest.quests.is_empty() {
                parts.push(almanac::plural(digest.quests.len(), "quest", "quests"));
            }
            if !digest.deaths.is_empty() {
                parts.push(almanac::plural(digest.deaths.len(), "death", "deaths"));
            }
            entries.push((
                session.started_at,
                format!("An evening in {where_}"),
                format!("{} · {}", digest.display_name, parts.join(" · ")),
            ));
        }

        for goal in &run.goals {
            if let Some(attestation) = &goal.attestation {
                entries.push((
                    attestation.at,
                    Self::title_of(goal.achievement_id, catalogue),
                    "Settled on your word".to_string(),
                ));
            }
        }

        entries.push((
            run.baseline.taken_at,
            format!("{} began", run.name),
            format!(
                "A snapshot of what the account already had — {} goals left out of the count",
                run.progress().excluded
            ),
        ));

        entries.sort_by_key(|(at, _, _)| std::cmp::Reverse(*at));

        // The spine, and the entries hanging off it. A `GtkGrid` rather than an
        // overlay: the spine has to run the height of the whole list, and the
        // dots have to sit in a gutter that the titles do not reflow into.
        let grid = gtk::Grid::builder()
            .column_spacing(14)
            .row_spacing(17)
            .build();
        grid.attach(
            &almanac::spine(),
            0,
            0,
            1,
            entries.len().min(ROAD_SHOWN) as i32,
        );

        for (index, (at, title, detail)) in entries.into_iter().take(ROAD_SHOWN).enumerate() {
            // No negative margin. The dot and the spine share this column and
            // both are centred in it, which is what puts the marker on the
            // line; nudging one of them sideways is what takes it off.
            let dot = almanac::spine_dot(index == 0);
            grid.attach(&dot, 0, index as i32, 1, 1);

            let block = almanac::column(3);
            block.set_hexpand(true);
            block.append(&almanac::serif(&title, "al-entry-title"));
            // The newest entry carries no date. It is the one the gold dot is
            // pointing at, and "last night" said in a caption is the page
            // repeating itself.
            block.append(&almanac::caption(&if index == 0 {
                detail
            } else {
                format!("{} · {detail}", at.format("%-d %B"))
            }));
            grid.attach(&block, 1, index as i32, 1, 1);
        }
        road.append(&grid);
    }

    // -- the rail -------------------------------------------------------------

    fn draw_rail(&self, run: &Run, catalogue: &HashMap<u32, Achievement>) {
        let Some(rail) = self.imp().rail.borrow().clone() else {
            return;
        };
        while let Some(child) = rail.first_child() {
            rail.remove(&child);
        }

        let needle = String::new();
        let (within, _) = self.rows_for(Tab::ToDo, run, catalogue, &needle);
        let (attest, settle) = self.rows_for(Tab::Attest, run, catalogue, &needle);

        // WITHIN REACH — the three nearest to closing. Sorted by what is left
        // rather than by percentage, because what somebody wants from this list
        // is something they can finish tonight.
        let cards = almanac::column(9);
        for (index, goal) in within.iter().take(RAIL_SHOWN).enumerate() {
            cards.append(&self.within_reach_card(goal, catalogue, index));
        }
        if within.is_empty() {
            cards.append(&almanac::caption("Nothing measurable is left."));
        }
        rail.append(&almanac::titled("WITHIN REACH", &cards));

        let more = self.rail_button(&format!("All {} goals", run.goals.len()), Tab::ToDo);
        rail.append(&more);

        rail.append(&almanac::hairline());

        // ONLY YOU CAN SETTLE — the count is the point. Three switches is what
        // fits; the rest are a press away.
        let switches = almanac::card(0);
        for (index, goal) in attest.iter().take(RAIL_SHOWN).enumerate() {
            let (row, switch) = almanac::switch_row(
                &Self::title_of(goal.achievement_id, catalogue),
                &Self::subtitle_of(goal.achievement_id, catalogue, ""),
                goal.attestation.is_some(),
            );
            switch.add_css_class("al-switch");
            if index % 2 == 1 {
                row.add_css_class("al-alternate");
            }
            let id = goal.achievement_id;
            let page = self.clone();
            switch.connect_active_notify(move |switch| {
                if let Some(handler) = page.imp().on_attest.borrow().as_ref() {
                    handler(id, switch.is_active());
                }
            });
            switches.append(&row);
        }

        let block = almanac::column(9);
        block.append(&almanac::section(&format!(
            "ONLY YOU CAN SETTLE — {settle}"
        )));
        // The short form of `Tab::Attest::description`. The rail is three
        // switches wide and the full paragraph pushes them off the page; the
        // whole argument is still on the bucket the button opens.
        block.append(&almanac::caption(
            "The game keeps no per-character record of these. \
             Tick the ones you have done again.",
        ));
        if settle > 0 {
            block.append(&switches);
            block.append(&self.rail_button(&format!("All {settle} to settle"), Tab::Attest));
        } else {
            block.append(&almanac::caption("Nothing is waiting on you."));
        }
        rail.append(&block);
    }

    /// A goal near enough to close tonight, and who is nearest to it.
    fn within_reach_card(
        &self,
        goal: &Goal,
        catalogue: &HashMap<u32, Achievement>,
        index: usize,
    ) -> gtk::Box {
        let card = almanac::card(9);
        card.add_css_class("al-activatable");

        let top = almanac::row(11);
        top.append(&self.icon(goal.achievement_id, RAIL_ART));

        let text = almanac::column(3);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let title = almanac::label(
            &Self::title_of(goal.achievement_id, catalogue),
            &["al-row-title"],
        );
        title.set_wrap(true);
        text.append(&title);

        // Whichever enrolled character the figure was measured against. The
        // number means nothing without them: "eleven to go" is a fact about
        // somebody in particular, not about the account.
        if let Some(who) = goal.nearest.as_ref().and_then(|key| self.character(key)) {
            let line = almanac::row(6);
            line.append(&almanac::class_dot(&who.1));
            line.append(&almanac::label(&who.0, &["al-caption"]));
            text.append(&line);
        }
        top.append(&text);
        card.append(&top);

        let bottom = almanac::row(10);
        let bar = Bar::new(4);
        bar.set(goal.fraction().unwrap_or(0.0), 80 * index as u32);
        bottom.append(&bar.widget);

        let remaining = goal
            .evaluation
            .as_ref()
            .map(|e| e.required.saturating_sub(e.progress))
            .unwrap_or(0);
        let count = almanac::mono(&remaining.to_string(), &["al-price", "al-gold"]);
        count.set_halign(gtk::Align::End);
        bottom.append(&count);
        card.append(&bottom);

        let goal = goal.clone();
        let achievement = catalogue.get(&goal.achievement_id).cloned();
        let click = gtk::GestureClick::new();
        click.connect_released(move |gesture, _, _, _| {
            if let Some(widget) = gesture.widget() {
                super::achievement_dialog::present(&widget, &goal, achievement.as_ref());
            }
        });
        card.add_controller(click);
        card
    }

    /// A character's display name and class, for a key.
    fn character(&self, key: &CharacterKey) -> Option<(String, String)> {
        let context = self.imp().context.borrow();
        context
            .roster
            .get(key)
            .map(|character| (character.display_name.clone(), character.class.clone()))
    }

    /// The rail's way into the full list.
    fn rail_button(&self, label: &str, tab: Tab) -> gtk::Button {
        let button = gtk::Button::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .build();
        button.add_css_class("flat");
        let page = self.clone();
        button.connect_clicked(move |_| page.show_bucket(tab.name()));
        button
    }

    // -- the buckets ----------------------------------------------------------

    fn draw_buckets(&self, run: &Run, catalogue: &HashMap<u32, Achievement>) {
        let imp = self.imp();
        let Some(buckets) = imp.buckets.borrow().clone() else {
            return;
        };

        // Which bucket is open survives a redraw. Ticking something off
        // re-plans the whole run, and being thrown back to the first tab every
        // time would make working through a list impossible.
        let open = buckets.visible_child_name().map(|name| name.to_string());
        while let Some(child) = buckets.first_child() {
            buckets.remove(&child);
        }

        let needle = imp.needle.borrow().clone();
        for bucket in Tab::ALL {
            // An empty shell per bucket, filled when it is looked at. Building
            // all four eagerly is six hundred rows and six hundred images for
            // the three nobody is looking at, every time anything changes.
            let shell = adw::Bin::new();
            let (_, total) = self.rows_for(bucket, run, catalogue, &needle);
            // The count in the label rather than as a badge. A badge is drawn
            // as a corner overlay on the button's content, so on a text-only
            // switcher it lands on top of the last letter of the word — and a
            // number sitting on a letter is neither.
            buckets.add_titled(
                &shell,
                Some(bucket.name()),
                &format!("{}  {total}", bucket.label()),
            );
        }

        if let Some(name) = open {
            buckets.set_visible_child_name(&name);
        }
        self.fill_visible();
    }

    /// Build the rows for whichever bucket is on screen.
    fn fill_visible(&self) {
        let imp = self.imp();
        let (Some(buckets), Some((run, catalogue))) =
            (imp.buckets.borrow().clone(), imp.held.borrow().clone())
        else {
            return;
        };
        let Some(shell) = buckets.visible_child().and_downcast::<adw::Bin>() else {
            return;
        };
        if shell.child().is_some() {
            return;
        }
        let Some(name) = buckets.visible_child_name() else {
            return;
        };
        let Some(tab) = Tab::ALL.into_iter().find(|tab| tab.name() == name) else {
            return;
        };

        let needle = imp.needle.borrow().clone();
        let (rows, total) = self.rows_for(tab, &run, &catalogue, &needle);
        shell.set_child(Some(&self.bucket_page(tab, rows, total, &needle)));
    }

    /// Fill in the achievement icons that have arrived since the last draw.
    pub fn set_art(&self, art: &HashMap<u32, String>) {
        if art.is_empty() {
            return;
        }
        let fresh = {
            let mut held = self.imp().art.borrow_mut();
            let before = held.len();
            held.extend(art.iter().map(|(id, url)| (*id, url.clone())));
            held.len() != before
        };
        if fresh {
            self.redraw();
        }
    }

    /// Which goals on screen still have no icon.
    ///
    /// The ones being shown, not every goal in the run: a run over a decade-old
    /// account has thousands, and this is one request each.
    pub fn art_wanted(&self, limit: usize) -> Vec<u32> {
        let Some((run, _)) = self.imp().held.borrow().clone() else {
            return Vec::new();
        };
        let held = self.imp().art.borrow();
        run.goals
            .iter()
            .filter(|goal| goal.standing.is_poisoned() && !goal.is_done())
            .filter(|goal| goal.bucket != Bucket::Excluded(crate::model::run::Exclusion::ByHand))
            .map(|goal| goal.achievement_id)
            .filter(|id| !held.contains_key(id))
            .take(limit)
            .collect()
    }

    /// One goal's icon, or the placeholder that stands in for it.
    fn icon(&self, achievement_id: u32, size: i32) -> Art {
        let art = Art::new(size, "starred-symbolic");
        art.set_valign(gtk::Align::Center);
        art.add_css_class("achievement-icon");

        let url = self.imp().art.borrow().get(&achievement_id).cloned();
        if let Some(images) = self.imp().images.borrow().as_ref() {
            art.show(images, url.as_deref(), size);
        }
        art
    }

    /// The goals in one bucket that answer the search, and how many there are.
    ///
    /// The count is of everything in the bucket, not of what fits on screen:
    /// the badge has to say how much work there is, and a badge that changed as
    /// somebody typed would be reporting the search rather than the run.
    fn rows_for<'a>(
        &self,
        tab: Tab,
        run: &'a Run,
        catalogue: &HashMap<u32, Achievement>,
        needle: &str,
    ) -> (Vec<&'a Goal>, usize) {
        let mut held: Vec<&Goal> = run.goals.iter().filter(|goal| tab.holds(goal)).collect();
        let total = held.len();

        if !needle.is_empty() {
            held.retain(|goal| {
                Self::title_of(goal.achievement_id, catalogue)
                    .to_lowercase()
                    .contains(needle)
                    || catalogue
                        .get(&goal.achievement_id)
                        .is_some_and(|a| a.category.to_lowercase().contains(needle))
            });
        }

        match tab {
            // By how much is left rather than how far along, so a goal needing
            // one more quest outranks one 90% through something enormous. What
            // a person wants from this list is something they can finish
            // tonight.
            Tab::ToDo => held.sort_by_key(|goal| {
                let remaining = goal
                    .evaluation
                    .as_ref()
                    .map(|e| e.required.saturating_sub(e.progress))
                    .unwrap_or(u64::MAX);
                (remaining, goal.achievement_id)
            }),
            _ => held.sort_by_key(|goal| {
                (
                    Self::title_of(goal.achievement_id, catalogue).to_lowercase(),
                    goal.achievement_id,
                )
            }),
        }

        (held, total)
    }

    /// One bucket, as a scrolling page of its own.
    fn bucket_page(&self, tab: Tab, rows: Vec<&Goal>, total: usize, needle: &str) -> gtk::Widget {
        let Some((_, catalogue)) = self.imp().held.borrow().clone() else {
            return gtk::Box::new(gtk::Orientation::Vertical, 0).upcast();
        };

        if rows.is_empty() {
            let status = adw::StatusPage::builder()
                .icon_name(if needle.is_empty() {
                    tab.icon()
                } else {
                    "system-search-symbolic"
                })
                .title(if needle.is_empty() {
                    match tab {
                        Tab::ToDo => "Nothing measurable left",
                        Tab::Attest => "Nothing waiting on you",
                        Tab::Done => "Nothing finished yet",
                        Tab::Spent => "Nothing spent",
                    }
                } else {
                    "No matches"
                })
                .description(if needle.is_empty() {
                    tab.description()
                } else {
                    "No goal in this list matches that."
                })
                .vexpand(true)
                .build();
            return status.upcast();
        }

        let group = adw::PreferencesGroup::builder()
            .description(tab.description())
            .build();

        for goal in rows.iter().take(SHOWN) {
            group.add(&self.goal_row(tab, goal, &catalogue));
        }

        if rows.len() > SHOWN {
            let more = adw::ActionRow::builder()
                .title(format!("and {} more", rows.len() - SHOWN))
                .subtitle("Search to reach them")
                .build();
            more.add_css_class("dimmed");
            group.add(&more);
        }
        let _ = total;

        let column = almanac::column(0);
        column.set_margin_top(6);
        column.set_margin_bottom(24);
        column.set_margin_start(12);
        column.set_margin_end(12);
        column.append(&group);

        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(
                &adw::Clamp::builder()
                    .maximum_size(760)
                    .child(&column)
                    .build(),
            )
            .build()
            .upcast()
    }

    /// One goal, dressed for the list it is in.
    fn goal_row(
        &self,
        tab: Tab,
        goal: &Goal,
        catalogue: &HashMap<u32, Achievement>,
    ) -> gtk::Widget {
        let title = Self::title_of(goal.achievement_id, catalogue);

        // Only the attestable list gets switches: a switch is for something the
        // person decides, and every other bucket is something measured.
        if tab == Tab::Attest {
            let row = adw::SwitchRow::builder()
                .title(&title)
                .subtitle(Self::subtitle_of(goal.achievement_id, catalogue, ""))
                .active(goal.attestation.is_some())
                .build();
            row.add_prefix(&self.icon(goal.achievement_id, ART));

            let id = goal.achievement_id;
            let page = self.clone();
            row.connect_active_notify(move |row| {
                if let Some(handler) = page.imp().on_attest.borrow().as_ref() {
                    handler(id, row.is_active());
                }
            });
            row.add_suffix(&self.drop_button(goal.achievement_id));
            return row.upcast();
        }

        let lead = match tab {
            Tab::ToDo => goal
                .evaluation
                .as_ref()
                .map(|e| match e.required.saturating_sub(e.progress) {
                    1 => "1 to go".to_string(),
                    remaining => format!("{remaining} to go"),
                })
                .unwrap_or_default(),
            Tab::Done => match &goal.attestation {
                Some(attestation) => {
                    format!("Marked done on {}", attestation.at.format("%-d %B %Y"))
                }
                None => "Earned during this run".to_string(),
            },
            _ => String::new(),
        };

        let row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(Self::subtitle_of(goal.achievement_id, catalogue, &lead))
            .activatable(true)
            .build();
        row.add_prefix(&self.icon(goal.achievement_id, ART));
        Self::open_on_activate(&row, goal, catalogue);

        match tab {
            Tab::ToDo => {
                if let Some(fraction) = goal.fraction() {
                    let bar = Bar::new(5);
                    bar.widget.set_size_request(120, 5);
                    bar.widget.set_hexpand(false);
                    bar.set(fraction, 0);
                    row.add_suffix(&bar.widget);
                }
                row.add_suffix(&self.drop_button(goal.achievement_id));
            }
            Tab::Done => {
                row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
            }
            Tab::Spent => {
                row.add_css_class("excluded");
                // Putting one back is the only action that makes sense here,
                // and it is the exact inverse of the button that removed it.
                let restore = gtk::Button::builder()
                    .icon_name("list-add-symbolic")
                    .tooltip_text("Put this back into the run")
                    .valign(gtk::Align::Center)
                    .build();
                restore.add_css_class("flat");
                let id = goal.achievement_id;
                let page = self.clone();
                restore.connect_clicked(move |_| {
                    if let Some(handler) = page.imp().on_exclude.borrow().as_ref() {
                        handler(id, false);
                    }
                });
                row.add_suffix(&restore);
            }
            Tab::Attest => unreachable!("handled above"),
        }

        row.upcast()
    }

    /// Take a goal out of the run.
    fn drop_button(&self, achievement_id: u32) -> gtk::Button {
        let button = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text("Leave this out of the run")
            .valign(gtk::Align::Center)
            .build();
        button.add_css_class("flat");

        let page = self.clone();
        button.connect_clicked(move |_| {
            if let Some(handler) = page.imp().on_exclude.borrow().as_ref() {
                handler(achievement_id, true);
            }
        });
        button
    }

    /// An achievement's name, or its id when the catalogue has not synced.
    ///
    /// Showing the id is ugly and honest. Showing nothing would hide a row that
    /// is otherwise perfectly actionable — but a page full of numbers is what
    /// the addon's name table exists to prevent, and it fills the whole
    /// catalogue in one logout where the web API does two hundred a sync.
    fn title_of(id: u32, catalogue: &HashMap<u32, Achievement>) -> String {
        catalogue
            .get(&id)
            .map(|achievement| achievement.name.clone())
            .unwrap_or_else(|| format!("Achievement {id}"))
    }

    /// The category and points, with whatever the caller wants said first.
    fn subtitle_of(id: u32, catalogue: &HashMap<u32, Achievement>, lead: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !lead.is_empty() {
            parts.push(lead.to_string());
        }
        if let Some(achievement) = catalogue.get(&id) {
            if !achievement.category.is_empty() {
                parts.push(achievement.category.clone());
            }
            if achievement.points > 0 {
                parts.push(format!("{} points", achievement.points));
            }
        }
        parts.join("  ·  ")
    }

    /// Open the detail view when a row is activated.
    fn open_on_activate(row: &adw::ActionRow, goal: &Goal, catalogue: &HashMap<u32, Achievement>) {
        let achievement = catalogue.get(&goal.achievement_id).cloned();
        let goal = goal.clone();
        row.connect_activated(move |row| {
            super::achievement_dialog::present(row, &goal, achievement.as_ref());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::character::Faction;

    fn session(days_ago: i64, minutes: i64) -> Session {
        let started = chrono::Utc::now() - chrono::Duration::days(days_ago);
        Session {
            character: CharacterKey::new("emerald-dream", "Somechar"),
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            class: "Shaman".into(),
            race: "Orc".into(),
            faction: Faction::Horde,
            started_at: started,
            ended_at: started + chrono::Duration::minutes(minutes),
            start_level: 80,
            end_level: 80,
            start_money: 0,
            end_money: 0,
            start_item_level: 0,
            end_item_level: 0,
            moments: Vec::new(),
            kills: 0,
            risen: Vec::new(),
            travelled: 0,
            longest_fight: 0,
            worst_hit: 0,
            worst_hit_by: None,
            lowest_health: 100,
        }
    }

    #[test]
    fn a_day_nobody_played_is_an_absence_and_not_a_zero() {
        // The whole reason the strip is drawn by hand rather than as a row of
        // bars: a zero-height bar says somebody played for no time, which is a
        // different claim from their not having played.
        let strip = RunPage::fortnight(&[session(0, 120), session(2, 60)]);
        assert_eq!(strip.len(), DAYS);
        assert_eq!(strip[DAYS - 1], Some(1.0));
        assert_eq!(strip[DAYS - 2], None);
        assert_eq!(strip[DAYS - 3], Some(0.5));
    }

    #[test]
    fn an_evening_older_than_the_fortnight_is_not_in_it() {
        let strip = RunPage::fortnight(&[session(40, 300)]);
        assert!(strip.iter().all(Option::is_none));
    }

    #[test]
    fn two_evenings_on_one_day_are_one_bar() {
        // The strip is a day per bar, not a session per bar. Somebody who
        // logged in twice played once.
        let strip = RunPage::fortnight(&[session(0, 60), session(0, 60), session(1, 60)]);
        assert_eq!(strip[DAYS - 1], Some(1.0));
        assert_eq!(strip[DAYS - 2], Some(0.5));
    }
}
