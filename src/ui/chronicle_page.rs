//! The journal: one card an evening, hanging off a dated spine.
//!
//! Every other page here is a table of state. This one is a stack of evenings,
//! and that difference decides the layout: a card per session in a single
//! column, wide enough to read a paragraph in and no wider. A journal that
//! looks like a spreadsheet is one nobody reads twice.
//!
//! The Almanac's two panes apply here as everywhere: the main column is the
//! evenings themselves, and the rail carries what a character has done over
//! months — counters that belong to a person rather than to a night, and would
//! be a lie repeated nightly if they were drawn on a card.
//!
//! **A card is complete before anything is written.** The log — where the
//! character went, what they turned in, what fell over — is drawn from the
//! session alone and is what most cards will only ever show. Prose is opt-in
//! and is a flourish on top of a record that stands up without it. That
//! ordering is why an unwritten card still carries its facts, and why "Write
//! Entry" is an ordinary outlined button rather than the page's suggested
//! action: the page is not trying to sell anybody an entry.
//!
//! The one thing on screen that is not a fact from this machine is the prose,
//! and it says who wrote it and when, on every card that has one.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::almanac::{self, Tone};
use super::run_page::RunPage;
use crate::model::character::CharacterKey;
use crate::model::chronicle::{
    money, plural, purse, spell, standing, Digest, Entry, Picture, Purpose, Session, SessionId,
};
use crate::model::tally::{self, Counting, Tallies, Tally};

/// How wide the rail is. The same 288 as every page whose rail carries one
/// list; only the Market and the Run page need more.
const RAIL: f64 = 288.0;

/// How wide the column of evenings is allowed to get.
///
/// A property of the column rather than of the page: the filter row and the
/// rail run to the window's edge, and only the prose is held to a measure. Much
/// past seventy characters a line is measurably harder to read, and this is the
/// one page here made of sentences.
const MEASURE: i32 = 760;

/// How many rows of one lifetime counter its tooltip lists.
///
/// A profession has hundreds of recipes and somebody who levelled one has made
/// most of them once; the same shape holds for zones and for what has killed
/// somebody. The tail is noise and the head is the answer to "what does this
/// character actually do".
const TALLY_SHOWN: usize = 6;

/// How many stops of a route the chain draws before it says "and more".
const ROUTE_SHOWN: usize = 4;

/// How many overheard lines the log shows.
///
/// A busy evening records forty and a card is not a transcript. The whole set
/// still goes to the model, which is where they earn their keep.
const OVERHEARD_SHOWN: usize = 5;

/// How many lines an NPC said to the character the log shows.
const TOLD_SHOWN: usize = 3;

/// One book's worth of the gold bar: each entry's share of it, and whether it
/// is the first — which is the one drawn at full strength.
type Segments = Vec<(f64, bool)>;

/// Told an evening should be written up, or taken out of the journal.
type SessionHandler = Box<dyn Fn(SessionId)>;
/// Told to open the journal's setup.
type SetupHandler = Box<dyn Fn()>;

