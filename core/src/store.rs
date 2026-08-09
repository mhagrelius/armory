//! Local storage: the roster, the cohort, and the response cache.
//!
//! One of the two places under `model/` that is not a pure function. It touches
//! a SQLite file and nothing else — no network, no GTK — so it is tested against
//! real storage in a temporary directory rather than behind a fake.
//!
//! SQLite rather than the JSON file the sibling applications rewrite whole,
//! because that pattern does not survive this workload. One sync of a
//! twenty-three character account writes tens of thousands of criteria rows, and
//! the expiry below is a `DELETE ... WHERE` rather than a rewrite of everything
//! the application knows.
//!
//! That expiry is not an optimisation. Blizzard's API terms require a maximum
//! 30-day time-to-live on data obtained through the API, so [`Store::purge`] is
//! an obligation with a deadline, and it runs at startup rather than when
//! something happens to notice.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection, OptionalExtension};

use super::achievement::CriterionKind;
use super::addon::collector::Collected;
use super::adventure::{Encounter, Guide, Instance};
use super::character::{Character, CharacterKey, Detail, Faction, Roster};
use super::chronicle::{Entry, Session, SessionId};
use super::cohort::Cohort;
use super::market::{Listed, Reagent, Recipe, RecipeBooks, Sample, Series};
use super::provenance::{EarnedCurrency, EarnedReputation, Provenance};
use super::run::{Goal, Run};
use super::source::blizzard::auctions::Depth;
use super::source::blizzard::collections::{Collectible, Kind};
use super::source::blizzard::gamedata::Achievement;
use super::source::blizzard::gamedata::Item;
use super::sync;
use super::tally::{Counting, Tallies, Tally};

/// How long anything from Blizzard may be kept. Set by their terms, not by us.
pub const MAX_TTL_DAYS: i64 = 30;

/// Every price series recorded for one item on one realm, keyed by variant.
///
/// For item 82800 that is one series per pet species and quality, which is the
/// only way to ask a caged pet's price: it has no item id of its own.
pub type PriceSeries = HashMap<String, Vec<(DateTime<Utc>, u64, u32)>>;

/// A list of ids as one column, and back again.
///
/// Joined rather than given a table of their own: an encounter's loot is read
/// whole every single time and never queried by item, so a join table would be
/// two indexes and a query for something a `split` already answers.
fn joined(ids: &[u32]) -> String {
    ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn split(text: &str) -> Vec<u32> {
    text.split(',').filter_map(|id| id.parse().ok()).collect()
}

/// Write a whole set of rows, and remove whatever is no longer among them.
///
/// The shape every table that is *replaced* rather than merged now takes, and
/// the reason is the change log rather than performance. `DELETE FROM
/// criterion` followed by fifty thousand inserts leaves the table exactly as
/// it was and tells the log that fifty thousand rows moved; one addon read
/// would then queue the whole account to be sent to every other machine,
/// saying nothing. An upsert whose values match writes nothing and the
/// triggers stay quiet, so a repeat write costs one statement a row and
/// enqueues none of them.
///
/// `within` narrows both halves to one realm, one recipe, one kind — a
/// snapshot replaces a realm and must not delete the other realms while it is
/// at it.
///
/// The keys are gathered into a temporary table rather than an `IN (?, ?, …)`
/// list because the lists here run to tens of thousands and SQLite's variable
/// ceiling is a few hundred; a list that long fails at the point the account
/// gets large, which is the point nobody is testing.
fn reconcile(
    connection: &Connection,
    table: &str,
    keys: &[&str],
    values: &[&str],
    within: Option<(&str, Vec<SqlValue>)>,
    rows: &[Vec<SqlValue>],
) -> Result<()> {
    let names: Vec<&str> = keys.iter().chain(values.iter()).copied().collect();
    let placeholders = (1..=names.len())
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let conflict = if values.is_empty() {
        "DO NOTHING".to_string()
    } else {
        format!(
            "DO UPDATE SET {}",
            values
                .iter()
                .map(|column| format!("{column} = excluded.{column}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let insert = format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders}) ON CONFLICT ({}) {conflict}",
        names.join(", "),
        keys.join(", ")
    );

    {
        let mut statement = connection.prepare(&insert)?;
        for row in rows {
            statement.execute(rusqlite::params_from_iter(row.iter()))?;
        }
    }

    // A temporary table per call rather than one kept around: these run
    // inside a transaction and two of them can be open at once.
    let columns = keys
        .iter()
        .enumerate()
        .map(|(index, _)| format!("k{index}"))
        .collect::<Vec<_>>();
    connection.execute_batch(&format!(
        "DROP TABLE IF EXISTS temp.keeping;
         CREATE TEMP TABLE keeping ({});",
        columns.join(", ")
    ))?;
    {
        let mut statement = connection.prepare(&format!(
            "INSERT INTO temp.keeping VALUES ({})",
            (1..=keys.len())
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))?;
        for row in rows {
            statement.execute(rusqlite::params_from_iter(row.iter().take(keys.len())))?;
        }
    }

    let (scope, bound) = match within {
        Some((clause, values)) => (format!("{clause} AND "), values),
        None => (String::new(), Vec::new()),
    };
    let delete = format!(
        "DELETE FROM {table} WHERE {scope}({}) NOT IN (SELECT {} FROM temp.keeping)",
        keys.join(", "),
        columns.join(", ")
    );
    connection.execute(&delete, rusqlite::params_from_iter(bound.iter()))?;
    connection.execute_batch("DROP TABLE IF EXISTS temp.keeping;")?;
    Ok(())
}

/// What a run is called between machines.
///
/// `run.id` is an `AUTOINCREMENT` and two machines will pick different ones
/// for the same run, so a goal could not name its run on the wire and a
/// pulled run would arrive as a second one. Every other table here is keyed by
/// something the game already agreed on — a realm and a name, an achievement
/// id, an item id — and this is the one that had to be given a key.
///
/// Derived rather than random, from the moment the baseline was taken, so that
/// the same run saved again is the same run and a replan does not rename it.
/// Two runs started in the same second under the same name would collide; they
/// would also be the same run by every measure that matters here.
fn run_key(run: &Run) -> String {
    format!("run-{}", run.baseline.taken_at.timestamp())
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

fn number(value: impl TryInto<i64>) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap_or_default())
}

/// One `price` row as the model wants it.
///
/// Shared by both series readers because the column list is the same and a
/// second copy is how the two drift apart.
fn sample(row: &rusqlite::Row<'_>) -> rusqlite::Result<Sample> {
    Ok(Sample {
        at: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
            .map(|stamp| stamp.to_utc())
            .unwrap_or_else(|_| Utc::now()),
        cheapest: row.get::<_, i64>(2)? as u64,
        quantity: row.get::<_, i64>(3)? as u32,
        listings: row.get::<_, i64>(4)? as u32,
        tenth: row.get::<_, i64>(5)? as u64,
        median: row.get::<_, i64>(6)? as u64,
    })
}

/// What went wrong talking to the database.
///
/// Storage failing is not an expected outcome of using the application the way
/// a source refusing is, so this is a plain error type at the boundary and
/// nothing above it works in exceptions.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sqlite(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Sqlite(error)
    }
}

type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    /// `pub(crate)` for `model::replica`, which is the rest of this type kept
    /// in a file of its own: what two Armories say to each other, rather than
    /// what one asks its database.
    pub(crate) connection: Connection,
}

impl Store {
    /// Open, or create, the database at `path`.
    pub fn open(path: &Path) -> Result<Store> {
        let store = Store {
            connection: Connection::open(path)?,
        };
        store.migrate()?;
        Ok(store)
    }

    /// An in-memory database, for tests.
    pub fn in_memory() -> Result<Store> {
        let store = Store {
            connection: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS character (
               realm_slug      TEXT NOT NULL,
               name            TEXT NOT NULL,
               character_id    INTEGER NOT NULL,
               realm_id        INTEGER NOT NULL,
               display_name    TEXT NOT NULL,
               realm_name      TEXT NOT NULL,
               level           INTEGER NOT NULL,
               class           TEXT NOT NULL,
               race            TEXT NOT NULL,
               faction         TEXT NOT NULL,
               wow_account_id  INTEGER NOT NULL,
               PRIMARY KEY (realm_slug, name)
             );

             CREATE TABLE IF NOT EXISTS enrolment (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name)
             );

             -- The expensive half of a character, as JSON.
             --
             -- One row read whole, rather than a column per field: this is a
             -- record shown together and never queried across, half its fields
             -- are absent for any given character, and it grows a field every
             -- time Blizzard adds an endpoint. A wide table would be a
             -- migration each time for no query that wants one.
             CREATE TABLE IF NOT EXISTS detail (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               json       TEXT NOT NULL,
               fetched_at TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name)
             );

             -- Which character originally earned each account-wide achievement.
             --
             -- From the addon's `GetAchievementInfo` and from nowhere else: the
             -- web API has no attribution field at all. This is what decides
             -- whether a goal is poisoned, and therefore what keeps
             -- recomputation down to the goals that need it.
             CREATE TABLE IF NOT EXISTS attribution (
               achievement_id INTEGER PRIMARY KEY,
               realm_slug     TEXT NOT NULL,
               name           TEXT NOT NULL
             );

             -- Currencies, and the Warband bank. No endpoint returns either.
             CREATE TABLE IF NOT EXISTS currency (
               realm_slug  TEXT NOT NULL,
               name        TEXT NOT NULL,
               currency_id INTEGER NOT NULL,
               amount      INTEGER NOT NULL,
               PRIMARY KEY (realm_slug, name, currency_id)
             );

             -- Who actually earned the account's account-wide progress.
             --
             -- The reputation half is what lets a run measure a standing that
             -- was at the ceiling before the run began: the standing cannot
             -- move, but the work is counted as it arrives. The currency half
             -- is what tells earned from transferred from already-held.
             --
             -- Cumulative and never replaced wholesale, unlike `currency`
             -- beside it: that table is a snapshot of what is held and this one
             -- is a record of what was done, and a record you overwrite is not
             -- one. The addon's own totals are already cumulative, so a write
             -- here takes the larger of the two — a reinstalled addon starting
             -- from zero must not erase what it observed before.
             CREATE TABLE IF NOT EXISTS earned_reputation (
               realm_slug  TEXT NOT NULL,
               name        TEXT NOT NULL,
               faction_id  INTEGER NOT NULL,
               points      INTEGER NOT NULL,
               renown      INTEGER NOT NULL,
               renown_seen INTEGER NOT NULL,
               account_wide INTEGER NOT NULL,
               PRIMARY KEY (realm_slug, name, faction_id)
             );

             CREATE TABLE IF NOT EXISTS earned_currency (
               realm_slug   TEXT NOT NULL,
               name         TEXT NOT NULL,
               currency_id  INTEGER NOT NULL,
               gained       INTEGER NOT NULL,
               earned       INTEGER NOT NULL,
               tracks_earned INTEGER NOT NULL,
               account_wide INTEGER NOT NULL,
               transferable INTEGER NOT NULL,
               PRIMARY KEY (realm_slug, name, currency_id)
             );

