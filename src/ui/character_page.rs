//! One character, as a body of work rather than a paper doll.
//!
//! The roster answers "who is on this account". This answers "who is this",
//! and the difference decides everything about the layout. There is no
//! full-body render and no equipment mannequin: a character sheet drawn as a
//! paper doll puts the artwork in the middle and the numbers around the edge,
//! and the numbers are the reason anybody opened the page. The identity is
//! carried by a portrait the size of a thumbnail and the name set large, and
//! everything below it is evidence.
//!
//! ## What is gold here
//!
//! The same rule as everywhere: gold is work this character can be credited
//! with. The item level is gold, the Mythic+ rating is gold, the hours the
//! addon watched are gold — and the account's achievement points sit in the
//! same strip in plain type, because they are the account's and putting them in
//! gold beside three figures that belong to one character would quietly claim
//! them for that character. The strip is the whole rule in miniature.
//!
//! ## Nothing here has a gender
//!
//! Armory does not know one. Blizzard's summary carries a `gender` field and
//! the addon could read one too, but neither says anything about the person
//! playing — and the copy on a page about somebody's own characters is not the
//! place to guess. Every line here is written in the plural or with no pronoun
//! at all.
//!
//! ## The gear list is sorted weakest first
//!
//! An average hides the one actionable fact about a set of gear, which is which
//! slot is dragging it down. So the list is ascending by item level and an
//! empty slot is drawn as an empty slot rather than folded into a number — the
//! same argument as `Evaluation::observable`, applied to a character sheet.
//!
//! ## Two sources, two facts, never merged
//!
//! Raid progress arrives two ways and they are not the same claim.
//! `Detail::raids` is every boss this character has ever killed and comes from
//! the web API; `Detail::raid_locks` is what they are saved to this week and
//! comes from the addon, because the client is the only thing that knows it.
//! An account with no Battle.net client has only the second, and the page says
//! which it is showing rather than presenting one as the other.

use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use gtk::glib;

use super::almanac::{self, Bar, Tone};
use super::images::{Art, Images};
use crate::model::character::{Character, Detail, Equipped, RaidLock, RaidTier};
use crate::model::chronicle::Digest;
use crate::model::source::blizzard::{media, Region};
use crate::model::tally::{Counting, Tally};

/// How wide this page's rail is. The same 288 the roster and reputations use —
/// it carries counts and short lines rather than a price book.
const RAIL: f64 = 288.0;

/// How big the portrait is. Small on purpose: this page is a body of work and
/// not a paper doll, and a render big enough to admire is a render the numbers
/// have to fit around.
const PORTRAIT: i32 = 52;

/// How many evenings the history spine draws.
///
/// It is a summary of a character, not the journal — the chronicle is the page
/// for reading every evening, and repeating it here would be two answers to one
/// question that drift apart.
const SPINE_SHOWN: usize = 4;

/// How many zones the hours breakdown names before it says "and the rest".
const ZONES_SHOWN: usize = 3;

/// How many companions and questgivers the rail lists.
const PEOPLE_SHOWN: usize = 5;

/// How many keystone runs the card lists.
const KEYS_SHOWN: usize = 6;

/// How many raids the card lists, newest first.
const RAIDS_SHOWN: usize = 3;

/// Everything one draw of this page needs.
///
/// A struct rather than a dozen arguments, the same as `zone_page::Held`: the
/// page reads from six different parts of the model and a positional signature
/// of that length is one transposition away from a silent bug.
#[derive(Clone, Default)]
pub struct Held {
    pub character: Option<Character>,
    pub detail: Detail,
    /// The character's portrait, once a sync has earned the URL.
    pub portrait: Option<String>,
    /// This character's evenings, newest first.
    pub evenings: Vec<Digest>,
    /// This character's lifetime counters.
    pub tallies: Vec<Tally>,
    /// Closed goals credited to this character, the run's total, and whoever
    /// else has the most.
    pub share: Share,
    pub region: Region,
}

