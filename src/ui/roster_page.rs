//! The roster: who is on the account, and which of them a run is about.
//!
//! Realms are the grouping rather than class or level, because a realm is a
//! separate auction house for everything except commodities, and because with
//! thirty-one characters across nine realms it is the division a person
//! actually thinks in.
//!
//! Every card carries its class crest, which costs nothing: the render service
//! addresses class icons by a texture name that is the class name with its
//! spaces taken out, so `Death Knight` is a URL and not a request. A signed-in
//! account can do better still — Blizzard renders each character's own portrait
//! — and when one of those has been fetched it replaces the crest. The crest is
//! the fallback rather than the placeholder: it is a smaller picture of a true
//! thing, not a blank waiting to be filled.
//!
//! ## What the page is
//!
//! Two panes. The main column is the account as people — a grid of cards, two
//! abreast, under a mono realm label. The rail is the account as figures: how
//! many characters there are, how many of them the run is about, and what the
//! addon can see that no endpoint will ever answer.
//!
//! ## Gold here is membership, not work
//!
//! Everywhere else in the almanac gold means "you earned this". On this page it
//! means "this one counts": an enrolled character is who the run is measured
//! against, so their card is tinted and their gold and item level are gold,
//! while everybody else's are the same true numbers said quietly. It is the one
//! deliberate exception to the accent's rule and it is worth knowing before
//! reading that rule anywhere else.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::almanac::{self, Tone};
use super::character_page::CharacterPage;
use super::images::{Art, Images};
use crate::model::character::{Character, CharacterKey, Detail, Roster};
use crate::model::cohort::Cohort;
use crate::model::source::blizzard::{media, Region};

use super::almanac::thousands;

/// What the page calls when a character is enrolled or withdrawn.
type ToggleHandler = Box<dyn Fn(CharacterKey)>;
/// Called with whether a character is now open on top of the roster.
type NavigateHandler = Box<dyn Fn(bool)>;

/// How big a character's portrait is on a card.
const ART: i32 = 48;

/// How wide the rail is. The account's standing and the addon's counts, at the
/// width somebody read them at.
const RAIL: f64 = 300.0;

/// How many cards sit side by side. Two, because a card carries a portrait, a
/// sentence and a switch and a third column makes all three too narrow to read.
const ACROSS: i32 = 2;

