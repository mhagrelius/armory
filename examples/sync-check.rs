//! Two machines against one server, driven by the real client code.
//!
//! **A sync that has never been contradicted has not been tested.** Everything
//! in `replica` is a pure function with unit tests, and everything in
//! `armory-server` has its own — but between them sit a wire format, a
//! transport, and the question of whether the two ends agree about what a row
//! is. One machine can never ask that question.
//!
//! So: two stores, one throwaway server, the real `ui::sync::Service`. No GTK
//! — a machine here is a `Store` and a `Service`, which is precisely what the
//! application holds. The transport is the shell's, so a change that breaks
//! the real client breaks this.
//!
//! Run it through `./sync-check.sh`, which starts the server.

use std::path::PathBuf;

use armory::model::addon::collector::Collected;
use armory::model::character::{Character, CharacterKey, Faction, Roster};
use armory::model::chronicle::Session;
use armory::model::replica;
use armory::model::store::Store;
use armory::model::tally::{Counting, Tally};
use armory::ui::Service;

const BATCH: usize = 500;

struct Machine {
    name: &'static str,
    store: Store,
    server: Service,
}

impl Machine {
    fn new(name: &'static str, directory: PathBuf, url: &str, token: &str) -> Machine {
        std::fs::create_dir_all(&directory).expect("a directory");
        let store = Store::open(&directory.join("armory.db")).expect("a store");
        store.set_machine(name).expect("named");
        Machine {
            name,
            server: Service::new(url, token, name, "sync-check").expect("a server"),
            store,
        }
    }

    fn pass(&mut self) -> replica::Report {
        replica::pass(&mut self.store, &self.server, BATCH)
            .unwrap_or_else(|error| panic!("{}: {error}", self.name))
    }
}

fn main() {
    let url = std::env::var("SYNC_CHECK_URL").expect("set SYNC_CHECK_URL");
    let token = std::env::var("SYNC_CHECK_TOKEN").expect("set SYNC_CHECK_TOKEN");
    let one = PathBuf::from(std::env::var("SYNC_CHECK_A").expect("set SYNC_CHECK_A"));
    let two = PathBuf::from(std::env::var("SYNC_CHECK_B").expect("set SYNC_CHECK_B"));

    let mut a = Machine::new("machine-a", one, &url, &token);
    let mut b = Machine::new("machine-b", two, &url, &token);

    let mut failures = 0;
    let mut check = |name: &str, held: bool| {
        if held {
            println!("  ok   {name}");
        } else {
            println!("  FAIL {name}");
            failures += 1;
        }
    };

    // -- an evening recorded on one machine reaches the other -----------------

    let evening = evening("Somechar", 1);
    a.store
        .save_sessions(std::slice::from_ref(&evening))
        .expect("saved");
    a.store.save_roster(&roster()).expect("saved");

    let sent = a.pass();
    check("a machine with something to say sends it", sent.sent > 0);

    let got = b.pass();
    check("and the other machine gets it", got.landed > 0);
    check(
        "an evening travels whole",
        b.store.sessions(10).expect("read").len() == 1,
    );
    check(
        "and so does the roster",
        b.store.roster().expect("read").characters.len() == 1,
    );

    // -- a quiet pass is quiet ------------------------------------------------

    // The failure this catches: a pass that is never empty means something is
    // re-uploading the account on a timer.
    check(
        "a pass with nothing to do does nothing",
        a.pass().is_empty(),
    );
    check("on both machines", b.pass().is_empty());

    // -- nothing bounces back --------------------------------------------------

    check(
        "what arrived is not sent straight back",
        b.store.queued().expect("read").is_empty(),
    );

    // -- the same evening again is not a second evening ------------------------

    b.store
        .save_sessions(std::slice::from_ref(&evening))
        .expect("saved");
    b.pass();
    a.pass();
    check(
        "an evening both machines saw is one evening",
        a.store.sessions(10).expect("read").len() == 1,
    );

    // -- a counter never goes backwards ----------------------------------------

    // The rule the lifetime counters exist under, across the wire this time.
    a.store.save_collected(&tallied(412)).expect("saved");
    a.pass();
    b.pass();

    b.store.save_collected(&tallied(1)).expect("saved");
    b.pass();
    a.pass();
    b.pass();

    let on_a = counted(&a.store);
    let on_b = counted(&b.store);
    check(
        "a machine that was behind cannot take a counter back",
        on_a == 412,
    );
    check("and both machines agree on the larger one", on_b == 412);

    // -- a deletion travels, and only when it is meant --------------------------

    a.store.watch_item(4306, "Silk Cloth").expect("watched");
    a.pass();
    b.pass();
    check(
        "a watch travels",
        b.store.watched().expect("read").len() == 1,
    );

    a.store.unwatch_item(4306).expect("unwatched");
    a.pass();
    b.pass();
    check(
        "and so does taking it off",
        b.store.watched().expect("read").is_empty(),
    );

    // -- a sweep is not a deletion ----------------------------------------------

    a.store
        .store_response("https://example.test/kept", b"body", None)
        .expect("stored");
    a.pass();
    b.pass();
    let before = b.store.machine();
    a.store.purge().expect("swept");
    a.pass();
    b.pass();
    check(
        "an expiry on one machine is not a deletion on the other",
        b.store
            .response("https://example.test/kept", chrono::Duration::days(30))
            .expect("read")
            .is_some(),
    );
    let _ = before;

    // -- a run, its goals, and the ids that mean nothing elsewhere ---------------

    a.store.save_run(None, &a_run()).expect("saved");
    a.pass();
    b.pass();
    let held = b.store.current_run().expect("read");
    check("a run travels", held.is_some());
    check(
        "and its goals come with it, hung off the right run",
        held.map(|(_, run)| run.goals.len()) == Some(1),
    );

    // -- and at the end of it the two machines hold the same thing ---------------

    a.pass();
    b.pass();
    a.pass();
    check(
        "the two machines end up holding the same account",
        same(&a.store, &b.store),
    );

    println!();
    if failures == 0 {
        println!("sync-check: all good.");
    } else {
        println!("sync-check: {failures} failed.");
        std::process::exit(1);
    }
}