/// How the page is filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Everything,
    /// Evenings that have no entry yet — the queue, for somebody about to
    /// spend a few minutes catching up.
    Unwritten,
    Written,
}

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct ChroniclePage {
        pub cards: RefCell<Option<gtk::Box>>,
        pub rail: RefCell<Option<gtk::Box>>,
        pub search: RefCell<Option<gtk::SearchBar>>,
        pub who: RefCell<Option<gtk::DropDown>>,
        pub held: RefCell<Option<super::Held>>,
        pub needle: RefCell<String>,
        pub showing: Cell<super::Showing>,
        /// Which character is selected, or `None` for all of them. Held rather
        /// than read back off the dropdown so that a redraw does not have to
        /// care whether the dropdown has been rebuilt since.
        pub only: RefCell<Option<String>>,
        /// Display name to class, so the filter can carry a class dot. The
        /// dropdown's model is strings; this is what turns one back into a
        /// colour.
        pub classes: RefCell<HashMap<String, String>>,
        /// Sessions with a request in flight, so a card can show a spinner and
        /// refuse to start a second one.
        pub writing: RefCell<Vec<SessionId>>,

        pub on_write: RefCell<Option<super::SessionHandler>>,
        pub on_forget: RefCell<Option<super::SessionHandler>>,
        pub on_setup: RefCell<Option<super::SetupHandler>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChroniclePage {
        const NAME: &'static str = "ArmoryChroniclePage";
        type Type = super::ChroniclePage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for ChroniclePage {}
    impl WidgetImpl for ChroniclePage {}
    impl BinImpl for ChroniclePage {}
}

/// One redraw's worth of input.
///
/// Public only because a field on the private implementation names it.
#[derive(Clone)]
pub struct Held {
    sessions: Vec<Session>,
    entries: HashMap<SessionId, Entry>,
    /// Screenshots the client wrote, with the time each landed.
    ///
    /// Matched to an evening's recorded moments rather than filed against one:
    /// the addon has no way to learn a filename, so the join is by time and it
    /// happens at draw. See `Digest::pictures`.
    shots: Vec<(chrono::DateTime<chrono::Utc>, String)>,
    /// Whether the journal has somewhere to write from. Without it the page
    /// offers setup instead of a button that would fail.
    configured: bool,
    /// Every counter no Blizzard system keeps, per character.
    ///
    /// Not evenings' facts and so not on any digest: they span every evening
    /// since the addon was installed, which is why they are drawn in the rail
    /// rather than on a card, and only when a single character is being looked
    /// at. "Everyone has made four hundred flasks" is nobody's achievement.
    tallies: Tallies,
}

glib::wrapper! {
    pub struct ChroniclePage(ObjectSubclass<imp::ChroniclePage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ChroniclePage {
    fn default() -> Self {
        Self::new()
    }
}

impl ChroniclePage {
    pub fn new() -> Self {
        let page: Self = glib::Object::builder().build();
        page.build();
        page
    }

    fn build(&self) {
        let cards = almanac::column(0);

        let column = almanac::column(16);
        column.add_css_class("al-main-column");
        // Packed to the top. Without this the scroller hands the column the
        // whole viewport and the spine's rows share the slack out between them,
        // which draws three evenings a hundred pixels apart.
        column.set_valign(gtk::Align::Start);
        column.append(&self.controls());
        column.append(&cards);

        let rail = almanac::rail_column();

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search entries, quests and places")
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

        let view = adw::ToolbarView::builder()
            .content(&almanac::split(
                &almanac::main_column(&column),
                &almanac::rail_pane(&rail),
                RAIL,
            ))
            .build();
        view.add_top_bar(&search);

        let imp = self.imp();
        *imp.cards.borrow_mut() = Some(cards);
        *imp.rail.borrow_mut() = Some(rail);
        *imp.search.borrow_mut() = Some(search);

        self.set_child(Some(&view));
        self.redraw();
    }

    /// The filter row: which evenings, and whose.
    fn controls(&self) -> gtk::Widget {
        let page = self.clone();
        let showing = almanac::segments(&["All", "Unwritten", "Written"], 0, move |index| {
            page.imp().showing.set(match index {
                1 => Showing::Unwritten,
                2 => Showing::Written,
                _ => Showing::Everything,
            });
            page.redraw();
        });

        // A dropdown rather than a toggle per character: an account here has
        // twenty-three of them, and twenty-three toggles is not a filter bar.
        let who = gtk::DropDown::builder()
            .model(&gtk::StringList::new(&["Everyone"]))
            .factory(&self.character_factory())
            .valign(gtk::Align::Center)
            .build();
        who.set_tooltip_text(Some("Show one character's evenings"));
        let page = self.clone();
        who.connect_selected_notify(move |dropdown| {
            let chosen = dropdown
                .selected_item()
                .and_downcast::<gtk::StringObject>()
                .map(|item| item.string().to_string())
                .filter(|name| name != "Everyone");
            *page.imp().only.borrow_mut() = chosen;
            page.redraw();
        });

        let wrap = adw::WrapBox::builder()
            .child_spacing(8)
            .line_spacing(8)
            .build();
        wrap.append(&showing);
        wrap.append(&who);

        *self.imp().who.borrow_mut() = Some(who);
        wrap.upcast()
    }

    /// A character in the dropdown: their class colour, then their name.
    ///
    /// The class is never text and never anything larger than a dot — the same
    /// rule the roster follows, and the reason Priest being white and Rogue
    /// being pale yellow cannot make anything unreadable.
    fn character_factory(&self) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();

        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let line = almanac::row(8);
            line.append(&almanac::class_dot(""));
            line.append(&almanac::label("", &[]));
            item.set_child(Some(&line));
        });

        let page = self.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let (Some(name), Some(line)) = (
                item.item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|string| string.string().to_string()),
                item.child().and_downcast::<gtk::Box>(),
            ) else {
                return;
            };
            let (Some(dot), Some(label)) = (
                line.first_child(),
                line.last_child().and_downcast::<gtk::Label>(),
            ) else {
                return;
            };

            label.set_label(&name);
            for held in dot.css_classes() {
                if held.starts_with("class-") && held != "class-dot" {
                    dot.remove_css_class(&held);
                }
            }
            match page.imp().classes.borrow().get(&name) {
                Some(class) => {
                    dot.add_css_class(almanac::class_style(class));
                    dot.set_visible(true);
                }
                // "Everyone" is not a character and has no colour. A dot in the
                // unknown grey there would read as a class this build does not
                // know rather than as the absence of a filter.
                None => dot.set_visible(false),
            }
        });

        factory
    }

    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    /// Ask for an evening to be written up.
    pub fn connect_write<F: Fn(SessionId) + 'static>(&self, handler: F) {
        *self.imp().on_write.borrow_mut() = Some(Box::new(handler));
    }

    /// Take an evening out of the journal altogether.
    pub fn connect_forget<F: Fn(SessionId) + 'static>(&self, handler: F) {
        *self.imp().on_forget.borrow_mut() = Some(Box::new(handler));
    }

    /// Open the journal's setup.
    pub fn connect_setup<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_setup.borrow_mut() = Some(Box::new(handler));
    }

    pub fn show(
        &self,
        sessions: &[Session],
        entries: &HashMap<SessionId, Entry>,
        shots: &[(chrono::DateTime<chrono::Utc>, String)],
        configured: bool,
        tallies: &Tallies,
    ) {
        *self.imp().held.borrow_mut() = Some(Held {
            sessions: sessions.to_vec(),
            entries: entries.clone(),
            shots: shots.to_vec(),
            configured,
            tallies: tallies.clone(),
        });
        self.refresh_characters(sessions);
        self.redraw();
    }

    /// Mark a session as being written, or no longer being written.
    pub fn set_writing(&self, id: &SessionId, writing: bool) {
        {
            let mut pending = self.imp().writing.borrow_mut();
            pending.retain(|held| held != id);
            if writing {
                pending.push(id.clone());
            }
        }
        self.redraw();
    }

    /// Keep the character filter in step with who has actually played.
    fn refresh_characters(&self, sessions: &[Session]) {
        let mut classes = HashMap::new();
        for session in sessions {
            classes.insert(session.display_name.clone(), session.class.clone());
        }
        let mut names: Vec<String> = classes.keys().cloned().collect();
        names.sort();
        *self.imp().classes.borrow_mut() = classes;

        let Some(dropdown) = self.imp().who.borrow().clone() else {
            return;
        };

        let chosen = self.imp().only.borrow().clone();
        let mut options = vec!["Everyone".to_string()];
        options.extend(names);

        let strings: Vec<&str> = options.iter().map(String::as_str).collect();
        dropdown.set_model(Some(&gtk::StringList::new(&strings)));
        // Putting the model back resets the selection, which would silently
        // widen the filter under somebody mid-read.
        if let Some(chosen) = chosen {
            if let Some(index) = options.iter().position(|name| name == &chosen) {
                dropdown.set_selected(index as u32);
            }
        }
        dropdown.set_visible(options.len() > 2);
    }

    fn redraw(&self) {
        let imp = self.imp();
        let (Some(cards), Some(rail)) = (imp.cards.borrow().clone(), imp.rail.borrow().clone())
        else {
            return;
        };
        for container in [&cards, &rail] {
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }
        }

        let Some(held) = imp.held.borrow().clone() else {
            return;
        };

        // Only evenings worth showing. Logging in to post an auction is
        // recorded, because taking it back out later is impossible, but a
        // journal whose first screen is nine of those is one nobody opens
        // twice.
        let mut digests: Vec<Digest> = held
            .sessions
            .iter()
            .map(Session::digest)
            .filter(Digest::is_worth_writing)
            .collect();
        digests.sort_by_key(|digest| std::cmp::Reverse(digest.started_at));

        let only = imp.only.borrow().clone();
        self.draw_rail(&rail, &held, &digests, only.as_deref());

        if digests.is_empty() {
            cards.append(&self.nothing_yet(&held));
            return;
        }

        let needle = imp.needle.borrow().clone();
        let showing = imp.showing.get();
        let writing = imp.writing.borrow().clone();

        let shown: Vec<&Digest> = digests
            .iter()
            .filter(|digest| {
                let entry = held.entries.get(&digest.id());
                match showing {
                    Showing::Unwritten if entry.is_some() => return false,
                    Showing::Written if entry.is_none() => return false,
                    _ => {}
                }
                if only
                    .as_ref()
                    .is_some_and(|name| name != &digest.display_name)
                {
                    return false;
                }
                needle.is_empty() || matches(digest, entry, &needle)
            })
            .collect();

        if shown.is_empty() {
            cards.append(
                &adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title("No matching evenings")
                    .description("Nothing in the journal matches that.")
                    .vexpand(true)
                    .build(),
            );
            return;
        }

        // The spine, the date gutter and the cards. A `GtkGrid` rather than an
        // overlay: the spine has to run the height of the whole list, and the
        // date markers have to sit in a gutter the cards do not reflow into.
        let grid = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(18)
            .build();
        // The spine's drawing area is two pixels wide and the markers hanging
        // off it are twenty, and both live in this one column. Giving the spine
        // a gutter the width of a marker is what stops the grid sizing the
        // column to the line and squeezing every dot into three pixels.
        let gutter = almanac::column(0);
        gutter.set_size_request(20, -1);
        gutter.append(&almanac::spine());
        grid.attach(&gutter, 1, 0, 1, shown.len() as i32);

        for (index, digest) in shown.iter().enumerate() {
            let row = index as i32;
            let newest = index == 0;
            grid.attach(&Self::date_marker(digest, newest), 0, row, 1, 1);

            let dot = almanac::spine_dot(newest);
            // Down onto the first line of the card rather than level with its
            // top edge, which on the written card is the art band. One offset
            // for both kinds: `spine_dot` draws two markers at one widget size
            // precisely so a caller does not have to compensate per state.
            dot.set_margin_top(2);
            grid.attach(&dot, 1, row, 1, 1);

            let entry = held.entries.get(&digest.id());
            let card = match entry {
                Some(entry) => self.written_card(digest, entry, &held),
                None => self.unwritten_card(digest, &held, writing.contains(&digest.id())),
            };
            card.set_hexpand(true);
            card.set_margin_start(8);
            grid.attach(&card, 2, row, 1, 1);
        }

        cards.append(
            &adw::Clamp::builder()
                .maximum_size(MEASURE)
                .halign(gtk::Align::Start)
                .child(&grid)
                .build(),
        );
    }

    /// Where a card sits on the calendar: the day over the month.
    ///
    /// The newest day is gold and every other one is quiet, which is the same
    /// distinction the spine's dot makes and the reason neither needs a label
    /// saying "last night".
    fn date_marker(digest: &Digest, newest: bool) -> gtk::Box {
        let block = almanac::column(3);
        block.set_valign(gtk::Align::Start);
        block.set_halign(gtk::Align::End);
        block.set_size_request(44, -1);

        let mut classes = vec!["al-day"];
        if newest {
            classes.push("al-gold");
        }
        let day = almanac::mono(&digest.started_at.format("%d").to_string(), &classes);
        day.set_xalign(1.0);
        day.set_halign(gtk::Align::End);
        block.append(&day);

        let month = almanac::meta(&digest.started_at.format("%b").to_string());
        month.set_xalign(1.0);
        month.set_halign(gtk::Align::End);
        block.append(&month);

        block
    }

    /// What the page says before the addon has written anything.
    ///
    /// Two different nothings, and they need two different answers: nobody has
    /// installed the addon, or nobody has set the journal up. Saying "no
    /// entries" to the first would leave somebody waiting for a file that will
    /// never be written.
    fn nothing_yet(&self, held: &Held) -> adw::StatusPage {
        let page = adw::StatusPage::builder()
            .icon_name("document-edit-symbolic")
            .title("No evenings recorded yet")
            .description(
                "The Chronicle records a session when you log out — where you went, \
                 what you turned in, what dropped. Install the collector addon, play, \
                 and log out once.",
            )
            .vexpand(true)
            .build();

        if !held.configured {
            let button = gtk::Button::builder()
                .label("Set Up the Journal")
                .halign(gtk::Align::Center)
                .build();
            button.add_css_class("pill");
            button.add_css_class("suggested-action");
            let this = self.clone();
            button.connect_clicked(move |_| {
                if let Some(open) = this.imp().on_setup.borrow().as_ref() {
                    open();
                }
            });
            page.set_child(Some(&button));
        }

        page
    }

    // -- the cards ------------------------------------------------------------

    /// An evening somebody has an entry for.
    ///
    /// Art band, then the entry, then the working out. Which is the order a
    /// person reads a diary page in, and the reason this is assembled by hand
    /// rather than as one `AdwPreferencesGroup` — that widget appends anything
    /// which is not a row *underneath* its boxed list, however early it is
    /// added, so built that way every card put its entry below its own
    /// footnotes.
    fn written_card(&self, digest: &Digest, entry: &Entry, held: &Held) -> gtk::Box {
        let card = almanac::column(0);
        card.add_css_class("al-card");
        card.add_css_class("al-flush");

        card.append(&self.band(digest, &entry.title));

        let body = almanac::column(13);
        body.set_margin_top(14);
        body.set_margin_bottom(16);
        body.set_margin_start(18);
        body.set_margin_end(18);

        body.append(&almanac::prose(&entry.body));
        // Said on every entry, not in a tooltip and not once in an about box.
        // The rest of this card is measurements from the person's own machine;
        // this paragraph is not, and the difference should never need looking
        // up.
        body.append(&almanac::caption(&format!(
            "Written by {} on {}",
            entry.model,
            entry.written_at.format("%-d %B %Y")
        )));

        self.append_evening(&body, digest, held);
        card.append(&body);
        card
    }

    /// An evening with nothing written about it yet.
    ///
    /// Deliberately the compact form: no art band, a smaller title, and the
    /// facts a press away rather than laid out. Somebody looking at this list
    /// is working through a queue, and the log is still here for the card that
    /// will never have prose on it.
    fn unwritten_card(&self, digest: &Digest, held: &Held, writing: bool) -> gtk::Box {
        let card = almanac::column(13);
        card.add_css_class("al-card");
        card.add_css_class("al-roomy");

        let titles = almanac::column(4);
        titles.set_hexpand(true);
        titles.set_valign(gtk::Align::Center);
        titles.append(&almanac::serif(&digest.headline(), "al-entry-title"));
        titles.append(&almanac::meta(&meta_line(digest)));

        let header = almanac::row(16);
        header.append(&titles);

        if writing {
            let waiting = almanac::row(8);
            waiting.set_valign(gtk::Align::Center);
            waiting.append(&adw::Spinner::new());
            waiting.append(&almanac::caption("Writing…"));
            header.append(&waiting);
        } else if held.configured {
            // An ordinary outlined button, and not the suggested action. The
            // card is complete without an entry; making this the page's accent
            // would be the journal selling somebody prose it does not need.
            let write = gtk::Button::builder()
                .label("Write Entry")
                .valign(gtk::Align::Center)
                .build();
            let this = self.clone();
            let id = digest.id();
            write.connect_clicked(move |_| {
                if let Some(write) = this.imp().on_write.borrow().as_ref() {
                    write(id.clone());
                }
            });
            header.append(&write);
        }
        header.append(&self.card_menu(digest, false));
        card.append(&header);

        card.append(&self.facts(digest));
        let pictures = digest.pictures(&held.shots);
        if !pictures.is_empty() {
            card.append(&gallery(&pictures));
        }
        card
    }

    /// The card's head: the zone's key art, a wash, and the evening's name.
    ///
    /// Zone art is not fetched from anywhere yet — there is no endpoint Armory
    /// asks for it and no corpus shipping it — so the band is its own card
    /// colour under the same scrim the real picture will take. The design works
    /// without it, and a card that waited for a picture would never draw.
    fn band(&self, digest: &Digest, title: &str) -> gtk::Overlay {
        let art = gtk::Box::new(gtk::Orientation::Vertical, 0);
        art.add_css_class("al-band");
        art.set_size_request(-1, 96);

        let titles = almanac::column(5);
        titles.set_vexpand(true);
        titles.set_valign(gtk::Align::End);
        titles.set_margin_top(12);
        titles.set_margin_bottom(12);
        titles.set_margin_start(16);
        titles.set_margin_end(16);
        titles.append(&almanac::serif(title, "al-card-title"));
        titles.append(&almanac::meta(&meta_line(digest)));

        let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scrim.add_css_class("al-scrim");
        scrim.append(&titles);

        let overlay = gtk::Overlay::builder().child(&art).build();
        overlay.add_overlay(&scrim);

        let menu = self.card_menu(digest, true);
        menu.set_halign(gtk::Align::End);
        menu.set_valign(gtk::Align::Start);
        menu.set_margin_top(6);
        menu.set_margin_end(6);
        overlay.add_overlay(&menu);

        overlay
    }

    /// Everything a card says about the evening below its title.
    fn append_evening(&self, body: &gtk::Box, digest: &Digest, held: &Held) {
        if let Some(chain) = route_chain(digest) {
            body.append(&chain);
        }

        // The same three facts the Run page leads with, at the smaller size —
        // they are about this evening, and this is the evening they came from.
        let numbers = RunPage::three_numbers(digest, true);
        if !numbers.is_empty() {
            let row = almanac::row(9);
            row.set_homogeneous(true);
            for card in numbers {
                row.append(&card);
            }
            body.append(&row);
        }

        if let Some(books) = ledger(digest) {
            body.append(&books);
        }
        if let Some(quote) = overheard(digest) {
            body.append(&quote);
        }

        body.append(&self.facts(digest));

        // Under the facts, because a picture is the evening's punctuation
        // rather than its content — and because a card that led with one would
        // be a gallery with a paragraph attached.
        let pictures = digest.pictures(&held.shots);
        if !pictures.is_empty() {
            body.append(&gallery(&pictures));
        }
    }

    /// The working out: the whole log, and where to read more.
    fn facts(&self, digest: &Digest) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        group.add(&self.log(digest));
        if let Some(reading) = self.reading(digest) {
            group.add(&reading);
        }
        group
    }

    /// The menu on a card: the things you do to an evening rather than in it.
    fn card_menu(&self, digest: &Digest, written: bool) -> gtk::MenuButton {
        let menu = gtk::gio::Menu::new();
        if written {
            menu.append(Some("Write It Again"), Some("card.write"));
        }
        menu.append(Some("Forget This Evening"), Some("card.forget"));

        let actions = gtk::gio::SimpleActionGroup::new();

        let write = gtk::gio::SimpleAction::new("write", None);
        let this = self.clone();
        let id = digest.id();
        write.connect_activate(move |_, _| {
            if let Some(write) = this.imp().on_write.borrow().as_ref() {
                write(id.clone());
            }
        });
        actions.add_action(&write);

        let forget = gtk::gio::SimpleAction::new("forget", None);
        let this = self.clone();
        let id = digest.id();
        let when = digest.started_at.format("%A %-d %B").to_string();
        let who = digest.display_name.clone();
        forget.connect_activate(move |_, _| {
            // Destructive, and the only thing here that cannot be undone by
            // syncing again — the addon keeps its last forty sessions and this
            // one may have fallen off the end. So it is confirmed rather than
            // toasted with an undo that might not be able to deliver.
            let dialog = adw::AlertDialog::new(
                Some("Forget this evening?"),
                Some(&format!(
                    "{who}'s session on {when} will be removed from the journal, along \
                     with anything written about it. This cannot be undone."
                )),
            );
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("forget", "Forget");
            dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");

            let page = this.clone();
            let id = id.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "forget" {
                    if let Some(forget) = page.imp().on_forget.borrow().as_ref() {
                        forget(id.clone());
                    }
                }
            });
            dialog.present(Some(&this));
        });
        actions.add_action(&forget);

        let button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Actions for this evening")
            .valign(gtk::Align::Center)
            .menu_model(&menu)
            .build();
        button.add_css_class("flat");
        button.insert_action_group("card", Some(&actions));
        button
    }

    /// What actually happened, in a row that starts closed.
    ///
    /// Closed because on a card that has prose this is the working out, and
    /// because on a card that has not, it is a press away rather than a screen
    /// of rows between one evening and the next.
    fn log(&self, digest: &Digest) -> adw::ExpanderRow {
        let row = adw::ExpanderRow::builder()
            .title("What happened")
            .subtitle(tally(digest))
            .expanded(false)
            .build();

        if !digest.route.is_empty() {
            row.add_row(&fact(
                "Route",
                &digest
                    .route
                    .iter()
                    .map(|stop| stop.zone.clone())
                    .collect::<Vec<_>>()
                    .join(" → "),
            ));
        }
        for (name, summary) in &digest.campaigns {
            let campaign = adw::ActionRow::builder()
                .title(name)
                .subtitle(summary.as_deref().unwrap_or("Storyline"))
                .subtitle_lines(3)
                .build();
            campaign.add_prefix(&gtk::Image::from_icon_name(
                "accessories-text-editor-symbolic",
            ));
            row.add_row(&campaign);
        }
        for key in &digest.keystones {
            row.add_row(&fact(
                &format!("+{} {}", key.level, key.dungeon),
                &format!(
                    "{} in {}{}",
                    if key.in_time {
                        "Timed"
                    } else {
                        "Over the timer"
                    },
                    spell(chrono::Duration::seconds(i64::from(key.seconds))),
                    match key.upgrades {
                        0 => String::new(),
                        n => format!(" · key up {n}"),
                    }
                ),
            ));
        }
        if !digest.instances.is_empty() && digest.keystones.is_empty() {
            row.add_row(&fact(
                "Instances",
                &digest
                    .instances
                    .iter()
                    .map(|(name, kind)| format!("{name} ({kind})"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.scenarios.is_empty() {
            row.add_row(&fact("Scenarios", &digest.scenarios.join(", ")));
        }
        for quest in &digest.quests {
            let quest_row = adw::ActionRow::builder()
                .title(&quest.title)
                .subtitle(
                    quest
                        .story
                        .as_deref()
                        .or(quest.premise.as_deref())
                        .unwrap_or("Completed"),
                )
                .subtitle_lines(3)
                .build();
            quest_row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
            row.add_row(&quest_row);
        }
        if !digest.felled.is_empty() {
            row.add_row(&fact("Defeated", &digest.felled.join(", ")));
        }
        if !digest.lost_to.is_empty() {
            row.add_row(&fact("Wiped on", &digest.lost_to.join(", ")));
        }
        if !digest.rares.is_empty() {
            row.add_row(&fact("Rares", &digest.rares.join(", ")));
        }
        for (_, name) in &digest.achievements {
            row.add_row(&fact("Achievement", name));
        }
        if !digest.acquired.is_empty() {
            row.add_row(&fact(
                "Collected",
                &digest
                    .acquired
                    .iter()
                    .map(|(kind, name)| format!("{name} ({})", kind.label()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.loot.is_empty() {
            row.add_row(&fact(
                "Loot",
                &digest
                    .loot
                    .iter()
                    .map(|(_, name, _)| name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.sales.is_empty() {
            row.add_row(&fact(
                "Sold",
                &digest
                    .sales
                    .iter()
                    .map(|(subject, amount)| format!("{subject} — {}", money(*amount)))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.deaths.is_empty() {
            row.add_row(&fact(
                "Deaths",
                &digest
                    .deaths
                    .iter()
                    .map(|death| match &death.to {
                        // What killed you is the half of a death worth reading.
                        Some(to) => format!("{} — {to}", death.zone),
                        None => death.zone.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.risen.is_empty() {
            row.add_row(&fact(
                "Standing",
                &digest
                    .risen
                    .iter()
                    .map(|(name, rank)| format!("{name} → {}", standing(*rank)))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.equipped.is_empty() {
            row.add_row(&fact(
                "Upgraded",
                &digest
                    .equipped
                    .iter()
                    .take(5)
                    .map(|gear| match &gear.from {
                        Some(source) => {
                            format!("{} ({}, off {source})", gear.name, gear.item_level)
                        }
                        None => format!("{} ({})", gear.name, gear.item_level),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.practised.is_empty() {
            row.add_row(&fact(
                "Professions",
                &digest
                    .practised
                    .iter()
                    .map(|(name, skill)| format!("{name} {skill}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.appearances.is_empty() {
            row.add_row(&fact("Appearances", &digest.appearances.join(", ")));
        }
        if !digest.questgivers.is_empty() {
            row.add_row(&fact(
                "Sent out by",
                &digest
                    .questgivers
                    .iter()
                    .map(|(who, given)| match given {
                        1 => who.clone(),
                        given => format!("{who} ×{given}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.learned.is_empty() {
            row.add_row(&fact("Learned", &digest.learned.join(", ")));
        }
        // Its own row per line rather than one joined row: these are sentences,
        // and a dozen sentences run together with commas is not readable as
        // anything.
        for (who, line) in digest.overheard.iter().take(OVERHEARD_SHOWN) {
            row.add_row(&fact(if who.is_empty() { "Overheard" } else { who }, line));
        }
        // Fewer than the overheard lines get, and never merged with them: this
        // is the half that is mostly shopkeepers, and a card is not a
        // transcript of an errand.
        for (who, line) in digest.told.iter().take(TOLD_SHOWN) {
            row.add_row(&fact(
                if who.is_empty() { "Said to you" } else { who },
                line,
            ));
        }
        if !digest.cutscenes.is_empty() {
            row.add_row(&fact(
                "Cutscenes",
                &digest
                    .cutscenes
                    .iter()
                    .map(|(zone, movie)| match movie {
                        // The id is worth showing: it is the only thing that
                        // names a cinematic, and it is what somebody would
                        // search for to find out which one it was.
                        Some(id) => format!("{zone} (movie {id})"),
                        None => zone.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !digest.expired.is_empty() {
            row.add_row(&fact("Came back unsold", &digest.expired.join(", ")));
        }
        if !digest.companions.is_empty() {
            row.add_row(&fact("Alongside", &digest.companions.join(", ")));
        }
        if digest.kills > 0 {
            row.add_row(&fact("Killed", &format!("about {}", digest.kills)));
        }
        if !digest.crafted.is_empty() {
            row.add_row(&fact(
                "Crafted",
                &digest
                    .crafted
                    .iter()
                    .map(|(name, made)| match made {
                        1 => name.clone(),
                        made => format!("{name} ×{made}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if digest.flights > 0 {
            row.add_row(&fact(
                "Flights",
                &plural(digest.flights as usize, "flight", "flights"),
            ));
        }
        if digest.travelled > 0 {
            row.add_row(&fact("Travelled", &tally::far(digest.travelled)));
        }
        if digest.longest_fight > 0 {
            row.add_row(&fact(
                "Longest fight",
                &tally::spent(u64::from(digest.longest_fight)),
            ));
        }
        if digest.worst_hit > 0 {
            row.add_row(&fact(
                "Hardest hit taken",
                &match &digest.worst_hit_by {
                    Some(who) => format!("{} from {who}", digest.worst_hit),
                    None => digest.worst_hit.to_string(),
                },
            ));
        }
        // 100 is "nothing ever touched them", which is not a fact worth a row.
        if digest.lowest_health < 100 {
            row.add_row(&fact(
                "Closest call",
                &format!("{}% health", digest.lowest_health),
            ));
        }
        row.add_row(&fact("Purse", &purse(digest.purse)));

        // The books, biggest first. A net purse says an evening cost forty
        // gold; this says it earned three hundred questing and spent three
        // hundred and forty at the auction house, which is a different evening.
        for (title, book, incoming) in [
            ("Money in", &digest.income, true),
            ("Money out", &digest.spending, false),
        ] {
            if book.is_empty() {
                continue;
            }
            row.add_row(&fact(
                title,
                &book
                    .iter()
                    .map(|(purpose, amount)| {
                        format!(
                            "{} {}",
                            money(*amount),
                            purpose.label(incoming).to_lowercase()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            ));
        }

        row
    }

    /// Where to read more. Links, and only links.
    fn reading(&self, digest: &Digest) -> Option<adw::ExpanderRow> {
        let links = digest.further_reading();
        if links.is_empty() {
            return None;
        }

        let row = adw::ExpanderRow::builder()
            .title("Further reading")
            .subtitle("Opens in your browser")
            .expanded(false)
            .build();

        for link in links {
            let link_row = adw::ActionRow::builder()
                .title(&link.label)
                .subtitle(link.sort.label())
                .activatable(true)
                .build();
            link_row.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));

            let url = link.url.clone();
            link_row.connect_activated(move |row| {
                gtk::UriLauncher::new(&url).launch(
                    row.root().and_downcast::<gtk::Window>().as_ref(),
                    gtk::gio::Cancellable::NONE,
                    |_| {},
                );
            });
            row.add_row(&link_row);
        }

        Some(row)
    }

    // -- the rail -------------------------------------------------------------

    /// What one character has done over months, and how long the journal is.
    ///
    /// The counters are deliberately not on a card. Every number in the main
    /// column belongs to an evening; these belong to a character and would be a
    /// lie repeated nightly if they were drawn beside one.
    fn draw_rail(&self, rail: &gtk::Box, held: &Held, digests: &[Digest], only: Option<&str>) {
        let scope: Vec<&Digest> = digests
            .iter()
            .filter(|digest| only.is_none() || only == Some(digest.display_name.as_str()))
            .collect();

        match self.single_character(held, only) {
            Some((name, counted)) => {
                let block = almanac::column(9);
                block.append(&almanac::section(&format!("{name}, over time")));
                block.append(&almanac::caption(
                    "Counters the game does not keep, since the addon was installed.",
                ));
                if let Some(rows) = counters(&counted) {
                    block.append(&rows);
                }
                rail.append(&block);
            }
            None => {
                let block = almanac::column(9);
                block.append(&almanac::section("Over time"));
                block.append(&almanac::caption(
                    "Pick one character to see the counters the game does not keep. \
                     Everyone having made four hundred flasks is nobody's achievement.",
                ));
                rail.append(&block);
            }
        }

        if let Some(oldest) = scope.iter().map(|digest| digest.started_at).min() {
            let minutes: i64 = scope
                .iter()
                .map(|digest| digest.duration().num_minutes().max(0))
                .sum();
            rail.append(&almanac::hairline());

            let footer = almanac::column(2);
            footer.append(&almanac::meta(&format!(
                "{} · {}",
                almanac::plural(scope.len(), "evening", "evenings"),
                almanac::plural((minutes / 60) as usize, "hour", "hours"),
            )));
            footer.append(&almanac::meta(&format!(
                "since {}",
                oldest.format("%-d %B %Y")
            )));
            rail.append(&footer);
        }
    }

    /// The one character being looked at, and their counters.
    ///
    /// One character, whether because the filter says so or because only one
    /// has ever played. The dropdown hides itself in the second case, so
    /// requiring the filter would keep these off a single-character account's
    /// rail permanently.
    fn single_character(&self, held: &Held, only: Option<&str>) -> Option<(String, Vec<Tally>)> {
        let mut keys: Vec<(&CharacterKey, &str)> = held
            .sessions
            .iter()
            .filter(|session| only.is_none() || only == Some(session.display_name.as_str()))
            .map(|session| (&session.character, session.display_name.as_str()))
            .collect();
        keys.sort();
        keys.dedup();
        let [(key, name)] = keys[..] else {
            return None;
        };
        let counted = held.tallies.get(key)?;
        Some((name.to_string(), counted.clone()))
    }
}

/// The lifetime counters, one row each.
///
/// The head of each list only: the biggest one is the answer most of the time,
/// and the rest are in the tooltip rather than in eight open lists. This is a
/// journal, and pushing tonight's evening below how many flasks somebody has
/// made would be the wrong page.
fn counters(counted: &[Tally]) -> Option<gtk::Box> {
    let group = almanac::card(10);
    let mut any = false;

    // In this order, because it is roughly descending by how much somebody
    // wants to know: what they do, who they do it with, where it happens, and
    // then the ones that are jokes about how much of their life this is.
    for kind in [
        Counting::Recipe,
        Counting::Companion,
        Counting::Victory,
        Counting::Attempt,
        Counting::Delve,
        Counting::Zone,
        Counting::Killer,
        Counting::Distance,
        Counting::Flight,
    ] {
        let rows = tally::of(counted, kind);
        let Some(first) = rows.first() else {
            continue;
        };
        if any {
            group.append(&almanac::hairline());
        }

        let block = almanac::column(3);
        block.append(&almanac::label(kind.title(), &[]));

        let line = almanac::row(8);
        let label = almanac::label(&first.label, &["al-caption"]);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&label);
        let count = almanac::mono(&counted_as(kind, first.count), &["al-price", "al-gold"]);
        count.set_halign(gtk::Align::End);
        line.append(&count);
        block.append(&line);

        // The rest of the list, where it costs no height. Somebody who has made
        // two hundred different things wants to know that; they do not want to
        // scroll past it to reach their journal.
        block.set_tooltip_text(Some(
            &rows
                .iter()
                .take(TALLY_SHOWN)
                .map(|entry| format!("{} — {}", entry.label, said(kind, entry.count)))
                .collect::<Vec<_>>()
                .join("\n"),
        ));

        group.append(&block);
        any = true;
    }

    any.then_some(group)
}

/// The route, as pills joined end to end.
///
/// The zone the evening was actually *spent* in is the one it stayed longest
/// at, not the one it finished in — an evening in Nagrand that ended with a
/// hearthstone to Dornogal was an evening in Nagrand. That stop is the only
/// gold thing here, and it carries how long.
fn route_chain(digest: &Digest) -> Option<gtk::Widget> {
    if digest.route.is_empty() {
        return None;
    }
    let longest = digest
        .route
        .iter()
        .enumerate()
        .max_by_key(|(_, stop)| stop.stayed)
        .map(|(index, _)| index);

    let chain = adw::WrapBox::builder()
        .child_spacing(0)
        .line_spacing(6)
        .build();

    for (index, stop) in digest.route.iter().take(ROUTE_SHOWN).enumerate() {
        if index > 0 {
            let rule = almanac::hairline();
            rule.set_size_request(18, 1);
            rule.set_valign(gtk::Align::Center);
            chain.append(&rule);
        }
        let chip = if Some(index) == longest {
            almanac::chip(
                &format!("{} · {}", stop.zone, tally::spent(u64::from(stop.stayed))),
                Tone::Gold,
            )
        } else {
            almanac::chip(&stop.zone, Tone::Plain)
        };
        chain.append(&chip);
    }

    if digest.route.len() > ROUTE_SHOWN {
        let rule = almanac::hairline();
        rule.set_size_request(18, 1);
        rule.set_valign(gtk::Align::Center);
        chain.append(&rule);
        chain.append(&almanac::chip(
            &format!("{} more", digest.route.len() - ROUTE_SHOWN),
            Tone::Plain,
        ));
    }

    Some(chain.upcast())
}

/// Where the gold went, as one bar.
///
/// **The ledger is the only set of books.** `Paid` and `Sold` say *what* —
/// "Mycobloom sold for 374g" — and the ledger says how much moved and why.
/// Deriving these totals from the itemised moments instead double-counts every
/// quest reward and every auction sale, which is exactly what the `questPaid`
/// flag in `Chronicle.lua` exists to prevent.
fn ledger(digest: &Digest) -> Option<gtk::Box> {
    let (income, spending) = ledger_shares(&digest.income, &digest.spending);
    if income.is_empty() && spending.is_empty() {
        return None;
    }

    let card = almanac::card(8);
    card.add_css_class("al-tight");

    let heading = almanac::row(8);
    let title = almanac::meta("Where the gold went");
    title.set_hexpand(true);
    heading.append(&title);
    let (word, tone) = if digest.purse < 0 {
        ("down", Tone::Negative)
    } else {
        ("up", Tone::Positive)
    };
    let net = almanac::mono(
        &format!("{word} {}", gold(digest.purse.unsigned_abs())),
        &["al-meta", tone_class(tone)],
    );
    net.set_halign(gtk::Align::End);
    heading.append(&net);
    card.append(&heading);

    card.append(&almanac::ledger(income, spending));

    let legend = adw::WrapBox::builder()
        .child_spacing(14)
        .line_spacing(4)
        .build();
    for (book, incoming) in [(&digest.income, true), (&digest.spending, false)] {
        for (purpose, amount) in book.iter().take(2) {
            legend.append(&almanac::caption(&format!(
                "{} {}",
                gold(*amount),
                purpose.label(incoming).to_lowercase()
            )));
        }
    }
    card.append(&legend);

    Some(card)
}

/// Each book's segments, as shares of everything that moved.
///
/// The denominator is income *and* spending together, so the bar is the shape
/// of the evening's money rather than of either half: a night that earned three
/// hundred and spent ten reads as almost all green, and one that earned three
/// hundred and spent three hundred and forty reads as the shopping trip it was.
fn ledger_shares(income: &[(Purpose, u64)], spending: &[(Purpose, u64)]) -> (Segments, Segments) {
    let total: u64 = income.iter().map(|(_, amount)| amount).sum::<u64>()
        + spending.iter().map(|(_, amount)| amount).sum::<u64>();
    if total == 0 {
        return (Vec::new(), Vec::new());
    }
    let share = |book: &[(Purpose, u64)]| -> Segments {
        book.iter()
            .enumerate()
            .map(|(index, (_, amount))| (*amount as f64 / total as f64, index == 0))
            .collect()
    };
    (share(income), share(spending))
}

/// One line the world said, pulled out of the evening.
///
/// `Said` only. What an NPC said when the character walked up and asked is
/// `Told`, it is mostly a shopkeeper's greeting, and it keeps its own budget in
/// the log rather than competing for this one line.
fn overheard(digest: &Digest) -> Option<gtk::Box> {
    let (who, line) = digest.overheard.first()?;

    let block = almanac::column(4);
    block.add_css_class("al-said");
    block.append(&almanac::serif(&format!("“{line}”"), "al-quote"));

    let mut parts = Vec::new();
    if !who.is_empty() {
        parts.push(who.clone());
    }
    if let Some(stop) = digest.route.iter().max_by_key(|stop| stop.stayed) {
        parts.push(stop.zone.clone());
    }
    let mut attribution = parts.join(", ");
    if digest.overheard.len() > 1 {
        attribution.push_str(&format!(" · {} more", digest.overheard.len() - 1));
    }
    if !attribution.is_empty() {
        block.append(&almanac::meta(&attribution));
    }

    Some(block)
}

/// The evening's screenshots, in a row that scrolls sideways.
///
/// Loaded straight off disk by path. These are the player's own files, written
/// by their own client into their own folder — there is nothing to fetch and
/// nothing to cache.
fn gallery(pictures: &[Picture]) -> gtk::ScrolledWindow {
    let strip = almanac::row(8);

    for picture in pictures {
        let frame = almanac::column(4);

        let image = gtk::Picture::for_filename(&picture.path);
        image.set_content_fit(gtk::ContentFit::Cover);
        image.set_size_request(240, 135);
        image.add_css_class("art-large");
        image.set_tooltip_text(Some(&picture.path));

        let caption = almanac::caption(&picture.subject);
        caption.set_wrap(false);
        caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
        caption.set_max_width_chars(28);

        frame.append(&image);
        frame.append(&caption);
        strip.append(&frame);
    }

    gtk::ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&strip)
        .build()
}

/// One labelled fact.
fn fact(title: &str, value: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(value)
        .subtitle_lines(3)
        .build()
}

/// A card's meta line: when it started, how long, and what it was.
///
/// "20:14 — 3h 41m · 12 quests · 1 death", uppercased by the stylesheet. The
/// instance is last because on an evening that had one it is the answer to
/// "what was this", and on an evening that did not there is nothing to say.
fn meta_line(digest: &Digest) -> String {
    let minutes = digest.duration().num_minutes().max(0);
    let mut parts = vec![format!(
        "{} — {}h {}m",
        digest.started_at.format("%H:%M"),
        minutes / 60,
        minutes % 60
    )];
    if !digest.quests.is_empty() {
        parts.push(almanac::plural(digest.quests.len(), "quest", "quests"));
    }
    if !digest.deaths.is_empty() {
        parts.push(almanac::plural(digest.deaths.len(), "death", "deaths"));
    }
    if let Some((name, _)) = digest.instances.first() {
        parts.push(name.clone());
    }
    parts.join(" · ")
}

/// Copper as gold alone, which is all a ledger legend has room for.
fn gold(copper: u64) -> String {
    match copper / 10_000 {
        0 => "under 1g".to_string(),
        amount => format!("{}g", almanac::thousands(amount)),
    }
}

/// The style class a tone carries as text.
fn tone_class(tone: Tone) -> &'static str {
    match tone {
        Tone::Gold => "al-gold",
        Tone::Positive => "al-positive",
        Tone::Negative => "al-negative",
        Tone::Plain => "al-caption",
    }
}

/// The one-line count under "What happened".
fn tally(digest: &Digest) -> String {
    let mut parts = Vec::new();
    if !digest.quests.is_empty() {
        parts.push(plural(digest.quests.len(), "quest", "quests"));
    }
    if !digest.felled.is_empty() {
        parts.push(plural(digest.felled.len(), "boss", "bosses"));
    }
    if !digest.deaths.is_empty() {
        parts.push(plural(digest.deaths.len(), "death", "deaths"));
    }
    if parts.is_empty() {
        return digest.headline();
    }
    parts.join(" · ")
}

/// A counter's number in the rail, where the label beside it says what it is.
///
/// The two that are a measurement keep their unit — "68400" for an afternoon in
/// Nagrand would be nonsense — and everything else is the bare count.
fn counted_as(kind: Counting, count: u64) -> String {
    match kind {
        Counting::Zone => tally::spent(count),
        Counting::Distance => tally::far(count),
        _ => almanac::thousands(count),
    }
}

/// The same number said in full, for a tooltip that has room for the noun.
fn said(kind: Counting, count: u64) -> String {
    match kind {
        Counting::Zone => tally::spent(count),
        Counting::Distance => tally::far(count),
        Counting::Recipe => plural(count as usize, "time", "times"),
        Counting::Companion => plural(count as usize, "evening", "evenings"),
        Counting::Delve => plural(count as usize, "delve", "delves"),
        Counting::Questgiver => plural(count as usize, "quest", "quests"),
        Counting::Rare => plural(count as usize, "kill", "kills"),
        Counting::Attempt => plural(count as usize, "attempt", "attempts"),
        Counting::Victory | Counting::Killer | Counting::Flight => count.to_string(),
    }
}

/// Whether an evening matches what is being searched for.
///
/// Across the prose *and* the log, because somebody looking for "Nagrand" does
/// not know or care which of the two it is written in.
fn matches(digest: &Digest, entry: Option<&Entry>, needle: &str) -> bool {
    if let Some(entry) = entry {
        if entry.title.to_lowercase().contains(needle) || entry.body.to_lowercase().contains(needle)
        {
            return true;
        }
    }
    digest.display_name.to_lowercase().contains(needle)
        || digest
            .route
            .iter()
            .any(|stop| stop.zone.to_lowercase().contains(needle))
        || digest
            .quests
            .iter()
            .any(|quest| quest.title.to_lowercase().contains(needle))
        || digest
            .felled
            .iter()
            .any(|name| name.to_lowercase().contains(needle))
        || digest
            .achievements
            .iter()
            .any(|(_, name)| name.to_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::character::{CharacterKey, Faction};
    use crate::model::chronicle::{Happening, Moment};
    use chrono::{TimeZone, Utc};

    fn digest() -> Digest {
        Session {
            character: CharacterKey::new("emerald-dream", "Somechar"),
            display_name: "Somechar".into(),
            realm_name: "Emerald Dream".into(),
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            started_at: Utc.with_ymd_and_hms(2026, 8, 3, 19, 0, 0).unwrap(),
            ended_at: Utc.with_ymd_and_hms(2026, 8, 3, 21, 0, 0).unwrap(),
            start_level: 70,
            end_level: 70,
            start_money: 0,
            end_money: 0,
            start_item_level: 600,
            end_item_level: 600,
            moments: vec![
                Moment {
                    at: 0,
                    what: Happening::Arrived {
                        zone: "Nagrand".into(),
                        subzone: None,
                        map: None,
                    },
                },
                Moment {
                    at: 10,
                    what: Happening::Completed {
                        quest: 1,
                        title: "Hero of the Mag'har".into(),
                        story: None,
                    },
                },
            ],
            kills: 0,
            risen: Vec::new(),
            travelled: 0,
            longest_fight: 0,
            worst_hit: 0,
            worst_hit_by: None,
            lowest_health: 100,
        }
        .digest()
    }

    fn entry() -> Entry {
        Entry {
            session: digest().id(),
            title: "Halaa Again".into(),
            body: "The wind came off the plains all evening.".into(),
            model: "claude-opus-5".into(),
            written_at: Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap(),
        }
    }

    #[test]
    fn a_search_reaches_the_prose_and_the_log_alike() {
        // Somebody looking for "Nagrand" does not know or care which of the two
        // it is written in.
        let digest = digest();
        let entry = entry();

        assert!(matches(&digest, Some(&entry), "wind"));
        assert!(matches(&digest, Some(&entry), "halaa"));
        assert!(matches(&digest, None, "nagrand"));
        assert!(matches(&digest, None, "mag'har"));
        assert!(matches(&digest, None, "somechar"));
        assert!(!matches(&digest, None, "wind"));
        assert!(!matches(&digest, Some(&entry), "orgrimmar"));
    }

    #[test]
    fn a_tally_falls_back_to_the_headline_when_there_is_nothing_to_count() {
        let mut quiet = digest();
        quiet.quests.clear();
        assert_eq!(tally(&quiet), quiet.headline());
        assert_eq!(tally(&digest()), "1 quest");
    }

    #[test]
    fn the_ledger_bar_is_the_share_of_everything_that_moved() {
        let income = [(Purpose::Quest, 300_u64), (Purpose::Loot, 100)];
        let spending = [(Purpose::Repair, 100_u64)];
        let (earned, spent) = ledger_shares(&income, &spending);

        assert_eq!(earned.len(), 2);
        assert_eq!(spent.len(), 1);
        // The first segment of each book is the full-strength colour.
        assert!(earned[0].1 && !earned[1].1 && spent[0].1);
        assert!((earned.iter().map(|(share, _)| share).sum::<f64>() - 0.8).abs() < 1e-9);
        assert!((spent[0].0 - 0.2).abs() < 1e-9);
    }

    #[test]
    fn an_evening_that_moved_no_money_draws_no_bar() {
        // Not a bar at zero. An empty ledger is an evening nobody bought or
        // sold anything in, and drawing an empty track says the opposite of
        // nothing.
        let (income, spending) = ledger_shares(&[], &[]);
        assert!(income.is_empty() && spending.is_empty());
    }

    #[test]
    fn a_meta_line_says_what_the_evening_was() {
        // Uppercased by the stylesheet rather than here, so the count and its
        // noun still agree when they are read out.
        assert_eq!(meta_line(&digest()), "19:00 — 2h 0m · 1 quest");
    }
}