             -- Counters no Blizzard system keeps: recipes made, people
             -- played with, bosses pulled, hours per zone, what has killed
             -- this character.
             --
             -- The same argument as the two tables above and the same merge:
             -- cumulative, never replaced, MAX on conflict. There is nowhere
             -- to get any of it back from if a cleared addon folder takes it.
             -- One table rather than one per kind, because five near-identical
             -- tables is what `model::tally` exists to prevent.
             CREATE TABLE IF NOT EXISTS tally (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               kind       TEXT NOT NULL,
               key        TEXT NOT NULL,
               count      INTEGER NOT NULL,
               label      TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name, kind, key)
             );

             -- What each character can make, and what it takes to make it.
             --
             -- Merged and never wholesale-replaced, because the addon writes
             -- one profession at a time: the recipe book can only be read with
             -- a profession window open, so a character who has opened Alchemy
             -- and not Herbalism would lose their Herbalism recipes to a
             -- replace. Recipes are also never unlearnt, so there is nothing a
             -- replace would be correcting.
             CREATE TABLE IF NOT EXISTS recipe (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               recipe_id  INTEGER NOT NULL,
               recipe     TEXT NOT NULL,
               output_id  INTEGER NOT NULL,
               makes      INTEGER NOT NULL,
               PRIMARY KEY (realm_slug, name, recipe_id)
             );

             -- One required reagent slot, with every quality tier it accepts.
             --
             -- The tiers are separate item ids rather than variants of one,
             -- which the auction house proves: reagents are commodities and a
             -- commodity carries no bonus ids to vary by. Stored joined
             -- because a slot is read whole and never queried by tier.
             CREATE TABLE IF NOT EXISTS recipe_reagent (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               recipe_id  INTEGER NOT NULL,
               slot       INTEGER NOT NULL,
               quantity   INTEGER NOT NULL,
               tiers      TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name, recipe_id, slot)
             );

             -- The Adventure Guide: what a dungeon or raid was, and what is
             -- in it.
             --
             -- Kept like the achievement catalogue rather than like a cached
             -- response, and for the same reason: it is static game data, not
             -- somebody's play record and not a price. It is also Blizzard's
             -- own prose, which is why it lives here and never in `data/` —
             -- the zone corpus is our writing and ships; this is theirs and is
             -- fetched.
             CREATE TABLE IF NOT EXISTS instance (
               id          INTEGER PRIMARY KEY,
               name        TEXT NOT NULL,
               map         INTEGER,
               description TEXT NOT NULL,
               expansion   TEXT NOT NULL DEFAULT '',
               encounters  TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS encounter (
               id          INTEGER PRIMARY KEY,
               name        TEXT NOT NULL,
               description TEXT NOT NULL,
               loot        TEXT NOT NULL
             );

             -- What each achievement criterion measures, from the addon.
             -- Blizzard's profile response gives the tree's shape and never its
             -- meaning, so without these rows every criterion is Unknown and
             -- every achievement falls to attestation.
             CREATE TABLE IF NOT EXISTS criterion (
               criterion_id INTEGER PRIMARY KEY,
               kind         TEXT NOT NULL
             );

             -- Account-wide by nature, so not keyed by character.
             CREATE TABLE IF NOT EXISTS warband_item (
               item_id INTEGER PRIMARY KEY,
               count   INTEGER NOT NULL
             );

             -- How many of each pet species the journal holds. Account state
             -- like the owned set, not a property of the pet, which is why it
             -- is here and not on the catalogue row. A count above one is the
             -- whole of what makes a pet a spare: caging the only copy of one
             -- removes it from the collection.
             CREATE TABLE IF NOT EXISTS pet_held (
               species_id INTEGER PRIMARY KEY,
               count      INTEGER NOT NULL
             );

             -- Goals, per run. `standing` and `bucket` are JSON because both
             -- are enums with payloads that will grow; the columns that are
             -- queried across — run, achievement — are columns.
             CREATE TABLE IF NOT EXISTS goal (
               run_id         INTEGER NOT NULL,
               achievement_id INTEGER NOT NULL,
               standing       TEXT NOT NULL,
               bucket         TEXT NOT NULL,
               attestation    TEXT,
               PRIMARY KEY (run_id, achievement_id)
             );

             -- A run, and the baseline it is measured from.
             --
             -- `id` is this machine's row id and means nothing on another
             -- one; `key` is what the run is called between machines, derived
             -- from the moment its baseline was taken. Every other table on
             -- the wire is keyed by something the game already agreed on — a
             -- realm and a name, an achievement id — and this is the one that
             -- had to be given one.
             CREATE TABLE IF NOT EXISTS run (
               id       INTEGER PRIMARY KEY AUTOINCREMENT,
               name     TEXT NOT NULL,
               baseline TEXT NOT NULL,
               cohort   TEXT NOT NULL,
               is_current INTEGER NOT NULL DEFAULT 0,
               key      TEXT NOT NULL DEFAULT ''
             );

             -- Mounts, pets and toys: the catalogue, and what is owned.
             -- `kind` is part of the key because the three id spaces are
             -- separate and a mount 42 is not a pet 42.
             CREATE TABLE IF NOT EXISTS collectible (
               kind    TEXT NOT NULL,
               id      INTEGER NOT NULL,
               json    TEXT NOT NULL,
               owned   INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (kind, id)
             );

             -- The achievement catalogue: names, points, categories.
             CREATE TABLE IF NOT EXISTS achievement (
               id            INTEGER PRIMARY KEY,
               name          TEXT NOT NULL,
               category      TEXT NOT NULL,
               points        INTEGER NOT NULL,
               description   TEXT NOT NULL,
               unrepeatable  INTEGER NOT NULL
             );

             -- Price history: per-item deltas, never stored snapshots.
             --
             -- Five connected realms of raw auction JSON is roughly 3 GB a day.
             -- Undermine Exchange holds 186 realms in about 56 GB by storing
             -- deltas instead, which is not an optimisation here so much as the
             -- only shape that works. `realm` is 0 for region-wide commodities.
             --
             -- A row is written only when the price moves, so a stable item
             -- costs one row a week rather than one an hour.
             -- `unit_price` is the cheapest and `quantity` the total across
             -- every auction; the three beside them are the shape of the book,
             -- because the cheapest price alone cannot tell one lowball at a
             -- hundred gold from four hundred units at a hundred gold, and
             -- every interesting question about a market is about the shape.
             CREATE TABLE IF NOT EXISTS price (
               realm      INTEGER NOT NULL,
               item_id    INTEGER NOT NULL,
               variant    TEXT NOT NULL,
               unit_price INTEGER NOT NULL,
               quantity   INTEGER NOT NULL,
               seen_at    TEXT NOT NULL,
               listings   INTEGER NOT NULL DEFAULT 0,
               tenth      INTEGER NOT NULL DEFAULT 0,
               median     INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (realm, item_id, variant, seen_at)
             );

             CREATE INDEX IF NOT EXISTS price_item ON price (item_id, realm, seen_at);
             CREATE INDEX IF NOT EXISTS price_seen_at ON price (seen_at);

             -- The latest snapshot of every item on a realm, replaced whole
             -- each sync rather than accumulated.
             --
             -- The distinction from `price` beside it is the whole design of
             -- this half. `price` is *history*, which is expensive to keep and
             -- carries a thirty-day obligation, so it stays opt-in. This is one
             -- moment, it is thrown away and rewritten every hour, and it costs
             -- nothing to collect because the response it comes from was
             -- downloaded in full and then discarded. Browsing the market is a
             -- question about now; watching an item is what starts a history.
             CREATE TABLE IF NOT EXISTS snapshot (
               realm      INTEGER NOT NULL,
               item_id    INTEGER NOT NULL,
               variant    TEXT NOT NULL,
               cheapest   INTEGER NOT NULL,
               quantity   INTEGER NOT NULL,
               listings   INTEGER NOT NULL,
               tenth      INTEGER NOT NULL,
               median     INTEGER NOT NULL,
               seen_at    TEXT NOT NULL,
               PRIMARY KEY (realm, item_id, variant)
             );

             -- Item names, which the auction house does not supply.
             --
             -- A listing carries an item id and nothing else, and there is no
             -- endpoint that turns a list of ids into a list of names — the
             -- search endpoint goes the other way. So names are fetched one at
             -- a time and kept, the way the achievement catalogue is: not
             -- purged, because a name is a fact about the game rather than a
             -- price.
             CREATE TABLE IF NOT EXISTS item (
               item_id  INTEGER PRIMARY KEY,
               name     TEXT NOT NULL,
               -- 0 for Bind-on-Pickup, which is most raid loot and is why the
               -- worth-looking-for list is short rather than a wall.
               sellable INTEGER NOT NULL DEFAULT 1,
               quality  TEXT NOT NULL DEFAULT ''
             );

             -- The items a person actually asked to watch. Ingesting every
             -- item on five realms to answer questions nobody asked is how a
             -- desktop application becomes a service.
             CREATE TABLE IF NOT EXISTS watched (
               item_id INTEGER PRIMARY KEY,
               name    TEXT NOT NULL DEFAULT \'\'
             );

             -- The connected realms a person opted into. Not every realm they
             -- have a character on — that is a suggestion, not a subscription.
             CREATE TABLE IF NOT EXISTS watched_realm (
               realm_id INTEGER PRIMARY KEY,
               name     TEXT NOT NULL DEFAULT \'\'
             );

             -- One recorded play session, as JSON, and the entry written from
             -- it. Both keyed by the character and the moment they logged in,
             -- which is the only pair that identifies an evening.
             --
             -- **Neither of these is ever purged**, and that is the whole
             -- reason they are separate tables rather than rows in `response`.
             -- The thirty-day term is a condition on data obtained through
             -- Blizzard's API. None of this was: it comes from the addon, which
             -- is to say from the user's own client recording the user's own
             -- play. A journal you are not allowed to keep is not a journal.
             CREATE TABLE IF NOT EXISTS session (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               started_at TEXT NOT NULL,
               ended_at   TEXT NOT NULL,
               json       TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name, started_at)
             );

             CREATE INDEX IF NOT EXISTS session_started_at ON session (started_at);

             -- The prose. Separate from the session because the session stands
             -- on its own: writing is opt-in, costs the user's own credit, and
             -- most evenings will never have one. A missing row here is the
             -- normal case, not a gap.
             CREATE TABLE IF NOT EXISTS entry (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               started_at TEXT NOT NULL,
               title      TEXT NOT NULL,
               body       TEXT NOT NULL,
               model      TEXT NOT NULL,
               written_at TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name, started_at)
             );

             -- Bodies from the API, and the Last-Modified they arrived with.
             -- The stamp is what makes the next sync a conditional request, and
             -- it is why syncing an account this size costs almost nothing.
             CREATE TABLE IF NOT EXISTS response (
               url           TEXT PRIMARY KEY,
               body          BLOB NOT NULL,
               last_modified TEXT,
               fetched_at    TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS response_fetched_at
               ON response (fetched_at);

             -- Evenings deliberately thrown away, so they stay thrown away.
             --
             -- Deleting the row is not enough on its own. The addon's own file
             -- is the source and it is re-read on every launch, so a forgotten
             -- evening is re-imported by the next read and reappears — which
             -- looks exactly like the application ignoring the instruction.
             -- The addon keeps its last forty sessions, so this outlives the
             -- file that caused it by design.
             --
             -- It travels, because forgetting is a decision rather than an
             -- observation: an evening thrown away here must not come back
             -- from the machine that still has it in its own SavedVariables.
             CREATE TABLE IF NOT EXISTS forgotten (
               realm_slug TEXT NOT NULL,
               name       TEXT NOT NULL,
               started_at TEXT NOT NULL,
               at         TEXT NOT NULL,
               PRIMARY KEY (realm_slug, name, started_at)
             );

             -- Which rows have moved, and in what order.
             --
             -- One entry a row rather than one an edit: a write deletes the
             -- entry it finds and inserts a new one, so `seq` moves to the end
             -- of the queue and the log stays the size of the data. See
             -- `model::replica` for who reads it — on a client this is an
             -- outbox that empties, on the server it is a log that is kept.
             --
             -- `machine` is who wrote it. A pull excludes the caller's own,
             -- which is what stops a client's first push arriving straight
             -- back as a pull of fifty thousand rows it already has.
             CREATE TABLE IF NOT EXISTS change (
               seq     INTEGER PRIMARY KEY AUTOINCREMENT,
               scope   TEXT NOT NULL,
               key     TEXT NOT NULL,
               gone    INTEGER NOT NULL DEFAULT 0,
               at      TEXT NOT NULL,
               machine TEXT NOT NULL DEFAULT '',
               UNIQUE (scope, key)
             );

             CREATE INDEX IF NOT EXISTS change_machine ON change (machine, seq);

             -- Cursors and this installation's id. Small enough that a table
             -- of its own would be five tables of one row.
             CREATE TABLE IF NOT EXISTS sync_state (
               name  TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );",
        )?;
        self.add_columns()?;

        // Indexes over columns that arrive through `ADDED` rather than through
        // the batch above, and so cannot be created with it: on a database
        // made before the column existed, `CREATE INDEX` on it fails, and a
        // failure in that one batch takes the whole schema down and drops the
        // application into the in-memory fallback. Everything here runs after
        // `add_columns` has made the column real.
        //
        // Every run made before this column existed carries the default empty
        // key, and two of those would collide the moment the index below is
        // created — so they are given theirs first.
        //
        // The index cannot be a partial one over `key <> ''`, which was the
        // obvious way to avoid the backfill: SQLite matches an upsert's
        // conflict target against a unique index *including its `WHERE`*, so
        // `ON CONFLICT (key)` would find no index, and an arriving run would
        // fail — silently, because `Store::apply` counts a row it cannot
        // write rather than losing the batch it came in.
        self.name_runs()?;
        self.connection
            .execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS run_key ON run (key);")?;

        self.connection.execute_batch(&Self::triggers())?;

        // Recording is only ever off inside `Store::apply` and `Store::purge`,
        // both of which put it back. A process that died between the two would
        // otherwise leave an installation that quietly never syncs again, and
        // nothing about it would look wrong.
        self.set_setting("recording", "1")?;
        Ok(())
    }

    /// The triggers that keep the change log, one set a table, generated from
    /// [`sync::TABLES`].
    ///
    /// Written as triggers rather than as a `note()` call at the end of every
    /// write, and that is the whole design of this half. There are twenty-four
    /// ways to write to this store and each of them would have to remember;
    /// the twenty-fifth, added in a year, would not, and what that looks like
    /// is one table that silently stops travelling between machines. Here
    /// recording is a property of the table, and a new writer gets it without
    /// knowing this exists.
    ///
    /// Two things fall out of it for free. `WHEN old.c IS NOT new.c` means an
    /// upsert that writes the values already there logs nothing, so a sync
    /// that changed nothing enqueues nothing — without which two clients hand
    /// each other the same rows forever. And `json_array` builds the key in
    /// exactly the encoding `serde_json` produces for the same values, which
    /// is what lets `model::replica` read the key back out with no agreement
    /// to maintain between the two.
    ///
    /// Keys are never updated in this schema — every one of them is what the
    /// row *is* — so the update trigger logs the new key and does not chase an
    /// old one.
    fn triggers() -> String {
        // NULL when the row is absent, which `IS NOT '0'` reads as on. A store
        // that has never been told is a store that records.
        const ON: &str = "(SELECT value FROM sync_state WHERE name = 'recording') IS NOT '0'";
        const WHO: &str = "COALESCE((SELECT value FROM sync_state WHERE name = 'machine'), '')";
        const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

        let mut out = String::new();
        for table in sync::TABLES {
            let name = table.name;
            let key_of = |side: &str| {
                format!(
                    "json_array({})",
                    table
                        .key
                        .iter()
                        .map(|column| format!("{side}.{column}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let mut record = |suffix: &str, event: &str, side: &str, gone: u8, extra: String| {
                let key = key_of(side);
                out.push_str(&format!(
                    "DROP TRIGGER IF EXISTS log_{name}_{suffix};
                     CREATE TRIGGER log_{name}_{suffix} AFTER {event} ON {name}
                     WHEN {ON}{extra}
                     BEGIN
                       DELETE FROM change WHERE scope = '{name}' AND key = {key};
                       INSERT INTO change (scope, key, gone, at, machine)
                         VALUES ('{name}', {key}, {gone}, {NOW}, {WHO});
                     END;\n"
                ));
            };

            record("ins", "INSERT", "new", 0, String::new());
            record("del", "DELETE", "old", 1, String::new());

            // A table that is nothing but its key — `enrolment` — has no
            // update to notice. Being there is the whole record.
            let differs = table
                .columns
                .iter()
                .filter(|column| column.rule != sync::Rule::Stamp)
                .map(|column| format!("old.{0} IS NOT new.{0}", column.name))
                .collect::<Vec<_>>();
            if !differs.is_empty() {
                record(
                    "upd",
                    "UPDATE",
                    "new",
                    0,
                    format!(" AND ({})", differs.join(" OR ")),
                );
            }
        }
        out
    }

    /// Columns added to a table that had already shipped without them.
    ///
    /// `CREATE TABLE IF NOT EXISTS` does nothing at all to a table that exists,
    /// so a column added to a definition above never reaches a database made
    /// before it. Every statement naming that column then fails — and the
    /// writes here are mostly `let _ =`, because a name that has not arrived
    /// yet is not an error — so the failure is silent and total rather than
    /// noisy and partial. `item`'s two and `price`'s three were both added this
    /// way, which left a running install naming no items and recording no price
    /// history whatsoever while looking like it was working.
    ///
    /// Every entry carries a default, which is what SQLite requires of
    /// `ADD COLUMN` on a `NOT NULL` column, and is also what makes the rows
    /// already there mean the same thing they did before. Anything that cannot
    /// be expressed as an additive column with a sensible default is a rebuild,
    /// not a line in this table.
    const ADDED: &'static [(&'static str, &'static str, &'static str)] = &[
        ("item", "sellable", "INTEGER NOT NULL DEFAULT 1"),
        ("item", "quality", "TEXT NOT NULL DEFAULT ''"),
        ("price", "listings", "INTEGER NOT NULL DEFAULT 0"),
        ("price", "tenth", "INTEGER NOT NULL DEFAULT 0"),
        ("price", "median", "INTEGER NOT NULL DEFAULT 0"),
        ("run", "key", "TEXT NOT NULL DEFAULT ''"),
    ];

    /// Give a run made before `key` existed the same key it would get now.
    ///
    /// Computed here rather than in SQL because it comes out of the baseline,
    /// which is a JSON column, and `json_extract` would hand back a stamp
    /// written in serde's format rather than the one `run_key` builds. Two
    /// spellings of the same instant is exactly the sort of near-miss that
    /// makes one run arrive as two.
    fn name_runs(&self) -> Result<()> {
        let mut select = self
            .connection
            .prepare("SELECT id, baseline FROM run WHERE key = \'\'")?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(select);

        for (id, baseline) in rows {
            let key = serde_json::from_str::<crate::run::Baseline>(&baseline)
                .map(|baseline| format!("run-{}", baseline.taken_at.timestamp()))
                // A baseline that will not parse is a run that cannot be
                // measured against anyway. It still needs a key of its own so
                // the index below can be built.
                .unwrap_or_else(|_| format!("run-local-{id}"));
            self.connection
                .execute("UPDATE run SET key = ?2 WHERE id = ?1", params![id, key])?;
        }
        Ok(())
    }

    fn add_columns(&self) -> Result<()> {
        for (table, column, definition) in Self::ADDED {
            let held: Option<i64> = self
                .connection
                .query_row(
                    "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2",
                    params![table, column],
                    |row| row.get(0),
                )
                .optional()?;
            if held.is_none() {
                // The names are literals from the table above, never input.
                self.connection.execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} {definition}"
                ))?;
            }
        }
        Ok(())
    }

    /// Replace the roster wholesale.
    ///
    /// Characters are deleted and transferred, so a merge would leave ghosts
    /// behind that go on settling goals nothing can account for.
    pub fn save_roster(&mut self, roster: &Roster) -> Result<()> {
        let rows: Vec<Vec<SqlValue>> = roster
            .characters
            .iter()
            .map(|character| {
                vec![
                    text(&character.key.realm_slug),
                    text(&character.key.name),
                    number(character.id),
                    number(character.realm_id),
                    text(&character.display_name),
                    text(&character.realm_name),
                    number(character.level),
                    text(&character.class),
                    text(&character.race),
                    text(character.faction.label()),
                    number(character.wow_account_id),
                ]
            })
            .collect();

        let transaction = self.connection.transaction()?;
        reconcile(
            &transaction,
            "character",
            &["realm_slug", "name"],
            &[
                "character_id",
                "realm_id",
                "display_name",
                "realm_name",
                "level",
                "class",
                "race",
                "faction",
                "wow_account_id",
            ],
            None,
            &rows,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn roster(&self) -> Result<Roster> {
        let mut select = self.connection.prepare(
            "SELECT realm_slug, name, character_id, realm_id, display_name,
                    realm_name, level, class, race, faction, wow_account_id
             FROM character",
        )?;
        let characters = select
            .query_map([], |row| {
                Ok(Character {
                    key: CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    // SQLite integers are signed, so the ids come back as i64
                    // and are cast. Blizzard's are well inside the range; the
                    // cast is a type mismatch, not a truncation risk.
                    id: row.get::<_, i64>(2)? as u64,
                    realm_id: row.get::<_, i64>(3)? as u64,
                    display_name: row.get(4)?,
                    realm_name: row.get(5)?,
                    level: row.get(6)?,
                    class: row.get(7)?,
                    race: row.get(8)?,
                    faction: match row.get::<_, String>(9)?.as_str() {
                        "Alliance" => Faction::Alliance,
                        "Horde" => Faction::Horde,
                        _ => Faction::Neutral,
                    },
                    wow_account_id: row.get::<_, i64>(10)? as u64,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Roster::new(characters))
    }

    pub fn save_cohort(&mut self, cohort: &Cohort) -> Result<()> {
        let rows: Vec<Vec<SqlValue>> = cohort
            .keys()
            .map(|key| vec![text(&key.realm_slug), text(&key.name)])
            .collect();

        let transaction = self.connection.transaction()?;
        reconcile(
            &transaction,
            "enrolment",
            &["realm_slug", "name"],
            &[],
            None,
            &rows,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn cohort(&self) -> Result<Cohort> {
        let mut select = self
            .connection
            .prepare("SELECT realm_slug, name FROM enrolment")?;
        let keys = select
            .query_map([], |row| {
                Ok(CharacterKey {
                    realm_slug: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(Cohort::from(keys))
    }

    /// Save one character's detail.
    pub fn save_detail(&self, key: &CharacterKey, detail: &Detail) -> Result<()> {
        let json = serde_json::to_string(detail).unwrap_or_else(|_| "{}".into());
        self.connection.execute(
            "INSERT INTO detail (realm_slug, name, json, fetched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (realm_slug, name) DO UPDATE SET
               json = excluded.json,
               fetched_at = excluded.fetched_at",
            params![key.realm_slug, key.name, json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Every character's detail, keyed for joining onto the roster.
    ///
    /// A row whose JSON no longer parses is dropped rather than failing the
    /// read: the detail is refetchable, and losing the whole roster's worth
    /// because one field changed shape would be a poor trade.
    pub fn details(&self) -> Result<HashMap<CharacterKey, Detail>> {
        let mut select = self
            .connection
            .prepare("SELECT realm_slug, name, json FROM detail")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(key, json)| Some((key, serde_json::from_str(&json).ok()?)))
            .collect())
    }

    // -- what the addon collected -------------------------------------------

    /// Replace everything the collector addon reported.
    ///
    /// Wholesale rather than merged: the addon rewrites its file in full on
    /// every logout, so a merge would keep attributions for achievements that
    /// have since been accounted to somebody else — and a stale attribution
    /// un-poisons a goal that should be poisoned, which is the wrong direction
    /// to be wrong in.
    pub fn save_collected(&mut self, collected: &Collected) -> Result<()> {
        let transaction = self.connection.transaction()?;

        reconcile(
            &transaction,
            "attribution",
            &["achievement_id"],
            &["realm_slug", "name"],
            None,
            &collected
                .earned_by
                .iter()
                .map(|(id, key)| vec![number(*id), text(&key.realm_slug), text(&key.name)])
                .collect::<Vec<_>>(),
        )?;

        reconcile(
            &transaction,
            "currency",
            &["realm_slug", "name", "currency_id"],
            &["amount"],
            None,
            &collected
                .currencies
                .iter()
                .flat_map(|(key, amounts)| {
                    amounts.iter().map(move |(id, amount)| {
                        vec![
                            text(&key.realm_slug),
                            text(&key.name),
                            number(*id),
                            number(*amount),
                        ]
                    })
                })
                .collect::<Vec<_>>(),
        )?;

        // Merged rather than replaced, and by taking the larger of the two.
        // These are a record of what somebody did over months; the addon's own
        // totals are cumulative, so a straight write is normally identical —
        // but a reinstalled addon starts from zero, and a run that forgot a
        // year of a character's work because a folder was cleared would be
        // worse than one that never recorded it.
        {
            let mut insert = transaction.prepare(
                "INSERT INTO earned_reputation
                   (realm_slug, name, faction_id, points, renown, renown_seen, account_wide)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (realm_slug, name, faction_id) DO UPDATE SET
                   points       = MAX(points, excluded.points),
                   renown       = MAX(renown, excluded.renown),
                   renown_seen  = MAX(renown_seen, excluded.renown_seen),
                   account_wide = excluded.account_wide",
            )?;
            for (key, earned) in &collected.earned {
                for (faction, with) in &earned.reputation {
                    insert.execute(params![
                        key.realm_slug,
                        key.name,
                        *faction as i64,
                        i64::from(with.points),
                        i64::from(with.renown),
                        i64::from(with.renown_seen),
                        with.account_wide as i64,
                    ])?;
                }
            }
        }
        {
            let mut insert = transaction.prepare(
                "INSERT INTO earned_currency
                   (realm_slug, name, currency_id, gained, earned, tracks_earned,
                    account_wide, transferable)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (realm_slug, name, currency_id) DO UPDATE SET
                   gained        = MAX(gained, excluded.gained),
                   earned        = MAX(earned, excluded.earned),
                   tracks_earned = excluded.tracks_earned,
                   account_wide  = excluded.account_wide,
                   transferable  = excluded.transferable",
            )?;
            for (key, earned) in &collected.earned {
                for (currency, held) in &earned.currency {
                    insert.execute(params![
                        key.realm_slug,
                        key.name,
                        *currency as i64,
                        held.gained as i64,
                        held.earned as i64,
                        held.tracks_earned as i64,
                        held.account_wide as i64,
                        held.transferable as i64,
                    ])?;
                }
            }
        }

        {
            let mut insert = transaction.prepare(
                "INSERT INTO tally (realm_slug, name, kind, key, count, label)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (realm_slug, name, kind, key) DO UPDATE SET
                   count = MAX(count, excluded.count),
                   label = excluded.label",
            )?;
            for (character, counted) in &collected.tallies {
                for tally in counted {
                    insert.execute(params![
                        character.realm_slug,
                        character.name,
                        tally.kind.as_token(),
                        tally.key,
                        tally.count as i64,
                        tally.label,
                    ])?;
                }
            }
        }

        {
            let mut insert = transaction.prepare(
                "INSERT INTO recipe (realm_slug, name, recipe_id, recipe, output_id, makes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (realm_slug, name, recipe_id) DO UPDATE SET
                   recipe    = excluded.recipe,
                   output_id = excluded.output_id,
                   makes     = excluded.makes",
            )?;
            for (character, book) in &collected.recipes {
                for recipe in book {
                    insert.execute(params![
                        character.realm_slug,
                        character.name,
                        recipe.id as i64,
                        recipe.name,
                        recipe.output as i64,
                        recipe.makes as i64,
                    ])?;
                }
            }
        }

        // The slots of a recipe, reconciled one recipe at a time. A recipe
        // whose reagents Blizzard changed must not keep both sets and be
        // costed against a slot it no longer has, which is why the slots that
        // are gone are deleted rather than left; scoping it to the recipe is
        // what stops one profession window's read emptying another's.
        {
            for (character, book) in &collected.recipes {
                for recipe in book {
                    reconcile(
                        &transaction,
                        "recipe_reagent",
                        &["realm_slug", "name", "recipe_id", "slot"],
                        &["quantity", "tiers"],
                        Some((
                            "realm_slug = ?1 AND name = ?2 AND recipe_id = ?3",
                            vec![
                                text(&character.realm_slug),
                                text(&character.name),
                                number(recipe.id),
                            ],
                        )),
                        &recipe
                            .reagents
                            .iter()
                            .enumerate()
                            .map(|(index, reagent)| {
                                vec![
                                    text(&character.realm_slug),
                                    text(&character.name),
                                    number(recipe.id),
                                    number(index as u32),
                                    number(reagent.quantity),
                                    text(
                                        &reagent
                                            .tiers
                                            .iter()
                                            .map(u32::to_string)
                                            .collect::<Vec<_>>()
                                            .join(","),
                                    ),
                                ]
                            })
                            .collect::<Vec<_>>(),
                    )?;
                }
            }
        }

        reconcile(
            &transaction,
            "criterion",
            &["criterion_id"],
            &["kind"],
            None,
            &collected
                .criteria
                .iter()
                .map(|(id, kind)| {
                    vec![
                        number(*id),
                        text(&serde_json::to_string(kind).unwrap_or_default()),
                    ]
                })
                .collect::<Vec<_>>(),
        )?;

        reconcile(
            &transaction,
            "warband_item",
            &["item_id"],
            &["count"],
            None,
            &collected
                .warband_bank
                .iter()
                .map(|(id, count)| vec![number(*id), number(*count)])
                .collect::<Vec<_>>(),
        )?;

        // Replaced wholesale, like the bank: a species that has dropped out of
        // the file is a species the journal no longer holds, and merging would
        // leave a sold pet looking like a spare forever. Only when the file
        // carried counts at all, though — an older collector writes none, and
        // emptying the table on its say-so would silently un-spare everything.
        if !collected.pets_held.is_empty() {
            reconcile(
                &transaction,
                "pet_held",
                &["species_id"],
                &["count"],
                None,
                &collected
                    .pets_held
                    .iter()
                    .map(|(species, count)| vec![number(*species), number(*count)])
                    .collect::<Vec<_>>(),
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    /// Who earned each account-wide achievement.
    pub fn attributions(&self) -> Result<HashMap<u32, CharacterKey>> {
        let mut select = self
            .connection
            .prepare("SELECT achievement_id, realm_slug, name FROM attribution")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    CharacterKey {
                        realm_slug: row.get(1)?,
                        name: row.get(2)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    /// What each criterion measures.
    pub fn criteria(&self) -> Result<HashMap<u64, CriterionKind>> {
        let mut select = self
            .connection
            .prepare("SELECT criterion_id, kind FROM criterion")?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, kind)| Some((id, serde_json::from_str(&kind).ok()?)))
            .collect())
    }

    /// Currencies, per character.
    pub fn currencies(&self) -> Result<HashMap<CharacterKey, HashMap<u32, u64>>> {
        let mut select = self
            .connection
            .prepare("SELECT realm_slug, name, currency_id, amount FROM currency")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get::<_, i64>(2)? as u32,
                    row.get::<_, i64>(3)? as u64,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: HashMap<CharacterKey, HashMap<u32, u64>> = HashMap::new();
        for (key, id, amount) in rows {
            out.entry(key).or_default().insert(id, amount);
        }
        Ok(out)
    }

    /// What each character has personally earned, reputation and currency both.
    ///
    /// The answer to "who actually did this", and the only source for it —
    /// nothing in the API attributes a point of reputation or a copper of a
    /// currency to a character.
    pub fn provenance(&self) -> Result<Provenance> {
        let mut out: Provenance = HashMap::new();

        let mut select = self.connection.prepare(
            "SELECT realm_slug, name, faction_id, points, renown, renown_seen, account_wide
             FROM earned_reputation",
        )?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get::<_, i64>(2)? as u32,
                    EarnedReputation {
                        points: row.get::<_, i64>(3)? as u32,
                        renown: row.get::<_, i64>(4)? as u32,
                        renown_seen: row.get::<_, i64>(5)? as u32,
                        account_wide: row.get::<_, i64>(6)? != 0,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (key, faction, earned) in rows {
            out.entry(key)
                .or_default()
                .reputation
                .insert(faction, earned);
        }

        let mut select = self.connection.prepare(
            "SELECT realm_slug, name, currency_id, gained, earned, tracks_earned,
                    account_wide, transferable
             FROM earned_currency",
        )?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get::<_, i64>(2)? as u32,
                    EarnedCurrency {
                        gained: row.get::<_, i64>(3)? as u64,
                        earned: row.get::<_, i64>(4)? as u64,
                        tracks_earned: row.get::<_, i64>(5)? != 0,
                        account_wide: row.get::<_, i64>(6)? != 0,
                        transferable: row.get::<_, i64>(7)? != 0,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (key, currency, held) in rows {
            out.entry(key).or_default().currency.insert(currency, held);
        }

        Ok(out)
    }

    /// Every counter, per character, biggest first.
    ///
    /// Read back rather than taken from the dump for the reason the table
    /// exists: the write merges by taking the larger count, so the dump alone
    /// is the wrong number the moment an addon folder has been cleared.
    pub fn tallies(&self) -> Result<Tallies> {
        let mut select = self.connection.prepare(
            "SELECT realm_slug, name, kind, key, count, label
             FROM tally ORDER BY count DESC, label ASC",
        )?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    CharacterKey {
                        realm_slug: row.get(0)?,
                        name: row.get(1)?,
                    },
                    row.get::<_, String>(2)?,
                    Tally {
                        // Overwritten below; the kind is a string in the row
                        // and a value here, and the parse can fail.
                        kind: Counting::Recipe,
                        key: row.get(3)?,
                        count: row.get::<_, i64>(4)? as u64,
                        label: row.get(5)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: Tallies = HashMap::new();
        for (character, token, mut tally) in rows {
            // A row written by a newer Armory against an older one's database.
            // Skipped for the same reason the addon reader skips it.
            let Some(kind) = Counting::from_token(&token) else {
                continue;
            };
            tally.kind = kind;
            out.entry(character).or_default().push(tally);
        }
        Ok(out)
    }

    /// Replace one realm's snapshot with what was just seen.
    ///
    /// Whole-table-per-realm rather than a merge: an item that has left the
    /// auction house entirely must disappear from the browser, and a merge
    /// would leave last hour's price sitting there looking current.
    pub fn record_snapshot(&mut self, realm: u32, book: &[Depth], at: DateTime<Utc>) -> Result<()> {
        let stamp = at.to_rfc3339();
        let rows: Vec<Vec<SqlValue>> = book
            .iter()
            .map(|entry| {
                vec![
                    number(realm),
                    number(entry.item_id),
                    text(&entry.variant),
                    number(entry.cheapest),
                    number(entry.quantity),
                    number(entry.listings),
                    number(entry.tenth),
                    number(entry.median),
                    text(&stamp),
                ]
            })
            .collect();

        let transaction = self.connection.transaction()?;
        reconcile(
            &transaction,
            "snapshot",
            &["realm", "item_id", "variant"],
            &[
                "cheapest", "quantity", "listings", "tenth", "median", "seen_at",
            ],
            Some(("realm = ?1", vec![number(realm)])),
            &rows,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// What is for sale on one realm: its own listings *and* the region-wide
    /// commodities.
    ///
    /// Both, because that is what the auction house in the game is. Standing at
    /// an auctioneer on Emerald Dream you see Copper Ore — a region-wide
    /// commodity that has no realm of its own — beside the gear listed on that
    /// realm alone, and a browser that showed only one half would answer
    /// "nothing" for every stackable trade good in the game. Realm 0 is where
    /// the commodities are recorded, so it is always in the query; asking for
    /// realm 0 itself asks for them alone.
    ///
    /// The two sets do not overlap and cannot: Blizzard runs the split, a
    /// commodity is listed nowhere but region-wide and a piece of gear is
    /// listed nowhere but on its realm.
    ///
    /// Only rows with no variant. Gear listed with bonus ids is a different
    /// kind of question — one item id is a hundred different actual items and
    /// Blizzard publishes no dictionary for the numbers that tell them apart —
    /// so the browser answers the part of the market where an item id means one
    /// thing.
    pub fn snapshot(&self, realm: u32) -> Result<Vec<Listed>> {
        let mut select = self.connection.prepare(
            "SELECT s.item_id, s.cheapest, s.quantity, s.listings, s.tenth, s.median, i.name
             FROM snapshot s LEFT JOIN item i ON i.item_id = s.item_id
             WHERE s.realm IN (?1, 0) AND s.variant = ''",
        )?;
        let rows = select
            .query_map(params![realm], |row| {
                Ok(Listed {
                    item_id: row.get::<_, i64>(0)? as u32,
                    cheapest: row.get::<_, i64>(1)? as u64,
                    quantity: row.get::<_, i64>(2)? as u32,
                    listings: row.get::<_, i64>(3)? as u32,
                    tenth: row.get::<_, i64>(4)? as u64,
                    median: row.get::<_, i64>(5)? as u64,
                    name: row.get::<_, Option<String>>(6)?,
                    sold: 0,
                    span_hours: 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every row of one realm's snapshot as `(item, cheapest, quantity)`.
    ///
    /// Variants included and folded by the caller. `snapshot` beside this
    /// answers the browser and takes only commodities, because there an item id
    /// means one thing; here a piece of gear with three bonus-id variants is
    /// three rows for one item and the cheapest of them is the answer.
    pub fn snapshot_all(&self, realm: u32) -> Result<Vec<(u32, u64, u32)>> {
        let mut select = self
            .connection
            .prepare("SELECT item_id, cheapest, quantity FROM snapshot WHERE realm = ?1")?;
        let rows = select
            .query_map(params![realm], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u32,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Item names already known.
    pub fn item_names(&self) -> Result<HashMap<u32, String>> {
        let mut select = self.connection.prepare("SELECT item_id, name FROM item")?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    /// Remember what one item is.
    pub fn name_item(&self, item_id: u32, item: &Item) -> Result<()> {
        self.connection.execute(
            "INSERT INTO item (item_id, name, sellable, quality) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (item_id) DO UPDATE SET
               name = excluded.name, sellable = excluded.sellable,
               quality = excluded.quality",
            params![
                item_id as i64,
                item.name,
                item.sellable as i64,
                item.quality.clone().unwrap_or_default()
            ],
        )?;
        Ok(())
    }

    /// Record an item's name, and nothing else about it.
    ///
    /// The name comes from `/data/wow/search/item`, which answers a name and an
    /// id and carries no `preview_item` — so there is no binding in it, and
    /// writing one would be inventing the answer to the question `Place::spoils`
    /// asks. `sellable` is therefore left at its default on insert and left
    /// *alone* on conflict: a row that already carries a real binding must not
    /// have it overwritten by a search that never saw one.
    ///
    /// Unknown is the honest state and it is also the safe one. `Place::spoils`
    /// skips an item that is not sellable and skips one that is absent from the
    /// table entirely, so a name-only row behaves exactly as it did before the
    /// name arrived — which is the point.
    pub fn name_found_item(&self, item_id: u32, name: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO item (item_id, name, sellable, quality) VALUES (?1, ?2, 0, '')
             ON CONFLICT (item_id) DO UPDATE SET name = excluded.name",
            params![item_id as i64, name],
        )?;
        Ok(())
    }

    /// Everything known about the items named so far.
    pub fn items(&self) -> Result<HashMap<u32, Item>> {
        let mut select = self
            .connection
            .prepare("SELECT item_id, name, sellable, quality FROM item")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    Item {
                        name: row.get(1)?,
                        sellable: row.get::<_, i64>(2)? != 0,
                        quality: Some(row.get::<_, String>(3)?).filter(|q| !q.is_empty()),
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    /// Every commodity price series on one realm, for the items asked for.
    ///
    /// Keyed by item id rather than by [`Listing::series`], and only rows with
    /// no variant: a reagent is a commodity, a commodity carries no bonus ids,
    /// and anything here that *does* carry a variant is a piece of gear that
    /// happens to share an id space. Region-wide commodities are realm 0.
    pub fn commodity_series(&self, realm: u32, items: &HashSet<u32>) -> Result<Series> {
        if items.is_empty() {
            return Ok(HashMap::new());
        }
        let mut select = self.connection.prepare(
            "SELECT item_id, seen_at, unit_price, quantity, listings, tenth, median
             FROM price WHERE realm = ?1 AND variant = '' ORDER BY item_id, seen_at",
        )?;
        let rows = select
            .query_map(params![realm], |row| {
                Ok((row.get::<_, i64>(0)? as u32, sample(row)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: Series = HashMap::new();
        for (item, sample) in rows {
            if items.contains(&item) {
                out.entry(item.to_string()).or_default().push(sample);
            }
        }
        Ok(out)
    }

    /// Remember one instance.
    pub fn save_instance(&self, instance: &Instance) -> Result<()> {
        self.connection.execute(
            "INSERT INTO instance (id, name, map, description, expansion, encounters)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (id) DO UPDATE SET
               name = excluded.name, map = excluded.map,
               description = excluded.description,
               expansion = excluded.expansion, encounters = excluded.encounters",
            params![
                instance.id as i64,
                instance.name,
                instance.map.map(|m| m as i64),
                instance.description,
                instance.expansion.clone().unwrap_or_default(),
                joined(&instance.encounters),
            ],
        )?;
        Ok(())
    }

    /// Remember one encounter.
    pub fn save_encounter(&self, encounter: &Encounter) -> Result<()> {
        self.connection.execute(
            "INSERT INTO encounter (id, name, description, loot)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
               name = excluded.name, description = excluded.description,
               loot = excluded.loot",
            params![
                encounter.id as i64,
                encounter.name,
                encounter.description,
                joined(&encounter.loot),
            ],
        )?;
        Ok(())
    }

    /// Everything the guide has told us so far.
    pub fn guide(&self) -> Result<Guide> {
        let mut guide = Guide::default();

        let mut select = self
            .connection
            .prepare("SELECT id, name, map, description, expansion, encounters FROM instance")?;
        let rows = select
            .query_map([], |row| {
                Ok(Instance {
                    id: row.get::<_, i64>(0)? as u32,
                    name: row.get(1)?,
                    map: row.get::<_, Option<i64>>(2)?.map(|m| m as u32),
                    description: row.get(3)?,
                    expansion: Some(row.get::<_, String>(4)?).filter(|e| !e.is_empty()),
                    encounters: split(&row.get::<_, String>(5)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for instance in rows {
            guide.instances.insert(instance.id, instance);
        }

        let mut select = self
            .connection
            .prepare("SELECT id, name, description, loot FROM encounter")?;
        let rows = select
            .query_map([], |row| {
                Ok(Encounter {
                    id: row.get::<_, i64>(0)? as u32,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    loot: split(&row.get::<_, String>(3)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for encounter in rows {
            guide.encounters.insert(encounter.id, encounter);
        }

        Ok(guide)
    }

    /// Which instances and encounters have not been fetched yet.
    ///
    /// Answered from the store rather than held in memory, because the budget
    /// that spends against it runs once a sync and the alternative is a second
    /// copy of the guide that can disagree with the first.
    pub fn guide_gaps(&self, known: &[(u32, String)]) -> Result<(Vec<u32>, Vec<u32>)> {
        let have: HashSet<u32> = self
            .connection
            .prepare("SELECT id FROM instance")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .filter_map(|id| id.ok().map(|id| id as u32))
            .collect();
        let instances: Vec<u32> = known
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !have.contains(id))
            .collect();

        let seen: HashSet<u32> = self
            .connection
            .prepare("SELECT id FROM encounter")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .filter_map(|id| id.ok().map(|id| id as u32))
            .collect();
        let wanted: Vec<u32> = self
            .guide()?
            .instances
            .values()
            .flat_map(|i| i.encounters.iter().copied())
            .filter(|id| !seen.contains(id))
            .collect();

        Ok((instances, wanted))
    }

    /// What every character can make.
    ///
    /// Read back rather than taken from the dump for the reason the table
    /// merges: one dump is one profession window, and the answer wanted here is
    /// every window ever opened.
    pub fn recipes(&self) -> Result<RecipeBooks> {
        let mut slots: HashMap<(String, String, u32), Vec<Reagent>> = HashMap::new();
        let mut select = self.connection.prepare(
            "SELECT realm_slug, name, recipe_id, quantity, tiers
             FROM recipe_reagent ORDER BY slot",
        )?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? as u32,
                    row.get::<_, i64>(3)? as u32,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (realm_slug, name, recipe, quantity, tiers) in rows {
            let tiers: Vec<u32> = tiers.split(',').filter_map(|id| id.parse().ok()).collect();
            if tiers.is_empty() {
                continue;
            }
            slots
                .entry((realm_slug, name, recipe))
                .or_default()
                .push(Reagent { quantity, tiers });
        }

        let mut select = self
            .connection
            .prepare("SELECT realm_slug, name, recipe_id, recipe, output_id, makes FROM recipe")?;
        let rows = select
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    Recipe {
                        id: row.get::<_, i64>(2)? as u32,
                        name: row.get(3)?,
                        output: row.get::<_, i64>(4)? as u32,
                        makes: row.get::<_, i64>(5)? as u32,
                        reagents: Vec::new(),
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: RecipeBooks = HashMap::new();
        for (realm_slug, name, mut recipe) in rows {
            let Some(reagents) = slots.remove(&(realm_slug.clone(), name.clone(), recipe.id))
            else {
                // A recipe row whose slots did not survive is not a free
                // recipe, it is an unreadable one.
                continue;
            };
            recipe.reagents = reagents;
            out.entry(CharacterKey { realm_slug, name })
                .or_default()
                .push(recipe);
        }
        Ok(out)
    }

    /// Every item any known recipe names, as a reagent or as its output.
    ///
    /// What the auction snapshots are filtered against, so that the price net
    /// is bounded by the account's own recipe books rather than being "record
    /// the whole market".
    pub fn recipe_items(&self) -> Result<HashSet<u32>> {
        let mut out = HashSet::new();
        let mut select = self.connection.prepare("SELECT output_id FROM recipe")?;
        for id in select.query_map([], |row| row.get::<_, i64>(0))? {
            out.insert(id? as u32);
        }
        let mut select = self
            .connection
            .prepare("SELECT tiers FROM recipe_reagent")?;
        for tiers in select.query_map([], |row| row.get::<_, String>(0))? {
            out.extend(tiers?.split(',').filter_map(|id| id.parse::<u32>().ok()));
        }
        Ok(out)
    }

    /// The Warband bank.
    pub fn warband_bank(&self) -> Result<HashMap<u32, u64>> {
        let mut select = self
            .connection
            .prepare("SELECT item_id, count FROM warband_item")?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u64))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    /// How many of each pet species the journal holds.
    pub fn pets_held(&self) -> Result<HashMap<u32, u32>> {
        let mut select = self
            .connection
            .prepare("SELECT species_id, count FROM pet_held")?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    // -- the achievement catalogue ------------------------------------------

    pub fn save_achievements(&mut self, achievements: &[Achievement]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO achievement (id, name, category, points, description, unrepeatable)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (id) DO UPDATE SET
                   name = excluded.name, category = excluded.category,
                   points = excluded.points, description = excluded.description,
                   unrepeatable = excluded.unrepeatable",
            )?;
            for achievement in achievements {
                insert.execute(params![
                    achievement.id as i64,
                    achievement.name,
                    achievement.category,
                    achievement.points as i64,
                    achievement.description,
                    achievement.is_unrepeatable as i64,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn achievements(&self) -> Result<HashMap<u32, Achievement>> {
        let mut select = self.connection.prepare(
            "SELECT id, name, category, points, description, unrepeatable FROM achievement",
        )?;
        let rows = select
            .query_map([], |row| {
                let id = row.get::<_, i64>(0)? as u32;
                Ok((
                    id,
                    Achievement {
                        id,
                        name: row.get(1)?,
                        category: row.get(2)?,
                        points: row.get::<_, i64>(3)? as u32,
                        description: row.get(4)?,
                        is_unrepeatable: row.get::<_, i64>(5)? != 0,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    // -- collections ---------------------------------------------------------

    /// Add or update catalogue entries, leaving ownership alone.
    ///
    /// The catalogue and the ownership flag arrive from different endpoints and
    /// at different times, so writing one must never clear the other.
    /// Merged rather than replaced.
    ///
    /// The journal knows the sentence, the artwork and the faction lock; the
    /// web API knows a name. Overwriting one record with the other loses
    /// whichever half arrived first, and the half most often lost is the
    /// artwork — see [`Collectible::merge`].
    pub fn save_collectibles(&mut self, entries: &[Collectible]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut existing =
                transaction.prepare("SELECT json FROM collectible WHERE kind = ?1 AND id = ?2")?;
            let mut insert = transaction.prepare(
                "INSERT INTO collectible (kind, id, json) VALUES (?1, ?2, ?3)
                 ON CONFLICT (kind, id) DO UPDATE SET json = excluded.json",
            )?;

            for entry in entries {
                let kind = format!("{:?}", entry.kind);
                let mut merged = entry.clone();

                if let Ok(json) = existing.query_row(params![kind, entry.id as i64], |row| {
                    row.get::<_, String>(0)
                }) {
                    if let Ok(held) = serde_json::from_str::<Collectible>(&json) {
                        merged.merge(&held);
                    }
                }

                insert.execute(params![
                    kind,
                    entry.id as i64,
                    serde_json::to_string(&merged).unwrap_or_default()
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Replace what is owned for one kind.
    ///
    /// Wholesale, because the profile response is the whole truth about what
    /// the account has — and because nothing is ever un-collected, a merge
    /// would be indistinguishable until the day Blizzard removes something.
    pub fn save_owned(&mut self, kind: Kind, owned: &HashSet<u32>) -> Result<()> {
        let kind_name = format!("{kind:?}");
        let transaction = self.connection.transaction()?;

        // What is owned goes in first, then what is no longer owned comes out
        // — rather than clearing the column and setting it again.
        //
        // The old order wrote every collectible twice on every sync, 1 → 0 and
        // 0 → 1, which is invisible locally and is a thousand rows an account
        // has to send to every other machine to say nothing changed. Nothing
        // is ever un-collected in practice, so the second statement is almost
        // always a no-op and the whole call is silent.
        {
            // An owned id the catalogue has not reached yet still has to be
            // recorded, or a slow catalogue sync would look like a lost
            // collection.
            let mut insert = transaction.prepare(
                "INSERT INTO collectible (kind, id, json, owned) VALUES (?1, ?2, \'{}\', 1)
                 ON CONFLICT (kind, id) DO UPDATE SET owned = 1",
            )?;
            for id in owned {
                insert.execute(params![kind_name, *id as i64])?;
            }
        }

        transaction
            .execute_batch("DROP TABLE IF EXISTS temp.owning; CREATE TEMP TABLE owning (id);")?;
        {
            let mut keep = transaction.prepare("INSERT INTO temp.owning VALUES (?1)")?;
            for id in owned {
                keep.execute(params![*id as i64])?;
            }
        }
        transaction.execute(
            "UPDATE collectible SET owned = 0
             WHERE kind = ?1 AND owned <> 0 AND id NOT IN (SELECT id FROM temp.owning)",
            params![kind_name],
        )?;
        transaction.execute_batch("DROP TABLE IF EXISTS temp.owning;")?;

        transaction.commit()?;
        Ok(())
    }

    /// The catalogue for one kind, and what of it is owned.
    pub fn collectibles(&self, kind: Kind) -> Result<(Vec<Collectible>, HashSet<u32>)> {
        let kind_name = format!("{kind:?}");
        let mut select = self
            .connection
            .prepare("SELECT id, json, owned FROM collectible WHERE kind = ?1 ORDER BY id")?;
        let rows = select
            .query_map(params![kind_name], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut catalogue = Vec::new();
        let mut owned = HashSet::new();
        for (id, json, is_owned) in rows {
            if is_owned {
                owned.insert(id);
            }
            if let Ok(entry) = serde_json::from_str::<Collectible>(&json) {
                catalogue.push(entry);
            }
        }

        if kind == Kind::Toy {
            collapse_toys(&mut catalogue, &mut owned);
        }
        Ok((catalogue, owned))
    }

    // -- prices ---------------------------------------------------------------

    /// Record a snapshot, writing only what moved.
    ///
    /// Returns how many rows were actually written. A stable market writes
    /// almost nothing, which is the entire reason this is affordable.
    pub fn record_prices(
        &mut self,
        realm: u32,
        book: &[Depth],
        at: DateTime<Utc>,
    ) -> Result<usize> {
        let transaction = self.connection.transaction()?;
        let mut written = 0;
        {
            let mut latest = transaction.prepare(
                "SELECT unit_price, quantity, listings, tenth, median FROM price
                 WHERE realm = ?1 AND item_id = ?2 AND variant = ?3
                 ORDER BY seen_at DESC LIMIT 1",
            )?;
            let mut insert = transaction.prepare(
                "INSERT OR REPLACE INTO price
                   (realm, item_id, variant, unit_price, quantity, seen_at,
                    listings, tenth, median)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;

            for entry in book {
                let previous: Option<(i64, i64, i64, i64, i64)> = latest
                    .query_row(params![realm, entry.item_id as i64, entry.variant], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    })
                    .optional()?;

                // Quantity moving without price moving is a sale or a listing,
                // and both are worth a row: the whole inference of "what sold"
                // comes from quantity deltas, since Blizzard records no sale.
                //
                // The three shape columns count as movement too. A book whose
                // floor holds while forty listings become one has changed in
                // the way that matters most, and comparing the floor alone
                // would throw exactly that away.
                let moved = match previous {
                    Some(before) => {
                        before
                            != (
                                entry.cheapest as i64,
                                i64::from(entry.quantity),
                                i64::from(entry.listings),
                                entry.tenth as i64,
                                entry.median as i64,
                            )
                    }
                    None => true,
                };
                if moved {
                    insert.execute(params![
                        realm,
                        entry.item_id as i64,
                        entry.variant,
                        entry.cheapest as i64,
                        i64::from(entry.quantity),
                        at.to_rfc3339(),
                        i64::from(entry.listings),
                        entry.tenth as i64,
                        entry.median as i64,
                    ])?;
                    written += 1;
                }
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    /// One item's price history on one realm, oldest first.
    pub fn price_history(
        &self,
        realm: u32,
        item_id: u32,
    ) -> Result<Vec<(DateTime<Utc>, u64, u32)>> {
        let mut select = self.connection.prepare(
            "SELECT seen_at, unit_price, quantity FROM price
             WHERE realm = ?1 AND item_id = ?2 ORDER BY seen_at",
        )?;
        let rows = select
            .query_map(params![realm, item_id as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u32,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(at, price, quantity)| {
                Some((
                    DateTime::parse_from_rfc3339(&at).ok()?.to_utc(),
                    price,
                    quantity,
                ))
            })
            .collect())
    }

    /// Every series recorded for one item on one realm, oldest first.
    ///
    /// [`Store::price_history`] answers for an item; this answers per variant,
    /// which for item 82800 is per pet. There is no other way to ask: a caged
    /// pet has no item id of its own, so "the price history of Sprite Darter"
    /// is a history of one variant of one item and reading the item whole would
    /// interleave fifteen hundred different pets into one line.
    pub fn price_series(&self, realm: u32, item_id: u32) -> Result<Series> {
        let mut select = self.connection.prepare(
            "SELECT variant, seen_at, unit_price, quantity, listings, tenth, median
             FROM price WHERE realm = ?1 AND item_id = ?2 ORDER BY variant, seen_at",
        )?;
        let rows = select
            .query_map(params![realm, item_id as i64], |row| {
                Ok((row.get::<_, String>(0)?, sample(row)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out: Series = HashMap::new();
        for (variant, sample) in rows {
            out.entry(variant).or_default().push(sample);
        }
        Ok(out)
    }

    /// The items being watched.
    pub fn watched(&self) -> Result<Vec<(u32, String)>> {
        let mut select = self
            .connection
            .prepare("SELECT item_id, name FROM watched ORDER BY name, item_id")?;
        let rows = select
            .query_map([], |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn watch_item(&self, item_id: u32, name: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO watched (item_id, name) VALUES (?1, ?2)
             ON CONFLICT (item_id) DO UPDATE SET name = excluded.name",
            params![item_id as i64, name],
        )?;
        Ok(())
    }

    pub fn unwatch_item(&self, item_id: u32) -> Result<()> {
        self.connection.execute(
            "DELETE FROM watched WHERE item_id = ?1",
            params![item_id as i64],
        )?;
        Ok(())
    }

    /// The connected realms opted into.
    pub fn watched_realms(&self) -> Result<Vec<(u32, String)>> {
        let mut select = self
            .connection
            .prepare("SELECT realm_id, name FROM watched_realm ORDER BY name, realm_id")?;
        let rows = select
            .query_map([], |row| Ok((row.get::<_, i64>(0)? as u32, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn watch_realm(&self, realm_id: u32, name: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO watched_realm (realm_id, name) VALUES (?1, ?2)
             ON CONFLICT (realm_id) DO UPDATE SET name = excluded.name",
            params![realm_id as i64, name],
        )?;
        Ok(())
    }

    pub fn unwatch_realm(&self, realm_id: u32) -> Result<()> {
        self.connection.execute(
            "DELETE FROM watched_realm WHERE realm_id = ?1",
            params![realm_id as i64],
        )?;
        Ok(())
    }

    // -- runs ----------------------------------------------------------------

    /// Save a run, replacing its goals. Returns the run's id.
    ///
    /// Marked current, and everything else un-marked: one run is the one being
    /// looked at, and two current runs would make "the run" ambiguous
    /// everywhere it is read.
    pub fn save_run(&mut self, id: Option<i64>, run: &Run) -> Result<i64> {
        let baseline = serde_json::to_string(&run.baseline).unwrap_or_else(|_| "{}".into());
        let cohort = serde_json::to_string(&run.cohort).unwrap_or_else(|_| "{}".into());

        let key = run_key(run);

        let transaction = self.connection.transaction()?;
        let id = match id {
            Some(id) => {
                transaction.execute(
                    "UPDATE run SET name = ?2, baseline = ?3, cohort = ?4, key = ?5 WHERE id = ?1",
                    params![id, run.name, baseline, cohort, key],
                )?;
                id
            }
            None => {
                // On the key rather than a plain insert: a run is identified
                // by the moment its baseline was taken, and starting one
                // twice from the same baseline is the same run rather than a
                // second one. Before the key existed this made a duplicate
                // quietly; now it would fail on the unique index instead,
                // which is not an improvement.
                transaction.execute(
                    "INSERT INTO run (name, baseline, cohort, is_current, key)
                     VALUES (?1, ?2, ?3, 1, ?4)
                     ON CONFLICT (key) DO UPDATE SET
                       name       = excluded.name,
                       baseline   = excluded.baseline,
                       cohort     = excluded.cohort,
                       is_current = 1",
                    params![run.name, baseline, cohort, key],
                )?;
                transaction.query_row("SELECT id FROM run WHERE key = ?1", params![key], |row| {
                    row.get(0)
                })?
            }
        };
        transaction.execute("UPDATE run SET is_current = (id = ?1)", params![id])?;

        // Upserted rather than deleted and rewritten, and confined to this
        // run. A replan runs on every addon read and produces a goal list that
        // is almost entirely the same one; rewriting it would enqueue every
        // achievement on the account each time, to say that three of them
        // moved.
        reconcile(
            &transaction,
            "goal",
            &["run_id", "achievement_id"],
            &["standing", "bucket", "attestation"],
            Some(("run_id = ?1", vec![SqlValue::Integer(id)])),
            &run.goals
                .iter()
                .map(|goal| {
                    vec![
                        SqlValue::Integer(id),
                        number(goal.achievement_id),
                        text(&serde_json::to_string(&goal.standing).unwrap_or_default()),
                        text(&serde_json::to_string(&goal.bucket).unwrap_or_default()),
                        goal.attestation
                            .as_ref()
                            .and_then(|attestation| serde_json::to_string(attestation).ok())
                            .map(SqlValue::Text)
                            .unwrap_or(SqlValue::Null),
                    ]
                })
                .collect::<Vec<_>>(),
        )?;
        transaction.commit()?;
        Ok(id)
    }

    /// Forget a run and everything planned for it.
    ///
    /// The deliberate end of a run, not a tidy-up: the baseline, the goals,
    /// every attestation and every exclusion go. It exists because a run's
    /// cohort is frozen the moment the baseline is taken, so a run started
    /// around one character cannot be re-aimed at another — the only honest
    /// way to change who a run is about is to start a different one.
    ///
    /// The goals go first. A run row deleted while its goals remained would
    /// leave rows keyed to a run that no longer exists, and `outward_key`
    /// would then refuse to name them on the wire — stranding them on this
    /// machine with nothing to say why.
    pub fn forget_run(&mut self, id: i64) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM goal WHERE run_id = ?1", params![id])?;
        transaction.execute("DELETE FROM run WHERE id = ?1", params![id])?;
        transaction.commit()?;
        Ok(())
    }

    /// The run currently being looked at, if there is one.
    pub fn current_run(&self) -> Result<Option<(i64, Run)>> {
        let row: Option<(i64, String, String, String)> = self
            .connection
            .query_row(
                "SELECT id, name, baseline, cohort FROM run WHERE is_current = 1 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        let Some((id, name, baseline, cohort)) = row else {
            return Ok(None);
        };
        let (Ok(baseline), Ok(cohort)) = (
            serde_json::from_str(&baseline),
            serde_json::from_str(&cohort),
        ) else {
            // A run whose baseline will not parse is a run that cannot be
            // measured against. Reporting no run is honest; reporting an empty
            // one would look like the run had lost its progress.
            return Ok(None);
        };

        let mut select = self.connection.prepare(
            "SELECT achievement_id, standing, bucket, attestation FROM goal WHERE run_id = ?1",
        )?;
        let goals = select
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|(achievement_id, standing, bucket, attestation)| {
                Some(Goal {
                    achievement_id,
                    standing: serde_json::from_str(&standing).ok()?,
                    bucket: serde_json::from_str(&bucket).ok()?,
                    attestation: attestation.and_then(|a| serde_json::from_str(&a).ok()),
                    // Neither is persisted: both are derived from primary data
                    // on every sync, and a stale copy would outlive the data
                    // behind it.
                    nearest: None,
                    evaluation: None,
                })
            })
            .collect();

        Ok(Some((
            id,
            Run {
                name,
                baseline,
                cohort,
                goals,
            },
        )))
    }

    /// Store a response body and the stamp it came with.
    pub fn store_response(
        &self,
        url: &str,
        body: &[u8],
        last_modified: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO response (url, body, last_modified, fetched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (url) DO UPDATE SET
               body = excluded.body,
               last_modified = excluded.last_modified,
               fetched_at = excluded.fetched_at",
            params![url, body, last_modified, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// A stored body, if it is there and still inside its time-to-live.
    pub fn response(&self, url: &str, ttl: Duration) -> Result<Option<Vec<u8>>> {
        let row: Option<(Vec<u8>, String)> = self
            .connection
            .query_row(
                "SELECT body, fetched_at FROM response WHERE url = ?1",
                params![url],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((body, fetched_at)) = row else {
            return Ok(None);
        };
        let Ok(fetched_at) = DateTime::parse_from_rfc3339(&fetched_at) else {
            return Ok(None);
        };

        // Never longer than the terms allow, whatever the caller asked for.
        let ttl = ttl.min(Duration::days(MAX_TTL_DAYS));
        if Utc::now().signed_duration_since(fetched_at.to_utc()) > ttl {
            Ok(None)
        } else {
            Ok(Some(body))
        }
    }

    /// Every stored body whose URL contains `needle` and is still inside `ttl`.
    ///
    /// For reading a whole family of small responses back at once. Restoring
    /// artwork wants the two thousand item-media bodies a session accumulated,
    /// and asking for them one URL at a time means knowing every id first —
    /// which means building the catalogue before the window has drawn anything.
    /// The scan is over a table of a few thousand rows and costs well under a
    /// millisecond.
    pub fn responses_matching(
        &self,
        needle: &str,
        ttl: Duration,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let ttl = ttl.min(Duration::days(MAX_TTL_DAYS));
        let cutoff = (Utc::now() - ttl).to_rfc3339();

        let mut select = self.connection.prepare(
            "SELECT url, body FROM response
             WHERE url LIKE '%' || ?1 || '%' AND fetched_at > ?2",
        )?;
        let rows = select
            .query_map(params![needle, cutoff], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The `Last-Modified` a URL last answered with, for the next conditional
    /// request.
    ///
    /// Deliberately separate from [`Store::response`]: the stamp outlives the
    /// caller's own freshness window, because a body we consider stale is still
    /// the body the server will confirm with a `304`.
    pub fn last_modified(&self, url: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT last_modified FROM response WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Mark a URL as confirmed current without rewriting its body.
    ///
    /// What a `304` means: the copy we hold is still good, and its clock
    /// restarts.
    pub fn touch_response(&self, url: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE response SET fetched_at = ?2 WHERE url = ?1",
            params![url, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    // -- the chronicle -------------------------------------------------------

    /// File the sessions the addon has written, skipping any already held.
    ///
    /// Insert-or-ignore rather than replace, because a session is finished the
    /// moment it is written and never changes again — but the addon keeps its
    /// last forty and rewrites the whole file at every logout, so the same
    /// evenings arrive over and over. Returns how many were new, which is what
    /// decides whether there is anything to tell the person about.
    pub fn save_sessions(&mut self, sessions: &[Session]) -> Result<usize> {
        let transaction = self.connection.transaction()?;
        let mut added = 0;
        {
            // `WHERE NOT EXISTS` rather than a filter in Rust, so that an
            // evening thrown away on another machine is refused the moment
            // that decision arrives, without this having to be told twice.
            let mut insert = transaction.prepare(
                "INSERT OR IGNORE INTO session (realm_slug, name, started_at, ended_at, json)
                 SELECT ?1, ?2, ?3, ?4, ?5
                 WHERE NOT EXISTS (
                   SELECT 1 FROM forgotten
                   WHERE realm_slug = ?1 AND name = ?2 AND started_at = ?3
                 )",
            )?;
            for session in sessions {
                let json = serde_json::to_string(session).unwrap_or_default();
                added += insert.execute(params![
                    session.character.realm_slug,
                    session.character.name,
                    session.started_at.to_rfc3339(),
                    session.ended_at.to_rfc3339(),
                    json,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(added)
    }

    /// The most recent sessions, newest first.
    ///
    /// Bounded because the page draws a card each and a decade of evenings is
    /// not a page. A row that will not deserialise is skipped rather than
    /// failing the lot: one unreadable evening should not empty the journal.
    pub fn sessions(&self, limit: usize) -> Result<Vec<Session>> {
        let mut statement = self.connection.prepare(
            "SELECT json FROM session ORDER BY started_at DESC, realm_slug, name LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;

        let mut sessions = Vec::new();
        for row in rows {
            if let Ok(session) = serde_json::from_str::<Session>(&row?) {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    /// Every entry that has been written, keyed by the evening it is about.
    pub fn entries(&self) -> Result<HashMap<SessionId, Entry>> {
        let mut statement = self.connection.prepare(
            "SELECT realm_slug, name, started_at, title, body, model, written_at FROM entry",
        )?;
        let rows = statement.query_map([], |row| {
            let started_at: String = row.get(2)?;
            let written_at: String = row.get(6)?;
            Ok((
                CharacterKey {
                    realm_slug: row.get(0)?,
                    name: row.get(1)?,
                },
                started_at,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                written_at,
            ))
        })?;

        let mut entries = HashMap::new();
        for row in rows {
            let (character, started_at, title, body, model, written_at) = row?;
            let (Some(started_at), Some(written_at)) = (stamp(&started_at), stamp(&written_at))
            else {
                continue;
            };
            let id = SessionId {
                character,
                started_at,
            };
            entries.insert(
                id.clone(),
                Entry {
                    session: id,
                    title,
                    body,
                    model,
                    written_at,
                },
            );
        }
        Ok(entries)
    }

    /// Keep an entry, replacing any previous one for the same evening.
    ///
    /// Replace rather than ignore, the other way round from a session: asking
    /// for an entry a second time is a deliberate act that costs money, and
    /// somebody who does it wants the new one.
    pub fn save_entry(&self, entry: &Entry) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO entry
               (realm_slug, name, started_at, title, body, model, written_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.session.character.realm_slug,
                entry.session.character.name,
                entry.session.started_at.to_rfc3339(),
                entry.title,
                entry.body,
                entry.model,
                entry.written_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Forget an evening: the record of it and anything written about it.
    ///
    /// This is a journal about a person's own hours, kept on their machine and
    /// with a paragraph of it sent to a third party when they ask for one.
    /// Being able to take an evening back out is not a nicety.
    pub fn forget_session(&self, id: &SessionId) -> Result<()> {
        let key = params![
            id.character.realm_slug,
            id.character.name,
            id.started_at.to_rfc3339()
        ];
        self.connection.execute(
            "DELETE FROM entry WHERE realm_slug = ?1 AND name = ?2 AND started_at = ?3",
            key,
        )?;
        self.connection.execute(
            "DELETE FROM session WHERE realm_slug = ?1 AND name = ?2 AND started_at = ?3",
            key,
        )?;
        // The part that makes it stick. Without this the next addon read puts
        // the evening straight back, and nothing about that looks like a bug
        // from the outside — it looks like Forget doing nothing.
        self.connection.execute(
            "INSERT INTO forgotten (realm_slug, name, started_at, at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (realm_slug, name, started_at) DO NOTHING",
            params![
                id.character.realm_slug,
                id.character.name,
                id.started_at.to_rfc3339(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Drop everything past its time-to-live.
    ///
    /// Blizzard's terms require this, so it runs at startup rather than when
    /// something happens to notice. Returns how many rows went.
    ///
    /// `session` and `entry` are deliberately not here. That term is a
    /// condition on data obtained through Blizzard's API, and neither of those
    /// was: the addon records the user's own play on the user's own machine.
    /// Sweeping them up with the response cache would silently delete the one
    /// thing in this application somebody might still want in ten years.
    pub fn purge(&self) -> Result<usize> {
        // **The sweep is not a statement that the data is gone**, and this is
        // the line that says so. Recording is off for the whole of it, so a
        // deletion here stays on this machine.
        //
        // With it on, one laptop's expiry would travel to the server, the
        // server would delete what it holds, and every other machine would
        // delete its copy on the next pass. A machine that had been switched
        // off for a month would come back and take the last month off
        // everything else. Nothing about that would look like a bug — it
        // would look like the sweep working.
        self.record(false)?;
        let swept = self.sweep();
        self.record(true)?;
        swept
    }

    fn sweep(&self) -> Result<usize> {
        let cutoff = (Utc::now() - Duration::days(MAX_TTL_DAYS)).to_rfc3339();
        let responses = self.connection.execute(
            "DELETE FROM response WHERE fetched_at < ?1",
            params![cutoff],
        )?;
        let prices = self
            .connection
            .execute("DELETE FROM price WHERE seen_at < ?1", params![cutoff])?;
        Ok(responses + prices)
    }
}

/// Fold together the two id spaces a toy lives in.
///
/// Mounts and pets have one identity each and both sources agree on it. Toys do
/// not: the in-game toy box knows an item — `Kang's Bindstone` is item 86571 —
/// and the web API knows a toy, which is a separate, much smaller id space that
/// nothing in the client exposes. Neither number can be derived from the other
/// without asking Blizzard about every toy one at a time.
///
/// So an account with both sources holds most of its toys twice, and the page
/// shows every one of them twice with the collected count silently doubled.
/// Collapsing on the name is what closes that, and it is safe here in a way it
/// would not be for mounts: Blizzard ships several distinct mount ids called
/// `White Stallion`, and no two toys share a name.
///
/// The surviving row is the richer one, which is the one that came from the
/// journal — it carries the icon and the item id the Wowhead link needs. Owning
/// either counts as owning it.
/// Two joins, in order of how much they can be trusted.
///
/// **By item.** A toy's `link_id` is the item it is, and the journal's rows are
/// keyed by that item outright. So a web-API row whose `link_id` names another
/// row's `id` is provably the same toy, and that holds whether or not either
/// has a name.
///
/// **By name**, for what the first join cannot reach. The web API's *index*
/// gives a toy id and a name and never an item, so nothing joins those but the
/// name. It is safe here in a way it would not be for mounts: Blizzard ships
/// several distinct mount ids called `White Stallion`, and no two toys share a
/// name.
/// An RFC 3339 column back as a time, or nothing.
///
/// A row whose stamp will not parse is skipped by every caller rather than
/// defaulted to the epoch: a journal entry dated 1970 is worse than one that is
/// briefly missing.
fn stamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.to_utc())
}

fn collapse_toys(catalogue: &mut Vec<Collectible>, owned: &mut HashSet<u32>) {
    let mut keep: Vec<Collectible> = Vec::with_capacity(catalogue.len());
    let mut by_id: HashMap<u32, usize> = HashMap::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();

    // Richest first, so the row that survives a join is the one worth keeping.
    // In practice that is always the journal's: it is the only source with an
    // icon, and its id is the item id the Wowhead link needs.
    let mut ordered = std::mem::take(catalogue);
    ordered.sort_by_key(|entry| {
        (
            entry.icon.is_none(),
            entry.description.is_none(),
            entry.name.is_empty(),
            // Item ids run to six figures where toy ids are in the low
            // thousands, so the larger is the item when nothing else separates
            // them.
            std::cmp::Reverse(entry.id),
        )
    });

    for entry in ordered {
        let existing = by_id
            .get(&entry.link_id)
            .or_else(|| by_id.get(&entry.id))
            // A nameless row cannot be matched by name. Two of them share
            // nothing but their emptiness, and folding those together would
            // lose one.
            .or_else(|| {
                (!entry.name.is_empty())
                    .then(|| by_name.get(&entry.name))
                    .flatten()
            })
            .copied();

        match existing {
            None => {
                let at = keep.len();
                by_id.insert(entry.id, at);
                if entry.link_id != entry.id {
                    by_id.insert(entry.link_id, at);
                }
                if !entry.name.is_empty() {
                    by_name.insert(entry.name.clone(), at);
                }
                keep.push(entry);
            }
            Some(at) => {
                // Owning it under either id is owning it.
                if owned.remove(&entry.id) {
                    owned.insert(keep[at].id);
                }
                keep[at].merge(&entry);
                if !keep[at].name.is_empty() {
                    by_name.insert(keep[at].name.clone(), at);
                }
                by_id.insert(entry.id, at);
            }
        }
    }

    *catalogue = keep;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(realm: &str, name: &str) -> Character {
        Character {
            key: CharacterKey::new(realm, name),
            id: 5,
            realm_id: 61,
            display_name: name.to_string(),
            realm_name: realm.to_string(),
            level: 80,
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            wow_account_id: 11,
        }
    }

    #[test]
    fn a_roster_round_trips() {
        let mut store = Store::in_memory().expect("a store");
        let roster = Roster::new(vec![
            character("emerald-dream", "Somechar"),
            character("mannoroth", "Aeltor"),
        ]);
        store.save_roster(&roster).expect("saved");
        assert_eq!(store.roster().expect("read"), roster);
    }

    #[test]
    fn saving_a_roster_replaces_rather_than_merges() {
        // Characters get deleted and transferred. A merge would leave ghosts
        // that go on settling goals nothing can account for.
        let mut store = Store::in_memory().expect("a store");
        store
            .save_roster(&Roster::new(vec![
                character("emerald-dream", "Somechar"),
                character("gone", "Ghost"),
            ]))
            .expect("saved");
        store
            .save_roster(&Roster::new(vec![character("emerald-dream", "Somechar")]))
            .expect("saved");

        assert_eq!(store.roster().expect("read").len(), 1);
    }

    #[test]
    fn a_cohort_round_trips() {
        let mut store = Store::in_memory().expect("a store");
        let cohort = Cohort::from(vec![
            CharacterKey::new("emerald-dream", "Somechar"),
            CharacterKey::new("dalaran", "Moodivh"),
        ]);
        store.save_cohort(&cohort).expect("saved");
        assert_eq!(store.cohort().expect("read"), cohort);
    }

    #[test]
    fn a_fresh_body_comes_back_and_a_stale_one_does_not() {
        let store = Store::in_memory().expect("a store");
        store
            .store_response("https://example/x", b"hello", None)
            .expect("stored");

        assert_eq!(
            store
                .response("https://example/x", Duration::hours(1))
                .expect("read"),
            Some(b"hello".to_vec())
        );
        // A zero-length window makes everything already stale.
        assert_eq!(
            store
                .response("https://example/x", Duration::zero())
                .expect("read"),
            None
        );
    }

    #[test]
    fn purging_drops_what_is_past_its_ttl_and_keeps_what_is_not() {
        let store = Store::in_memory().expect("a store");
        store
            .store_response("https://example/fresh", b"new", None)
            .expect("stored");
        store
            .store_response("https://example/ancient", b"old", None)
            .expect("stored");

        // Backdate one past the limit the terms set.
        let long_ago = (Utc::now() - Duration::days(MAX_TTL_DAYS + 1)).to_rfc3339();
        store
            .connection
            .execute(
                "UPDATE response SET fetched_at = ?1 WHERE url = 'https://example/ancient'",
                params![long_ago],
            )
            .expect("backdated");

        assert_eq!(store.purge().expect("purged"), 1);
        assert!(store
            .response("https://example/fresh", Duration::hours(1))
            .expect("read")
            .is_some());
        assert!(store
            .response("https://example/ancient", Duration::hours(1))
            .expect("read")
            .is_none());
    }

    #[test]
    fn a_family_of_responses_comes_back_together_and_respects_the_term() {
        // Restoring artwork reads two thousand item-media bodies at once, and
        // asking for them one URL at a time would mean building the whole
        // catalogue before the window had drawn anything.
        let store = Store::in_memory().expect("a store");
        for url in [
            "https://us.api.blizzard.com/data/wow/media/item/1?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/media/item/2?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/media/achievement/9?namespace=static-us",
            "https://us.api.blizzard.com/data/wow/toy/1?namespace=static-us",
        ] {
            store.store_response(url, b"{}", None).expect("stored");
        }

        let items = store
            .responses_matching("/data/wow/media/item/", Duration::days(30))
            .expect("read");
        assert_eq!(items.len(), 2, "the achievement and the toy are not items");

        // Backdate one past the term. Artwork is not exempt from it: the URL
        // was obtained through the API like everything else.
        let long_ago = (Utc::now() - Duration::days(MAX_TTL_DAYS + 1)).to_rfc3339();
        store
            .connection
            .execute(
                "UPDATE response SET fetched_at = ?1
                 WHERE url LIKE '%/data/wow/media/item/1%'",
                params![long_ago],
            )
            .expect("backdated");

        let items = store
            .responses_matching("/data/wow/media/item/", Duration::days(30))
            .expect("read");
        assert_eq!(items.len(), 1);
        assert!(items[0].0.contains("/item/2"));
    }

    #[test]
    fn a_stamp_outlives_the_body_it_arrived_with() {
        // A body we consider stale is still the body the server will confirm
        // with a 304, so the stamp has to survive the freshness window.
        let store = Store::in_memory().expect("a store");
        store
            .store_response(
                "https://example/x",
                b"hello",
                Some("Wed, 21 Oct 2026 07:28:00 GMT"),
            )
            .expect("stored");

        assert_eq!(
            store
                .response("https://example/x", Duration::zero())
                .expect("read"),
            None
        );
        assert_eq!(
            store.last_modified("https://example/x").expect("read"),
            Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string())
        );
    }

    #[test]
    fn a_not_modified_restarts_the_clock_without_rewriting_the_body() {
        let store = Store::in_memory().expect("a store");
        store
            .store_response("https://example/x", b"hello", Some("stamp"))
            .expect("stored");

        let long_ago = (Utc::now() - Duration::days(10)).to_rfc3339();
        store
            .connection
            .execute("UPDATE response SET fetched_at = ?1", params![long_ago])
            .expect("backdated");
        assert!(store
            .response("https://example/x", Duration::days(1))
            .expect("read")
            .is_none());

        store.touch_response("https://example/x").expect("touched");
        assert_eq!(
            store
                .response("https://example/x", Duration::days(1))
                .expect("read"),
            Some(b"hello".to_vec())
        );
    }

    /// One commodity's book, flat: every unit at one price.
    fn depth_of(item_id: u32, price: u64, quantity: u32) -> Depth {
        Depth {
            item_id,
            variant: String::new(),
            cheapest: price,
            quantity,
            listings: 1,
            tenth: price,
            median: price,
        }
    }

    #[test]
    fn the_shape_of_a_book_changing_is_worth_a_row_on_its_own() {
        // A floor that holds while forty listings become one has changed in the
        // way that matters most, and comparing the cheapest price alone would
        // throw exactly that away.
        let mut store = Store::in_memory().expect("a store");
        let at = Utc::now();
        let mut before = depth_of(1, 100, 500);
        before.listings = 40;
        store.record_prices(0, &[before], at).expect("recorded");

        let mut after = depth_of(1, 100, 500);
        after.listings = 1;
        assert_eq!(
            store
                .record_prices(0, &[after], at + Duration::hours(1))
                .expect("recorded"),
            1
        );
    }

    #[test]
    fn a_stable_market_writes_almost_nothing() {
        // The whole reason a desktop application can hold price history at all.
        // Five realms of raw hourly JSON is about 3 GB a day; deltas are what
        // make it affordable.
        let mut store = Store::in_memory().expect("a store");
        let snapshot = [depth_of(197_794, 56_523, 400)];

        let first = Utc::now();
        assert_eq!(
            store.record_prices(0, &snapshot, first).expect("recorded"),
            1
        );
        // Same price, same quantity, an hour later: nothing to say.
        assert_eq!(
            store
                .record_prices(0, &snapshot, first + Duration::hours(1))
                .expect("recorded"),
            0
        );
    }

    #[test]
    fn a_quantity_moving_is_worth_a_row_even_when_the_price_does_not() {
        // Blizzard records no sale at all — quantity simply disappears between
        // snapshots — so the whole inference of "what sold" comes from these
        // deltas. Skipping them would throw that away.
        let mut store = Store::in_memory().expect("a store");
        let at = Utc::now();
        store
            .record_prices(0, &[depth_of(1, 100, 50)], at)
            .expect("recorded");

        assert_eq!(
            store
                .record_prices(0, &[depth_of(1, 100, 30)], at + Duration::hours(1))
                .expect("recorded"),
            1
        );
    }

    #[test]
    fn a_variant_is_priced_as_a_different_thing() {
        // Two copies of one item id with different bonus ids are not the same
        // item, and one history for both would be meaningless.
        let mut store = Store::in_memory().expect("a store");
        let at = Utc::now();
        let written = store
            .record_prices(
                0,
                &[depth_of(1, 100, 1), {
                    let mut gear = depth_of(1, 90_000, 1);
                    gear.variant = "b1532".into();
                    gear
                }],
                at,
            )
            .expect("recorded");
        assert_eq!(written, 2);
    }

    #[test]
    fn a_history_comes_back_oldest_first() {
        let mut store = Store::in_memory().expect("a store");
        let at = Utc::now();
        store
            .record_prices(0, &[depth_of(1, 300, 1)], at)
            .expect("recorded");
        store
            .record_prices(0, &[depth_of(1, 100, 1)], at + Duration::hours(1))
            .expect("recorded");
        store
            .record_prices(0, &[depth_of(1, 200, 1)], at + Duration::hours(2))
            .expect("recorded");

        let history = store.price_history(0, 1).expect("history");
        assert_eq!(
            history
                .iter()
                .map(|(_, price, _)| *price)
                .collect::<Vec<_>>(),
            [300, 100, 200]
        );
    }

    #[test]
    fn realms_and_commodities_keep_separate_histories() {
        // Commodities are region-wide and gear is realm-locked. One history for
        // both would average across markets that never meet.
        let mut store = Store::in_memory().expect("a store");
        let at = Utc::now();
        store
            .record_prices(0, &[depth_of(1, 100, 1)], at)
            .expect("recorded");
        store
            .record_prices(61, &[depth_of(1, 900, 1)], at)
            .expect("recorded");

        assert_eq!(store.price_history(0, 1).expect("history")[0].1, 100);
        assert_eq!(store.price_history(61, 1).expect("history")[0].1, 900);
    }

    #[test]
    fn purging_covers_price_history_because_the_terms_do() {
        let mut store = Store::in_memory().expect("a store");
        let ancient = Utc::now() - Duration::days(MAX_TTL_DAYS + 1);
        store
            .record_prices(0, &[depth_of(1, 100, 1)], ancient)
            .expect("recorded");
        store
            .record_prices(0, &[depth_of(2, 100, 1)], Utc::now())
            .expect("recorded");

        assert_eq!(store.purge().expect("purged"), 1);
        assert!(store.price_history(0, 1).expect("history").is_empty());
        assert_eq!(store.price_history(0, 2).expect("history").len(), 1);
    }

    /// One evening, `days` ago.
    fn evening(name: &str, days: i64) -> Session {
        use crate::character::Faction;
        use crate::chronicle::{Happening, Moment};

        let started_at = Utc::now() - Duration::days(days);
        Session {
            character: CharacterKey::new("emerald-dream", name),
            display_name: name.to_string(),
            realm_name: "Emerald Dream".into(),
            class: "Druid".into(),
            race: "Tauren".into(),
            faction: Faction::Horde,
            started_at,
            ended_at: started_at + Duration::hours(2),
            start_level: 70,
            end_level: 70,
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

    #[test]
    fn sessions_round_trip_newest_first() {
        let mut store = Store::in_memory().expect("a store");
        store
            .save_sessions(&[evening("Somechar", 3), evening("Velkurai", 1)])
            .expect("saved");

        let sessions = store.sessions(10).expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].display_name, "Velkurai");
        assert_eq!(sessions[0].moments.len(), 1);
    }

    #[test]
    fn an_evening_already_held_is_not_written_again() {
        // The addon keeps its last forty sessions and rewrites the whole file
        // at every logout, so the same evenings arrive over and over. Only the
        // new ones are worth telling anybody about.
        let mut store = Store::in_memory().expect("a store");
        let seen = evening("Somechar", 2);
        assert_eq!(
            store
                .save_sessions(std::slice::from_ref(&seen))
                .expect("saved"),
            1
        );
        assert_eq!(
            store
                .save_sessions(std::slice::from_ref(&seen))
                .expect("saved"),
            0
        );
        assert_eq!(
            store
                .save_sessions(&[seen, evening("Somechar", 1)])
                .expect("saved"),
            1
        );
        assert_eq!(store.sessions(10).expect("sessions").len(), 2);
    }

    #[test]
    fn an_entry_replaces_the_one_before_it() {
        // Asking for a second entry costs money and is a deliberate act, so
        // somebody who does it wants the new one.
        let mut store = Store::in_memory().expect("a store");
        let session = evening("Somechar", 1);
        store
            .save_sessions(std::slice::from_ref(&session))
            .expect("saved");

        let mut entry = Entry {
            session: session.id(),
            title: "First Light".into(),
            body: "The wind off the plains.".into(),
            model: "claude-opus-5".into(),
            written_at: Utc::now(),
        };
        store.save_entry(&entry).expect("saved");
        entry.title = "Second Thoughts".into();
        store.save_entry(&entry).expect("saved");

        let entries = store.entries().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[&session.id()].title, "Second Thoughts");
    }

    #[test]
    fn an_evening_thrown_away_does_not_come_back_on_the_next_addon_read() {
        // The failure this exists for: `forget_session` deleted the row and
        // nothing else, but the addon's own file is the source and is re-read
        // on every launch. The evening came straight back, which from the
        // outside looks exactly like Forget doing nothing at all.
        let mut store = Store::in_memory().expect("a store");
        let session = evening("Mattydormu", 4);
        store
            .save_sessions(std::slice::from_ref(&session))
            .expect("saved");
        assert_eq!(store.sessions(10).expect("read").len(), 1);

        store.forget_session(&session.id()).expect("forgotten");
        assert!(store.sessions(10).expect("read").is_empty());

        // The addon file still holds it, and the next read offers it again.
        let added = store
            .save_sessions(std::slice::from_ref(&session))
            .expect("read again");

        assert_eq!(added, 0, "the evening was re-imported");
        assert!(
            store.sessions(10).expect("read").is_empty(),
            "a forgotten evening came back"
        );
    }

    #[test]
    fn forgetting_one_evening_does_not_refuse_the_others() {
        let mut store = Store::in_memory().expect("a store");
        let gone = evening("Mattydormu", 4);
        let kept = evening("Mattydormu", 3);
        store
            .save_sessions(&[gone.clone(), kept.clone()])
            .expect("saved");

        store.forget_session(&gone.id()).expect("forgotten");
        let survivor = kept.started_at;
        store.save_sessions(&[gone, kept]).expect("read again");

        let held = store.sessions(10).expect("read");
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].started_at, survivor);
    }

    #[test]
    fn a_journal_is_never_purged_because_it_is_not_the_apis_to_take_back() {
        // The thirty-day term is a condition on data obtained through
        // Blizzard's API. A session came off the addon — the user's own client,
        // recording the user's own play — and sweeping it up with the response
        // cache would silently delete the one thing here somebody might still
        // want in ten years.
        let mut store = Store::in_memory().expect("a store");
        let ancient = evening("Somechar", MAX_TTL_DAYS + 400);
        store
            .save_sessions(std::slice::from_ref(&ancient))
            .expect("saved");
        store
            .save_entry(&Entry {
                session: ancient.id(),
                title: "A Long Time Ago".into(),
                body: "It was.".into(),
                model: "claude-opus-5".into(),
                written_at: Utc::now() - Duration::days(MAX_TTL_DAYS + 400),
            })
            .expect("saved");

        store.purge().expect("purged");

        assert_eq!(store.sessions(10).expect("sessions").len(), 1);
        assert_eq!(store.entries().expect("entries").len(), 1);
    }

    #[test]
    fn forgetting_an_evening_takes_the_entry_with_it() {
        // A journal about somebody's own hours, with a paragraph of it sent to
        // a third party when they ask. Taking one back out is not a nicety.
        let mut store = Store::in_memory().expect("a store");
        let session = evening("Somechar", 1);
        store
            .save_sessions(std::slice::from_ref(&session))
            .expect("saved");
        store
            .save_entry(&Entry {
                session: session.id(),
                title: "Best Forgotten".into(),
                body: "Quite.".into(),
                model: "claude-opus-5".into(),
                written_at: Utc::now(),
            })
            .expect("saved");

        store.forget_session(&session.id()).expect("forgotten");

        assert!(store.sessions(10).expect("sessions").is_empty());
        assert!(store.entries().expect("entries").is_empty());
    }

    #[test]
    fn a_tally_survives_an_addon_folder_being_cleared() {
        // Same merge, same reason: four hundred flasks is months of work and a
        // reinstall starts the tally at one.
        let mut store = Store::in_memory().expect("a store");
        let key = CharacterKey::new("emerald-dream", "Somechar");

        let flasks = |count| Tally {
            kind: Counting::Recipe,
            key: "371637".into(),
            label: "Flask of Alchemical Chaos".into(),
            count,
        };

        let mut collected = Collected::default();
        collected.tallies.insert(key.clone(), vec![flasks(412)]);
        store.save_collected(&collected).expect("save");

        // The addon comes back from zero.
        collected.tallies.insert(key.clone(), vec![flasks(1)]);
        store.save_collected(&collected).expect("save again");

        let counted = store.tallies().expect("read");
        assert_eq!(counted[&key][0].count, 412);
    }

    #[test]
    fn earned_reputation_survives_an_addon_that_started_counting_again() {
        // The merge that matters. A reinstalled addon starts from zero, and a
        // run that forgot a year of somebody's work because a folder was
        // cleared would be worse than one that never recorded it.
        use crate::provenance::{Earned, EarnedReputation};

        let mut store = Store::in_memory().expect("a store");
        let key = CharacterKey::new("emerald-dream", "Somechar");

        let mut collected = Collected::default();
        let mut earned = Earned::default();
        earned.reputation.insert(
            2170,
            EarnedReputation {
                points: 21_000,
                renown: 4,
                renown_seen: 25,
                account_wide: true,
            },
        );
        collected.earned.insert(key.clone(), earned);
        store.save_collected(&collected).expect("saved");

        // The addon comes back from zero after a reinstall.
        let mut fresh = Collected::default();
        let mut restarted = Earned::default();
        restarted.reputation.insert(
            2170,
            EarnedReputation {
                points: 500,
                renown: 0,
                renown_seen: 25,
                account_wide: true,
            },
        );
        fresh.earned.insert(key.clone(), restarted);
        store.save_collected(&fresh).expect("saved");

        let held = store.provenance().expect("provenance");
        let with = held[&key].with(2170);
        assert_eq!(with.points, 21_000, "the larger of the two survives");
        assert_eq!(with.renown, 4);
    }

    #[test]
    fn currency_provenance_round_trips_with_its_flags() {
        // The flags are the whole classification. Losing `tracks_earned` on the
        // way through would turn every transferable currency into a transfer.
        use crate::provenance::{Earned, EarnedCurrency, Origin};

        let mut store = Store::in_memory().expect("a store");
        let key = CharacterKey::new("emerald-dream", "Somechar");

        let mut collected = Collected::default();
        let mut earned = Earned::default();
        earned.currency.insert(
            3_008,
            EarnedCurrency {
                gained: 1_000,
                earned: 600,
                tracks_earned: true,
                account_wide: true,
                transferable: true,
            },
        );
        collected.earned.insert(key.clone(), earned);
        store.save_collected(&collected).expect("saved");

        let held = store.provenance().expect("provenance");
        let currency = held[&key].currency[&3_008];
        assert!(currency.tracks_earned);
        assert!(currency.transferable);
        assert_eq!(currency.origin(), Origin::Transferred);
        assert_eq!(currency.creditable(), 600);
    }

    #[test]
    fn watching_is_opt_in_and_round_trips() {
        // Ingesting every item on five realms to answer questions nobody asked
        // is how a desktop application becomes a service.
        let store = Store::in_memory().expect("a store");
        assert!(store.watched().expect("watched").is_empty());
        assert!(store.watched_realms().expect("realms").is_empty());

        store.watch_item(197794, "Mycobloom").expect("watched");
        store.watch_realm(61, "Emerald Dream").expect("watched");
        assert_eq!(
            store.watched().expect("watched"),
            [(197794, "Mycobloom".to_string())]
        );
        assert_eq!(
            store.watched_realms().expect("realms"),
            [(61, "Emerald Dream".to_string())]
        );

        store.unwatch_item(197794).expect("unwatched");
        store.unwatch_realm(61).expect("unwatched");
        assert!(store.watched().expect("watched").is_empty());
        assert!(store.watched_realms().expect("realms").is_empty());
    }

    #[test]
    fn a_store_survives_being_reopened() {
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("armory.db");

        let mut store = Store::open(&path).expect("a store");
        store
            .save_roster(&Roster::new(vec![character("emerald-dream", "Somechar")]))
            .expect("saved");
        drop(store);

        let store = Store::open(&path).expect("reopened");
        assert_eq!(store.roster().expect("read").len(), 1);
    }

    #[test]
    fn forgetting_a_run_takes_its_goals_with_it() {
        use crate::run::{Baseline, Bucket, Goal, Run, Standing};

        let mut store = Store::in_memory().expect("a store");
        let run = Run {
            name: "Fresh start".into(),
            baseline: Baseline {
                taken_at: Utc::now(),
                collected: vec![],
                completed: vec![],
            },
            cohort: Cohort::default(),
            goals: vec![Goal {
                achievement_id: 1234,
                standing: Standing::Unearned,
                bucket: Bucket::Observable,
                attestation: None,
                nearest: None,
                evaluation: None,
            }],
        };
        let id = store.save_run(None, &run).expect("saved");
        assert!(store.current_run().expect("read").is_some());

        store.forget_run(id).expect("forgotten");

        assert!(store.current_run().expect("read").is_none());
        let orphans: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM goal", [], |row| row.get(0))
            .expect("counted");
        assert_eq!(orphans, 0, "goals outlived the run they belong to");
    }

    #[test]
    fn a_database_from_before_sharing_opens_and_its_runs_get_names() {
        // The costliest migration here, because the fallback hides it: if
        // `migrate` fails, `open_store` quietly falls back to an in-memory
        // store and the account looks *empty* rather than broken.
        //
        // Two ways this could fail, and both are covered. The `run.key` column
        // arrives through `ADDED`, so a unique index over it created in the
        // same batch as the tables would fail on a database that has no such
        // column yet; and two runs carrying the default empty key would
        // collide on that index the moment it was built.
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("armory.db");

        {
            let connection = Connection::open(&path).expect("a connection");
            connection
                .execute_batch(
                    "CREATE TABLE run (
                       id       INTEGER PRIMARY KEY AUTOINCREMENT,
                       name     TEXT NOT NULL,
                       baseline TEXT NOT NULL,
                       cohort   TEXT NOT NULL,
                       is_current INTEGER NOT NULL DEFAULT 0
                     );
                     INSERT INTO run (name, baseline, cohort, is_current)
                       VALUES ('First',  '{\"taken_at\":\"2025-01-01T00:00:00Z\",\"collected\":[],\"completed\":[]}', '{}', 0);
                     INSERT INTO run (name, baseline, cohort, is_current)
                       VALUES ('Second', '{\"taken_at\":\"2026-01-01T00:00:00Z\",\"collected\":[],\"completed\":[]}', '{}', 1);
                     INSERT INTO run (name, baseline, cohort, is_current)
                       VALUES ('Broken', 'not json', '{}', 0);",
                )
                .expect("the older shape");
        }

        let store = Store::open(&path).expect("the old database still opens");

        let keys: Vec<String> = store
            .connection
            .prepare("SELECT key FROM run ORDER BY id")
            .expect("prepared")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collected");

        assert_eq!(keys.len(), 3);
        assert!(keys.iter().all(|key| !key.is_empty()), "{keys:?}");
        // Including the one whose baseline will not parse — it still needs a
        // key of its own, or the index cannot be built at all.
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), 3, "two runs share a key: {keys:?}");

        // And the key is the one `run_key` would give it, so saving the run
        // again does not rename it into a second run.
        assert_eq!(keys[0], "run-1735689600");
    }

    #[test]
    fn an_older_database_gains_the_columns_added_since() {
        // The failure this exists for was silent and total. `item` and `price`
        // both predate columns of their own; `CREATE TABLE IF NOT EXISTS` left
        // the older tables exactly as they were, so every insert naming one of
        // those columns failed against an install that looked healthy — item
        // ids where names should be, and not one row of price history ever.
        let directory = tempfile::tempdir().expect("a directory");
        let path = directory.path().join("armory.db");

        {
            let connection = Connection::open(&path).expect("a connection");
            connection
                .execute_batch(
                    "CREATE TABLE item (
                       item_id INTEGER PRIMARY KEY,
                       name    TEXT NOT NULL
                     );
                     CREATE TABLE price (
                       realm      INTEGER NOT NULL,
                       item_id    INTEGER NOT NULL,
                       variant    TEXT NOT NULL,
                       unit_price INTEGER NOT NULL,
                       quantity   INTEGER NOT NULL,
                       seen_at    TEXT NOT NULL,
                       PRIMARY KEY (realm, item_id, variant, seen_at)
                     );
                     INSERT INTO item (item_id, name)
                       VALUES (10607, 'Schematic: Deepdive Helmet');",
                )
                .expect("the older shape");
        }

        let mut store = Store::open(&path).expect("a store");

        // The rows already there still read, at the defaults.
        let items = store.items().expect("items");
        assert_eq!(items[&10607].name, "Schematic: Deepdive Helmet");
        assert!(items[&10607].sellable);

        // And the writes that had been failing land.
        store
            .name_item(
                2589,
                &Item {
                    name: "Linen Cloth".into(),
                    sellable: true,
                    quality: Some("COMMON".into()),
                },
            )
            .expect("named");
        assert_eq!(store.item_names().expect("names")[&2589], "Linen Cloth");

        let written = store
            .record_prices(
                0,
                &[Depth {
                    item_id: 2589,
                    variant: String::new(),
                    cheapest: 1200,
                    quantity: 40,
                    listings: 4,
                    tenth: 1300,
                    median: 1500,
                }],
                Utc::now(),
            )
            .expect("recorded");
        assert_eq!(written, 1);
    }

    // -- collectibles ---------------------------------------------------------

    fn collectible(kind: Kind, id: u32, name: &str) -> Collectible {
        Collectible {
            kind,
            id,
            name: name.to_string(),
            source: crate::source::blizzard::collections::Source::Unknown,
            description: None,
            flavour: None,
            icon: None,
            display: None,
            faction: None,
            link_id: id,
            tradeable: None,
        }
    }

    #[test]
    fn an_index_sync_does_not_take_the_artwork_off_a_mount() {
        // The journal has the creature display and the web API does not. If the
        // later write won outright, one sync after a logout would leave the
        // whole collection with no pictures.
        let mut store = Store::in_memory().expect("a store");

        let mut from_journal = collectible(Kind::Mount, 6, "Brown Horse");
        from_journal.display = Some(2404);
        from_journal.icon = Some(132261);
        from_journal.description = Some("Vendor: Unger Statforth".into());
        from_journal.link_id = 458;
        store
            .save_collectibles(&[from_journal])
            .expect("the journal's reading");

        // The index knows a name and nothing else.
        store
            .save_collectibles(&[collectible(Kind::Mount, 6, "Brown Horse")])
            .expect("the index's reading");

        let (catalogue, _) = store.collectibles(Kind::Mount).expect("read back");
        let entry = catalogue.first().expect("the mount");
        assert_eq!(
            entry.display,
            Some(2404),
            "the render is what draws the row"
        );
        assert_eq!(entry.icon, Some(132261));
        assert_eq!(entry.link_id, 458, "a mount is linked by its spell");
        assert!(entry.description.is_some());
    }

    #[test]
    fn a_toy_known_to_both_sources_is_one_toy() {
        // The toy box knows item 86571; the web API knows toy 1153. Left alone
        // that is two rows, two entries in the list, and a collected count with
        // every toy in it twice.
        let mut store = Store::in_memory().expect("a store");

        let mut from_journal = collectible(Kind::Toy, 86571, "Kang's Bindstone");
        from_journal.icon = Some(134458);
        let from_api = collectible(Kind::Toy, 1153, "Kang's Bindstone");

        store
            .save_collectibles(&[from_journal, from_api])
            .expect("both readings");
        store
            .save_owned(Kind::Toy, &HashSet::from([1153]))
            .expect("the web API says it is owned");

        let (catalogue, owned) = store.collectibles(Kind::Toy).expect("read back");
        assert_eq!(catalogue.len(), 1, "one toy, not two");

        let entry = catalogue.first().expect("the toy");
        assert_eq!(entry.id, 86571, "the item id survives, so the link works");
        assert_eq!(entry.icon, Some(134458));
        assert!(
            owned.contains(&86571),
            "owning it under either id is owning it"
        );
        assert!(!owned.contains(&1153), "the folded id stops being a thing");
    }

    #[test]
    fn two_mounts_that_share_a_name_stay_two_mounts() {
        // Blizzard ships several distinct mount ids called `White Stallion`.
        // Collapsing those would quietly shrink the collection.
        let mut store = Store::in_memory().expect("a store");
        store
            .save_collectibles(&[
                collectible(Kind::Mount, 8, "White Stallion"),
                collectible(Kind::Mount, 9, "White Stallion"),
            ])
            .expect("saved");

        let (catalogue, _) = store.collectibles(Kind::Mount).expect("read back");
        assert_eq!(catalogue.len(), 2);
    }

    #[test]
    fn owning_something_the_catalogue_has_not_reached_is_still_recorded() {
        // The ownership call and the catalogue call land at different times. An
        // id owned before its name arrives is kept as ownership without being
        // invented into a catalogue entry, so a slow catalogue sync does not
        // read as a lost collection.
        let mut store = Store::in_memory().expect("a store");
        store
            .save_owned(Kind::Toy, &HashSet::from([1, 2]))
            .expect("owned");

        let (catalogue, owned) = store.collectibles(Kind::Toy).expect("read back");
        assert!(catalogue.is_empty(), "nothing is known about them yet");
        assert_eq!(owned.len(), 2);
    }

    #[test]
    fn a_toy_is_joined_to_its_item_even_with_no_name_to_match_on() {
        // The web API's toy detail names the item it wraps; the journal keys
        // its rows by that item outright. So the two are provably the same toy
        // whether or not either has a name — and for a while the web API's rows
        // had none, because a toy's name lives on its item.
        let mut from_journal = collectible(Kind::Toy, 32566, "Muradin's Favor");
        from_journal.icon = Some(134458);
        let mut from_api = collectible(Kind::Toy, 4, "");
        from_api.link_id = 32566;

        let mut catalogue = vec![from_api, from_journal];
        let mut owned = HashSet::from([4u32]);
        collapse_toys(&mut catalogue, &mut owned);

        assert_eq!(catalogue.len(), 1);
        assert_eq!(catalogue[0].id, 32566);
        assert_eq!(catalogue[0].name, "Muradin's Favor");
        assert_eq!(owned, HashSet::from([32566]));
    }

    #[test]
    fn nameless_toys_are_not_folded_into_each_other() {
        // Two entries sharing nothing but an absent name are two toys. Matching
        // on that emptiness would lose one of them.
        let mut catalogue = vec![
            collectible(Kind::Toy, 1, ""),
            collectible(Kind::Toy, 2, ""),
            collectible(Kind::Toy, 3, "Kang's Bindstone"),
        ];
        let mut owned = HashSet::from([1u32]);

        collapse_toys(&mut catalogue, &mut owned);

        assert_eq!(catalogue.len(), 3);
        assert_eq!(owned, HashSet::from([1]));
    }
}
