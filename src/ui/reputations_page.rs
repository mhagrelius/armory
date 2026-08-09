//! Where each character stands with everybody, and who actually earned it.
//!
//! Every tool in this space has a reputations page and every one of them shows
//! the same thing: a character, a faction, a bar. That was honest until The War
//! Within, which syncs most reputations account-wide to the furthest-progressed
//! character — so a fresh alt now shows Renown 20 with a faction it has never
//! met, and a page that draws a bar for it is reporting somebody else's work as
//! this character's.
//!
//! ## The page is one bar with two readings
//!
//! Pale is where the *account* already stands; gold is what the character in
//! front of you was watched earning, login to logout. Only the gold moves, and
//! only the gold is ever claimed by the run. Three cards fall out of that:
//!
//! * a **standing**, where there is an observation to draw — gold over pale;
//! * an **inherited** standing, which was at its ceiling before the run began.
//!   Pale at the whole of it and no gold at all, because no amount of play will
//!   move it. The card is gold-*tinted* because it is the row somebody has to
//!   decide about; the tint says "look at this" and the fill says "you earned
//!   this", and those are not the same claim;
//! * a standing nobody watched, which gets **no bar**. The client keeps no
//!   per-character earned total for reputation, so who did the work cannot be
//!   said — and a floor drawn as a bar reads as a measurement.
//!
//! Without the addon there is no observation at all, and the answer is zero
//! rather than the account's standing. Falling back to the account's number
//! would be exactly the inflation the whole application exists to avoid.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::almanac::{self, Bar, Tone};
use crate::model::character::{Character, CharacterKey, Roster};
use crate::model::provenance::{
    fraction_earned, standing_earned, Earned, EarnedReputation, Provenance,
};
use crate::model::source::blizzard::profile::FactionStanding;

/// How wide this page's rail is.
const RAIL: f64 = 288.0;

/// How many standings one group draws before it stops.
///
/// A levelled character carries a couple of hundred factions and a cohort of
/// six carries them each. Every card holds a drawn bar, so the whole roster at
/// once is a page that takes a second to appear — and the ordering below puts
/// the standings somebody has actually worked on above the cap.
const SHOWN: usize = 60;

/// How far down a list the fill keeps staggering, and by how much.
///
/// The stagger is what makes a column of bars read as one gesture rather than
/// as sixty things happening at once. Past a dozen it is nobody's screen any
/// more, and carrying on would delay the last card by five seconds.
const STAGGER: usize = 12;
const STEP_MS: u32 = 80;

/// The classic ladder's ends, as ranks — [`standing_earned`] answers in these.
const NEUTRAL: u8 = 4;
const EXALTED: u8 = 8;

/// How the page is arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum By {
    /// One group per character, listing their factions. The shape a person
    /// thinks in when the question is "what should this alt work on".
    #[default]
    Character,
    /// One group per faction, listing the characters. The shape for "who is
    /// furthest with this lot".
    Faction,
}

/// What can honestly be said about who earned one standing.
///
/// Not [`crate::model::provenance::Origin`], which is the same question asked
/// of a currency and has a fourth answer — a currency can be *transferred*
/// between characters and a standing cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// The addon watched this character earn some of it.
    Earned,
    /// The addon was watching and this character earned none of it. An honest
    /// zero, which is a different fact from an unwatched one.
    Nothing,
    /// Warbands handed it over before the run began. It cannot move, so nothing
    /// about it counts towards the run.
    Inherited,
    /// Nobody was watching. Who did the work cannot be said.
    Unclear,
}

impl Reading {
    /// Whether there is a measurement here to draw.
    ///
    /// False for exactly one case, and it is the point of the enum: an unwatched
    /// standing has a floor and no measurement, and a bar drawn over a floor
    /// reads as the measurement it is not.
    fn measured(self) -> bool {
        !matches!(self, Reading::Unclear)
    }
}