/// Compare the two stores on everything a person would notice.
fn same(one: &Store, two: &Store) -> bool {
    one.sessions(500).ok() == two.sessions(500).ok()
        && one.roster().ok().map(|r| r.characters.len())
            == two.roster().ok().map(|r| r.characters.len())
        && one.watched().ok() == two.watched().ok()
        && one.tallies().ok() == two.tallies().ok()
        && one
            .current_run()
            .ok()
            .map(|run| run.map(|(_, run)| run.name))
            == two
                .current_run()
                .ok()
                .map(|run| run.map(|(_, run)| run.name))
}

fn who() -> CharacterKey {
    CharacterKey::new("emerald-dream", "Somechar")
}

fn roster() -> Roster {
    Roster::new(vec![Character {
        key: who(),
        id: 1,
        realm_id: 1,
        display_name: "Somechar".into(),
        realm_name: "Emerald Dream".into(),
        level: 80,
        class: "Shaman".into(),
        race: "Orc".into(),
        faction: Faction::Horde,
        wow_account_id: 7,
    }])
}

fn evening(name: &str, day: i64) -> Session {
    use armory::model::chronicle::{Happening, Moment};

    // A fixed moment rather than `now`, so a run is the same run twice: an
    // evening is keyed by when it started, and a clock-derived one would make
    // every run of this a new evening on a server somebody kept.
    let started_at =
        chrono::DateTime::from_timestamp(1_700_000_000 + day * 86_400, 0).expect("a moment");

    Session {
        character: CharacterKey::new("emerald-dream", name),
        display_name: name.to_string(),
        realm_name: "Emerald Dream".into(),
        class: "Shaman".into(),
        race: "Orc".into(),
        faction: Faction::Horde,
        started_at,
        ended_at: started_at + chrono::Duration::hours(3),
        start_level: 80,
        end_level: 80,
        start_money: 100,
        end_money: 200,
        start_item_level: 600,
        end_item_level: 600,
        moments: vec![Moment {
            at: 0,
            what: Happening::Arrived {
                zone: "Nagrand".into(),
                subzone: None,
                map: None,
            },
        }],
        risen: Vec::new(),
        travelled: 0,
        longest_fight: 0,
    }
}

fn tallied(count: u64) -> Collected {
    let mut collected = Collected::default();
    collected.tallies.insert(
        who(),
        vec![Tally {
            kind: Counting::Recipe,
            key: "371637".into(),
            label: "Flask of Alchemical Chaos".into(),
            count,
        }],
    );
    collected
}

fn counted(store: &Store) -> u64 {
    store
        .tallies()
        .expect("read")
        .get(&who())
        .and_then(|counted| counted.iter().find(|tally| tally.key == "371637"))
        .map(|tally| tally.count)
        .unwrap_or(0)
}

fn a_run() -> armory::model::run::Run {
    use armory::model::run::{Baseline, Bucket, Goal, Run, Standing};
    Run {
        name: "The Second Time".into(),
        baseline: Baseline {
            taken_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("a moment"),
            collected: vec![],
            completed: vec![],
        },
        cohort: armory::model::cohort::Cohort::from(vec![who()]),
        goals: vec![Goal {
            achievement_id: 1234,
            standing: Standing::Unearned,
            bucket: Bucket::Observable,
            attestation: None,
            nearest: None,
            evaluation: None,
        }],
    }
}
