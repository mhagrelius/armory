//! What a row is when it is between machines.
//!
//! Armory is not a document that two people edit. It is an account being
//! *recorded* — an addon writing down what happened, an API answering what is
//! held, and a person making a handful of decisions on top. Almost nothing in
//! it can genuinely conflict, and the merges that settle what little can are
//! already written and already tested in [`crate::store`]: a tally takes the
//! larger count, a collectible merges field by field, an evening is written
//! once and never again.
//!
//! So this is not Brain's three-way merge or Planner's base snapshot. It is a
//! change log and a cursor.
//!
//! # The change log
//!
//! Every write through [`crate::store::Store`] notes the rows it touched in a
//! `change` table: the scope, the key, and an autoincrementing `seq`. A row
//! written twice keeps one entry with the later `seq`, so the log is the size
//! of the data rather than the size of the history.
//!
//! A client pushes the entries it has and pulls everything above its cursor.
//! The server does the same thing in reverse and is otherwise the same store —
//! *literally* the same code, which is the whole argument for the split. Two
//! implementations of `save_collected`'s merge is two things to keep level, and
//! the one that drifts is the one nobody is looking at.
//!
//! # Only what changed
//!
//! The trap this exists to avoid: `save_collected` used to delete a table and
//! rewrite it, which is fine for a store nobody else reads and catastrophic
//! for a log — one addon read would enqueue every criterion the account has
//! ever seen, tens of thousands of rows, every two seconds. Every wholesale
//! rewrite in the store now upserts and reports what genuinely moved, so a
//! steady state enqueues nothing at all.
//!
//! **A pass that is not empty when nothing happened means something is
//! re-uploading the account on a timer**, and there is a test for exactly
//! that.
//!
//! # Whose row it is
//!
//! Each change carries the machine that made it, and a pull excludes the
//! caller's own. Without that, a client's first push comes straight back down
//! as a pull — fifty thousand rows it already has, applied to no effect. The
//! machine id is a random string made once per install and kept in the
//! settings file; it names an installation, not a person and not a computer.

use serde::{Deserialize, Serialize};

/// A table that travels.
///
/// Twenty-seven of them: everything the store holds except `change`, which
/// is the log, and `sync_state`, which is the cursor beside it. There is no
/// opt-out list to keep in step with the schema — a new table is either here
/// or it does not travel, and `no_table_is_left_out_of_the_wire_by_accident`
/// is the test that says so out loud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Character,
    Enrolment,
    Detail,
    Attribution,
    Currency,
    EarnedReputation,
    EarnedCurrency,
    Tally,
    Recipe,
    RecipeReagent,
    Instance,
    Encounter,
    Criterion,
    WarbandItem,
    PetHeld,
    Run,
    Goal,
    Collectible,
    Achievement,
    Price,
    Snapshot,
    Item,
    Watched,
    WatchedRealm,
    Session,
    Entry,
    Response,
}

impl Scope {
    /// The description of this scope's table.
    pub fn table(self) -> &'static Table {
        TABLES
            .iter()
            .find(|table| table.scope == self)
            .expect("every scope has a table; the test says so")
    }

    /// The scope a wire name refers to, if this build knows it.
    ///
    /// `None` for a scope a newer build sends and this one has no table for.
    /// The row is counted and dropped rather than guessed at — the same rule
    /// `CriterionKind::from_catalogue` follows, and for the same reason: a
    /// wrong mapping writes a number that means something else.
    pub fn named(name: &str) -> Option<Scope> {
        TABLES
            .iter()
            .find(|table| table.name == name)
            .map(|table| table.scope)
    }

    pub fn name(self) -> &'static str {
        self.table().name
    }
}

/// What a column is, and what happens when both sides have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub name: &'static str,
    pub rule: Rule,
}