/// Which of the three claims a standing supports, if any.
///
/// Order matters. Work this character was watched doing outranks the inherited
/// flag — an inherited faction somebody has been grinding anyway is the most
/// interesting row on the page and has a real gold fill. The flag outranks
/// silence, because it is a positive fact rather than an absence.
fn reading(standing: &FactionStanding, earned: Option<&Earned>) -> Reading {
    match earned {
        Some(earned) if earned.has_touched(standing.faction) => Reading::Earned,
        _ if standing.inherited => Reading::Inherited,
        Some(_) => Reading::Nothing,
        None => Reading::Unclear,
    }
}

/// How much of where the account stands this character can be shown to have
/// earned.
///
/// The pale bar is the whole of the account's standing — which is why an
/// inherited card draws it at full width — and this is the share of it that was
/// watched happening. Renown answers in levels because that is the shape it
/// has; the classic ladder answers in ranks, because a rank is what a player
/// reads and the point thresholds between them are wildly uneven.
fn share(standing: &FactionStanding, mine: &EarnedReputation) -> f64 {
    if standing.renown > 0 {
        return f64::from(mine.renown.min(standing.renown)) / f64::from(standing.renown);
    }
    let (rank, _) = standing_earned(mine);
    let climbed = f64::from(rank.saturating_sub(NEUTRAL));
    let partial = fraction_earned(mine).unwrap_or(0.0);
    ((climbed + partial) / f64::from(EXALTED - NEUTRAL)).clamp(0.0, 1.0)
}

/// How many of a character's standings support each of the three claims.
fn tally(standings: &[FactionStanding], earned: Option<&Earned>) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    for standing in standings {
        match reading(standing, earned) {
            Reading::Earned => counts.0 += 1,
            Reading::Inherited => counts.1 += 1,
            Reading::Unclear => counts.2 += 1,
            Reading::Nothing => {}
        }
    }
    counts
}

