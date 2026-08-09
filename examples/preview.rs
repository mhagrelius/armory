//! Render the real widget tree to a PNG.
//!
//! Screenshotting a live GNOME Wayland session needs interactive consent, which
//! makes "does this look right?" hard to answer while iterating. This builds the
//! actual pages against made-up data and paints them offscreen instead, so a
//! design change can be looked at in one command.
//!
//! The states worth a picture are the ones that are awkward to reach on demand:
//! a first run before anything is registered, a roster nobody has enrolled from,
//! and a run part-way through with goals in all three buckets.
//!
//! Pass a copy of a real store as a third argument and the collection and roster
//! are painted from it instead. Sample data cannot show what sixteen hundred
//! mounts do to a grid, and that is the layout most worth being sure about.
//! A *copy*, deliberately: opening the live database from a second process while
//! the application holds it is how a preview run corrupts somebody's data.
//!
//! Artwork is fetched for real — the render service is public and unauthenticated
//! — so this is the one thing in the tree outside `ui/http.rs` that waits on the
//! network. It is an example rather than a test, and a picture of a grid with no
//! pictures in it would be worth nothing.
//!
//! ```sh
//! cargo run --example preview -- /tmp/preview
//! cargo run --example preview -- /tmp/preview dark
//! cargo run --example preview -- /tmp/preview light /tmp/copy-of-armory.db
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use gtk::glib;