/// This character's part of the run.
#[derive(Clone, Default)]
pub struct Share {
    pub credited: usize,
    /// Every closed goal, credited or not.
    pub closed: usize,
    /// The character with the next most, for the sentence under the bar.
    pub runner_up: Option<(String, usize)>,
}

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct CharacterPage {
        pub column: RefCell<Option<gtk::Box>>,
        pub rail: RefCell<Option<gtk::Box>>,
        pub held: RefCell<Held>,
        pub images: RefCell<Option<Images>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CharacterPage {
        const NAME: &'static str = "ArmoryCharacterPage";
        type Type = super::CharacterPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for CharacterPage {}
    impl WidgetImpl for CharacterPage {}
    impl BinImpl for CharacterPage {}
}

glib::wrapper! {
    pub struct CharacterPage(ObjectSubclass<imp::CharacterPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CharacterPage {
    pub fn new(images: &Images) -> Self {
        let page: Self = glib::Object::builder().build();
        *page.imp().images.borrow_mut() = Some(images.clone());
        page.build();
        page
    }

    fn build(&self) {
        let column = almanac::column(20);
        column.add_css_class("al-main-column");
        // Packed to the top, or the scroller hands the column the whole
        // viewport and the sections drift apart down a long page.
        column.set_valign(gtk::Align::Start);

        let rail = almanac::rail_column();

        let imp = self.imp();
        *imp.column.borrow_mut() = Some(column.clone());
        *imp.rail.borrow_mut() = Some(rail.clone());

        self.set_child(Some(&almanac::split(
            &almanac::main_column(&column),
            &almanac::rail_pane(&rail),
            RAIL,
        )));
        self.redraw();
    }

    /// Hand the page one character.
    pub fn show(&self, held: Held) {
        *self.imp().held.borrow_mut() = held;
        self.redraw();
    }

    /// Who the page is currently about.
    pub fn character(&self) -> Option<Character> {
        self.imp().held.borrow().character.clone()
    }

    fn redraw(&self) {
        let imp = self.imp();
        let (Some(column), Some(rail)) = (imp.column.borrow().clone(), imp.rail.borrow().clone())
        else {
            return;
        };
        for pane in [&column, &rail] {
            while let Some(child) = pane.first_child() {
                pane.remove(&child);
            }
        }

        let held = imp.held.borrow().clone();
        let Some(character) = held.character.clone() else {
            column.append(
                &adw::StatusPage::builder()
                    .icon_name("system-users-symbolic")
                    .title("No character chosen")
                    .description("Open a character from the Roster.")
                    .vexpand(true)
                    .build(),
            );
            return;
        };

        self.draw_column(&column, &character, &held);
        self.draw_rail(&rail, &character, &held);
    }

    // -- the main column ------------------------------------------------------

    fn draw_column(&self, column: &gtk::Box, character: &Character, held: &Held) {
        column.append(&self.header(character, held));
        column.append(&Self::stat_strip(character, held));
        column.append(&Self::history(held));
        column.append(&Self::gear(&held.detail));

        // Two columns from here down. The record is a long list of small
        // numbers and the raids are a short list of cards, and stacking them
        // puts a screen of counters between the character and their raiding.
        let pair = almanac::row(26);
        pair.set_homogeneous(true);

        let left = almanac::column(20);
        left.append(&Self::record(held));
        left.append(&Self::hours(held));
        pair.append(&left);

        let right = almanac::column(20);
        right.append(&Self::keys(held));
        right.append(&Self::raids(&held.detail));
        pair.append(&right);

        column.append(&pair);
    }

    /// The portrait, the name, and how much of this character Armory has seen.
    fn header(&self, character: &Character, held: &Held) -> gtk::Box {
        let strip = almanac::column(0);

        let line = almanac::row(14);
        line.set_valign(gtk::Align::End);

        line.append(&self.portrait(character, held));

        let names = almanac::column(7);
        names.set_hexpand(true);
        let name = almanac::serif(&character.display_name, "al-hero-title");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        names.append(&name);

        let identity = almanac::mono(&Self::identity(character, &held.detail), &["al-meta"]);
        identity.set_xalign(0.0);
        identity.set_ellipsize(gtk::pango::EllipsizeMode::End);
        names.append(&identity);
        line.append(&names);

        // The design asked for the character's age here. Neither source can
        // answer it: the profile API has no creation date and no in-game API
        // reports one either, so the only honest half of that line is how much
        // of the character Armory has actually watched — which is the half the
        // application is about anyway.
        let watched = almanac::column(5);
        watched.set_halign(gtk::Align::End);
        let (headline, note) = Self::watched_for(held);
        let headline = almanac::mono(&headline, &["al-meta"]);
        headline.set_xalign(1.0);
        watched.append(&headline);
        let note = almanac::caption(&note);
        note.set_xalign(1.0);
        watched.append(&note);
        line.append(&watched);

        strip.append(&line);
        strip.append(&almanac::hairline());
        strip
    }

    /// The character's own render if there is one, and their class crest if
    /// not. The crest is a fallback rather than a placeholder: a smaller
    /// picture of a true thing, not a blank waiting to be filled.
    fn portrait(&self, character: &Character, held: &Held) -> Art {
        let art = Art::new(PORTRAIT, "avatar-default-symbolic");
        art.add_css_class("portrait");
        art.add_css_class(almanac::class_style(&character.class));
        art.set_valign(gtk::Align::End);

        let url = held
            .portrait
            .clone()
            .unwrap_or_else(|| media::class_icon(held.region, &character.class));
        if let Some(images) = self.imp().images.borrow().as_ref() {
            art.show(images, Some(&url), PORTRAIT);
        }
        art
    }

    /// `ORC ELEMENTAL SHAMAN · 80 · AREA 52 · <REINS AND RUIN>`.
    fn identity(character: &Character, detail: &Detail) -> String {
        let mut parts = vec![match &detail.spec {
            Some(spec) => format!("{} {} {}", character.race, spec, character.class),
            None => format!("{} {}", character.race, character.class),
        }];
        parts.push(character.level.to_string());
        parts.push(character.realm_name.clone());
        if let Some(guild) = &detail.guild {
            parts.push(format!("<{guild}>"));
        }
        parts.join(" · ").to_uppercase()
    }

    /// How long Armory has been watching, and how much of it there is.
    fn watched_for(held: &Held) -> (String, String) {
        let Some(first) = held.evenings.last() else {
            return (
                "NOT YET WATCHED".to_string(),
                "No evening on this character has been recorded.".to_string(),
            );
        };
        let since = first.started_at.with_timezone(&Local);
        let months = (Utc::now() - first.started_at).num_days().max(0) / 30;
        let span = match months {
            0 => "less than a month recorded".to_string(),
            1 => "one month recorded".to_string(),
            months => format!("{months} months recorded"),
        };
        (
            span.to_uppercase(),
            format!(
                "{} since {}",
                almanac::plural(held.evenings.len(), "evening", "evenings"),
                since.format("%-d %B %Y")
            ),
        )
    }

    /// Four figures, and which of them are hers.
    fn stat_strip(character: &Character, held: &Held) -> gtk::Box {
        let detail = &held.detail;
        // One pixel of spacing over a hairline background, so four cells read
        // as one object. `al-band` was the chronicle card's art band and only
        // rounded its top corners.
        let strip = almanac::row(1);
        strip.add_css_class("al-strip");
        strip.set_homogeneous(true);

        let (item_level, item_note) = match (detail.item_level, detail.equipped_item_level) {
            (Some(overall), Some(equipped)) if equipped < overall => (
                overall.to_string(),
                format!("{equipped} equipped — a slot is empty"),
            ),
            (Some(overall), _) => (overall.to_string(), "every slot filled".to_string()),
            (None, _) => ("—".to_string(), "not reported".to_string()),
        };
        strip.append(&almanac::stat_tile(
            "ITEM LEVEL",
            &item_level,
            &item_note,
            Tone::Gold,
        ));

        let (rating, keys) = match detail.mythic_rating {
            Some(rating) => (
                rating.to_string(),
                format!(
                    "this season · {}",
                    almanac::plural(Self::keystones(held).len(), "key", "keys")
                ),
            ),
            None => ("—".to_string(), "no rating this season".to_string()),
        };
        strip.append(&almanac::stat_tile(
            "MYTHIC+ RATING",
            &rating,
            &keys,
            Tone::Gold,
        ));

        // Deliberately not gold. Achievement points are the account's, and
        // three of this character's own figures beside them in gold is exactly
        // the claim the rest of the application exists to refuse.
        strip.append(&almanac::stat_tile(
            "ACHIEVEMENT POINTS",
            &detail
                .achievement_points
                .map(|points| almanac::thousands(u64::from(points)))
                .unwrap_or_else(|| "—".into()),
            &format!("account-wide, not {}'s alone", character.display_name),
            Tone::Plain,
        ));

        let hours = held
            .evenings
            .iter()
            .map(|evening| (evening.ended_at - evening.started_at).num_minutes().max(0))
            .sum::<i64>()
            / 60;
        strip.append(&almanac::stat_tile(
            "HOURS WATCHED",
            &hours.to_string(),
            &match held.evenings.first() {
                Some(_) => almanac::plural(held.evenings.len(), "evening", "evenings"),
                None => "nothing recorded yet".to_string(),
            },
            Tone::Gold,
        ));

        strip
    }

    /// The dated spine: the evenings that were firsts.
    fn history(held: &Held) -> gtk::Box {
        let section = almanac::column(12);
        section.append(&almanac::section("THEIR OWN HISTORY"));

        let moments = Self::moments(held);
        if moments.is_empty() {
            section.append(&almanac::caption(
                "Nothing recorded on this character yet. Install the collector \
                 addon and play an evening, and this fills.",
            ));
            return section;
        }

        // The same grid the chronicle uses: the spine has to run the height of
        // the whole list and the dates need a gutter the entries cannot reflow
        // into.
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(16)
            .build();
        let gutter = almanac::column(0);
        gutter.set_size_request(20, -1);
        gutter.append(&almanac::spine());
        grid.attach(&gutter, 1, 0, 1, moments.len() as i32);

        for (index, moment) in moments.iter().enumerate() {
            let row = index as i32;
            let newest = index == 0;

            let date = almanac::column(4);
            date.set_halign(gtk::Align::End);
            date.set_valign(gtk::Align::Start);
            let local = moment.at.with_timezone(&Local);
            let day = almanac::mono(
                &local.format("%d %b").to_string().to_uppercase(),
                if newest {
                    &["al-meta", "al-gold"]
                } else {
                    &["al-meta"]
                },
            );
            day.set_xalign(1.0);
            date.append(&day);
            let year = almanac::mono(&local.format("%Y").to_string(), &["al-footnote"]);
            year.set_xalign(1.0);
            date.append(&year);
            grid.attach(&date, 0, row, 1, 1);

            let dot = almanac::spine_dot(newest);
            dot.set_margin_top(2);
            grid.attach(&dot, 1, row, 1, 1);

            let text = almanac::column(3);
            text.set_hexpand(true);
            let title = almanac::label(&moment.title, &["al-entry-title"]);
            title.set_xalign(0.0);
            title.set_wrap(true);
            text.append(&title);
            let detail = almanac::caption(&moment.detail);
            detail.set_xalign(0.0);
            detail.set_wrap(true);
            text.append(&detail);
            grid.attach(&text, 2, row, 1, 1);
        }

        section.append(&grid);
        section
    }

    /// The evenings worth putting on a spine, newest first.
    ///
    /// Firsts and lasts rather than the last four evenings: the chronicle is
    /// where every evening lives, and repeating its top four here would be the
    /// same list drawn twice. What belongs to a *character* is the shape of her
    /// history — the most recent night, the levels that mattered, and the day
    /// Armory started watching.
    fn moments(held: &Held) -> Vec<Moment> {
        let mut moments: Vec<Moment> = Vec::new();

        if let Some(latest) = held.evenings.first() {
            let mut parts = Vec::new();
            let minutes = (latest.ended_at - latest.started_at).num_minutes().max(0);
            parts.push(format!("{}h {:02}m", minutes / 60, minutes % 60));
            if !latest.quests.is_empty() {
                parts.push(almanac::plural(latest.quests.len(), "quest", "quests"));
            }
            if !latest.deaths.is_empty() {
                parts.push(almanac::plural(latest.deaths.len(), "death", "deaths"));
            }
            moments.push(Moment {
                at: latest.started_at,
                title: match latest.route.first() {
                    Some(stop) => stop.zone.clone(),
                    None => "The most recent evening".to_string(),
                },
                detail: format!("Their most recent evening — {}.", parts.join(", ")),
            });
        }

        // The highest level reached, and the night it happened. A character
        // levels once and it is the fact they are defined by afterwards.
        let levelled = held
            .evenings
            .iter()
            .flat_map(|evening| {
                evening
                    .levels
                    .iter()
                    .map(move |(level, place)| (evening.started_at, *level, place.clone()))
            })
            .max_by_key(|(_, level, _)| *level);
        if let Some((at, level, place)) = levelled {
            moments.push(Moment {
                at,
                title: format!("Reached level {level}"),
                detail: format!("Standing in {place}."),
            });
        }

        if let Some(first) = held.evenings.last() {
            if held.evenings.len() > 1 {
                let minutes = (first.ended_at - first.started_at).num_minutes().max(0);
                moments.push(Moment {
                    at: first.started_at,
                    title: "First evening Armory watched".to_string(),
                    detail: format!(
                        "{}, {}h {:02}m. Nothing before this is recorded — not by \
                         Armory and not by anything else.",
                        first
                            .route
                            .first()
                            .map(|stop| stop.zone.clone())
                            .unwrap_or_else(|| "Somewhere unrecorded".into()),
                        minutes / 60,
                        minutes % 60
                    ),
                });
            }
        }

        moments.sort_by_key(|moment| std::cmp::Reverse(moment.at));
        moments.dedup_by(|a, b| a.at == b.at && a.title == b.title);
        moments.truncate(SPINE_SHOWN);
        moments
    }

    /// What they are wearing, weakest slot first.
    fn gear(detail: &Detail) -> gtk::Box {
        let section = almanac::column(12);

        let heading = almanac::row(10);
        heading.set_baseline_position(gtk::BaselinePosition::Bottom);
        heading.append(&almanac::section("WHAT THEY ARE WEARING"));
        heading.append(&almanac::caption(
            "weakest slot first — the average hides it",
        ));
        section.append(&heading);

        let Some(worn) = &detail.equipment else {
            section.append(&almanac::caption(
                "No equipment recorded. It arrives with the next sync, or with \
                 the collector addon the next time this character logs out.",
            ));
            return section;
        };

        let rows = Self::gear_rows(worn);
        if rows.is_empty() {
            section.append(&almanac::caption("Nothing equipped at all."));
            return section;
        }

        // Two columns, filled down the left and then down the right, so that
        // reading the first column top to bottom is reading the list in order.
        let split = rows.len().div_ceil(2);
        let pair = almanac::row(26);
        pair.set_homogeneous(true);
        for half in rows.chunks(split.max(1)) {
            let side = almanac::column(0);
            side.set_hexpand(true);
            for (index, row) in half.iter().enumerate() {
                side.append(&Self::gear_row(row));
                if index + 1 < half.len() {
                    side.append(&almanac::hairline());
                }
            }
            pair.append(&side);
        }
        section.append(&pair);

        if let Some(note) = Self::gear_note(detail, &rows) {
            section.append(&almanac::caption(&note));
        }
        section
    }

    /// Every slot, in the order the page reads them.
    ///
    /// Empty slots first and cosmetic slots last, and real gear ascending in
    /// between. The three groups are the point: an empty slot is the most
    /// actionable thing on the page, and a tabard has no item level to compare
    /// so it cannot take part in the ordering at all.
    fn gear_rows(worn: &[Equipped]) -> Vec<GearRow> {
        let held: HashMap<&str, &Equipped> =
            worn.iter().map(|item| (item.slot.as_str(), item)).collect();

        let mut empty = Vec::new();
        let mut filled = Vec::new();
        for (slot, name) in Equipped::SLOTS {
            match held.get(slot) {
                Some(item) => filled.push(GearRow {
                    slot: name.to_uppercase(),
                    name: item.name.clone(),
                    level: item.level,
                    cosmetic: false,
                }),
                None => empty.push(GearRow {
                    slot: name.to_uppercase(),
                    name: "nothing equipped".to_string(),
                    level: None,
                    cosmetic: false,
                }),
            }
        }
        filled.sort_by_key(|row| row.level.unwrap_or(u16::MAX));

        let cosmetic: Vec<GearRow> = worn
            .iter()
            .filter(|item| item.is_cosmetic())
            .map(|item| GearRow {
                slot: item.slot_name.to_uppercase(),
                name: item.name.clone(),
                level: None,
                cosmetic: true,
            })
            .collect();

        empty.into_iter().chain(filled).chain(cosmetic).collect()
    }

    fn gear_row(row: &GearRow) -> gtk::Box {
        let line = almanac::row(11);
        line.set_baseline_position(gtk::BaselinePosition::Center);
        line.set_margin_top(7);
        line.set_margin_bottom(7);

        let slot = almanac::mono(&row.slot, &["al-footnote"]);
        slot.set_xalign(0.0);
        slot.set_valign(gtk::Align::Baseline);
        slot.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&slot);

        let name = almanac::label(&row.name, &[]);
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_valign(gtk::Align::Baseline);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if row.level.is_none() {
            name.add_css_class("al-unknown");
        }
        line.append(&name);

        let figure = match (row.level, row.cosmetic) {
            // Never a fabricated number. A shirt has no item level and saying
            // so is shorter than pretending otherwise.
            (None, true) => almanac::mono("NO ILVL", &["al-footnote"]),
            (None, false) => almanac::mono("—", &["al-stat-figure", "al-negative"]),
            (Some(level), _) => almanac::mono(&level.to_string(), &["al-stat-figure"]),
        };
        figure.set_valign(gtk::Align::Baseline);
        figure.set_halign(gtk::Align::End);
        line.append(&figure);
        line
    }

    /// The sentence under the gear list, when there is one worth saying.
    fn gear_note(detail: &Detail, rows: &[GearRow]) -> Option<String> {
        let empty = rows
            .iter()
            .filter(|row| !row.cosmetic && row.level.is_none())
            .count();
        match (empty, detail.item_level, detail.equipped_item_level) {
            (0, _, _) => None,
            (empty, Some(overall), Some(equipped)) if equipped < overall => Some(format!(
                "{} is why the equipped average is {equipped} and the overall is \
                 {overall}. An empty slot is drawn as an empty slot rather than \
                 folded into a number.",
                match empty {
                    1 => "The empty slot".to_string(),
                    empty => format!("{empty} empty slots"),
                }
            )),
            (empty, _, _) => Some(format!(
                "{}. An empty slot is drawn as an empty slot rather than folded \
                 into a number.",
                almanac::plural(empty, "slot is empty", "slots are empty")
            )),
        }
    }

    /// The lifetime counters, as a ruled list.
    fn record(held: &Held) -> gtk::Box {
        let card = almanac::card(9);
        let section = almanac::titled("THE RECORD", &card);

        let count = |kind: Counting| -> u64 {
            held.tallies
                .iter()
                .filter(|tally| tally.kind == kind)
                .map(|tally| tally.count)
                .sum()
        };

        let quests: usize = held.evenings.iter().map(|e| e.quests.len()).sum();
        let deaths = count(Counting::Killer);
        let yards = count(Counting::Distance);
        let gold: u64 = held
            .evenings
            .iter()
            .flat_map(|evening| evening.income.iter().map(|(_, amount)| *amount))
            .sum();

        let lines: [(&str, String, Tone); 6] = [
            (
                "Quests turned in",
                almanac::thousands(quests as u64),
                Tone::Plain,
            ),
            (
                "Bosses beaten",
                almanac::thousands(count(Counting::Victory)),
                Tone::Plain,
            ),
            ("Deaths", almanac::thousands(deaths), Tone::Negative),
            (
                "Distance covered",
                format!("{} miles", almanac::thousands(yards / 1760)),
                Tone::Plain,
            ),
            (
                "Flights taken",
                almanac::thousands(count(Counting::Flight)),
                Tone::Plain,
            ),
            (
                "Earned, all sources",
                format!("{}g", almanac::thousands(gold / 10_000)),
                Tone::Gold,
            ),
        ];

        for (index, (name, value, tone)) in lines.iter().enumerate() {
            if index > 0 {
                card.append(&almanac::hairline());
            }
            card.append(&almanac::stat_line(name, value, *tone));
        }

        if held.tallies.is_empty() {
            card.append(&almanac::caption(
                "These are the addon's counters. Nothing in the game or the API \
                 can give them back, so they start the day it is installed.",
            ));
        }
        section
    }

    /// Where the watched hours went.
    fn hours(held: &Held) -> gtk::Box {
        let card = almanac::card(11);
        let section = almanac::titled("WHERE THE HOURS WENT", &card);

        let mut zones: Vec<&Tally> = held
            .tallies
            .iter()
            .filter(|tally| tally.kind == Counting::Zone)
            .collect();
        zones.sort_by_key(|zone| std::cmp::Reverse(zone.count));

        if zones.is_empty() {
            card.append(&almanac::caption(
                "The addon records seconds per zone. Nothing yet.",
            ));
            return section;
        }

        let most = zones.first().map(|zone| zone.count).unwrap_or(1).max(1);

        for zone in zones.iter().take(ZONES_SHOWN) {
            let row = almanac::column(5);
            let head = almanac::row(8);
            let name = almanac::label(&zone.label, &["al-row-title"]);
            name.set_hexpand(true);
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            head.append(&name);
            let figure = almanac::mono(
                &format!("{}h", zone.count / 3600),
                &["al-stat-figure", "al-gold"],
            );
            figure.set_halign(gtk::Align::End);
            head.append(&figure);
            row.append(&head);

            let bar = Bar::new(6);
            bar.set_full(zone.count as f64 / most as f64, 0.0, Tone::Gold, 0);
            row.append(&bar.widget);
            card.append(&row);
        }

        if zones.len() > ZONES_SHOWN {
            let rest: u64 = zones.iter().skip(ZONES_SHOWN).map(|zone| zone.count).sum();
            card.append(&almanac::caption(&format!(
                "and {} hours across {}",
                rest / 3600,
                almanac::plural(zones.len() - ZONES_SHOWN, "other place", "other places")
            )));
        }
        section
    }

    // -- keys and raids -------------------------------------------------------

    /// Every keystone this character has finished, newest first.
    fn keystones(held: &Held) -> Vec<(DateTime<Utc>, &crate::model::chronicle::Keystone)> {
        held.evenings
            .iter()
            .flat_map(|evening| {
                evening
                    .keystones
                    .iter()
                    .map(move |key| (evening.started_at, key))
            })
            .collect()
    }

    fn keys(held: &Held) -> gtk::Box {
        let card = almanac::card(9);
        let section = almanac::titled("THIS SEASON'S KEYS", &card);

        let runs = Self::keystones(held);
        if runs.is_empty() {
            card.append(&almanac::caption(
                "No keystone finished on this character while the addon was \
                 watching. The rating above comes from Blizzard and covers the \
                 whole season; these are the runs Armory saw.",
            ));
        }

        for (index, (_, key)) in runs.iter().take(KEYS_SHOWN).enumerate() {
            if index > 0 {
                card.append(&almanac::hairline());
            }
            let line = almanac::row(9);
            line.set_baseline_position(gtk::BaselinePosition::Center);

            let name = almanac::label(&key.dungeon, &[]);
            name.set_hexpand(true);
            name.set_xalign(0.0);
            name.set_valign(gtk::Align::Baseline);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            line.append(&name);

            let level = almanac::mono(&format!("+{}", key.level), &["al-stat-figure", "al-gold"]);
            level.set_valign(gtk::Align::Baseline);
            line.append(&level);

            // An untimed key is a real outcome rather than a failure to hide,
            // so it is stated in the same place the timed ones are.
            let outcome = if key.in_time {
                almanac::mono("TIMED", &["al-footnote"])
            } else {
                almanac::mono("OVER", &["al-footnote", "al-negative"])
            };
            outcome.set_valign(gtk::Align::Baseline);
            line.append(&outcome);
            card.append(&line);
        }

        if !runs.is_empty() {
            card.append(&almanac::hairline());
            let best = runs.iter().map(|(_, key)| key.level).max().unwrap_or(0);
            card.append(&almanac::stat_line(
                &format!("{} recorded", almanac::plural(runs.len(), "key", "keys")),
                &format!("BEST +{best}"),
                Tone::Gold,
            ));
        }

        // This sentence ships. It is the comment in `model/character.rs` said
        // out loud, and it is the answer to the question this card raises.
        card.append(&almanac::caption(
            "There is no Great Vault endpoint — the weekly frame is client-side \
             state. Armory shows the runs that feed a slot and not the slot itself.",
        ));
        section
    }

    fn raids(detail: &Detail) -> gtk::Box {
        let column = almanac::column(11);
        let section = almanac::titled("RAIDS", &column);

        match &detail.raids {
            Some(tiers) if !tiers.is_empty() => {
                // Newest last in Blizzard's ordering, so the current tier is at
                // the end and the page reads the other way.
                for (index, tier) in tiers.iter().rev().take(RAIDS_SHOWN).enumerate() {
                    column.append(&Self::raid_card(tier, index == 0));
                }
            }
            _ => column.append(&Self::lockouts(detail)),
        }
        section
    }

    fn raid_card(tier: &RaidTier, current: bool) -> gtk::Box {
        let card = if current {
            almanac::earned_card(9)
        } else {
            almanac::card(9)
        };

        let head = almanac::row(8);
        let name = almanac::label(&tier.name, &["al-card-title"]);
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_wrap(true);
        head.append(&name);
        if current {
            head.append(&almanac::chip("CURRENT", Tone::Gold));
        }
        card.append(&head);

        for difficulty in &tier.difficulties {
            let row = almanac::row(9);
            row.set_baseline_position(gtk::BaselinePosition::Center);

            let label = almanac::mono(&difficulty.name.to_uppercase(), &["al-footnote"]);
            label.set_xalign(0.0);
            label.set_valign(gtk::Align::Baseline);
            row.append(&label);

            let bar = Bar::new(5);
            bar.set_full(
                f64::from(difficulty.defeated) / f64::from(difficulty.total.max(1)),
                0.0,
                Tone::Gold,
                0,
            );
            bar.widget.set_hexpand(true);
            row.append(&bar.widget);

            let count = almanac::mono(
                &format!("{}/{}", difficulty.defeated, difficulty.total),
                &["al-stat-figure", "al-gold"],
            );
            count.set_valign(gtk::Align::Baseline);
            row.append(&count);
            card.append(&row);
        }

        if let Some((boss, at, difficulty)) = tier.last_kill() {
            card.append(&almanac::caption(&format!(
                "Last kill {} — {boss}, {}.",
                at.with_timezone(&Local).format("%-d %B"),
                difficulty.to_lowercase()
            )));
        }
        card
    }

    /// What the client knows when the web API has said nothing.
    fn lockouts(detail: &Detail) -> gtk::Box {
        let card = almanac::card(9);
        let Some(locks) = &detail.raid_locks else {
            card.append(&almanac::caption(
                "No raid progress recorded. It arrives with a sync, or with the \
                 collector addon the next time this character logs out.",
            ));
            return card;
        };

        if locks.is_empty() {
            card.append(&almanac::caption(
                "Not saved to any raid this week. The lifetime record comes from \
                 Blizzard's API, which has not answered for this character.",
            ));
            return card;
        }

        for (index, lock) in locks.iter().enumerate() {
            if index > 0 {
                card.append(&almanac::hairline());
            }
            card.append(&Self::lock_row(lock));
        }
        // Said plainly, because these two facts look alike and are not.
        card.append(&almanac::caption(
            "This week's lockouts, from the game client — not a lifetime. The \
             client cannot see what it has ever killed and the API can, so this \
             is what an account with no Battle.net client has.",
        ));
        card
    }

    fn lock_row(lock: &RaidLock) -> gtk::Box {
        let row = almanac::row(9);
        row.set_baseline_position(gtk::BaselinePosition::Center);
        let name = almanac::label(&lock.name, &[]);
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_valign(gtk::Align::Baseline);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&name);
        let difficulty = almanac::mono(&lock.difficulty.to_uppercase(), &["al-footnote"]);
        difficulty.set_valign(gtk::Align::Baseline);
        row.append(&difficulty);
        let count = almanac::mono(
            &format!("{}/{}", lock.defeated, lock.total),
            &["al-stat-figure", "al-gold"],
        );
        count.set_valign(gtk::Align::Baseline);
        row.append(&count);
        row
    }

    // -- the rail -------------------------------------------------------------

    fn draw_rail(&self, rail: &gtk::Box, character: &Character, held: &Held) {
        rail.append(&Self::professions(&held.detail));
        rail.append(&Self::run_share(character, held));
        rail.append(&Self::weekdays(held));
        rail.append(&Self::people(held));

        rail.append(&almanac::hairline());
        rail.append(&almanac::caption(
            "Everything Blizzard reports here was true when this character last \
             logged out. The counters and the evenings come from the addon and \
             are true as of the last one it recorded.",
        ));
        if let Some(last) = held.detail.last_login {
            let stamp = almanac::mono(
                &format!(
                    "TRUE AT LOGOUT · {}",
                    last.with_timezone(&Local)
                        .format("%-d %b %Y %H:%M")
                        .to_string()
                        .to_uppercase()
                ),
                &["al-footnote"],
            );
            stamp.set_xalign(0.0);
            stamp.set_wrap(true);
            rail.append(&stamp);
        }
    }

    fn professions(detail: &Detail) -> gtk::Box {
        let card = almanac::card(7);
        let section = almanac::titled("SPEC & PROFESSIONS", &card);

        if let Some(spec) = &detail.spec {
            card.append(&almanac::stat_line("Specialisation", spec, Tone::Plain));
        }
        if detail.professions.is_empty() {
            card.append(&almanac::caption("No professions reported."));
            return section;
        }
        for profession in &detail.professions {
            card.append(&almanac::hairline());
            let value = match (profession.skill, profession.max_skill) {
                (Some(skill), Some(max)) => format!("{skill}/{max}"),
                (Some(skill), None) => skill.to_string(),
                _ => "—".to_string(),
            };
            card.append(&almanac::stat_line(&profession.name, &value, Tone::Plain));
            // The tier is the API's half and the addon has no way to know it,
            // so its absence is silence rather than a profession with no tier.
            let note = match &profession.tier {
                Some(tier) => tier.clone(),
                None => "NO TIER".to_string(),
            };
            let note = almanac::mono(&note.to_uppercase(), &["al-footnote"]);
            note.set_xalign(0.0);
            note.set_ellipsize(gtk::pango::EllipsizeMode::End);
            card.append(&note);
        }
        section
    }

    fn run_share(character: &Character, held: &Held) -> gtk::Box {
        let card = almanac::card(9);
        let section = almanac::titled("THEIR SHARE OF THE RUN", &card);

        let figure = almanac::mono(
            &held.share.credited.to_string(),
            &["al-figure-large", "al-gold"],
        );
        figure.set_xalign(0.0);
        card.append(&figure);
        card.append(&almanac::caption(&format!(
            "of {} closed",
            almanac::thousands(held.share.closed as u64)
        )));

        let bar = Bar::new(6);
        bar.set_full(
            held.share.credited as f64 / (held.share.closed.max(1)) as f64,
            0.0,
            Tone::Gold,
            0,
        );
        card.append(&bar.widget);

        // A floor, and said so. Most of a run is account-wide work nothing can
        // pin on one character, and sharing it out evenly would invent an
        // answer nobody measured.
        card.append(&almanac::caption(&match &held.share.runner_up {
            Some((name, count)) if *count > held.share.credited => format!(
                "{name} has more, with {count}. Only goals somebody attested to \
                 or that were measured against one character are credited at \
                 all — the rest is the account's."
            ),
            Some((name, count)) => format!(
                "The most of any character in the cohort. {name} is second with \
                 {count}. Only goals somebody attested to or that were measured \
                 against one character are credited at all."
            ),
            None => format!(
                "Only goals somebody attested to, or that were measured against \
                 {}, are credited at all — the rest is the account's.",
                character.display_name
            ),
        }));
        section
    }

    /// Seven bars, one a weekday.
    fn weekdays(held: &Held) -> gtk::Box {
        let card = almanac::card(9);
        let section = almanac::titled("WHEN THEY PLAY", &card);

        if held.evenings.is_empty() {
            card.append(&almanac::caption("No evenings recorded yet."));
            return section;
        }

        let mut days = [0usize; 7];
        let mut earliest: Option<u32> = None;
        for evening in &held.evenings {
            let local = evening.started_at.with_timezone(&Local);
            days[local.weekday().num_days_from_monday() as usize] += 1;
            earliest = Some(match earliest {
                Some(hour) => hour.min(local.hour()),
                None => local.hour(),
            });
        }
        let most = days.iter().copied().max().unwrap_or(1).max(1);
        let modal = days
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| **count)
            .map(|(index, _)| index)
            .unwrap_or(0);

        let strip = almanac::row(6);
        strip.set_homogeneous(true);
        for (index, count) in days.iter().enumerate() {
            let day = almanac::column(5);
            let bar = almanac::tally_bar(
                *count as f64 / most as f64,
                14,
                if index == modal {
                    Tone::Gold
                } else {
                    Tone::Plain
                },
            );
            day.append(&bar);
            let letter = almanac::mono(
                ["M", "T", "W", "T", "F", "S", "S"][index],
                if index == modal {
                    &["al-footnote", "al-gold"]
                } else {
                    &["al-footnote"]
                },
            );
            letter.set_halign(gtk::Align::Center);
            day.append(&letter);
            strip.append(&day);
        }
        card.append(&strip);

        const NAMES: [&str; 7] = [
            "Mondays",
            "Tuesdays",
            "Wednesdays",
            "Thursdays",
            "Fridays",
            "Saturdays",
            "Sundays",
        ];
        card.append(&almanac::caption(&match earliest {
            Some(hour) => format!(
                "{}, mostly, and never before {}.",
                NAMES[modal],
                clock(hour)
            ),
            None => format!("{}, mostly.", NAMES[modal]),
        }));
        section
    }

    /// Who they play with, and who sends them out.
    fn people(held: &Held) -> gtk::Box {
        let card = almanac::card(7);
        let section = almanac::titled("THEIR PEOPLE", &card);

        let listed = |kind: Counting| -> Vec<&Tally> {
            let mut rows: Vec<&Tally> = held
                .tallies
                .iter()
                .filter(|tally| tally.kind == kind)
                .collect();
            rows.sort_by_key(|row| std::cmp::Reverse(row.count));
            rows.truncate(PEOPLE_SHOWN);
            rows
        };

        let companions = listed(Counting::Companion);
        let questgivers = listed(Counting::Questgiver);
        if companions.is_empty() && questgivers.is_empty() {
            card.append(&almanac::caption(
                "Nobody recorded yet. The addon counts who is in the party and \
                 who hands over a quest; nothing else does.",
            ));
            return section;
        }

        for tally in &companions {
            card.append(&almanac::stat_line(
                &tally.label,
                &almanac::plural(tally.count as usize, "evening", "evenings"),
                Tone::Plain,
            ));
        }
        if !companions.is_empty() && !questgivers.is_empty() {
            card.append(&almanac::hairline());
        }
        for tally in &questgivers {
            card.append(&almanac::stat_line(
                &tally.label,
                &almanac::plural(tally.count as usize, "quest", "quests"),
                Tone::Plain,
            ));
        }
        section
    }
}

/// An hour of the day, as somebody says it rather than as a number.
fn clock(hour: u32) -> String {
    match hour {
        0 => "midnight".to_string(),
        12 => "midday".to_string(),
        hour if hour < 12 => format!("{hour}am"),
        hour => format!("{}pm", hour - 12),
    }
}

/// One entry on the history spine.
struct Moment {
    at: DateTime<Utc>,
    title: String,
    detail: String,
}

/// One row of the gear list.
struct GearRow {
    slot: String,
    name: String,
    level: Option<u16>,
    /// A shirt or a tabard: worn, and not gear.
    cosmetic: bool,
}