mod imp {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    pub struct ReputationsPage {
        pub groups: RefCell<Option<gtk::Box>>,
        pub headline: RefCell<Option<gtk::Label>>,
        /// The rail's three cohort counts. The rest of the rail is controls and
        /// prose and is built once — rebuilding it would destroy the very
        /// switch whose handler asked for the redraw.
        pub totals: RefCell<Option<gtk::Box>>,
        pub search: RefCell<Option<gtk::SearchBar>>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub held: RefCell<Option<super::Held>>,
        pub needle: RefCell<String>,
        pub by: Cell<super::By>,
        /// Whether standings Warbands handed over are drawn at all.
        pub show_inherited: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReputationsPage {
        const NAME: &'static str = "ArmoryReputationsPage";
        type Type = super::ReputationsPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for ReputationsPage {}
    impl WidgetImpl for ReputationsPage {}
    impl BinImpl for ReputationsPage {}
}

/// One redraw's worth of input.
///
/// Public only because it is named by a field on the private implementation
/// struct; nothing outside this file constructs one.
#[derive(Clone)]
pub struct Held {
    roster: Roster,
    standings: HashMap<CharacterKey, Vec<FactionStanding>>,
    /// What each character has personally earned, from the addon.
    ///
    /// The other half of every row. A standing says where the *account* is; a
    /// character who has ground a maxed faction from nothing has done real work
    /// that the standing cannot express, and this is the only thing that can.
    earned: Provenance,
}

glib::wrapper! {
    pub struct ReputationsPage(ObjectSubclass<imp::ReputationsPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ReputationsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl ReputationsPage {
    pub fn new() -> Self {
        let page: Self = glib::Object::builder().build();
        page.imp().show_inherited.set(true);
        page.build();
        page
    }

    fn build(&self) {
        let column = almanac::column(15);
        column.add_css_class("al-main-column");
        // Packed to the top, or the scroller hands the column the whole
        // viewport and the cards share the slack out between them.
        column.set_valign(gtk::Align::Start);

        let headline = almanac::serif("What this character actually earned", "al-headline");
        let header = almanac::column(4);
        header.append(&headline);
        header.append(&almanac::caption(
            "Gold is work the addon watched happen, session by session. \
             The pale bar behind it is where the account already stands.",
        ));
        column.append(&header);

        let groups = almanac::column(15);
        column.append(&groups);

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search factions and characters")
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
                &almanac::rail_pane(&self.rail()),
                RAIL,
            ))
            .build();
        view.add_top_bar(&search);

        let imp = self.imp();
        *imp.groups.borrow_mut() = Some(groups);
        *imp.headline.borrow_mut() = Some(headline);
        *imp.search.borrow_mut() = Some(search);
        *imp.entry.borrow_mut() = Some(entry);

        self.set_child(Some(&view));
        self.redraw();
    }

    // -- the rail -------------------------------------------------------------

    /// The rail, built once.
    ///
    /// Only the cohort counts change with the data. The controls stay put on
    /// purpose: the "Show inherited" switch asks for a redraw from inside its
    /// own handler, and a redraw that rebuilt the rail would be disposing of
    /// the switch mid-signal.
    fn rail(&self) -> gtk::Box {
        let rail = almanac::rail_column();

        let page = self.clone();
        let toggle = almanac::segments(&["By character", "By faction"], 0, move |index| {
            let by = if index == 1 {
                By::Faction
            } else {
                By::Character
            };
            // Only on a real change. The segments are stock `GtkToggleButton`s
            // and emit on being set as well as on being pressed.
            if page.imp().by.get() != by {
                page.imp().by.set(by);
                page.redraw();
            }
        });
        // Two equal segments across the rail rather than two label-width ones.
        toggle.set_halign(gtk::Align::Fill);
        let mut segment = toggle.first_child();
        while let Some(button) = segment {
            button.set_hexpand(true);
            segment = button.next_sibling();
        }
        rail.append(&toggle);

        // READING THE BARS. The swatches are the page's own `Bar` at the page's
        // own height rather than two coloured boxes, so the legend cannot come
        // to disagree with the thing it is explaining.
        let legend = almanac::column(9);
        legend.append(&Self::swatch(
            true,
            "Earned by this character, watched login to logout",
        ));
        legend.append(&Self::swatch(false, "Where the account already stands"));
        rail.append(&almanac::titled("READING THE BARS", &legend));

        rail.append(&almanac::hairline());

        let totals = almanac::column(8);
        rail.append(&almanac::titled("ACROSS THE COHORT", &totals));

        let (row, switch) = almanac::switch_row("Show inherited", "", true);
        let card = almanac::card(0);
        card.append(&row);
        let page = self.clone();
        switch.connect_active_notify(move |switch| {
            page.imp().show_inherited.set(switch.is_active());
            page.redraw();
        });
        rail.append(&card);

        rail.append(&almanac::hairline());

        // The standing rule, said where somebody can read it. It is the reason
        // an unwatched standing gets no bar, and it is the one sentence on the
        // page that is about the application rather than about the account.
        let rule = almanac::label(
            "Without the addon there is no observation, and the answer is zero rather \
             than the account's standing. Falling back would be the inflation the rule \
             exists to prevent.",
            &["al-footnote"],
        );
        rule.set_wrap(true);
        rail.append(&rule);

        *self.imp().totals.borrow_mut() = Some(totals);
        rail
    }

    /// One line of the legend: the bar itself, and what it means.
    fn swatch(gold: bool, meaning: &str) -> gtk::Box {
        let line = almanac::row(10);
        let bar = Bar::new(7);
        bar.widget.set_hexpand(false);
        bar.widget.set_size_request(34, 7);
        if gold {
            bar.set_full(1.0, 0.0, Tone::Gold, 0);
        } else {
            bar.set_full(0.0, 1.0, Tone::Gold, 0);
        }
        line.append(&bar.widget);
        let text = almanac::label(meaning, &["al-caption"]);
        text.set_wrap(true);
        text.set_hexpand(true);
        line.append(&text);
        line
    }

    pub fn search(&self) -> Option<gtk::SearchBar> {
        self.imp().search.borrow().clone()
    }

    pub fn show(
        &self,
        roster: &Roster,
        standings: &HashMap<CharacterKey, Vec<FactionStanding>>,
        earned: &Provenance,
    ) {
        *self.imp().held.borrow_mut() = Some(Held {
            roster: roster.clone(),
            standings: standings.clone(),
            earned: earned.clone(),
        });
        self.redraw();
    }

    fn redraw(&self) {
        let imp = self.imp();
        let Some(groups) = imp.groups.borrow().clone() else {
            return;
        };
        while let Some(child) = groups.first_child() {
            groups.remove(&child);
        }

        let by = imp.by.get();
        if let Some(headline) = imp.headline.borrow().as_ref() {
            headline.set_label(match by {
                By::Character => "What this character actually earned",
                By::Faction => "What each character actually earned",
            });
        }

        let Some(held) = imp.held.borrow().clone() else {
            self.draw_totals(None);
            return;
        };
        self.draw_totals(Some(&held));

        if held.standings.is_empty() {
            groups.append(
                &adw::StatusPage::builder()
                    .icon_name("emblem-shared-symbolic")
                    .title("No standings yet")
                    .description(
                        "Reputations are fetched per enrolled character. Enrol somebody \
                         on Roster and sync — Blizzard has no account-wide reputation \
                         endpoint, so this is one call each.",
                    )
                    .vexpand(true)
                    .build(),
            );
            return;
        }

        let needle = imp.needle.borrow().clone();
        let inherited = imp.show_inherited.get();

        let drawn = match by {
            By::Character => self.by_character(&groups, &held, &needle, inherited),
            By::Faction => self.by_faction(&groups, &held, &needle, inherited),
        };

        if drawn == 0 {
            groups.append(
                &adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title("No matches")
                    .description(if inherited {
                        "Nothing here matches that."
                    } else {
                        "Nothing here matches that — and inherited standings are hidden."
                    })
                    .vexpand(true)
                    .build(),
            );
        }
    }

    /// The three claims, across the whole cohort.
    ///
    /// Counted over everything the account has, not over what the search left
    /// on screen: the rail is the page's standing and a figure that moved as
    /// somebody typed would be reporting the search instead.
    fn draw_totals(&self, held: Option<&Held>) {
        let Some(totals) = self.imp().totals.borrow().clone() else {
            return;
        };
        while let Some(child) = totals.first_child() {
            totals.remove(&child);
        }

        let mut counts = (0usize, 0usize, 0usize);
        if let Some(held) = held {
            for character in &held.roster.characters {
                let Some(standings) = held.standings.get(&character.key) else {
                    continue;
                };
                let one = tally(standings, held.earned.get(&character.key));
                counts = (counts.0 + one.0, counts.1 + one.1, counts.2 + one.2);
            }
        }

        // Only the first is gold. The other two are real numbers about the
        // account and neither is work this run may claim.
        totals.append(&almanac::stat_line(
            "Earned in this run",
            &counts.0.to_string(),
            Tone::Gold,
        ));
        totals.append(&almanac::stat_line(
            "Inherited",
            &counts.1.to_string(),
            Tone::Plain,
        ));
        let unclear = almanac::stat_line("Cannot tell", &counts.2.to_string(), Tone::Plain);
        unclear.add_css_class("al-unknown");
        totals.append(&unclear);
    }

    // -- the main column ------------------------------------------------------

    /// A group per character: who they are, then what they stand at.
    fn by_character(&self, groups: &gtk::Box, held: &Held, needle: &str, keep: bool) -> usize {
        let mut drawn = 0;

        for character in &held.roster.characters {
            let Some(standings) = held.standings.get(&character.key) else {
                continue;
            };
            let earned = held.earned.get(&character.key);
            let matching = Self::matching(standings, earned, needle, keep, |standing| {
                standing.name.to_lowercase().contains(needle)
                    || character.display_name.to_lowercase().contains(needle)
            });
            if matching.is_empty() {
                continue;
            }

            let group = almanac::column(11);
            group.append(&Self::strip(character, standings, earned));

            // The ones nothing can answer are gathered into a single card
            // rather than drawn one apiece. On an account with no collector
            // addon *every* standing is unclear, and a card each is two hundred
            // identical grey panels repeating one sentence — which reads as a
            // page that has not been finished rather than as the honest answer
            // it is. Said once, with the factions named under it, the same fact
            // is legible in a glance.
            let (unclear, measured): (Vec<&FactionStanding>, Vec<&FactionStanding>) = matching
                .iter()
                .copied()
                .partition(|standing| reading(standing, earned) == Reading::Unclear);

            for (index, standing) in measured.iter().take(SHOWN).enumerate() {
                group.append(&Self::card(
                    &standing.name,
                    &character.display_name,
                    standing,
                    earned,
                    index,
                ));
                drawn += 1;
            }
            if measured.len() > SHOWN {
                group.append(&almanac::caption(&format!(
                    "and {} more — search to reach them",
                    measured.len() - SHOWN
                )));
            }
            if !unclear.is_empty() {
                drawn += unclear.len();
                group.append(&Self::unclear_card(
                    unclear
                        .iter()
                        .map(|standing| standing.name.clone())
                        .collect(),
                ));
            }
            groups.append(&group);
        }
        drawn
    }

    /// A group per faction, so a whole roster can be compared down a column.
    fn by_faction(&self, groups: &gtk::Box, held: &Held, needle: &str, keep: bool) -> usize {
        /// A faction's name, and who stands where with it.
        type Members<'a> = (
            String,
            Vec<(String, &'a FactionStanding, Option<&'a Earned>)>,
        );

        let mut by_faction: HashMap<u32, Members<'_>> = HashMap::new();

        for character in &held.roster.characters {
            let Some(standings) = held.standings.get(&character.key) else {
                continue;
            };
            for standing in standings {
                if !keep && standing.inherited {
                    continue;
                }
                if !needle.is_empty()
                    && !standing.name.to_lowercase().contains(needle)
                    && !character.display_name.to_lowercase().contains(needle)
                {
                    continue;
                }
                by_faction
                    .entry(standing.faction)
                    .or_insert_with(|| (standing.name.clone(), Vec::new()))
                    .1
                    .push((
                        character.display_name.clone(),
                        standing,
                        held.earned.get(&character.key),
                    ));
            }
        }

        let mut ordered: Vec<Members<'_>> = by_faction.into_values().collect();
        ordered.sort_by(|a, b| a.0.cmp(&b.0));

        let mut drawn = 0;
        for (faction, mut members) in ordered {
            // Furthest first: the question this arrangement answers is which
            // character is closest, and that is the top row. Sorted on what
            // each was *watched earning*, not on the account's standing —
            // that number is the same for all of them on an account-wide
            // faction and would order the column by nothing at all.
            members.sort_by(|a, b| {
                let key = |(_, standing, earned): &(String, &FactionStanding, Option<&Earned>)| {
                    earned.map_or(0.0, |earned| {
                        share(standing, &earned.with(standing.faction))
                    })
                };
                key(b)
                    .total_cmp(&key(a))
                    .then_with(|| b.1.renown.cmp(&a.1.renown))
                    .then_with(|| a.0.cmp(&b.0))
            });

            let group = almanac::column(11);
            group.append(&almanac::section(&faction.to_uppercase()));

            // Same collapse as the by-character view, for the same reason: on
            // an account the addon has never watched, every member of every
            // faction is unclear.
            let (unclear, measured): (Vec<_>, Vec<_>) = members
                .iter()
                .partition(|(_, standing, earned)| reading(standing, *earned) == Reading::Unclear);

            for (index, (who, standing, earned)) in measured.iter().enumerate() {
                group.append(&Self::card(who, who, standing, *earned, index));
                drawn += 1;
            }
            if !unclear.is_empty() {
                drawn += unclear.len();
                group.append(&Self::unclear_card(
                    unclear.iter().map(|(who, _, _)| who.clone()).collect(),
                ));
            }
            groups.append(&group);
        }
        drawn
    }