use armory::model::achievement::Evaluation;
use armory::model::character::{Character, CharacterKey, Detail, Faction, Profession, Roster};
use armory::model::chronicle::{Entry as JournalEntry, Happening, Moment, Session, SessionId};
use armory::model::cohort::Cohort;
use armory::model::market::{Crafting, Making, Unmeasured};
use armory::model::run::{Attestation, Baseline, Bucket, Exclusion, Goal, Run, Standing};
use armory::model::source::blizzard::collections::{Collectible, Kind, Source};
use armory::model::source::blizzard::gamedata::Achievement;
use armory::model::source::blizzard::profile::FactionStanding;
use armory::model::source::blizzard::{media, Region};
use armory::model::tally::{Counting, Tallies, Tally};
use armory::ui::{
    AchievementDialog, ChroniclePage, CollectibleDialog, CollectionPage, Images, MarketPage,
    Onboarding, Quote, ReputationsPage, RosterPage, RunPage, Warband,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "/tmp/preview".to_string());
    let dark = args.next().is_some_and(|scheme| scheme == "dark");
    let store = args.next().map(std::path::PathBuf::from);

    gtk::init().expect("a display — run under xvfb-run if there is none");
    adw::init().expect("libadwaita");

    // An animating widget is a widget that is not finished being laid out.
    // Turning animations off makes a snapshot deterministic rather than a race
    // against a transition.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }

    adw::StyleManager::default().set_color_scheme(if dark {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    });
    if let Some(display) = gtk::gdk::Display::default() {
        armory::ui::load_stylesheet(&display);
    }

    fs::create_dir_all(&out).expect("output directory");
    let suffix = if dark { "dark" } else { "light" };

    // A first run. This is the page that has to justify asking someone to
    // register their own API client, so it is the one most worth looking at.
    render(
        &Onboarding::new(),
        900,
        1180,
        &format!("{out}/onboarding-{suffix}.png"),
    );

    // Every character, nobody enrolled and no detail fetched yet — which is
    // what the page looks like between the first sync landing and the fan-out
    // finishing.
    let images = Images::new();

    let roster = sample_roster();
    let page = RosterPage::new(&images);
    // No addon: the Warband group explains itself rather than sitting empty.
    page.show(
        &roster,
        &Cohort::new(),
        &HashMap::new(),
        &HashMap::new(),
        &Warband::default(),
        Region::Us,
    );
    render(&page, 900, 900, &format!("{out}/roster-empty-{suffix}.png"));

    // Three enrolled, with their detail in. Only the enrolled characters are
    // fetched, so the unenrolled rows stay on the cheap summary — that contrast
    // is the thing worth looking at here.
    let (roster, cohort, details) = match &store {
        Some(path) => real_roster(path),
        None => (roster, sample_cohort(), sample_details()),
    };
    let page = RosterPage::new(&images);
    page.show(
        &roster,
        &cohort,
        &details,
        &HashMap::new(),
        &Warband {
            installed: true,
            bank_items: 412,
            currencies: 63,
            // All three answers, because the row exists to show the third one.
            earned_currencies: 41,
            transferred_currencies: 7,
            unclear_currencies: 15,
            written_at: Some(at("2026-08-02T21:14:00Z")),
        },
        Region::Us,
    );
    render_after(&page, 1000, 900, &format!("{out}/roster-{suffix}.png"), 4);

    // No run started.
    let page = RunPage::new(&images);
    page.show_no_run(3);
    render(&page, 900, 620, &format!("{out}/run-none-{suffix}.png"));

    // The run itself: the standing, the fortnight, last night and the road,
    // with the rail beside it. This is the home page and the one picture most
    // worth looking at.
    let page = RunPage::new(&images);
    page.set_context(armory::ui::run_page::Context {
        roster: sample_roster(),
        cohort: sample_cohort(),
        sessions: sample_sessions(),
    });
    page.show(&sample_run(), &sample_catalogue());
    render_after(&page, 1180, 800, &format!("{out}/run-{suffix}.png"), 4);

    // A run under way, with goals in all four buckets. One picture per bucket,
    // because each is now a page of its own and three of them would otherwise
    // never be looked at.
    for bucket in ["todo", "attest", "done", "spent"] {
        let page = RunPage::new(&images);
        page.set_context(armory::ui::run_page::Context {
            roster: sample_roster(),
            cohort: sample_cohort(),
            sessions: sample_sessions(),
        });
        page.show(&sample_run(), &sample_catalogue());
        page.show_bucket(bucket);
        render(&page, 900, 760, &format!("{out}/run-{bucket}-{suffix}.png"));
    }

    // The collection, which is the page the whole application is about for
    // anyone who collects.
    for kind in Kind::ALL {
        let (catalogue, owned) = match &store {
            Some(path) => real_collection(path, kind),
            // Sample data is only worth painting for the kind the layout was
            // designed against; the other two would be three copies of the
            // same eight rows.
            None if kind == Kind::Mount => sample_collection(),
            None => continue,
        };

        let page = CollectionPage::new(kind, &images);
        page.show(
            &catalogue,
            &owned,
            Faction::Alliance,
            Region::Us,
            &sample_attempts(),
            &sample_chances(),
        );
        if let Some(path) = &store {
            page.set_art(&real_art(path));
        }
        render_after(
            &page,
            1000,
            820,
            &format!("{out}/collection-{}-{suffix}.png", kind.singular()),
            6,
        );
    }

    // The collected half, which the page does not open on and so would never
    // otherwise be looked at. The ticks live here.
    {
        let (catalogue, owned) = match &store {
            Some(path) => real_collection(path, Kind::Mount),
            None => sample_collection(),
        };
        let page = CollectionPage::new(Kind::Mount, &images);
        page.show(
            &catalogue,
            &owned,
            Faction::Alliance,
            Region::Us,
            &sample_attempts(),
            &sample_chances(),
        );
        page.set_showing("collected");
        render_after(
            &page,
            1000,
            820,
            &format!("{out}/collection-collected-{suffix}.png"),
            6,
        );
    }

    // Reputations, with the distinction the other tools do not draw: a standing
    // The War Within handed to a character is shown and marked rather than
    // counted as theirs.
    let page = ReputationsPage::new();
    page.show(
        &roster,
        &sample_standings(&roster),
        &sample_provenance(&roster),
    );
    render(&page, 900, 820, &format!("{out}/reputations-{suffix}.png"));

    // Prices, with one item watched across two realms and the region.
    let page = MarketPage::new();
    page.show(
        &sample_quotes(),
        &[(61, "Emerald Dream".into()), (13, "Mannoroth".into())],
        Some(2_500_000_000),
        &sample_offers(),
        &sample_resale(),
        &Crafting {
            worth: sample_making(),
            unmeasured: Unmeasured {
                missing_reagent: 12,
                missing_output: 3,
            },
        },
    );
    render(&page, 900, 820, &format!("{out}/market-{suffix}.png"));

    // The browser, which is the other half of the same page: a whole realm's
    // commodity market rather than the handful somebody asked to watch.
    let page = MarketPage::new();
    page.show_market(0, &sample_listed(), &HashSet::new());
    page.show_browsing();
    render(
        &page,
        900,
        700,
        &format!("{out}/market-browse-{suffix}.png"),
    );

    // One item opened. The half of the browser that answers "should I bother",
    // and the only place the offer to start a history is made.
    let page = MarketPage::new();
    page.show_market(0, &sample_listed(), &HashSet::new());
    page.show_browsing();
    page.open_item(&sample_listed()[3]);
    render(&page, 900, 620, &format!("{out}/market-item-{suffix}.png"));

    // One character, as a body of work. Long on purpose: the main column is a
    // scroller and the only thing that has to be above the fold is the header
    // and the stat strip.
    let page = armory::ui::CharacterPage::new(&images);
    page.show(sample_life());
    render(&page, 1000, 1500, &format!("{out}/character-{suffix}.png"));

    // Zones: the corpus, and one place somebody has actually been.
    let page = armory::ui::ZonePage::new();
    let sessions = sample_sessions();
    page.show(&armory::ui::zone_page::Held {
        sessions: sessions.clone(),
        tallies: sample_tallies(&sessions),
        guide: Default::default(),
        items: Default::default(),
        market: Default::default(),
    });
    render(&page, 900, 760, &format!("{out}/zones-{suffix}.png"));

    // One place opened: the history, the dungeons in it, and the evenings.
    let page = armory::ui::ZonePage::new();
    page.show(&armory::ui::zone_page::Held {
        sessions: sessions.clone(),
        tallies: sample_tallies(&sessions),
        guide: Default::default(),
        items: Default::default(),
        market: Default::default(),
    });
    page.open_named("Nagrand");
    render(&page, 900, 900, &format!("{out}/zone-{suffix}.png"));

    // The journal, with one evening written up and one not. Both states in one
    // picture on purpose: an unwritten card is what most of this page will ever
    // be, and a preview showing only the pretty one would be a preview of a
    // feature nobody has paid for yet.
    let page = ChroniclePage::new();
    let sessions = sample_sessions();
    page.show(
        &sessions,
        &sample_entries(&sessions),
        &[],
        true,
        &sample_tallies(&sessions),
    );
    render(&page, 900, 3400, &format!("{out}/chronicle-{suffix}.png"));

    // One character's evenings, which is also the only way the lifetime
    // crafting tally is drawn — "everyone has made four hundred flasks" is
    // nobody's achievement, so it appears for one character or not at all.
    let page = ChroniclePage::new();
    let alone = std::slice::from_ref(&sessions[0]);
    page.show(
        alone,
        &sample_entries(alone),
        &[],
        true,
        &sample_tallies(alone),
    );
    render(
        &page,
        900,
        2600,
        &format!("{out}/chronicle-one-{suffix}.png"),
    );

    // The same page before there is a key, which is the first thing anybody
    // sees and the one that has to explain what this costs.
    let page = ChroniclePage::new();
    page.show(&[], &HashMap::new(), &[], false, &HashMap::new());
    render(
        &page,
        900,
        620,
        &format!("{out}/chronicle-empty-{suffix}.png"),
    );

    // The detail view: everything the in-game journals know about one entry.
    // Real ids, so the render that appears is the render that will appear. A
    // made-up display id resolves to whatever creature happens to hold it, and
    // the first version of this preview illustrated a skeletal warhorse with a
    // picture of a human woman.
    let detail = Collectible {
        kind: armory::model::source::blizzard::collections::Kind::Mount,
        id: 69,
        name: "Rivendare's Deathcharger".into(),
        source: Source::Drop,
        description: Some("Drop: Lord Aurius Rivendare\nLocation: Stratholme".into()),
        flavour: Some(
            "When Baron Rivendare became a champion of the Scourge, he condemned \
             his favorite horse to join him in undeath."
                .into(),
        ),
        icon: Some(132264),
        display: Some(10718),
        faction: None,
        link_id: 17481,
        tradeable: None,
    };
    render_after(
        &CollectibleDialog::content(
            &detail,
            false,
            Some(&images),
            Some(&armory::model::source::blizzard::media::creature_render(
                Region::Us,
                10718,
            )),
        ),
        520,
        820,
        &format!("{out}/collectible-{suffix}.png"),
        4,
    );

    // A poisoned goal explaining itself. This is the page that has to justify
    // why something the account finished in 2016 is on a backlog.
    let poisoned = Goal {
        achievement_id: 4956,
        standing: Standing::Poisoned {
            by: Some(CharacterKey::new("mannoroth", "Aeltor")),
        },
        bucket: Bucket::Observable,
        attestation: None,
        nearest: Some(CharacterKey::new("mannoroth", "Aeltor")),
        evaluation: Some(Evaluation {
            progress: 47,
            required: 62,
            observable: true,
            inherited: false,
        }),
    };
    let loremaster = Achievement {
        id: 4956,
        name: "Loremaster of Kalimdor".into(),
        category: "Quests".into(),
        points: 50,
        description: "Complete the Kalimdor quest achievements listed below.".into(),
        is_unrepeatable: false,
    };
    render(
        &AchievementDialog::content(&poisoned, Some(&loremaster)),
        520,
        700,
        &format!("{out}/achievement-{suffix}.png"),
    );

    // The whole window, assembled. Every picture above is of a page with no
    // chrome around it, and a page that reads well on its own can still be
    // wrong once there is a sidebar taking two hundred pixels off it and a
    // header bar over the top.
    render_window(&out, suffix, store.as_deref());

    println!("wrote {suffix} previews to {out}");
}

