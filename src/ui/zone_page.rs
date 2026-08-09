//! Zones: what a place is, and what you did there.
//!
//! A list of every place Armory has words for, and behind each one a page that
//! puts three things side by side that nothing else does:
//!
//! * Blizzard's world, in Armory's words — the lore corpus, shipped rather than
//!   fetched, so a zone page costs no request and works with no network.
//! * Blizzard's own account of the dungeons in it, from the Adventure Guide,
//!   which states a raid's premise rather than summarising its plot. For every
//!   raid older than Mists of Pandaria that account is empty, and ours stands
//!   in — labelled, because the two are not interchangeable.
//! * The evenings *you* spent there, out of the chronicle.
//!
//! Places you have been come first and places you have not are still listed,
//! because the lore is worth reading either way — but a page opening on a
//! hundred and forty-three zones nobody has visited would bury the four that
//! matter.
//!
//! ## What the opened page is
//!
//! A hero band, a main column and a rail. The main column is the place: what it
//! is, what happened here, what you did here, and what its dungeons are. The
//! rail is what the place is *worth* — what keeps killing you, what is worth
//! carrying out, and where the words came from.
//!
//! Three fields carry the whole page and none of them may be folded away behind
//! a disclosure: [`Delve::ours`] says whose words these are, `assumes` says what
//! the place takes for granted and the game never tells you, and `disputed`
//! says where the sources contradict each other. They are the reason the corpus
//! was written by hand instead of pasted, and a reader who has to press
//! something to find them will not.
//!
//! The attribution in the rail is not decoration either: the corpus is
//! summarised from CC BY-SA material and naming the sources is a condition of
//! using it. It renders whenever any of that prose does.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use std::cell::RefCell;

use super::almanac::{self, Tone};
use crate::model::adventure::Guide;
use crate::model::chronicle::Session;
use crate::model::place::{self, Delve, Lore, Place, Visit};

/// How many bosses a dungeon lists before it says "and more".
///
/// Icecrown Citadel has thirteen and Naxxramas fifteen. A page is a place to
/// read about somewhere, not a raid roster.
const BOSSES_SHOWN: usize = 6;

/// How many evenings one zone shows.
const VISITS_SHOWN: usize = 8;

/// How many of an evening's deaths or rares are named beside it.
const CHIPS_SHOWN: usize = 5;

/// How many things one zone's rail lists.
const KILLERS_SHOWN: usize = 6;
const SPOILS_SHOWN: usize = 8;

/// How many quests an evening names before it counts the rest.
const QUESTS_NAMED: usize = 3;