/// What the addon has reported, and whether it is there at all.
#[derive(Debug, Clone, Default)]
pub struct Warband {
    pub installed: bool,
    pub bank_items: usize,
    pub currencies: usize,
    /// How the currencies the addon has watched actually arrived.
    ///
    /// The Warband can move some currencies between characters, so an amount
    /// on a character is not evidence that character earned it. These are the
    /// counts per answer — see [`crate::model::provenance::Origin`], which is
    /// where the reasoning lives and where the honest "cannot tell" comes from.
    pub earned_currencies: usize,
    pub transferred_currencies: usize,
    pub unclear_currencies: usize,
    pub written_at: Option<chrono::DateTime<chrono::Utc>>,
}

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct RosterPage {
        pub images: RefCell<Option<Images>>,
        pub column: RefCell<Option<gtk::Box>>,
        pub rail: RefCell<Option<gtk::Box>>,
        pub search: RefCell<Option<gtk::SearchBar>>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub on_toggle: RefCell<Option<super::ToggleHandler>>,
        /// Asked to fill the character page, once it has been pushed.
        pub on_open: RefCell<Option<super::ToggleHandler>>,
        /// Told when a character is pushed, or gone back from.
        pub on_navigate: RefCell<Option<super::NavigateHandler>>,
        /// The roster, and the one character pushed on top of it.
        pub navigation: RefCell<Option<adw::NavigationView>>,
        pub character: RefCell<Option<super::CharacterPage>>,
        /// Who was opened. Held here rather than read back off the character
        /// page, so the header does not depend on whoever fills that page
        /// having got there first.
        pub opened: RefCell<Option<Character>>,

        /// Everything the last redraw was given, so a search can redraw from it
        /// without the application being asked again.
        pub held: RefCell<Option<super::Held>>,
        pub needle: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RosterPage {
        const NAME: &'static str = "ArmoryRosterPage";
        type Type = super::RosterPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for RosterPage {}
    impl WidgetImpl for RosterPage {}
    impl BinImpl for RosterPage {}
}

glib::wrapper! {
    pub struct RosterPage(ObjectSubclass<imp::RosterPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// One redraw's worth of input, kept so filtering does not need another.
///
/// Public only because it is named by a field on the private implementation
/// struct; nothing outside this file constructs one.
#[derive(Clone)]
pub struct Held {
    roster: Roster,
    cohort: Cohort,
    details: HashMap<CharacterKey, Detail>,
    portraits: HashMap<CharacterKey, String>,
    warband: Warband,
    region: Region,
}

impl RosterPage {
    pub fn new(images: &Images) -> Self {
        let page: Self = glib::Object::builder().build();
        *page.imp().images.borrow_mut() = Some(images.clone());
        page.build();
        page
    }

    fn build(&self) {
        let column = almanac::column(14);
        column.add_css_class("al-main-column");
        // Packed to the top. Without it the scroller hands the column the whole
        // viewport and the realm groups share the slack out between them, which
        // draws two realms a hundred pixels apart.
        column.set_valign(gtk::Align::Start);

        let rail = almanac::rail_column();

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search characters, realms and classes")
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
        *imp.column.borrow_mut() = Some(column);
        *imp.rail.borrow_mut() = Some(rail);
        *imp.search.borrow_mut() = Some(search);
        *imp.entry.borrow_mut() = Some(entry);

        self.set_child(Some(&self.roster_view(&view)));
    }

    /// The roster, and the one character pushed on top of it.
    ///
    /// An `AdwNavigationView` inside the page rather than a place of its own in
    /// the window's sidebar: a character is reached *from* the roster and there
    /// is no list of characters in the sidebar to return to. It is also what
    /// puts the back button and the character's name in the header bar without
    /// the window having to know a character page exists.
    fn roster_view(&self, roster: &adw::ToolbarView) -> adw::NavigationView {
        let navigation = adw::NavigationView::new();
        navigation.add(
            &adw::NavigationPage::builder()
                .title("Roster")
                .tag("roster")
                .child(roster)
                .build(),
        );

        // **No header bar on the pushed page.** The window already has one —
        // it belongs to the content pane of the split view and carries the sync
        // button, the menu and the place's name — and libadwaita will happily
        // draw a second one underneath it, which is exactly what this did. The
        // window is told instead, and puts the character's name and a back
        // button in the one header there is.
        let images = self.imp().images.borrow().clone().unwrap_or_default();
        let character = CharacterPage::new(&images);
        navigation.add(
            &adw::NavigationPage::builder()
                .title("Character")
                .tag("character")
                .child(&character)
                .build(),
        );

        // Swiping back, pressing Escape and the window's own back button all
        // arrive here, so the header follows the page rather than the gesture.
        let page = self.clone();
        navigation.connect_visible_page_notify(move |_| {
            if let Some(handler) = page.imp().on_navigate.borrow().as_ref() {
                handler(page.showing_character());
            }
        });

        let imp = self.imp();
        *imp.navigation.borrow_mut() = Some(navigation.clone());
        *imp.character.borrow_mut() = Some(character);
        navigation
    }

    /// Told whenever the roster pushes a character or comes back from one.
    pub fn connect_navigated<F: Fn(bool) + 'static>(&self, handler: F) {
        *self.imp().on_navigate.borrow_mut() = Some(Box::new(handler));
    }

    /// Go back to the roster. What the window's back button calls.
    pub fn show_roster(&self) {
        if let Some(navigation) = self.imp().navigation.borrow().as_ref() {
            navigation.pop();
        }
    }

    /// Who is open, for the window's title.
    pub fn open_character_name(&self) -> Option<(String, String)> {
        self.imp()
            .opened
            .borrow()
            .as_ref()
            .map(|character| (character.display_name.clone(), character.realm_name.clone()))
    }

    /// The character page, so the application can fill it.
    pub fn character_page(&self) -> Option<CharacterPage> {
        self.imp().character.borrow().clone()
    }

    /// Told that somebody has opened a character.
    pub fn connect_open_character<F: Fn(CharacterKey) + 'static>(&self, handler: F) {
        *self.imp().on_open.borrow_mut() = Some(Box::new(handler));
    }

    /// Push one character's page.
    ///
    /// The handler fills it and the push shows it, in that order: pushing an
    /// empty page and filling it a moment later is a visible flash of the empty
    /// state on every open.
    pub fn open_character(&self, character: &Character) {
        *self.imp().opened.borrow_mut() = Some(character.clone());
        if let Some(handler) = self.imp().on_open.borrow().as_ref() {
            handler(character.key.clone());
        }
        if let Some(navigation) = self.imp().navigation.borrow().as_ref() {
            navigation.push_by_tag("character");
        }
    }

    /// Whether a character is open, so the window knows the page is not the
    /// roster right now.
    pub fn showing_character(&self) -> bool {
        self.imp()
            .navigation
            .borrow()
            .as_ref()
            .and_then(|navigation| navigation.visible_page())
            .and_then(|page| page.tag())
            .is_some_and(|tag| tag == "character")
    }

    pub fn connect_toggled<F: Fn(CharacterKey) + 'static>(&self, handler: F) {
        *self.imp().on_toggle.borrow_mut() = Some(Box::new(handler));
    }

    /// The search bar, so the window's header toggle can drive it.
    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    /// Redraw from the roster, the current enrolment, and whatever detail and
    /// artwork have landed so far.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &self,
        roster: &Roster,
        cohort: &Cohort,
        details: &HashMap<CharacterKey, Detail>,
        portraits: &HashMap<CharacterKey, String>,
        warband: &Warband,
        region: Region,
    ) {
        *self.imp().held.borrow_mut() = Some(Held {
            roster: roster.clone(),
            cohort: cohort.clone(),
            details: details.clone(),
            portraits: portraits.clone(),
            warband: warband.clone(),
            region,
        });
        self.redraw();
    }

    fn redraw(&self) {
        let imp = self.imp();
        let Some(held) = imp.held.borrow().clone() else {
            return;
        };
        let (Some(column), Some(rail)) = (imp.column.borrow().clone(), imp.rail.borrow().clone())
        else {
            return;
        };

        for pane in [&column, &rail] {
            while let Some(child) = pane.first_child() {
                pane.remove(&child);
            }
        }

        self.draw_column(&column, &held);
        self.draw_rail(&rail, &held);
    }

    // -- the main column ------------------------------------------------------

    fn draw_column(&self, column: &gtk::Box, held: &Held) {
        if held.roster.is_empty() {
            column.append(
                &adw::StatusPage::builder()
                    .icon_name("system-users-symbolic")
                    .title("No characters yet")
                    .description(
                        "Sync to fetch your roster. Characters appear once they have \
                         logged out at least once — Blizzard writes profile data on \
                         logout, never while you are playing.",
                    )
                    .vexpand(true)
                    .build(),
            );
            return;
        }

        let headline = almanac::column(4);
        headline.append(&almanac::serif(
            &Self::headline(held.cohort.len(), held.roster.len()),
            "al-headline",
        ));
        headline.append(&almanac::caption(
            "The rest are kept only to explain why something is already spent.",
        ));
        column.append(&headline);

        let needle = self.imp().needle.borrow().clone();
        let mut shown = 0;

        for (slug, name) in held.roster.realms() {
            let matching: Vec<&Character> = held
                .roster
                .characters
                .iter()
                .filter(|character| character.key.realm_slug == slug)
                .filter(|character| matches(character, held, &needle))
                .collect();

            if matching.is_empty() {
                continue;
            }
            shown += matching.len();

            column.append(&almanac::section(&name));
            // A `GtkGrid` rather than a `GtkFlowBox`: the flow box balances a
            // line's children rather than filling it, so three cards came out
            // as three rows of one and a realm with a single character was
            // drawn at a different width from a realm with two. Where each
            // card goes is not a layout question, it is the design.
            let grid = gtk::Grid::builder()
                .column_spacing(10)
                .row_spacing(10)
                .column_homogeneous(true)
                .build();
            let count = matching.len() as i32;
            for (index, character) in matching.into_iter().enumerate() {
                let index = index as i32;
                grid.attach(
                    &self.card(character, held),
                    index % ACROSS,
                    index / ACROSS,
                    1,
                    1,
                );
            }
            // A realm with an odd number of characters leaves the grid one
            // column wide, and a lone card at twice the width of every other
            // card reads as a different kind of thing. An empty cell is what
            // holds the second column open.
            if count % ACROSS != 0 {
                let filler = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                filler.set_hexpand(true);
                grid.attach(&filler, ACROSS - 1, (count - 1) / ACROSS, 1, 1);
            }
            column.append(&grid);
        }

        if shown == 0 {
            column.append(
                &adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title("No matches")
                    .description("No character, realm or class here matches that.")
                    .vexpand(true)
                    .build(),
            );
        }
    }

    /// The sentence at the top of the page.
    ///
    /// Spelled rather than reported, because it is the one line here that is
    /// written about the account rather than measured off it. Past twenty
    /// [`almanac::spelled`] hands back a figure, which is where a word stops
    /// being easier to read than a number.
    fn headline(enrolled: usize, total: usize) -> String {
        let of = almanac::spelled(total).to_lowercase();
        match enrolled {
            0 => "Nobody is enrolled, so there is nothing for a run to be about".to_string(),
            1 => format!("One of {of} is what this run is about"),
            many => format!(
                "{} of {of} are what this run is about",
                almanac::spelled(many)
            ),
        }
    }

    /// One character.
    fn card(&self, character: &Character, held: &Held) -> gtk::Box {
        let detail = held.details.get(&character.key);
        let enrolled = held.cohort.contains(&character.key);

        // The gold card is membership rather than work: this is who the run is
        // measured against. Nothing else on the page takes the accent.
        let card = if enrolled {
            almanac::earned_card(0)
        } else {
            almanac::card(0)
        };

        let line = almanac::row(13);

        // Everything but the switch lives inside a button. A `GtkGestureClick`
        // on the card would work with a mouse and with nothing else — no focus,
        // no Enter, nothing for a screen reader to announce as activatable —
        // and it would have to be careful not to fire when somebody pressed the
        // switch. A button is all three of those for free, and the switch as
        // its sibling cannot be inside its hit area by construction.
        let content = almanac::row(13);
        content.append(&self.portrait(character, held));

        let text = almanac::column(2);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);

        let name = almanac::label(&character.display_name, &["al-row-title"]);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&name);

        let subtitle = almanac::label(&Self::subtitle(character, detail), &["al-caption"]);
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&subtitle);

        if let Some(figures) = Self::figures(detail, enrolled) {
            text.append(&figures);
        }
        content.append(&text);

        let open = gtk::Button::builder()
            .child(&content)
            .hexpand(true)
            .tooltip_text(format!("Open {}", character.display_name))
            .build();
        open.add_css_class("flat");
        open.add_css_class("al-card-button");
        let page = self.clone();
        let who = character.clone();
        open.connect_clicked(move |_| page.open_character(&who));
        line.append(&open);

        let switch = gtk::Switch::builder()
            .active(enrolled)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .hexpand(false)
            .build();
        switch.add_css_class("al-switch");
        // Pinned, not merely given a minimum: a switch in a box beside a name
        // that wants the room is otherwise stretched into an oval.
        switch.set_size_request(36, 21);
        switch.set_tooltip_text(Some(if enrolled {
            "Take this character out of the run"
        } else {
            "Measure this character's progress as part of the run"
        }));

        let key = character.key.clone();
        let page = self.clone();
        // `notify::active` rather than a clicked handler, so setting the switch
        // programmatically during a redraw does not read as the person having
        // pressed it.
        switch.connect_active_notify(move |_| {
            if let Some(handler) = page.imp().on_toggle.borrow().as_ref() {
                handler(key.clone());
            }
        });
        line.append(&switch);

        // Specialisations stay in the tooltip. They are real information with
        // no endpoint behind them, and two professions carrying a tree name
        // each is enough to push the card past the height of the one beside it.
        if let Some(more) = detail.and_then(Self::depths) {
            card.set_tooltip_text(Some(&more));
        }

        card.append(&line);
        card
    }

    /// The character's own render if we have one, and their class crest if not.
    fn portrait(&self, character: &Character, held: &Held) -> Art {
        let art = Art::new(ART, "avatar-default-symbolic");
        art.add_css_class("portrait");
        art.add_css_class(almanac::class_style(&character.class));
        art.set_valign(gtk::Align::Center);
        art.set_tooltip_text(Some(&format!(
            "{} {} — {}",
            character.race,
            character.class,
            character.faction.label()
        )));

        let url = held
            .portraits
            .get(&character.key)
            .cloned()
            .unwrap_or_else(|| media::class_icon(held.region, &character.class));

        if let Some(images) = self.imp().images.borrow().as_ref() {
            art.show(images, Some(&url), ART);
        }
        art
    }

    /// The line under a character's name.
    ///
    /// Level, race and class always; the spec and the primary professions once
    /// they have arrived. A character whose detail has not landed yet reads the
    /// same as one with none rather than showing a row of dashes that look like
    /// missing data.
    fn subtitle(character: &Character, detail: Option<&Detail>) -> String {
        let mut who = format!(
            "Level {} {} {}",
            character.level, character.race, character.class
        );

        let Some(detail) = detail else { return who };

        if let Some(spec) = &detail.spec {
            // The spec replaces the bare class: "Restoration Shaman" says more
            // than "Shaman" and takes the same room.
            who = format!(
                "Level {} {} {spec} {}",
                character.level, character.race, character.class
            );
        }

        let primaries: Vec<&str> = detail
            .professions
            .iter()
            .filter(|profession| profession.is_primary)
            .map(|profession| profession.name.as_str())
            .collect();
        if primaries.is_empty() {
            who
        } else {
            format!("{who} · {}", primaries.join(", "))
        }
    }

    /// The mono line: what this character is worth and what they are wearing.
    ///
    /// Gold when the run is about them, and the same figures said quietly when
    /// it is not. The Mythic+ rating is not here — three numbers do not fit in
    /// half a column — so it goes in the tooltip with the specialisations.
    fn figures(detail: Option<&Detail>, enrolled: bool) -> Option<gtk::Label> {
        let detail = detail?;
        let mut parts = Vec::new();
        if let Some(money) = detail.money {
            parts.push(format!("{}G", thousands(money / 10_000)));
        }
        if let Some(item_level) = detail.item_level {
            parts.push(format!("ILVL {item_level}"));
        }
        if parts.is_empty() {
            return None;
        }

        let tone = if enrolled { "al-gold" } else { "al-unknown" };
        // `al-figures` is eleven-point, which is what fits beside a portrait
        // and a switch in half a column. `al-price` under it is the fallback
        // size until that rule exists.
        let line = almanac::mono(&parts.join(" · "), &["al-price", "al-figures", tone]);
        line.set_ellipsize(gtk::pango::EllipsizeMode::End);
        Some(line)
    }

    /// What the card has no room for: the specialisation trees, and the rating.
    ///
    /// Two characters with Alchemy at 100 can have spent a year of weekly
    /// knowledge in completely different places, and the profile API cannot say
    /// which — it has the expansion tier and stops there.
    fn depths(detail: &Detail) -> Option<String> {
        let mut lines: Vec<String> = detail
            .professions
            .iter()
            .filter_map(|profession| {
                let open: Vec<&str> = profession
                    .specialisations
                    .iter()
                    .filter(|(_, unlocked)| *unlocked)
                    .map(|(name, _)| name.as_str())
                    .collect();
                if open.is_empty() {
                    return None;
                }
                let knowledge = match profession.knowledge {
                    0 => String::new(),
                    learned => format!(" — {learned} knowledge"),
                };
                Some(format!(
                    "{}: {}{knowledge}",
                    profession.name,
                    open.join(", ")
                ))
            })
            .collect();

        if let Some(rating) = detail.mythic_rating {
            lines.push(format!("Mythic+ rating {rating}"));
        }

        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    // -- the rail -------------------------------------------------------------

    fn draw_rail(&self, rail: &gtk::Box, held: &Held) {
        rail.append(&almanac::titled("THE ACCOUNT", &Self::account(held)));
        rail.append(&almanac::hairline());
        rail.append(&almanac::titled("WHAT THE ADDON SEES", &Self::addon(held)));
    }

    /// The account at a glance: characters, how many the run is about, how many
    /// have reached the cap, and what they are all sitting on.
    fn account(held: &Held) -> gtk::Box {
        let gold: u64 = held
            .details
            .values()
            .filter_map(|detail| detail.money)
            .sum();
        let at_cap = held
            .roster
            .characters
            .iter()
            .filter(|character| character.level >= 80)
            .count();

        let column = almanac::column(9);
        column.append(&almanac::stat_line(
            "Characters",
            &thousands(held.roster.len() as u64),
            Tone::Plain,
        ));

        let enrolled = almanac::stat_line(
            "Enrolled",
            &held.cohort.len().to_string(),
            // The one gold figure in the rail, for the same reason the enrolled
            // cards are tinted — and plain at zero, because gold on a nought is
            // the accent claiming something that is not there.
            if held.cohort.is_empty() {
                Tone::Plain
            } else {
                Tone::Gold
            },
        );
        enrolled.set_tooltip_text(Some("Only enrolled characters count towards a run."));
        column.append(&enrolled);

        let cap = almanac::stat_line("At level cap", &at_cap.to_string(), Tone::Plain);
        cap.set_tooltip_text(Some("Level 80 or above"));
        column.append(&cap);

        // Only for what has actually been fetched. Gold comes from a separate
        // call per character and only for the enrolled ones, so a total across
        // an unsynced roster would be a confident understatement — which is
        // what the tooltip is there to say.
        if gold > 0 {
            let line = almanac::stat_line(
                "Gold",
                &format!("{}g", thousands(gold / 10_000)),
                Tone::Plain,
            );
            line.set_tooltip_text(Some(
                "Across the characters whose detail has been fetched, which is the \
                 enrolled ones. Not a total for the account.",
            ));
            column.append(&line);
        }

        column
    }

    /// What only the addon can see.
    ///
    /// The Warband bank and currencies have no endpoint at all — Blizzard has
    /// said a character inventory API is not planned — so this block is either
    /// real data or an explanation of why there is none. An empty card with no
    /// explanation would read as a bug in Armory rather than as a missing addon.
    fn addon(held: &Held) -> gtk::Box {
        let warband = &held.warband;
        let block = almanac::column(10);

        if !warband.installed {
            let card = almanac::card(4);
            card.append(&almanac::label(
                "The collector addon is not installed",
                &["al-row-title"],
            ));
            card.append(&almanac::caption(
                "Blizzard exposes no endpoint for the Warband bank, currencies or which \
                 character earned each achievement. Run ./install-addon.sh, then log in \
                 and out once — the game writes its file on logout.",
            ));
            block.append(&card);
            block.append(&Self::snapshot_note(warband));
            return block;
        }

        let card = almanac::card(8);
        card.append(&count_line(
            "Warband bank",
            &format!("{} items", thousands(warband.bank_items as u64)),
            &[],
        ));
        card.append(&count_line(
            "Currencies",
            &warband.currencies.to_string(),
            &[],
        ));
        // Where those amounts came from, which the count above cannot say. A
        // currency the Warband moved is not work the character did, and a run
        // that counted it would be crediting somebody else's afternoon.
        card.append(&count_line(
            "Earned here",
            &warband.earned_currencies.to_string(),
            &["al-gold"],
        ));
        if warband.transferred_currencies > 0 {
            card.append(&count_line(
                "Moved across the Warband",
                &warband.transferred_currencies.to_string(),
                &[],
            ));
        }
        // Shown even at zero. The game maintains an earned total only for
        // currencies with a moving maximum, so for the rest a rise genuinely
        // cannot be attributed — and a row that disappears when the answer is
        // "none" teaches somebody that the question is never asked.
        card.append(&count_line(
            "Cannot tell",
            &warband.unclear_currencies.to_string(),
            &["al-unknown"],
        ));
        block.append(&card);
        block.append(&Self::snapshot_note(warband));
        block
    }

    /// When this was true, which is never now.
    ///
    /// Both halves of the page are snapshots: Blizzard writes a character's
    /// profile when they log out and the addon writes its file at the same
    /// moment. Saying so is not a caveat, it is what the figures mean.
    fn snapshot_note(warband: &Warband) -> gtk::Label {
        let when = match (warband.installed, warband.written_at) {
            (false, _) => {
                "Blizzard writes a character's profile only when they log out.".to_string()
            }
            (true, Some(at)) => format!("Written at logout, {}.", at.format("%-d %B %Y at %H:%M")),
            (true, None) => "Not scanned yet — visit a banker in game.".to_string(),
        };
        let note = almanac::label(&format!("{when} Nothing here is live."), &["al-footnote"]);
        note.set_wrap(true);
        note
    }
}