/// Paint the real window, sidebar and header bar and all.
///
/// `AdwApplicationWindow` needs an application to belong to, but not a running
/// one: constructing it registers the window and is enough to lay it out.
fn render_window(out: &str, suffix: &str, store: Option<&std::path::Path>) {
    use armory::ui::ArmoryWindow;

    let application = adw::Application::builder()
        .application_id("com.hagrelius.Armory.Preview")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let images = Images::new();
    let window = ArmoryWindow::new(&application, &images);

    window.show_onboarding(false);

    let (roster, cohort, details) = match store {
        Some(path) => real_roster(path),
        None => (sample_roster(), sample_cohort(), sample_details()),
    };
    window.roster_page().show(
        &roster,
        &cohort,
        &details,
        &HashMap::new(),
        &Warband::default(),
        Region::Us,
    );
    window.run_page().show(&sample_run(), &sample_catalogue());
    let sessions = sample_sessions();
    window.chronicle_page().show(
        &sessions,
        &sample_entries(&sessions),
        &[],
        true,
        &sample_tallies(&sessions),
    );
    window.reputations_page().show(
        &roster,
        &sample_standings(&roster),
        &sample_provenance(&roster),
    );
    window.market_page().show(
        &sample_quotes(),
        &[(61, "Emerald Dream".into()), (13, "Mannoroth".into())],
        Some(2_500_000_000),
        &sample_offers(),
        &sample_resale(),
        &Crafting {
            worth: sample_making(),
            unmeasured: Unmeasured {
                missing_reagent: 12,
                missing_output: 3,
            },
        },
    );

    for kind in Kind::ALL {
        let (catalogue, owned) = match store {
            Some(path) => real_collection(path, kind),
            None if kind == Kind::Mount => sample_collection(),
            None => continue,
        };
        window.collection_page(kind).show(
            &catalogue,
            &owned,
            Faction::Alliance,
            Region::Us,
            &sample_attempts(),
            &sample_chances(),
        );
        window.set_tally(kind, owned.len(), catalogue.len());
    }

    // One picture per place, because the header bar and the sidebar's selection
    // change with each and those are the parts a page-only render cannot show.
    for place in [
        "run",
        "chronicle",
        "mounts",
        "decor",
        "roster",
        "reputations",
        "market",
    ] {
        window.open(place);
        window.set_default_size(1180, 800);
        window.present();
        settle();
        soak(6);
        snapshot(
            &window,
            window.width().max(1180),
            window.height().max(800),
            &format!("{out}/window-{place}-{suffix}.png"),
        );
    }
    // One character, pushed on top of the roster. The header bar is the point
    // of this one: there is one for the whole window, and the character's name
    // and a back button belong *in* it rather than in a second bar underneath.
    if let Some(character) = sample_life().character.clone() {
        window.open("roster");
        if let Some(page) = window.roster_page().character_page() {
            page.show(sample_life());
        }
        window.roster_page().open_character(&character);
        window.set_default_size(1180, 800);
        window.present();
        settle();
        soak(6);
        snapshot(
            &window,
            window.width().max(1180),
            window.height().max(800),
            &format!("{out}/window-character-{suffix}.png"),
        );
    }

    window.destroy();
}

/// A whole collection out of a real store.
fn real_collection(path: &std::path::Path, kind: Kind) -> (Vec<Collectible>, HashSet<u32>) {
    let store = armory::model::store::Store::open(path).expect("a copy of a store");
    store.collectibles(kind).unwrap_or_default()
}

/// The icon URLs a real store has bodies for.
///
/// The same read `Application::restore_art` does at startup. Without it a
/// preview of the toys is a grid of placeholders whatever the application would
/// actually draw, because a toy's picture is the one thing here that is looked
/// up rather than constructed.
fn real_art(path: &std::path::Path) -> HashMap<u32, String> {
    let store = armory::model::store::Store::open(path).expect("a copy of a store");
    let bodies = store
        .responses_matching(media::ITEM_MEDIA, chrono::Duration::days(30))
        .unwrap_or_default();

    bodies
        .into_iter()
        .filter_map(|(url, body)| {
            let id = media::media_id(&url, media::ITEM_MEDIA)?;
            match media::parse_icon(&body) {
                armory::model::source::Outcome::Found(url) => Some((id, url)),
                _ => None,
            }
        })
        .collect()
}

/// A real roster, with everyone enrolled who is enrolled.
fn real_roster(path: &std::path::Path) -> (Roster, Cohort, HashMap<CharacterKey, Detail>) {
    let store = armory::model::store::Store::open(path).expect("a copy of a store");
    let roster = store.roster().unwrap_or_default();
    let mut cohort = store.cohort().unwrap_or_default();
    cohort.prune(&roster);
    (roster, cohort, store.details().unwrap_or_default())
}

fn character(realm: &str, name: &str, level: u8, race: &str, class: &str) -> Character {
    Character {
        key: CharacterKey::new(armory::model::source::blizzard::realm_slug(realm), name),
        id: 1,
        realm_id: 2,
        display_name: name.to_string(),
        realm_name: realm.to_string(),
        level,
        class: class.to_string(),
        race: race.to_string(),
        faction: Faction::Horde,
        wow_account_id: 1,
    }
}

fn sample_roster() -> Roster {
    Roster::new(vec![
        character("Emerald Dream", "Somechar", 80, "Tauren", "Druid"),
        character("Emerald Dream", "Atulak", 80, "Orc", "Shaman"),
        character("Emerald Dream", "Velkurai", 71, "Troll", "Mage"),
        character("Mannoroth", "Aeltor", 80, "Orc", "Warrior"),
        character("Mannoroth", "Silentbeef", 62, "Tauren", "Warrior"),
        character("Dalaran", "Moodivh", 80, "Tauren", "Priest"),
        character("Thrall", "Ulahae", 45, "Undead", "Warlock"),
    ])
}