/// How wide this page's rail is.
const RAIL: f64 = 300.0;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ZonePage {
        pub nav: RefCell<Option<adw::NavigationView>>,
        pub list: RefCell<Option<gtk::Box>>,
        pub search: RefCell<Option<gtk::SearchBar>>,
        pub needle: RefCell<String>,
        pub held: RefCell<Option<super::Held>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZonePage {
        const NAME: &'static str = "ArmoryZonePage";
        type Type = super::ZonePage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for ZonePage {}
    impl WidgetImpl for ZonePage {}
    impl BinImpl for ZonePage {}
}

glib::wrapper! {
    pub struct ZonePage(ObjectSubclass<imp::ZonePage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// One redraw's worth of input.
#[derive(Clone)]
pub struct Held {
    pub sessions: Vec<Session>,
    pub tallies: crate::model::tally::Tallies,
    pub guide: Guide,
    /// What each item is, for the binding — most raid loot cannot be sold and
    /// a list that ignored that would be a list of things you cannot use.
    pub items: HashMap<u32, crate::model::source::blizzard::gamedata::Item>,
    /// The cheapest price and quantity listed, by item.
    pub market: HashMap<u32, (u64, u32)>,
}

impl Default for ZonePage {
    fn default() -> Self {
        Self::new()
    }
}

impl ZonePage {
    pub fn new() -> Self {
        let page: Self = glib::Object::builder().build();
        page.build();
        page
    }

    fn build(&self) {
        let list = almanac::column(16);
        list.add_css_class("al-main-column");
        // Packed to the top: without this the scroller hands the column the
        // whole viewport and a short list spreads itself out down the page.
        list.set_valign(gtk::Align::Start);
        *self.imp().list.borrow_mut() = Some(list.clone());

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search zones")
            .hexpand(true)
            .build();
        {
            let page = self.clone();
            entry.connect_search_changed(move |entry| {
                *page.imp().needle.borrow_mut() = entry.text().to_string();
                page.redraw();
            });
        }
        let bar = gtk::SearchBar::builder()
            .search_mode_enabled(true)
            .child(&entry)
            .build();
        bar.connect_entry(&entry);
        *self.imp().search.borrow_mut() = Some(bar.clone());

        let column = almanac::column(0);
        column.append(&bar);
        column.append(&almanac::main_column(&list));

        let nav = adw::NavigationView::new();
        nav.add(&adw::NavigationPage::new(&column, "Zones"));
        *self.imp().nav.borrow_mut() = Some(nav.clone());
        self.set_child(Some(&nav));

        self.show(&Held {
            sessions: Vec::new(),
            tallies: HashMap::new(),
            guide: Guide::default(),
            items: HashMap::new(),
            market: HashMap::new(),
        });
    }

    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    pub fn show(&self, held: &Held) {
        *self.imp().held.borrow_mut() = Some(held.clone());
        self.redraw();
    }

    /// Every place, visited ones first.
    ///
    /// Assembled on each draw rather than held: the corpus is compiled in and
    /// the chronicle is a few hundred sessions, so this is a walk over data
    /// already in memory — and holding a second copy is how the two come to
    /// disagree after a sync.
    fn places(&self, held: &Held) -> Vec<Place> {
        let written = place::unwritten();
        let mut out: Vec<Place> = place::corpus()
            .into_iter()
            .filter_map(|lore| {
                let map = lore.map?;
                Some(place::assemble(
                    map,
                    Some(&lore),
                    &held.guide,
                    &written,
                    &held.sessions,
                    &held.tallies,
                    &held.items,
                    &held.market,
                ))
            })
            .filter(Place::is_worth_showing)
            .collect();

        // Where you have been, longest first; then everywhere else, so the lore
        // is still reachable without burying the four zones that matter.
        out.sort_by(|a, b| {
            b.spent
                .cmp(&a.spent)
                .then_with(|| b.visits.len().cmp(&a.visits.len()))
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    // -- the index ------------------------------------------------------------

    fn redraw(&self) {
        let imp = self.imp();
        let (Some(list), Some(held)) = (imp.list.borrow().clone(), imp.held.borrow().clone())
        else {
            return;
        };
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        let needle = imp.needle.borrow().to_lowercase();
        let places: Vec<Place> = self
            .places(&held)
            .into_iter()
            .filter(|place| needle.is_empty() || place.name.to_lowercase().contains(&needle))
            .collect();

        if places.is_empty() {
            list.append(&self.nothing_yet(needle.is_empty()));
            return;
        }

        let been: Vec<&Place> = places.iter().filter(|p| p.spent > 0).collect();
        if !been.is_empty() {
            let cards = almanac::column(9);
            for place in been {
                cards.append(&self.place_card(place));
            }
            list.append(&block(
                "WHERE YOU HAVE BEEN",
                "Longest first, across every character",
                &cards,
            ));
        }

        let rest: Vec<&Place> = places.iter().filter(|p| p.spent == 0).collect();
        if !rest.is_empty() {
            let cards = almanac::column(9);
            for place in rest {
                cards.append(&self.place_card(place));
            }
            list.append(&block(
                "EVERYWHERE ELSE",
                "Nothing recorded here yet — the history is worth reading anyway",
                &cards,
            ));
        }
    }

    /// One place in the index.
    fn place_card(&self, place: &Place) -> gtk::Box {
        let card = almanac::card(0);
        card.add_css_class("al-activatable");

        let line = almanac::row(12);
        let text = almanac::column(3);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&almanac::serif(&place.name, "al-item-title"));

        let mut parts = Vec::new();
        if let Some(lore) = &place.lore {
            if !lore.expansion.is_empty() {
                parts.push(lore.expansion.clone());
            }
        }
        if !place.visits.is_empty() {
            parts.push(almanac::plural(place.visits.len(), "evening", "evenings"));
        }
        if !place.delves.is_empty() {
            parts.push(almanac::plural(place.delves.len(), "dungeon", "dungeons"));
        }
        if !parts.is_empty() {
            text.append(&almanac::meta(&parts.join(" · ")));
        }
        line.append(&text);

        // The hours are the account's own work, so they are the one gold thing
        // on the row. A zone nobody has been to carries no figure at all.
        if place.spent > 0 {
            let figure = almanac::figure(&span(place.spent));
            figure.set_valign(gtk::Align::Center);
            figure.set_halign(gtk::Align::End);
            line.append(&figure);
        }
        card.append(&line);

        let page = self.clone();
        let place = place.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| page.open(&place));
        card.add_controller(click);
        card
    }

    /// Open a place by name.
    ///
    /// For the preview, which has no pointer to click with. The list is what a
    /// person uses.
    pub fn open_named(&self, name: &str) {
        let Some(held) = self.imp().held.borrow().clone() else {
            return;
        };
        if let Some(place) = self.places(&held).into_iter().find(|p| p.name == name) {
            self.open(&place);
        }
    }

    // -- one place ------------------------------------------------------------

    /// One place, opened: the hero, the main column and the rail.
    fn open(&self, place: &Place) {
        let Some(nav) = self.imp().nav.borrow().clone() else {
            return;
        };

        // The hero is full-bleed and the body is inset, so the margins belong
        // to the body rather than to the column both sit in.
        let column = almanac::column(0);
        column.set_valign(gtk::Align::Start);
        column.append(&hero(place));

        let body = almanac::column(15);
        body.set_margin_top(16);
        body.set_margin_start(28);
        body.set_margin_end(28);
        body.set_margin_bottom(24);

        if let Some(lore) = place.lore.as_ref() {
            if !lore.summary.is_empty() {
                body.append(&almanac::prose(&lore.summary));
            }
            if !lore.history.is_empty() {
                body.append(&block("HISTORY", "", &paragraph(&lore.history, 1.68)));
            }
            if !lore.factions.is_empty() || !lore.notable.is_empty() {
                body.append(&who_and_what(lore));
            }
        }

        if !place.visits.is_empty() {
            body.append(&self.evenings(place));
        }
        if !place.delves.is_empty() {
            let cards = almanac::column(10);
            for delve in &place.delves {
                cards.append(&delve_card(delve));
            }
            body.append(&block("DUNGEONS HERE", "", &cards));
        }
        column.append(&body);

        nav.push(&adw::NavigationPage::new(
            &almanac::split(
                &almanac::main_column(&column),
                &almanac::rail_pane(&self.rail(place)),
                RAIL,
            ),
            &place.name,
        ));
    }

    /// The evenings spent here, as a spine.
    ///
    /// A `GtkGrid` rather than an overlay, for the same reason the Run page's
    /// road is one: the spine has to run the height of the whole list and the
    /// dots need a gutter the text does not reflow into.
    fn evenings(&self, place: &Place) -> gtk::Box {
        let shown = place.visits.len().min(VISITS_SHOWN);
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(14)
            .build();
        grid.attach(&almanac::spine(), 0, 0, 1, shown as i32);

        for (index, visit) in place.visits.iter().take(shown).enumerate() {
            // No negative margin. The dot and the spine share this column and
            // both are centred in it, which is what puts the marker on the
            // line; nudging one of them sideways is what takes it off.
            let dot = almanac::spine_dot(index == 0);
            grid.attach(&dot, 0, index as i32, 1, 1);
            grid.attach(&evening(visit), 1, index as i32, 1, 1);
        }

        let column = almanac::column(9);
        column.append(&grid);
        if place.visits.len() > shown {
            column.append(&almanac::caption(&format!(
                "and {} more",
                place.visits.len() - shown
            )));
        }

        block(
            "WHAT YOU DID HERE",
            "From the collector addon — nothing in any API records this",
            &column,
        )
    }

    // -- the rail -------------------------------------------------------------

    fn rail(&self, place: &Place) -> gtk::Box {
        let rail = almanac::rail_column();

        let killers = killers(place);
        if !killers.is_empty() {
            let most = killers.first().map(|(_, count)| *count).unwrap_or(1).max(1);
            let rows = almanac::column(7);
            for (name, count) in killers.iter().take(KILLERS_SHOWN) {
                let line = almanac::row(9);
                let label = almanac::label(name, &["al-caption"]);
                label.set_hexpand(true);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                line.append(&label);
                line.append(&almanac::tally_bar(
                    *count as f64 / most as f64,
                    56,
                    Tone::Negative,
                ));
                let figure = almanac::mono(&count.to_string(), &["al-price"]);
                figure.set_halign(gtk::Align::End);
                line.append(&figure);
                rows.append(&line);
            }
            rail.append(&block("WHAT KEEPS KILLING YOU", "", &rows));
            rail.append(&almanac::hairline());
        }

        if !place.spoils.is_empty() {
            let cards = almanac::column(8);
            for spoil in place.spoils.iter().take(SPOILS_SHOWN) {
                cards.append(&spoil_card(spoil));
            }
            rail.append(&block(
                "WORTH FARMING HERE",
                "Bind-on-Equip only — the rest has no market at any price.",
                &cards,
            ));
            rail.append(&almanac::hairline());
        }

        // Whenever any of the corpus's prose is on screen, so is where it came
        // from. Attribution is a condition of the licence the material is under,
        // not a footnote the page can decide it has no room for.
        if let Some(lore) = place.lore.as_ref() {
            let sources = almanac::column(5);
            for source in &lore.sources {
                sources.append(&self.source_link(source));
            }
            let note = almanac::label(
                "Summarised in Armory's own words under CC BY-SA 4.0. \
                 Shipped with the application; nothing is fetched.",
                &["al-footnote"],
            );
            note.set_wrap(true);
            sources.append(&note);
            rail.append(&block("SOURCES", "", &sources));
        }

        rail
    }

    /// One source, as a link out.
    ///
    /// A button rather than a `GtkLinkButton` so the URI goes through the same
    /// launcher every other link in the application does, and so the label can
    /// carry the accent rather than the theme's link colour.
    fn source_link(&self, source: &place::Source) -> gtk::Button {
        let label = almanac::label(&source.title, &["al-gold"]);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let button = gtk::Button::builder()
            .child(&label)
            .halign(gtk::Align::Start)
            .tooltip_text(&source.url)
            .build();
        button.add_css_class("flat");

        let url = source.url.clone();
        button.connect_clicked(move |button| {
            gtk::UriLauncher::new(&url).launch(
                button.root().and_downcast_ref::<gtk::Window>(),
                gtk::gio::Cancellable::NONE,
                |_| {},
            );
        });
        button
    }

    fn nothing_yet(&self, empty_search: bool) -> adw::StatusPage {
        adw::StatusPage::builder()
            .icon_name(if empty_search {
                "user-home-symbolic"
            } else {
                "system-search-symbolic"
            })
            .title(if empty_search {
                "No zones yet"
            } else {
                "No matching zones"
            })
            .description(if empty_search {
                "Armory ships a history for a hundred and forty-three places. Install the \
                 collector addon and play, and the ones you have been to rise to the top \
                 with your own evenings beside them."
            } else {
                "Nothing here matches that."
            })
            .vexpand(true)
            .build()
    }
}

// -- the pieces of a place ----------------------------------------------------

/// The 140px band a place opens on.
///
/// The art is a placeholder: Armory fetches no zone key art today, and the band
/// is drawn without it rather than the page waiting for a picture that may
/// never come. Everything the band says is legible against the empty fill.
fn hero(place: &Place) -> gtk::Overlay {
    let band = almanac::row(0);
    band.add_css_class("al-band");
    // Square rather than a card's rounded header: this band meets the window on
    // three sides, and a radius there leaves two notches of ground showing
    // through the corners.
    band.add_css_class("al-square");
    band.set_size_request(-1, 140);

    let content = almanac::row(14);
    content.set_valign(gtk::Align::End);
    content.set_vexpand(true);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.set_margin_bottom(18);

    let left = almanac::column(5);
    left.set_hexpand(true);
    left.set_valign(gtk::Align::End);
    // The expansion and the map id, and neither is gold: a UiMapID is Blizzard's
    // filing, not anybody's work.
    let mut said = Vec::new();
    if let Some(lore) = place.lore.as_ref() {
        if !lore.expansion.is_empty() {
            said.push(lore.expansion.clone());
        }
    }
    said.push(format!("UiMapID {}", place.map));
    left.append(&almanac::section(&said.join(" · ")));
    let title = almanac::serif(&place.name, "al-hero-title");
    title.set_wrap(true);
    left.append(&title);
    content.append(&left);

    // The hours are the account's own, so this is the gold half of the band.
    // Each half of it is a number somebody earned or it is not drawn: "0
    // evenings" beside a zone read for its history is a readout of nothing.
    let mut parts = Vec::new();
    if !place.visits.is_empty() {
        parts.push(almanac::plural(place.visits.len(), "evening", "evenings"));
    }
    let deaths: usize = place.visits.iter().map(|visit| visit.deaths.len()).sum();
    if deaths > 0 {
        parts.push(almanac::plural(deaths, "death", "deaths"));
    }

    if place.spent > 0 || !parts.is_empty() {
        let right = almanac::column(4);
        right.set_valign(gtk::Align::End);
        right.set_halign(gtk::Align::End);

        if place.spent > 0 {
            let figure = almanac::mono(&span(place.spent), &["al-figure-hero"]);
            figure.set_halign(gtk::Align::End);
            right.append(&figure);
        }
        if !parts.is_empty() {
            let caption = almanac::caption(&parts.join(" · "));
            caption.set_halign(gtk::Align::End);
            right.append(&caption);
        }
        content.append(&right);
    }

    let scrim = almanac::column(0);
    scrim.add_css_class("al-hero-scrim");
    scrim.set_vexpand(true);
    scrim.append(&content);

    let overlay = gtk::Overlay::builder().child(&band).build();
    overlay.add_overlay(&scrim);
    overlay
}

/// Who is here, and what is worth walking to.
fn who_and_what(lore: &Lore) -> gtk::Box {
    let columns = almanac::row(26);

    if !lore.factions.is_empty() {
        let flow = chips();
        for faction in &lore.factions {
            // Every faction plain. The corpus records who is here and never
            // whether they will shoot at you, and tinting a guess would be the
            // page inventing a fact about the zone.
            flow.append(&almanac::chip(faction, Tone::Plain));
        }
        let column = block("WHO IS HERE", "", &flow);
        // A width rather than a share. A faction is two words and a landmark is
        // a sentence, so an even split leaves half the left column empty and
        // squeezes the right one into a gutter — which is what it did.
        column.set_size_request(190, -1);
        column.set_hexpand(lore.notable.is_empty());
        columns.append(&column);
    }

    if !lore.notable.is_empty() {
        let rows = almanac::column(7);
        for named in &lore.notable {
            let line = almanac::row(10);
            let name = almanac::label(&named.name, &["al-row-title"]);
            name.set_wrap(true);
            name.set_size_request(130, -1);
            line.append(&name);
            let what = almanac::caption(&named.what);
            what.set_hexpand(true);
            line.append(&what);
            rows.append(&line);
        }
        let column = block("NOTABLE", "", &rows);
        column.set_hexpand(true);
        columns.append(&column);
    }

    columns
}

/// One evening, hanging off the spine.
fn evening(visit: &Visit) -> gtk::Box {
    let block = almanac::column(5);
    block.set_hexpand(true);

    // The year is carried, where the run's own road drops it: an evening in a
    // levelling zone is as likely to be three years old as three days, and a
    // bare "4 August" would read as this one.
    let heading = almanac::row(9);
    heading.append(&almanac::label(
        &visit.at.format("%-d %B %Y").to_string(),
        &["al-row-title"],
    ));
    let who = almanac::meta(&visit.character);
    who.set_valign(gtk::Align::Baseline);
    heading.append(&who);
    block.append(&heading);

    if !visit.quests.is_empty() {
        let quests = almanac::caption(&turned_in(&visit.quests));
        quests.set_wrap(true);
        block.append(&quests);
    }

    let deaths = tallied(&visit.deaths);
    let rares = tallied(&visit.rares);
    if !deaths.is_empty() || !rares.is_empty() {
        let flow = chips();
        for (name, count) in deaths.iter().take(CHIPS_SHOWN) {
            flow.append(&almanac::chip(
                &counted("Died to", name, *count),
                Tone::Negative,
            ));
        }
        if deaths.len() > CHIPS_SHOWN {
            flow.append(&almanac::chip(
                &format!("and {} more", deaths.len() - CHIPS_SHOWN),
                Tone::Negative,
            ));
        }
        for (name, count) in rares.iter().take(CHIPS_SHOWN) {
            flow.append(&almanac::chip(&counted("Rare:", name, *count), Tone::Gold));
        }
        if rares.len() > CHIPS_SHOWN {
            flow.append(&almanac::chip(
                &format!("and {} more", rares.len() - CHIPS_SHOWN),
                Tone::Gold,
            ));
        }
        block.append(&flow);
    }

    block
}

/// One dungeon or raid, with whoever's words are available.
///
/// `assumes` and `disputed` are drawn flat, never behind an expander. What a
/// place takes for granted and where its sources contradict each other are the
/// two things no other tool will tell anybody, and a reader who has to go
/// looking for them will not find them.
fn delve_card(delve: &Delve) -> gtk::Box {
    let card = almanac::card(9);

    let heading = almanac::row(9);
    let name = almanac::serif(&delve.name, "al-entry-title");
    heading.append(&name);
    // Said out loud, both ways round. Blizzard's own account and ours are not
    // interchangeable and a reader is entitled to know which they are looking
    // at — particularly here, where ours exists only because the Adventure
    // Guide never wrote one. Only ours is gold: it is the writing this
    // application did.
    let whose = if delve.ours {
        almanac::chip("ARMORY'S WORDS — THE GUIDE IS BLANK", Tone::Gold)
    } else {
        almanac::chip("THE ADVENTURE GUIDE", Tone::Plain)
    };
    whose.add_css_class("al-mono");
    whose.set_valign(gtk::Align::Center);
    heading.append(&whose);
    card.append(&heading);

    if !delve.description.is_empty() {
        card.append(&paragraph(&delve.description, 1.6));
    }

    if !delve.bosses.is_empty() {
        let flow = chips();
        for boss in delve.bosses.iter().take(BOSSES_SHOWN) {
            // Blizzard's own data carries trailing punctuation on a few.
            flow.append(&almanac::chip(
                boss.name.trim_end_matches([',', ' ']),
                Tone::Plain,
            ));
        }
        if delve.bosses.len() > BOSSES_SHOWN {
            flow.append(&almanac::chip(
                &format!("and {} more", delve.bosses.len() - BOSSES_SHOWN),
                Tone::Plain,
            ));
        }
        card.append(&flow);
    }

    // The most useful thing in the corpus: what the place takes for granted and
    // the game never says.
    if let Some(assumes) = &delve.assumes {
        let callout = almanac::column(4);
        callout.add_css_class("al-callout");
        callout.append(&almanac::section("ASSUMES YOU KNOW"));
        callout.append(&paragraph(assumes, 1.55));
        card.append(&callout);
    }

    for bit in &delve.disputed {
        let line = almanac::label(&format!("Disputed: {bit}"), &["al-footnote"]);
        line.set_wrap(true);
        card.append(&line);
    }

    card
}

/// One thing worth carrying out of here.
fn spoil_card(spoil: &crate::model::place::Spoil) -> gtk::Box {
    let card = almanac::card(3);
    card.add_css_class("al-tight");

    let line = almanac::row(8);
    // An item the name backfill has not reached yet is its id, in the mono face
    // and held back — an honest state rather than a blank row. An empty name is
    // the same silence as a missing one and is read the same way.
    let name = match spoil.name.as_deref().filter(|name| !name.is_empty()) {
        Some(name) => almanac::label(name, &[]),
        None => almanac::mono(&format!("Item {}", spoil.item), &["al-unknown"]),
    };
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    line.append(&name);

    let price = almanac::mono(
        &super::market_page::gold(spoil.cheapest),
        &["al-price", "al-gold"],
    );
    price.set_halign(gtk::Align::End);
    line.append(&price);
    card.append(&line);

    let note = almanac::label(
        &format!(
            "{} · {} listed",
            spoil.from,
            almanac::thousands(u64::from(spoil.quantity))
        ),
        &["al-footnote"],
    );
    note.set_wrap(true);
    card.append(&note);
    card
}

// -- the small parts ----------------------------------------------------------

/// A section: a mono label, an optional line saying what it is, and the thing.
///
/// [`almanac::titled`] without the note; the note is worth having here because
/// half of this page's sections are claims about where their contents came
/// from.
fn block(title: &str, note: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let column = almanac::column(9);
    column.append(&almanac::section(title));
    if !note.is_empty() {
        column.append(&almanac::caption(note));
    }
    column.append(child);
    column
}

/// A wrapping row of chips.
///
/// A `GtkFlowBox` because GTK has no wrapping box, and a row of factions or
/// bosses that runs off the side of the pane is a row nobody can read.
fn chips() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(6)
        .row_spacing(6)
        .homogeneous(false)
        .min_children_per_line(1)
        .max_children_per_line(24)
        .halign(gtk::Align::Start)
        .build()
}

/// Explanatory prose: the history, a dungeon's description, what it assumes.
///
/// The platform font rather than the serif, deliberately. The serif is for what
/// is written *about the player*; this is reference material, and setting it in
/// the narrative face would say the two are the same kind of thing. The line
/// height is a Pango attribute because GTK's CSS has no `line-height`.
fn paragraph(text: &str, height: f64) -> gtk::Label {
    let label = almanac::label(text, &["dimmed", "al-passage"]);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    let attributes = gtk::pango::AttrList::new();
    attributes.insert(gtk::pango::AttrFloat::new_line_height(height));
    label.set_attributes(Some(&attributes));
    label
}

/// A span as a figure rather than as a sentence.
///
/// [`crate::model::tally::spent`] writes "3 hr 41 min", which is right in a
/// line of prose and
/// wrong in a hero figure beside a zone's name. Hours where there are hours,
/// minutes where there are not.
fn span(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h", minutes / 60)
    }
}

/// The quests an evening closed, named and then counted.
fn turned_in(quests: &[String]) -> String {
    let named: Vec<&str> = quests
        .iter()
        .take(QUESTS_NAMED)
        .map(String::as_str)
        .collect();
    let rest = quests.len() - named.len();
    let list = named.join(", ");
    if rest == 0 {
        format!("Turned in {list}")
    } else {
        format!("Turned in {list} and {rest} more")
    }
}

/// The same name said twice in one evening, counted rather than repeated.
///
/// Dying to the same warlock three times is one fact about the evening; three
/// identical chips in a row read as a bug in the page rather than as three
/// deaths. First seen first, so the order is still the evening's.
fn tallied(names: &[String]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for name in names {
        match out.iter_mut().find(|(seen, _)| seen == name) {
            Some((_, count)) => *count += 1,
            None => out.push((name.clone(), 1)),
        }
    }
    out
}

/// "Died to a Gorian Warlock", or "Died to a Gorian Warlock ×3".
fn counted(lead: &str, name: &str, count: usize) -> String {
    if count > 1 {
        format!("{lead} {name} ×{count}")
    } else {
        format!("{lead} {name}")
    }
}

/// What keeps killing you here, most often first.
///
/// [`Place::killers`] is the tally when there is one. Nothing fills it today —
/// the addon counts deaths per session and not per map — so where it is empty
/// the evenings are counted instead. Both answer the same question from the
/// same records; the fallback is not a guess, it is the long way round.
fn killers(place: &Place) -> Vec<(String, u64)> {
    if !place.killers.is_empty() {
        return place.killers.clone();
    }
    let mut counted: HashMap<&str, u64> = HashMap::new();
    for visit in &place.visits {
        for death in &visit.deaths {
            *counted.entry(death.as_str()).or_default() += 1;
        }
    }
    let mut out: Vec<(String, u64)> = counted
        .into_iter()
        .map(|(name, count)| (name.to_string(), count))
        .collect();
    // By name where the counts tie, so two redraws of the same evening do not
    // shuffle the list.
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(deaths: &[&str]) -> Visit {
        Visit {
            character: "Somechar".into(),
            at: chrono::Utc::now(),
            quests: Vec::new(),
            deaths: deaths.iter().map(|d| (*d).to_string()).collect(),
            rares: Vec::new(),
        }
    }

    #[test]
    fn what_keeps_killing_you_is_counted_from_the_evenings() {
        let place = Place {
            visits: vec![
                visit(&["Gorian Warlock", "Warmaul Shaman"]),
                visit(&["Gorian Warlock"]),
            ],
            ..Place::default()
        };
        assert_eq!(
            killers(&place),
            vec![
                ("Gorian Warlock".to_string(), 2),
                ("Warmaul Shaman".to_string(), 1),
            ]
        );
    }

    #[test]
    fn a_tally_the_model_supplies_is_used_as_it_stands() {
        let place = Place {
            killers: vec![("Something else".into(), 9)],
            visits: vec![visit(&["Gorian Warlock"])],
            ..Place::default()
        };
        assert_eq!(killers(&place), vec![("Something else".to_string(), 9)]);
    }

    #[test]
    fn a_span_is_a_figure_and_not_a_sentence() {
        assert_eq!(span(0), "0m");
        assert_eq!(span(41 * 60), "41m");
        assert_eq!(span(13 * 3600 + 41 * 60), "13h");
    }

    #[test]
    fn an_evening_names_three_quests_and_counts_the_rest() {
        let quests: Vec<String> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|q| q.to_string())
            .collect();
        assert_eq!(turned_in(&quests), "Turned in A, B, C and 2 more");
        assert_eq!(turned_in(&quests[..2]), "Turned in A, B");
    }
}