impl Column {
    const fn take(name: &'static str) -> Column {
        Column {
            name,
            rule: Rule::Take,
        }
    }
    const fn max(name: &'static str) -> Column {
        Column {
            name,
            rule: Rule::Max,
        }
    }
    const fn stamp(name: &'static str) -> Column {
        Column {
            name,
            rule: Rule::Stamp,
        }
    }
}

/// How one column settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The arriving value wins, subject to the row's [`Guard`].
    Take,
    /// The larger number wins, whichever side it is on.
    ///
    /// The counters no Blizzard system keeps — tallies, earned reputation,
    /// earned currency. They are cumulative and there is nowhere to get them
    /// back from, so a machine that has been away and is behind must not be
    /// able to take them off a machine that is ahead. This is also the one
    /// rule under which the order rows arrive in does not matter at all.
    Max,
    /// Deserialise both sides and merge them field by field.
    ///
    /// One column, `collectible.json`, because the two sources of a
    /// collectible know different halves of it: the addon has the source prose
    /// and the artwork, the web API has the name and the expansion. Taking
    /// either whole takes the other's half off.
    MergeJson,
    /// A `Vec<u8>` column, base64 on the wire.
    ///
    /// One column, `response.body`. JSON has no bytes and hex would double
    /// what is already the largest thing here.
    Blob,
    /// Taken like [`Rule::Take`], but never the reason a row travels.
    ///
    /// Exactly the four columns a table is guarded on, and the correspondence
    /// is the point: **the stamp a row is judged by is not itself news.**
    ///
    /// Without this, every one of those tables re-sends itself on a timer.
    /// `store_response` writes `fetched_at` on every conditional request, so a
    /// body that came back unchanged would travel again in full; a realm's
    /// snapshot rewrites `seen_at` on tens of thousands of rows every hour,
    /// so an idle market would push a realm an hour to say nothing had
    /// happened. What is bought with that is small and worth naming: a row
    /// whose stamp moved and whose contents did not stays on the machine that
    /// refreshed it, so another machine's copy is stamped when it last
    /// *changed* rather than when it was last confirmed.
    Stamp,
}

/// When an arriving row is allowed to land at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// Always. For tables where every column carries its own rule, or where
    /// there is nothing to be later than.
    Always,
    /// Only if the arriving row's stamp is at or past the held one's.
    ///
    /// The columns named here are all timestamps the writer already keeps for
    /// its own reasons — `detail.fetched_at`, `snapshot.seen_at`,
    /// `entry.written_at`, `response.fetched_at` — so this costs no schema.
    Newer(&'static str),
    /// Never overwrite. The row is written once and what is held wins.
    ///
    /// An evening and a price at an instant are both statements about a moment
    /// that has passed. A second copy of one is the same copy.
    Keep,
}

/// One table, as the wire sees it.
#[derive(Debug, Clone, Copy)]
pub struct Table {
    pub scope: Scope,
    /// The SQL table name, and the name on the wire. One name, so a mismatch
    /// is impossible rather than merely unlikely.
    pub name: &'static str,
    /// The primary key columns, in order.
    pub key: &'static [&'static str],
    /// Everything else.
    pub columns: &'static [Column],
    pub guard: Guard,
    /// Which key column, if any, is a local row id that means nothing on
    /// another machine.
    ///
    /// One table: `goal.run_id`. `run.id` is an `AUTOINCREMENT` and two
    /// machines will pick different ones for the same run, so on the wire the
    /// column carries `run.key` instead and is translated back on the way in.
    /// A goal for a run this machine has never heard of is dropped rather than
    /// attached to whichever run happens to be current.
    pub local_id: Option<(usize, Scope)>,
}

impl Table {
    /// Every column, key first, in the order a row is written.
    pub fn all_columns(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.key
            .iter()
            .copied()
            .chain(self.columns.iter().map(|column| column.name))
    }
}