fn sample_cohort() -> Cohort {
    Cohort::from(vec![
        CharacterKey::new("emerald-dream", "Somechar"),
        CharacterKey::new("emerald-dream", "Velkurai"),
        CharacterKey::new("thrall", "Ulahae"),
    ])
}

fn sample_collection() -> (
    Vec<armory::model::source::blizzard::collections::Collectible>,
    HashSet<u32>,
) {
    use armory::model::source::blizzard::collections::{Collectible, Kind, Source};

    // Real boss names, because the journal's sentence is what a drop is joined
    // to the account's own pull count by — see `tally::attempts_at`.
    let entries = [
        (
            6,
            "Reins of the Onyxian Drake",
            Source::Drop,
            "Onyxia, Onyxia's Lair",
        ),
        (
            7,
            "Swift Zulian Tiger",
            Source::Drop,
            "High Priest Thekal, Zul'Gurub",
        ),
        (
            8,
            "Reins of the Grand Black War Mammoth",
            Source::Vendor,
            "Sold by Mei Francis, Dalaran",
        ),
        (
            9,
            "Ashes of Al'ar",
            Source::Drop,
            "Kael'thas Sunstrider, Tempest Keep",
        ),
        (10, "Mimiron's Head", Source::Drop, "Yogg-Saron, Ulduar"),
        (
            11,
            "Invincible's Reins",
            Source::Drop,
            "The Lich King, Icecrown Citadel",
        ),
        (12, "Reins of the Raven Lord", Source::Unknown, ""),
        (13, "Fiery Warhorse's Reins", Source::Unknown, ""),
    ];

    let catalogue: Vec<Collectible> = entries
        .into_iter()
        .map(|(id, name, source, whence)| Collectible {
            kind: Kind::Mount,
            id,
            name: name.into(),
            source,
            // What the in-game journal gives and the web API does not.
            description: (!whence.is_empty()).then(|| format!("{}: {whence}", source.label())),
            flavour: None,
            icon: None,
            display: None,
            faction: None,
            link_id: id * 100,
            tradeable: None,
        })
        .collect();

    (catalogue, HashSet::from([6, 8]))
}

fn sample_quotes() -> Vec<Quote> {
    let at = at("2026-08-03T09:00:00Z");
    let history = |prices: &[u64]| {
        prices
            .iter()
            .enumerate()
            .map(|(index, price)| (at + chrono::Duration::hours(index as i64), *price, 240))
            .collect::<Vec<_>>()
    };

    vec![
        Quote {
            item_id: 197794,
            name: "Mycobloom".into(),
            realm: 0,
            realm_name: "Region-wide".into(),
            history: history(&[54_000, 56_500, 61_200]),
        },
        Quote {
            item_id: 210796,
            name: "Crystalline Powder".into(),
            realm: 61,
            realm_name: "Emerald Dream".into(),
            history: history(&[1_240_000, 1_180_000]),
        },
        Quote {
            item_id: 210796,
            name: "Crystalline Powder".into(),
            realm: 13,
            realm_name: "Mannoroth".into(),
            history: history(&[980_000]),
        },
    ]
}

/// Missing collectibles that happen to be on sale. The join is the reason the
/// Market page is worth opening for somebody who does not play the auction
/// house at all, so it is worth a picture.
fn sample_offers() -> Vec<armory::model::market::Offer> {
    [
        ("Nether Faerie Dragon", 61u32, 1_250_000u64, 3u32),
        ("Sprite Darter Hatchling", 61, 4_800_000, 1),
        ("Tiny Crimson Whelpling", 13, 9_100_000, 2),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (name, realm, price, quantity))| armory::model::market::Offer {
            kind: Kind::Pet,
            collectible_id: index as u32 + 1,
            name: name.into(),
            realm,
            unit_price: price,
            quantity,
        },
    )
    .collect()
}

/// Spare pets worth selling, including the two cases the group exists to show:
/// one whose value is all in the rare version, and one nothing has moved at.
fn sample_resale() -> Vec<armory::model::market::Resale> {
    [
        // name, spare, realm, floor, ceiling, sold
        (
            "Nether Faerie Dragon",
            2u32,
            "Tichondrius",
            8_400_000u64,
            8_400_000u64,
            14u32,
        ),
        (
            "Sprite Darter Hatchling",
            1,
            "Emerald Dream",
            900_000,
            22_000_000,
            6,
        ),
        (
            "Tiny Crimson Whelpling",
            4,
            "Emerald Dream",
            310_000,
            310_000,
            0,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (name, spare, realm, floor, ceiling, sold))| armory::model::market::Resale {
            species: index as u32 + 1,
            name: name.into(),
            spare,
            realm: 61,
            realm_name: realm.into(),
            floor,
            ceiling,
            sold,
            samples: 180,
            span_hours: 216,
        },
    )
    .collect()
}