    /// The standings a search and the inherited toggle have left, in the order
    /// they are worth looking at.
    fn matching<'a>(
        standings: &'a [FactionStanding],
        earned: Option<&Earned>,
        needle: &str,
        keep: bool,
        names: impl Fn(&FactionStanding) -> bool,
    ) -> Vec<&'a FactionStanding> {
        let mut matching: Vec<&FactionStanding> = standings
            .iter()
            .filter(|standing| keep || !standing.inherited)
            .filter(|standing| needle.is_empty() || names(standing))
            .collect();

        // Work first, furthest first. The cap has to fall on the factions
        // nobody has touched rather than on the ones somebody is grinding.
        matching.sort_by(|a, b| {
            let key = |standing: &FactionStanding| match earned {
                Some(earned) if earned.has_touched(standing.faction) => {
                    share(standing, &earned.with(standing.faction))
                }
                _ => -1.0,
            };
            key(b).total_cmp(&key(a)).then_with(|| a.name.cmp(&b.name))
        });
        matching
    }

    /// Who this group is about, and how their standings divide up.
    fn strip(
        character: &Character,
        standings: &[FactionStanding],
        earned: Option<&Earned>,
    ) -> gtk::Box {
        let strip = almanac::card(0);
        strip.add_css_class("al-tight");

        // No portrait is fetched for this page — it is handed a roster and a
        // provenance and nothing else — so the ring is the whole picture. It is
        // also the part that identifies the character at a glance, and the
        // initials under it are the same ones Adwaita draws everywhere else.
        let ring = almanac::row(0);
        ring.add_css_class("portrait");
        ring.add_css_class(almanac::class_style(&character.class));
        ring.set_valign(gtk::Align::Center);
        ring.append(&adw::Avatar::new(26, Some(&character.display_name), true));

        let line = almanac::row(11);
        line.append(&ring);

        let text = almanac::column(1);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&almanac::label(&character.display_name, &["al-row-title"]));
        // Every character with standings is an enrolled one: reputations are
        // one call each and are only ever fetched for the cohort.
        text.append(&almanac::caption(&format!(
            "{} · enrolled",
            character.realm_name
        )));
        line.append(&text);

        let (earned_count, inherited, unclear) = tally(standings, earned);
        let mut parts = vec![
            format!("{earned_count} EARNED"),
            format!("{inherited} INHERITED"),
        ];
        if unclear > 0 {
            parts.push(format!("{unclear} CANNOT TELL"));
        }
        let counts = almanac::meta(&parts.join(" · "));
        counts.set_halign(gtk::Align::End);
        counts.set_valign(gtk::Align::Center);
        line.append(&counts);

        strip.append(&line);
        strip
    }

    /// One standing, and what this character personally did for it.
    ///
    /// The whole reason the addon watches reputation. An inherited standing
    /// says where the *account* is and can say nothing else — it was at the
    /// ceiling before the run began, so no amount of work will move it. What
    /// this character earned is a separate number that does move, and it is the
    /// one a replay is about.
    /// Everything the page cannot answer, in one card.
    ///
    /// Never hidden and never counted as nothing: the size of this list is half
    /// of what makes the rest of the page believable, and an account with no
    /// collector addon is *entirely* this list. It carries no bar for the same
    /// reason a single unclear card carries none — a floor is not a
    /// measurement, and `Origin::Unclear` drawn as a bar would be the page
    /// guessing.
    fn unclear_card(names: Vec<String>) -> gtk::Box {
        let card = almanac::card(9);
        card.set_opacity(0.75);

        let head = almanac::row(10);
        let title = almanac::label(
            &almanac::plural(names.len(), "standing", "standings"),
            &["al-row-title"],
        );
        title.set_hexpand(true);
        head.append(&title);
        head.append(&almanac::meta("CANNOT TELL"));
        card.append(&head);

        card.append(&almanac::caption(
            "The client does not maintain a total earned for these, so Armory will not \
             guess which character did the work. Install the collector addon and it will \
             watch them from the next login onwards.",
        ));

        // The factions themselves, named. A count with nothing under it invites
        // the question this card exists to answer.
        let chips = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .row_spacing(6)
            .column_spacing(6)
            .max_children_per_line(24)
            .build();
        for name in names {
            chips.append(&almanac::chip(&name, Tone::Plain));
        }
        card.append(&chips);
        card
    }

    fn card(
        title: &str,
        who: &str,
        standing: &FactionStanding,
        earned: Option<&Earned>,
        index: usize,
    ) -> gtk::Box {
        let mine = earned
            .map(|earned| earned.with(standing.faction))
            .unwrap_or_default();
        let reading = reading(standing, earned);

        // The inherited card is gold-tinted and its bar carries no gold, and
        // that is deliberate rather than an oversight. The tint is a callout —
        // this is the row somebody has to decide about — and the fill is the
        // claim "the run earned this". An inherited standing is the first and
        // not the second.
        let card = match reading {
            Reading::Inherited => almanac::earned_card(8),
            _ => almanac::card(8),
        };

        let head = almanac::row(10);
        let name = almanac::label(title, &["al-row-title"]);
        name.set_wrap(true);
        name.set_hexpand(true);
        head.append(&name);
        head.append(&Self::badge(reading, &mine, standing));
        card.append(&head);

        if reading.measured() {
            let bar = Bar::new(7);
            let fill = match reading {
                // Nothing at all for an inherited standing. The pale bar is the
                // whole of it; there is no share of it this run may claim.
                Reading::Inherited => 0.0,
                _ => share(standing, &mine),
            };
            // The pale reading is always the whole of the account's standing,
            // and the footer says in words what that standing is. Only the gold
            // moves — animating the pale one would say the account's position
            // was this run's doing.
            bar.set_full(fill, 1.0, Tone::Gold, STEP_MS * index.min(STAGGER) as u32);
            card.append(&bar.widget);
        }

        match reading {
            Reading::Inherited => {
                card.append(&almanac::caption(&format!(
                    "{} before this run began, earned by a character nobody enrolled. \
                     It cannot move, so nothing here counts towards the run — attest it \
                     or leave it out.",
                    Self::tier(standing)
                )));
            }
            Reading::Unclear => {
                // Reduced, because it is a card the page cannot finish. Never
                // hidden: the count of what could not be told is half of what
                // makes the rest of the page believable.
                card.set_opacity(0.75);
                card.append(&almanac::caption(
                    "The client does not maintain a total earned for this one, so Armory \
                     will not guess which character did the work.",
                ));
            }
            Reading::Earned | Reading::Nothing => {
                let footer = almanac::row(8);
                let left = almanac::caption(&Self::earned_line(standing, &mine, who));
                left.set_hexpand(true);
                footer.append(&left);
                let right =
                    almanac::caption(&format!("account stands at {}", Self::tier(standing)));
                right.set_halign(gtk::Align::End);
                footer.append(&right);
                card.append(&footer);
            }
        }
        card
    }

    /// The word to the right of a faction's name.
    ///
    /// Gold, and it is *this character's* standing rather than the account's —
    /// what their own work would have reached from nothing. The account's is in
    /// the footer, in plain text, where it belongs.
    fn badge(reading: Reading, mine: &EarnedReputation, standing: &FactionStanding) -> gtk::Label {
        match reading {
            Reading::Earned => {
                let (rank, name) = standing_earned(mine);
                let text = if name == "Renown" {
                    format!("RENOWN {rank}")
                } else {
                    name.to_uppercase()
                };
                almanac::mono(&text, &["al-price", "al-gold"])
            }
            Reading::Inherited => {
                let chip = almanac::chip("INHERITED", Tone::Gold);
                chip.add_css_class("al-mono");
                chip
            }
            Reading::Unclear => almanac::meta("CANNOT TELL"),
            // Watched, and nothing earned yet. Said in words rather than as a
            // gold "NEUTRAL", which would spend the accent on no work at all.
            Reading::Nothing => {
                let _ = standing;
                almanac::meta("NOTHING EARNED YET")
            }
        }
    }

    /// What this character was watched earning, in their own numbers.
    fn earned_line(standing: &FactionStanding, mine: &EarnedReputation, who: &str) -> String {
        if mine.points == 0 && mine.renown == 0 {
            return format!("Nothing watched being earned by {who} yet");
        }
        if standing.renown > 0 {
            return format!(
                "{} of {} renown earned by {who}",
                mine.renown, standing.renown
            );
        }
        let (_, reached) = standing_earned(mine);
        format!(
            "{} reputation earned by {who} — {reached} from nothing",
            almanac::thousands(u64::from(mine.points))
        )
    }

    /// Where the account stands, as a word.
    fn tier(standing: &FactionStanding) -> String {
        if !standing.tier.is_empty() {
            standing.tier.clone()
        } else if standing.renown > 0 {
            format!("Renown {}", standing.renown)
        } else {
            "no standing recorded".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing(inherited: bool) -> FactionStanding {
        FactionStanding {
            faction: 2_600,
            name: "The Assembly of the Deeps".into(),
            tier: "Renown 19".into(),
            value: 4_200,
            max: 8_500,
            renown: 19,
            inherited,
        }
    }

    fn watched(points: u32, renown: u32) -> Earned {
        let mut earned = Earned::default();
        earned.reputation.insert(
            2_600,
            EarnedReputation {
                points,
                renown,
                renown_seen: 19,
                account_wide: true,
            },
        );
        earned
    }

    #[test]
    fn a_standing_nobody_watched_is_never_drawn_as_a_bar() {
        // Without the addon there is no observation. A floor is not a
        // measurement, and a bar over one reads as the measurement it is not.
        assert_eq!(reading(&standing(false), None), Reading::Unclear);
        assert!(!reading(&standing(false), None).measured());
    }

    #[test]
    fn a_watched_zero_is_not_the_same_fact_as_an_unwatched_one() {
        // The addon was there and saw nothing earned, which is a measurement.
        // It draws a bar with no gold in it; the unwatched case draws none.
        let nothing = Earned::default();
        assert_eq!(reading(&standing(false), Some(&nothing)), Reading::Nothing);
        assert!(reading(&standing(false), Some(&nothing)).measured());
    }

    #[test]
    fn an_inherited_standing_carries_no_gold_at_all() {
        let nothing = Earned::default();
        assert_eq!(reading(&standing(true), Some(&nothing)), Reading::Inherited);
        // And the same with no addon: the flag is a positive fact and outranks
        // the silence.
        assert_eq!(reading(&standing(true), None), Reading::Inherited);
    }

    #[test]
    fn an_inherited_faction_somebody_is_grinding_anyway_is_the_interesting_row() {
        // The one case where work outranks the inherited flag. The standing
        // cannot move, and the work is still real and still the run's.
        let mine = watched(2_400, 9);
        assert_eq!(reading(&standing(true), Some(&mine)), Reading::Earned);
        assert_eq!(share(&standing(true), &mine.with(2_600)), 9.0 / 19.0);
    }

    #[test]
    fn the_share_is_of_where_the_account_stands_and_never_more() {
        // A character who earned more renown than the account currently shows —
        // which happens when a faction's standing was reset or misread — still
        // fills the bar and no further.
        let mine = watched(0, 40);
        assert_eq!(share(&standing(false), &mine.with(2_600)), 1.0);
    }

    #[test]
    fn a_classic_ladder_is_measured_in_ranks() {
        let classic = FactionStanding {
            renown: 0,
            tier: "Exalted".into(),
            max: 0,
            value: 0,
            ..standing(false)
        };
        // Exalted from nothing is the whole ladder.
        let done = EarnedReputation {
            points: 42_000,
            ..EarnedReputation::default()
        };
        assert_eq!(share(&classic, &done), 1.0);

        // Honored is two of the four ranks above Neutral, and the partial
        // progress towards Revered rides on top of it.
        let partway = EarnedReputation {
            points: 9_000,
            ..EarnedReputation::default()
        };
        assert_eq!(share(&classic, &partway), 0.5);

        assert_eq!(share(&classic, &EarnedReputation::default()), 0.0);
    }

    #[test]
    fn the_cohort_counts_are_three_separate_claims() {
        let standings = vec![standing(false), standing(true)];
        // Watched, one earned and one inherited-and-untouched.
        let (earned, inherited, unclear) = tally(&standings, Some(&watched(2_400, 9)));
        // Both factions share an id here, so the watched one answers for both:
        // the inherited flag loses to work in the first branch of `reading`.
        assert_eq!((earned, inherited, unclear), (2, 0, 0));

        // With nothing watching, the flag is all that is left to go on.
        let (earned, inherited, unclear) = tally(&standings, None);
        assert_eq!((earned, inherited, unclear), (0, 1, 1));
    }
}