/// A label and a small count on one line.
///
/// Not [`almanac::stat_line`], whose figure is the eighteen-point one the
/// account's standing is drawn at. These are a card's worth of counts and sit
/// at the size the rest of the page's numbers do.
fn count_line(name: &str, value: &str, classes: &[&str]) -> gtk::Box {
    let line = almanac::row(8);
    let name = almanac::label(name, &["al-caption"]);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    line.append(&name);

    let mut all = vec!["al-price"];
    all.extend_from_slice(classes);
    let value = almanac::mono(value, &all);
    value.set_halign(gtk::Align::End);
    line.append(&value);
    line
}

/// Whether a character answers the search.
fn matches(character: &Character, held: &Held, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let spec = held
        .details
        .get(&character.key)
        .and_then(|detail| detail.spec.clone())
        .unwrap_or_default();

    format!(
        "{} {} {} {} {} {}",
        character.display_name,
        character.realm_name,
        character.class,
        character.race,
        character.faction.label(),
        spec
    )
    .to_lowercase()
    .contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::character::Faction;

    #[test]
    fn every_class_the_game_ships_has_a_ring_of_its_own() {
        // A class falling through to the unknown ring is a card that looks
        // broken next to twelve that do not.
        for class in [
            "Death Knight",
            "Demon Hunter",
            "Druid",
            "Evoker",
            "Hunter",
            "Mage",
            "Monk",
            "Paladin",
            "Priest",
            "Rogue",
            "Shaman",
            "Warlock",
            "Warrior",
        ] {
            assert_ne!(almanac::class_style(class), "class-unknown", "{class}");
        }
        assert_eq!(almanac::class_style("Tinker"), "class-unknown");
    }

    #[test]
    fn the_class_ring_and_the_class_crest_agree_on_every_class() {
        // Both are derived from the same display string and neither is checked
        // by the compiler. A class that has a ring but no crest is a card with a
        // coloured circle and no picture in it.
        for class in ["Death Knight", "Demon Hunter", "Evoker"] {
            let crest = media::class_icon(Region::Us, class);
            let ring = almanac::class_style(class);
            let slug = ring.trim_start_matches("class-").replace('-', "");
            assert!(crest.contains(&slug), "{crest} does not match {ring}");
        }
    }

    #[test]
    fn the_headline_reads_as_a_sentence_at_every_count() {
        // Spelled up to twenty and a figure past it, and never "Nothing of
        // thirty-one are what this run is about".
        assert_eq!(
            RosterPage::headline(6, 12),
            "Six of twelve are what this run is about"
        );
        assert_eq!(
            RosterPage::headline(1, 12),
            "One of twelve is what this run is about"
        );
        assert_eq!(
            RosterPage::headline(6, 31),
            "Six of 31 are what this run is about"
        );
        assert!(RosterPage::headline(0, 31).starts_with("Nobody is enrolled"));
    }

    fn character(name: &str, realm: &str, class: &str) -> Character {
        Character {
            key: CharacterKey::new(crate::model::source::blizzard::realm_slug(realm), name),
            id: 1,
            realm_id: 1,
            display_name: name.to_string(),
            realm_name: realm.to_string(),
            level: 80,
            class: class.to_string(),
            race: "Orc".into(),
            faction: Faction::Horde,
            wow_account_id: 1,
        }
    }

    fn held(characters: Vec<Character>) -> Held {
        Held {
            roster: Roster::new(characters),
            cohort: Cohort::new(),
            details: HashMap::new(),
            portraits: HashMap::new(),
            warband: Warband::default(),
            region: Region::Us,
        }
    }

    #[test]
    fn a_search_reaches_the_realm_and_the_class_not_only_the_name() {
        // With thirty-one characters the question is as often "who is on
        // Mannoroth" or "where is my druid" as it is a name.
        let one = character("Aeltor", "Mannoroth", "Paladin");
        let held = held(vec![one.clone()]);

        assert!(matches(&one, &held, ""));
        assert!(matches(&one, &held, "aelt"));
        assert!(matches(&one, &held, "mannoroth"));
        assert!(matches(&one, &held, "paladin"));
        assert!(matches(&one, &held, "horde"));
        assert!(!matches(&one, &held, "stormrage"));
    }

    #[test]
    fn a_character_with_no_detail_still_says_who_they_are() {
        // An enrolled character whose detail has not landed reads the same as
        // one with none, rather than showing dashes that look like a failure.
        let one = character("Somechar", "Emerald Dream", "Shaman");
        assert_eq!(RosterPage::subtitle(&one, None), "Level 80 Orc Shaman");
    }
}