/// Two evenings: a full one, and a quiet one.
///
/// The quiet one is deliberate. Most sessions are, and a page that only ever
/// gets previewed with a two-hour raid night hides the layout question that
/// actually matters — what a card looks like when there is almost nothing in it.
fn sample_sessions() -> Vec<Session> {
    fn at(seconds: u32, what: Happening) -> Moment {
        Moment { at: seconds, what }
    }

    let busy = Session {
        character: CharacterKey::new("emerald-dream", "Somechar"),
        display_name: "Somechar".into(),
        realm_name: "Emerald Dream".into(),
        class: "Druid".into(),
        race: "Tauren".into(),
        faction: Faction::Horde,
        started_at: DateTime::parse_from_rfc3339("2026-08-03T19:04:00Z")
            .expect("a date")
            .to_utc(),
        ended_at: DateTime::parse_from_rfc3339("2026-08-03T21:38:00Z")
            .expect("a date")
            .to_utc(),
        start_level: 70,
        end_level: 71,
        start_money: 118_204_500,
        end_money: 118_108_500,
        start_item_level: 602,
        end_item_level: 606,
        moments: vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Orgrimmar".into(),
                    subzone: Some("The Drag".into()),
                    map: None,
                },
            ),
            at(
                420,
                Happening::Arrived {
                    zone: "Nagrand".into(),
                    subzone: Some("Halaa".into()),
                    map: None,
                },
            ),
            at(
                460,
                Happening::Accepted {
                    title: "Hero of the Mag'har".into(),
                    premise: Some(
                        "Garrosh has not left his tent since his father's name was spoken. \
                         Someone must go to him."
                            .into(),
                    ),
                },
            ),
            at(
                1_980,
                Happening::Completed {
                    quest: 9_923,
                    title: "Hero of the Mag'har".into(),
                    story: Some(
                        "You have given him back his father. Whatever comes of it now, \
                         the Mag'har will sing of this day."
                            .into(),
                    ),
                },
            ),
            at(
                1_980,
                Happening::Paid {
                    quest: 9_923,
                    money: 84_500,
                    experience: 12_400,
                },
            ),
            at(
                2_100,
                Happening::Levelled {
                    level: 71,
                    zone: "Nagrand".into(),
                },
            ),
            at(
                3_600,
                Happening::Fought {
                    name: "Durn the Hungerer".into(),
                    won: false,
                },
            ),
            at(
                3_900,
                Happening::Died {
                    zone: "Nagrand".into(),
                    subzone: Some("Halaa".into()),
                    to: Some("Durn the Hungerer".into()),
                },
            ),
            at(
                4_500,
                Happening::Felled {
                    name: "Durn the Hungerer".into(),
                },
            ),
            at(
                4_560,
                Happening::Looted {
                    item: 32_458,
                    name: "Collar of Cho'gall".into(),
                    quality: 4,
                },
            ),
            at(
                5_400,
                Happening::Acquired {
                    kind: armory::model::chronicle::Acquisition::Mount,
                    name: "Talbuk Doe".into(),
                },
            ),
            at(
                6_000,
                Happening::Sold {
                    subject: "Auction successful: Mycobloom".into(),
                    money: 3_745_800,
                },
            ),
            at(
                7_200,
                Happening::Alongside {
                    name: "Velkurai".into(),
                },
            ),
            at(
                1_100,
                Happening::Coin {
                    purpose: armory::model::chronicle::Purpose::Quest,
                    amount: 1_640_000,
                    incoming: true,
                },
            ),
            at(
                1_200,
                Happening::Coin {
                    purpose: armory::model::chronicle::Purpose::Loot,
                    amount: 812_000,
                    incoming: true,
                },
            ),
            at(
                4_400,
                Happening::Coin {
                    purpose: armory::model::chronicle::Purpose::Bid,
                    amount: 2_400_000,
                    incoming: false,
                },
            ),
            at(
                5_100,
                Happening::Coin {
                    purpose: armory::model::chronicle::Purpose::Repair,
                    amount: 148_000,
                    incoming: false,
                },
            ),
            at(
                5_300,
                Happening::Crafted {
                    recipe: 371_637,
                    name: "Flask of Alchemical Chaos".into(),
                },
            ),
            at(
                5_400,
                Happening::Crafted {
                    recipe: 371_637,
                    name: "Flask of Alchemical Chaos".into(),
                },
            ),
            at(
                400,
                Happening::Campaign {
                    name: "Hero of the Mag'har".into(),
                    summary: Some(
                        "Garrosh Hellscream leads the last uncorrupted orcs of Nagrand,                          and does not yet know what his father did."
                            .into(),
                    ),
                },
            ),
        ],
        kills: 214,
        risen: vec![("The Consortium".into(), 7)],
        travelled: 41_288,
        longest_fight: 664,
        worst_hit: 812_004,
        worst_hit_by: Some("Durn the Hungerer".into()),
        lowest_health: 7,
    };

    let quiet = Session {
        character: CharacterKey::new("mannoroth", "Aeltor"),
        display_name: "Aeltor".into(),
        realm_name: "Mannoroth".into(),
        class: "Warrior".into(),
        race: "Orc".into(),
        faction: Faction::Horde,
        started_at: DateTime::parse_from_rfc3339("2026-08-02T22:10:00Z")
            .expect("a date")
            .to_utc(),
        ended_at: DateTime::parse_from_rfc3339("2026-08-02T23:02:00Z")
            .expect("a date")
            .to_utc(),
        start_level: 80,
        end_level: 80,
        start_money: 44_120_000,
        end_money: 41_980_000,
        start_item_level: 641,
        end_item_level: 641,
        moments: vec![
            at(
                0,
                Happening::Arrived {
                    zone: "Dornogal".into(),
                    subzone: None,
                    map: None,
                },
            ),
            at(
                1_500,
                Happening::Arrived {
                    zone: "The Ringing Deeps".into(),
                    subzone: Some("Taelloch".into()),
                    map: None,
                },
            ),
            at(
                2_400,
                Happening::Completed {
                    quest: 82_311,
                    title: "A Weight Off My Chest".into(),
                    story: None,
                },
            ),
        ],
        kills: 18,
        risen: Vec::new(),
        travelled: 3_120,
        longest_fight: 0,
        worst_hit: 0,
        worst_hit_by: None,
        lowest_health: 100,
    };

    vec![busy, quiet]
}

/// A realm's commodity market, as the browser sees it.
fn sample_listed() -> Vec<armory::model::market::Listed> {
    [
        ("Mycobloom", 197_794u32, 37_400u64, 4_120u32, 96u32, 310u32),
        ("Algari Mana Potion", 211_880, 118_000, 1_880, 44, 94),
        ("Crystalline Powder", 210_930, 21_000, 12_400, 210, 880),
        ("Weavercloth", 208_766, 80_200, 940, 31, 0),
        ("Bismuth", 210_796, 9_400, 44_100, 512, 0),
    ]
    .into_iter()
    .map(
        |(name, item_id, cheapest, quantity, listings, sold)| armory::model::market::Listed {
            item_id,
            name: Some(name.to_string()),
            cheapest,
            quantity,
            listings,
            tenth: cheapest + cheapest / 8,
            median: cheapest + cheapest / 3,
            sold,
            span_hours: if sold > 0 { 216 } else { 0 },
        },
    )
    // One that has not been named yet, because that is what most of the market
    // looks like for the first few syncs and the page has to survive it.
    .chain(std::iter::once(armory::model::market::Listed {
        item_id: 219_873,
        name: None,
        cheapest: 500,
        quantity: 4,
        listings: 2,
        tenth: 500,
        median: 500,
        sold: 0,
        span_hours: 0,
    }))
    .collect()
}