/// Every table that travels.
pub const TABLES: &[Table] = &[
    Table {
        scope: Scope::Character,
        name: "character",
        key: &["realm_slug", "name"],
        columns: &[
            Column::take("character_id"),
            Column::take("realm_id"),
            Column::take("display_name"),
            Column::take("realm_name"),
            Column::take("level"),
            Column::take("class"),
            Column::take("race"),
            Column::take("faction"),
            Column::take("wow_account_id"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Enrolment,
        name: "enrolment",
        key: &["realm_slug", "name"],
        columns: &[],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Detail,
        name: "detail",
        key: &["realm_slug", "name"],
        columns: &[Column::take("json"), Column::stamp("fetched_at")],
        guard: Guard::Newer("fetched_at"),
        local_id: None,
    },
    Table {
        scope: Scope::Attribution,
        name: "attribution",
        key: &["achievement_id"],
        columns: &[Column::take("realm_slug"), Column::take("name")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Currency,
        name: "currency",
        key: &["realm_slug", "name", "currency_id"],
        columns: &[Column::take("amount")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::EarnedReputation,
        name: "earned_reputation",
        key: &["realm_slug", "name", "faction_id"],
        columns: &[
            Column::max("points"),
            Column::max("renown"),
            Column::max("renown_seen"),
            Column::take("account_wide"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::EarnedCurrency,
        name: "earned_currency",
        key: &["realm_slug", "name", "currency_id"],
        columns: &[
            Column::max("gained"),
            Column::max("earned"),
            Column::take("tracks_earned"),
            Column::take("account_wide"),
            Column::take("transferable"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Tally,
        name: "tally",
        key: &["realm_slug", "name", "kind", "key"],
        columns: &[Column::max("count"), Column::take("label")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Recipe,
        name: "recipe",
        key: &["realm_slug", "name", "recipe_id"],
        columns: &[
            Column::take("recipe"),
            Column::take("output_id"),
            Column::take("makes"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::RecipeReagent,
        name: "recipe_reagent",
        key: &["realm_slug", "name", "recipe_id", "slot"],
        columns: &[Column::take("quantity"), Column::take("tiers")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Instance,
        name: "instance",
        key: &["id"],
        columns: &[
            Column::take("name"),
            Column::take("map"),
            Column::take("description"),
            Column::take("expansion"),
            Column::take("encounters"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Encounter,
        name: "encounter",
        key: &["id"],
        columns: &[
            Column::take("name"),
            Column::take("description"),
            Column::take("loot"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Criterion,
        name: "criterion",
        key: &["criterion_id"],
        columns: &[Column::take("kind")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::WarbandItem,
        name: "warband_item",
        key: &["item_id"],
        columns: &[Column::take("count")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::PetHeld,
        name: "pet_held",
        key: &["species_id"],
        columns: &[Column::take("count")],
        guard: Guard::Always,
        local_id: None,
    },
    // Before `goal`, always: a goal names its run and is dropped if the run is
    // not here. Rows travel in `seq` order and `save_run` writes the run first,
    // so this is the order they arrive in as well as the order they are listed.
    Table {
        scope: Scope::Run,
        name: "run",
        key: &["key"],
        columns: &[
            Column::take("name"),
            Column::take("baseline"),
            Column::take("cohort"),
            Column::take("is_current"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Goal,
        name: "goal",
        key: &["run_id", "achievement_id"],
        columns: &[
            Column::take("standing"),
            Column::take("bucket"),
            Column::take("attestation"),
        ],
        guard: Guard::Always,
        local_id: Some((0, Scope::Run)),
    },
    Table {
        scope: Scope::Collectible,
        name: "collectible",
        key: &["kind", "id"],
        columns: &[
            Column {
                name: "json",
                rule: Rule::MergeJson,
            },
            Column::max("owned"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Achievement,
        name: "achievement",
        key: &["id"],
        columns: &[
            Column::take("name"),
            Column::take("category"),
            Column::take("points"),
            Column::take("description"),
            Column::take("unrepeatable"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Price,
        name: "price",
        key: &["realm", "item_id", "variant", "seen_at"],
        columns: &[
            Column::take("unit_price"),
            Column::take("quantity"),
            Column::take("listings"),
            Column::take("tenth"),
            Column::take("median"),
        ],
        guard: Guard::Keep,
        local_id: None,
    },
    Table {
        scope: Scope::Snapshot,
        name: "snapshot",
        key: &["realm", "item_id", "variant"],
        columns: &[
            Column::take("cheapest"),
            Column::take("quantity"),
            Column::take("listings"),
            Column::take("tenth"),
            Column::take("median"),
            Column::stamp("seen_at"),
        ],
        guard: Guard::Newer("seen_at"),
        local_id: None,
    },
    Table {
        scope: Scope::Item,
        name: "item",
        key: &["item_id"],
        columns: &[
            Column::take("name"),
            Column::take("sellable"),
            Column::take("quality"),
        ],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Watched,
        name: "watched",
        key: &["item_id"],
        columns: &[Column::take("name")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::WatchedRealm,
        name: "watched_realm",
        key: &["realm_id"],
        columns: &[Column::take("name")],
        guard: Guard::Always,
        local_id: None,
    },
    Table {
        scope: Scope::Session,
        name: "session",
        key: &["realm_slug", "name", "started_at"],
        columns: &[Column::take("ended_at"), Column::take("json")],
        guard: Guard::Keep,
        local_id: None,
    },
    Table {
        scope: Scope::Entry,
        name: "entry",
        key: &["realm_slug", "name", "started_at"],
        columns: &[
            Column::take("title"),
            Column::take("body"),
            Column::take("model"),
            Column::stamp("written_at"),
        ],
        guard: Guard::Newer("written_at"),
        local_id: None,
    },
    Table {
        scope: Scope::Response,
        name: "response",
        key: &["url"],
        columns: &[
            Column {
                name: "body",
                rule: Rule::Blob,
            },
            Column::take("last_modified"),
            Column::stamp("fetched_at"),
        ],
        guard: Guard::Newer("fetched_at"),
        local_id: None,
    },
];

/// The largest cached body worth carrying across the network.
///
/// Every profile, catalogue, media and game-data body Armory holds is a few
/// hundred kilobytes at most and passes this comfortably. One class of body
/// does not: a connected realm's auction dump is tens of megabytes and is
/// replaced every hour, which is gigabytes a day between three machines to
/// re-send something both ends can fetch in seconds and which is *already*
/// reduced into `snapshot` and `price` rows that do travel.
///
/// A body over the ceiling is left where it is rather than truncated. Nothing
/// depends on its presence — [`crate::store::Store::response`] answering
/// `None` is the ordinary cache miss.
pub const MAX_BODY: usize = 4 * 1024 * 1024;

/// How large a batch may get before it is sent short.
///
/// A batch is bounded by a row count *and* by this, and the second bound is
/// the one that matters. Most rows here are tens of bytes and two thousand of
/// them is nothing — but `response` rows carry a whole cached body, and two
/// thousand of those is hundreds of megabytes. The server refuses a body past
/// its own ceiling, and a client that had built one would rebuild the same one
/// on every pass: not a slow sync, a permanently stuck one.
///
/// A batch always carries at least one row, however large it is, so a single
/// body can never wedge the queue either. One row is bounded by [`MAX_BODY`]
/// at four megabytes, which is five and a half encoded — comfortably inside
/// what the server will read.
pub const MAX_PARCEL: usize = 16 * 1024 * 1024;

/// Roughly what a row will weigh on the wire.
///
/// The key and the small fields are counted properly; the point of the
/// exercise is the one field that can be enormous, so precision elsewhere buys
/// nothing.
pub fn weight(row: &Row) -> usize {
    let fields: usize = row
        .fields
        .iter()
        .flatten()
        .map(|value| match value {
            serde_json::Value::String(text) => text.len() + 2,
            other => other.to_string().len(),
        })
        .sum();
    row.scope.len() + row.key.len() * 8 + fields + 32
}

/// One row, on its way between machines.
///
/// `key` and `fields` are positional, in the order [`Table::key`] and
/// [`Table::columns`] give — the names are already agreed by both ends
/// through [`TABLES`], and repeating them on every row of a fifty-thousand-row
/// first push is most of the bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// The table's name, not the enum's — so a build that has never heard of a
    /// scope can still say which one it skipped.
    pub scope: String,
    pub key: Vec<serde_json::Value>,
    /// Absent when the row is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<serde_json::Value>>,
}

impl Row {
    /// A row that is no longer there.
    ///
    /// Deletion is rare here and every case of it is somebody or something
    /// saying so out loud: a character transferred off the account, an item
    /// unwatched, an evening forgotten. It is never inferred from absence —
    /// a table this machine has not filled in yet is not a table another
    /// machine should be emptied of.
    pub fn is_gone(&self) -> bool {
        self.fields.is_none()
    }
}

/// A batch, in `seq` order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Parcel {
    pub rows: Vec<Row>,
}

/// What applying a parcel did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub written: usize,
    pub removed: usize,
    /// Held back because what is here is newer, or because the table keeps
    /// what it has. Not a failure — the commonest way two machines agree.
    pub kept: usize,
    /// A scope this build has no table for, a row whose shape does not match
    /// the table, or a goal whose run has not arrived. Counted rather than
    /// guessed at, and reported, because a number climbing here is the shape
    /// of a version skew.
    pub unreadable: usize,
}

impl Applied {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn len(&self) -> usize {
        self.written + self.removed
    }
}

/// Why a pass could not finish.
///
/// One string, because every one of them means the same thing to the caller:
/// the server could not be reached or did not agree, so try again later. The
/// text is for the sync page and a log line, not for branching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError(pub String);

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

/// What a pull brought back, and where to ask from next time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pulled {
    pub parcel: Parcel,
    /// The cursor to send as `since` next time.
    pub cursor: i64,
    /// Whether the server has more above this cursor. A first sync is many
    /// batches, and a client that stopped after one would look like it had
    /// finished.
    pub more: bool,
}

/// The other side, whatever is carrying it.
///
/// A trait so the core does not learn what a socket is. The GTK shell answers
/// it over plain HTTP; `sync-check` answers it in-process against a real
/// server, which is what makes a two-machine test possible without two
/// machines.
pub trait Remote {
    /// Send these up. The server applies them with the same rules this store
    /// would and stamps them with this machine's id.
    fn push(&self, parcel: &Parcel) -> Result<Applied, SyncError>;

    /// Everything above `since` that this machine did not write itself.
    fn pull(&self, since: i64, limit: usize) -> Result<Pulled, SyncError>;

    /// Block until the server has something above `since`, or until it gives
    /// up waiting.
    ///
    /// What makes an evening recorded on the other machine appear here in
    /// about as long as the network takes rather than on the next timer.
    fn wait(&self, since: i64) -> Result<bool, SyncError>;
}

// -- base64 -------------------------------------------------------------------
//
// Forty lines rather than a dependency, for one column. The alphabet is the
// standard one with padding; nothing here is ever put in a URL.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `None` for anything that is not base64, rather than a partial decode.
///
/// A body half read is worse than one absent: the cache would answer with it
/// and every parser downstream would report a Blizzard problem.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut held = 0u32;
    for byte in text.bytes() {
        if byte == b'=' || byte == b'\n' || byte == b'\r' {
            continue;
        }
        let value = ALPHABET.iter().position(|c| *c == byte)? as u32;
        buffer = (buffer << 6) | value;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((buffer >> held) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_scope_has_exactly_one_table_and_every_table_its_own_name() {
        let mut scopes = HashSet::new();
        let mut names = HashSet::new();
        for table in TABLES {
            assert!(
                scopes.insert(table.scope),
                "{:?} is described twice",
                table.scope
            );
            assert!(names.insert(table.name), "two tables called {}", table.name);
        }
    }

    #[test]
    fn a_table_name_is_the_wire_name() {
        for table in TABLES {
            assert_eq!(Scope::named(table.name), Some(table.scope));
            assert_eq!(table.scope.name(), table.name);
        }
    }

    #[test]
    fn a_scope_this_build_has_never_heard_of_is_none_rather_than_a_guess() {
        assert_eq!(Scope::named("transmog"), None);
    }

    #[test]
    fn no_table_names_a_column_twice() {
        for table in TABLES {
            let mut seen = HashSet::new();
            for column in table.all_columns() {
                assert!(seen.insert(column), "{} names {column} twice", table.name);
            }
        }
    }

    #[test]
    fn a_guard_names_a_column_the_table_actually_has() {
        for table in TABLES {
            if let Guard::Newer(stamp) = table.guard {
                assert!(
                    table.all_columns().any(|column| column == stamp),
                    "{} guards on {stamp}, which it has no column for",
                    table.name
                );
            }
        }
    }

    #[test]
    fn the_column_a_table_is_guarded_on_is_never_itself_news() {
        // The rule `Rule::Stamp` exists for. A guard column that counted as a
        // change would make every one of these tables re-send itself every
        // time it was merely refreshed.
        for table in TABLES {
            let Guard::Newer(stamp) = table.guard else {
                continue;
            };
            let column = table
                .columns
                .iter()
                .find(|column| column.name == stamp)
                .unwrap_or_else(|| panic!("{} guards on a key column", table.name));
            assert_eq!(
                column.rule,
                Rule::Stamp,
                "{}.{stamp} is what the row is judged by and must not be what makes it travel",
                table.name
            );
        }
    }

    #[test]
    fn only_goal_carries_a_local_id_and_it_points_at_a_key_column() {
        for table in TABLES {
            let Some((position, scope)) = table.local_id else {
                continue;
            };
            assert_eq!(table.scope, Scope::Goal);
            assert_eq!(scope, Scope::Run);
            assert!(position < table.key.len());
        }
    }

    #[test]
    fn a_run_is_described_before_its_goals() {
        // Not decoration: a goal whose run has not arrived is dropped, and the
        // order rows are applied in is the order they were written in.
        let run = TABLES.iter().position(|t| t.scope == Scope::Run).unwrap();
        let goal = TABLES.iter().position(|t| t.scope == Scope::Goal).unwrap();
        assert!(run < goal);
    }

    #[test]
    fn base64_round_trips_including_the_awkward_lengths() {
        for length in 0..32 {
            let bytes: Vec<u8> = (0..length).map(|n| (n * 7 + 3) as u8).collect();
            let encoded = encode_base64(&bytes);
            assert_eq!(encoded.len() % 4, 0, "padding is not optional");
            assert_eq!(decode_base64(&encoded).as_deref(), Some(&bytes[..]));
        }
    }

    #[test]
    fn base64_matches_the_worked_example() {
        assert_eq!(encode_base64(b"Armory"), "QXJtb3J5");
        assert_eq!(encode_base64(b"M"), "TQ==");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(decode_base64("QXJtb3J5").unwrap(), b"Armory");
    }

    #[test]
    fn something_that_is_not_base64_decodes_to_nothing_rather_than_to_half() {
        assert_eq!(decode_base64("not base64!"), None);
    }

    #[test]
    fn a_row_without_fields_is_a_deletion() {
        let gone = Row {
            scope: "watched".into(),
            key: vec![serde_json::json!(1234)],
            fields: None,
        };
        assert!(gone.is_gone());
        // And it says so on the wire by leaving the field out entirely, rather
        // than by sending a null somebody could read as an empty row.
        let encoded = serde_json::to_string(&gone).unwrap();
        assert!(!encoded.contains("fields"), "{encoded}");
    }
}