/// Crafts worth making, and who should make them.
fn sample_making() -> Vec<Making> {
    let somechar = CharacterKey::new("emerald-dream", "Somechar");
    [
        (
            "Flask of Alchemical Chaos",
            371_637u32,
            4_200u64,
            121_400i64,
            118u32,
            1u32,
        ),
        ("Algari Mana Potion", 370_582, 1_180, 3_940, 2_204, 3),
        ("Sanctified Alchemist Stone", 391_012, 84_000, 219_600, 4, 1),
    ]
    .into_iter()
    .map(|(name, recipe, cost, margin, sold, makes)| Making {
        recipe,
        name: name.to_string(),
        by: somechar.clone(),
        by_name: "Somechar".into(),
        realm: 0,
        realm_name: "Region-wide".into(),
        makes,
        cost,
        each: (margin.max(0) as u64 + cost) * 100 / 95,
        revenue: margin.max(0) as u64 + cost,
        margin,
        sold,
        samples: 41,
        span_hours: 216,
        held: if recipe == 371_637 {
            vec![(210_797, 600)]
        } else {
            Vec::new()
        },
    })
    .collect()
}

/// A lifetime of counters, for the character whose evenings these are.
fn sample_tallies(sessions: &[Session]) -> Tallies {
    let Some(session) = sessions.first() else {
        return Tallies::new();
    };
    let counted = [
        (Counting::Recipe, "Flask of Alchemical Chaos", 412u64),
        (Counting::Recipe, "Algari Mana Potion", 188),
        (Counting::Recipe, "Sanctified Alchemist Stone", 9),
        (Counting::Companion, "Velkurai", 34),
        (Counting::Companion, "Tessuya", 11),
        (Counting::Victory, "Durn the Hungerer", 6),
        // Keyed by UiMapID, which is what the addon records and what the zone
        // corpus joins on — 107 is Outland's Nagrand, not Draenor's.
        (Counting::Zone, "107", 68_400),
        (Counting::Zone, "85", 9_120),
        (Counting::Killer, "Durn the Hungerer", 4),
        (Counting::Distance, "On foot", 412_880),
        (Counting::Flight, "Nagrand", 61),
        (Counting::Delve, "Tier 11", 40),
        (Counting::Delve, "Tier 8", 82),
    ];
    HashMap::from([(
        session.character.clone(),
        counted
            .into_iter()
            .map(|(kind, label, count)| Tally {
                kind,
                key: label.to_string(),
                label: if kind == Counting::Zone {
                    match label {
                        "107" => "Nagrand".to_string(),
                        "85" => "Orgrimmar".to_string(),
                        other => other.to_string(),
                    }
                } else {
                    label.to_string()
                },
                count,
            })
            .collect(),
    )])
}

/// One of the two evenings written up, the other not.
fn sample_entries(sessions: &[Session]) -> HashMap<SessionId, JournalEntry> {
    let Some(first) = sessions.first() else {
        return HashMap::new();
    };
    HashMap::from([(
        first.id(),
        JournalEntry {
            session: first.id(),
            title: "What the Mag'har Sing".into(),
            body: "I went to Halaa meaning only to clear the road, and came back with a \
                   name I will not put down again for a while.\n\nGarrosh would not look \
                   at me at first. He has his father's shoulders and none of his father's \
                   certainty, and when I told him what Hellscream had done at the end — \
                   not the drinking, the other thing, the thing the orcs of this world \
                   still owe him for — he stood there so long I thought I had broken \
                   him. The Mag'har will sing of it, someone said afterwards. Perhaps. \
                   They were singing before I reached the tent flap.\n\nDurn caught us in \
                   the open on the way back and I did not get up from it. Velkurai \
                   dragged me to the spirit healer and said nothing about it, which I \
                   appreciated more than I said. We went again an hour later and the \
                   gronn went down like a felled tree. The talbuk that has been following \
                   me since dusk seems to have decided the matter is settled."
                .into(),
            model: "qwen3-30b-a3b-instruct".into(),
            written_at: DateTime::parse_from_rfc3339("2026-08-04T08:12:00Z")
                .expect("a date")
                .to_utc(),
        },
    )])
}

/// What one character has personally earned, so the page can be looked at in
/// the state it exists for: an inherited standing that somebody is grinding
/// anyway.
fn sample_provenance(roster: &Roster) -> HashMap<CharacterKey, armory::model::provenance::Earned> {
    use armory::model::provenance::{Earned, EarnedReputation};

    // The second character, because that is the one `sample_standings` gives
    // inherited standings to. The two have to agree or the row the feature
    // exists for never appears.
    let Some(alt) = roster.characters.get(1) else {
        return HashMap::new();
    };

    let mut earned = Earned::default();
    // Halfway to Exalted by their own hand, with a faction the account maxed
    // out years ago. This is the row the whole feature is for: the standing
    // cannot move and the work is real.
    earned.reputation.insert(
        69,
        EarnedReputation {
            points: 12_000,
            renown: 0,
            renown_seen: 0,
            account_wide: true,
        },
    );
    // And a renown faction, counted in levels: nine of the account's twenty-
    // five earned here.
    earned.reputation.insert(
        2507,
        EarnedReputation {
            points: 4_200,
            renown: 9,
            renown_seen: 25,
            account_wide: true,
        },
    );
    HashMap::from([(alt.key.clone(), earned)])
}

/// Standings for the first few characters, including inherited ones — which is
/// the case the page exists to show and the one sample data usually omits.
fn sample_standings(roster: &Roster) -> HashMap<CharacterKey, Vec<FactionStanding>> {
    let factions = [
        (2570, "Dream Wardens", "Renown 20", 0u64, 0u64, 20u32),
        (2507, "Dragonscale Expedition", "Renown 25", 1400, 2500, 25),
        (69, "Darnassus", "Revered", 5000, 21000, 0),
        (
            2605,
            "The Assembly of the Deeps",
            "Renown 12",
            800,
            2500,
            12,
        ),
        (1090, "Kirin Tor", "Exalted", 0, 0, 0),
    ];

    roster
        .characters
        .iter()
        .take(3)
        .enumerate()
        .map(|(index, character)| {
            let standings = factions
                .iter()
                .map(|(id, name, tier, value, max, renown)| FactionStanding {
                    faction: *id,
                    name: (*name).into(),
                    tier: (*tier).into(),
                    value: *value,
                    max: *max,
                    renown: *renown,
                    // The second character is the fresh alt carrying somebody
                    // else's standings, which is the case worth looking at —
                    // classic reputations as well as renown, because the
                    // Warband syncs both.
                    inherited: index == 1,
                })
                .collect();
            (character.key.clone(), standings)
        })
        .collect()
}

fn profession(name: &str) -> Profession {
    Profession {
        name: name.to_string(),
        tier: Some(format!("Khaz Algar {name}")),
        skill: Some(84),
        max_skill: Some(100),
        is_primary: true,
        specialisations: vec![
            ("Potion Mastery".to_string(), true),
            ("Phial Mastery".to_string(), false),
        ],
        knowledge: 412,
    }
}

/// Detail for the enrolled three only. The fan-out fetches nobody else, so the
/// unenrolled rows have nothing to show and must not look broken for it.
fn sample_details() -> HashMap<CharacterKey, Detail> {
    HashMap::from([
        (
            CharacterKey::new("emerald-dream", "Somechar"),
            Detail {
                item_level: Some(642),
                equipped_item_level: Some(639),
                spec: Some("Restoration".into()),
                guild: Some("Dream Team".into()),
                money: Some(91_234_560_000),
                mythic_rating: Some(2418),
                professions: vec![profession("Alchemy"), profession("Herbalism")],
                ..Detail::default()
            },
        ),
        (
            CharacterKey::new("emerald-dream", "Velkurai"),
            Detail {
                item_level: Some(571),
                spec: Some("Frost".into()),
                money: Some(4_821_000_000),
                professions: vec![profession("Tailoring")],
                ..Detail::default()
            },
        ),
        (
            CharacterKey::new("thrall", "Ulahae"),
            Detail {
                item_level: Some(112),
                spec: Some("Affliction".into()),
                money: Some(123_000_000),
                ..Detail::default()
            },
        ),
    ])
}

/// One character with everything filled in, including the two facts that
/// arrive from different sources.
fn sample_life() -> armory::ui::character_page::Held {
    use armory::model::character::{Equipped, RaidDifficulty, RaidTier};
    use armory::model::tally::{Counting, Tally};

    let key = CharacterKey::new("emerald-dream", "Somechar");
    let mut detail = sample_details().remove(&key).expect("Somechar");
    detail.achievement_points = Some(28_940);
    detail.last_login = Some(at("2026-08-05T01:22:00Z"));
    detail.equipment = Some(
        [
            ("NECK", "Amulet of Earthen Binding", Some(627)),
            ("WRIST", "Coilfang Cuffs", Some(636)),
            ("FEET", "Treads of the Mag'har", Some(639)),
            ("WAIST", "Girdle of the Windrider", Some(639)),
            ("FINGER_1", "Band of Oshu'gun", Some(642)),
            ("HANDS", "Grips of Distant Thunder", Some(642)),
            ("BACK", "Drape of the Kurenai", Some(645)),
            ("TRINKET_1", "Spiritcaller's Totem", Some(645)),
            ("LEGS", "Leggings of the Broken", Some(645)),
            ("FINGER_2", "Signet of the Warsong", Some(649)),
            ("HEAD", "Crown of the Dreamer", Some(649)),
            ("SHOULDER", "Mantle of Deep Roots", Some(649)),
            ("CHEST", "Robes of the Emerald Wake", Some(652)),
            ("TRINKET_2", "Ephemeral Bloom", Some(652)),
            ("MAIN_HAND", "Staff of the Wild Heart", Some(658)),
            ("TABARD", "Tabard of the Kurenai", None),
        ]
        .into_iter()
        .map(|(slot, name, level)| Equipped {
            slot: slot.into(),
            slot_name: Equipped::SLOTS
                .iter()
                .find(|(key, _)| *key == slot)
                .map(|(_, name)| (*name).to_string())
                .unwrap_or_else(|| "Tabard".into()),
            name: name.into(),
            level,
        })
        .collect(),
    );
    detail.raids = Some(vec![
        RaidTier {
            name: "Nerub-ar Palace".into(),
            expansion: "The War Within".into(),
            difficulties: vec![RaidDifficulty {
                name: "Heroic".into(),
                defeated: 8,
                total: 8,
                last_kill: Some(("Queen Ansurek".into(), at("2026-02-11T22:10:00Z"))),
            }],
        },
        RaidTier {
            name: "Liberation of Undermine".into(),
            expansion: "The War Within".into(),
            difficulties: vec![
                RaidDifficulty {
                    name: "Normal".into(),
                    defeated: 8,
                    total: 8,
                    last_kill: Some(("Mug'Zee".into(), at("2026-07-30T22:41:00Z"))),
                },
                RaidDifficulty {
                    name: "Heroic".into(),
                    defeated: 2,
                    total: 8,
                    last_kill: Some(("Vexie".into(), at("2026-07-30T21:02:00Z"))),
                },
            ],
        },
    ]);

    let tally = |kind: Counting, key: &str, label: &str, count: u64| Tally {
        kind,
        key: key.into(),
        label: label.into(),
        count,
    };

    armory::ui::character_page::Held {
        character: sample_roster()
            .characters
            .iter()
            .find(|character| character.key == key)
            .cloned(),
        detail,
        portrait: None,
        evenings: sample_sessions()
            .iter()
            .filter(|session| session.character == key)
            .map(|session| session.digest())
            .collect(),
        tallies: vec![
            tally(Counting::Zone, "1970", "Zaralek Cavern", 61 * 3600),
            tally(Counting::Zone, "2022", "The Waking Shores", 38 * 3600),
            tally(Counting::Zone, "1527", "Uldum", 24 * 3600),
            tally(Counting::Zone, "84", "Stormwind City", 12 * 3600),
            tally(Counting::Victory, "boss", "Bosses", 418),
            tally(Counting::Killer, "Vexie", "Vexie", 61),
            tally(Counting::Distance, "walked", "Walked", 5_918_400),
            tally(Counting::Flight, "Valdrakken", "Valdrakken", 214),
            tally(Counting::Companion, "Bramblefoot", "Bramblefoot", 41),
            tally(Counting::Companion, "Sarrun", "Sarrun", 12),
            tally(Counting::Questgiver, "Khadgar", "Khadgar", 96),
        ],
        share: armory::ui::character_page::Share {
            credited: 184,
            closed: 418,
            runner_up: Some(("Bramblefoot".into(), 96)),
        },
        region: Region::Us,
    }
}

/// Pulls this account has made, as the addon counts them. Joined to a
/// collectible by the sentence its journal gives — see `tally::attempts_at`.
fn sample_attempts() -> Vec<armory::model::tally::Tally> {
    use armory::model::tally::{Counting, Tally};
    [
        ("Kael'thas Sunstrider", 58u64),
        ("The Lich King", 31),
        ("Yogg-Saron", 14),
    ]
    .into_iter()
    .map(|(label, count)| Tally {
        kind: Counting::Attempt,
        key: label.into(),
        label: label.into(),
        count,
    })
    .collect()
}

/// Drop chances as an installed Rarity would supply them. Keyed by the
/// collectible's `link_id`, which for a mount is its summoning spell.
fn sample_chances() -> armory::model::rarity::Chances {
    use armory::model::rarity::{Chance, Chances};
    Chances::from(
        [(900u32, 20u32), (1100, 100), (1000, 3000)]
            .into_iter()
            .map(|(spell, one_in)| Chance {
                name: String::new(),
                spell_id: Some(spell),
                creature_id: None,
                item_id: None,
                one_in,
            })
            .collect(),
    )
}

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("a timestamp")
        .to_utc()
}

/// Real achievement names, so the list reads the way it will in the app rather
/// than as a column of ids.
fn sample_catalogue() -> HashMap<u32, Achievement> {
    let names = [
        (100, "Loremaster of Kalimdor"),
        (101, "Explore Dustwallow Marsh"),
        (102, "The Deadmines"),
        (103, "Classic Dungeonmaster"),
        (104, "Ambassador of the Alliance"),
        (200, "Brewmaster"),
        (201, "To Honor One's Elders"),
        (202, "Fool For Love"),
        (203, "Hallowed Be Thy Name"),
        (204, "The Winter Veil Gourmet"),
    ];
    names
        .into_iter()
        .map(|(id, name)| {
            (
                id,
                Achievement {
                    id,
                    name: name.into(),
                    category: "Quests".into(),
                    points: 10,
                    description: String::new(),
                    is_unrepeatable: false,
                },
            )
        })
        .collect()
}

fn sample_run() -> Run {
    let poisoned = || Standing::Poisoned {
        by: Some(CharacterKey::new("mannoroth", "Aeltor")),
    };

    let mut goals = Vec::new();

    // Settled: earned since the baseline, so it belongs to the run outright.
    for id in 0..37 {
        goals.push(Goal {
            achievement_id: id,
            standing: Standing::EarnedDuringRun {
                at: at("2026-07-14T00:00:00Z"),
            },
            bucket: Bucket::Observable,
            attestation: None,
            nearest: None,
            evaluation: None,
        });
    }

    // Poisoned but observable, and part-way there.
    for id in 100..118 {
        goals.push(Goal {
            achievement_id: id,
            standing: poisoned(),
            bucket: Bucket::Observable,
            attestation: None,
            nearest: Some(CharacterKey::new("emerald-dream", "Somechar")),
            evaluation: Some(Evaluation {
                // Spread across the range, so the ranking has something to do.
                progress: 10 - u64::from(id % 10),
                required: 10,
                observable: true,
                inherited: false,
            }),
        });
    }

    // Poisoned, nothing measures it, and one has been marked done by hand.
    for id in 200..209 {
        goals.push(Goal {
            achievement_id: id,
            standing: poisoned(),
            bucket: Bucket::Attestable,
            attestation: (id == 200).then(|| Attestation {
                character: CharacterKey::new("emerald-dream", "Somechar"),
                at: at("2026-07-20T00:00:00Z"),
            }),
            nearest: None,
            evaluation: None,
        });
    }

    // Spent: outside the denominator entirely.
    for id in 300..314 {
        goals.push(Goal {
            achievement_id: id,
            standing: poisoned(),
            bucket: Bucket::Excluded(Exclusion::AlreadyOwned),
            attestation: None,
            nearest: None,
            evaluation: None,
        });
    }

    Run {
        name: "Fresh start".into(),
        baseline: Baseline {
            taken_at: at("2026-06-01T00:00:00Z"),
            collected: Vec::new(),
            completed: Vec::new(),
        },
        cohort: sample_cohort(),
        goals,
    }
}

/// Paint a page on its own.
///
/// No window decoration and no header bar: these are pictures of a layout, and
/// chrome around one reads as part of the design being reviewed.
fn render(widget: &impl IsA<gtk::Widget>, width: i32, height: i32, path: &str) {
    render_after(widget, width, height, path, 0);
}

/// Paint a page that has artwork to wait for.
///
/// `seconds` is how long the main loop turns before the snapshot is taken. A
/// grid that has just asked the render service for two hundred pictures has
/// none of them yet, and a picture of that is a picture of the placeholders.
fn render_after(widget: &impl IsA<gtk::Widget>, width: i32, height: i32, path: &str, seconds: u64) {
    // The height is a floor, not a promise. A page that needs more room gets it
    // rather than being cropped, which is how a layout that overflows would
    // otherwise look fine in the picture.
    for factor in [1, 2, 3] {
        if try_render(widget, width, height * factor, path, seconds) {
            return;
        }
    }
    eprintln!("{path}: nothing was drawn, even with room to spare");
}

fn try_render(
    widget: &impl IsA<gtk::Widget>,
    width: i32,
    height: i32,
    path: &str,
    seconds: u64,
) -> bool {
    let window = gtk::Window::builder()
        .default_width(width)
        .default_height(height)
        .child(widget)
        .build();
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.present();

    settle();
    if seconds > 0 {
        soak(seconds);
    }
    let drawn = snapshot(
        &window,
        window.width().max(width),
        window.height().max(height),
        path,
    );

    // Take the widget back before the window goes, so it can be painted again.
    window.set_child(gtk::Widget::NONE);
    window.destroy();
    drawn
}

/// Run the main loop until there is nothing left to lay out.
///
/// One drain is not enough: presenting a widget schedules work that schedules
/// more, so this pumps until it stops finding any, with a bound so a
/// misbehaving widget cannot hang the run.
fn settle() {
    let context = glib::MainContext::default();
    for _ in 0..100 {
        let mut worked = false;
        while context.iteration(false) {
            worked = true;
        }
        if !worked {
            break;
        }
    }
}

/// Keep the main loop turning for a while, so requests in flight can land.
///
/// [`settle`] returns the moment there is no work *ready*, which for a page
/// that has just asked for two hundred pictures is immediately. Blocking
/// iteration is what lets the replies arrive; the deadline is what stops a
/// service that is down from hanging the run.
fn soak(seconds: u64) {
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);

    // A timeout keeps the loop from blocking past the deadline when nothing at
    // all is pending.
    let tick = glib::timeout_add_local(std::time::Duration::from_millis(50), || {
        glib::ControlFlow::Continue
    });
    while std::time::Instant::now() < deadline {
        context.iteration(true);
    }
    tick.remove();
    settle();
}

/// Paint a realised window into a PNG. Reports whether anything was drawn.
fn snapshot(window: &impl IsA<gtk::Widget>, width: i32, height: i32, path: &str) -> bool {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));

    let Some(node) = snapshot.to_node() else {
        return false;
    };
    let renderer = gtk::gsk::CairoRenderer::new();
    renderer
        .realize(gtk::gdk::Surface::NONE)
        .expect("a renderer");
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).expect("write the png");
    renderer.unrealize();
    true
}
