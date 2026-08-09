//! The store as one of several copies of itself.
//!
//! [`crate::sync`] says what a row is; this is the half that reads one out of
//! SQLite and writes one back in. It is deliberately not in `store.rs`: that
//! file is what Armory asks the database, and this is what two Armories say to
//! each other, and mixing them makes both harder to read.
//!
//! Both ends run this. The client's `change` table is an outbox — filled by
//! local writes, drained when the server has taken them. The server's is a
//! log — filled by what clients push, kept, and read back by every *other*
//! client. One table, two lifecycles, and the difference is entirely in who
//! calls [`Store::drain`].

use rusqlite::types::{ToSqlOutput, Value as SqlValue, ValueRef};
use rusqlite::{params_from_iter, OptionalExtension};
use serde_json::Value;

use crate::source::blizzard::collections::Collectible;
use crate::store::{Store, StoreError};
use crate::sync::{Applied, Guard, Parcel, Pulled, Row, Rule, Scope, Table, MAX_BODY, MAX_PARCEL};

type Result<T> = std::result::Result<T, StoreError>;

/// Whether writing a row should also put it in the log, and under whose name.
///
/// The distinction that stops a sync looping. A client applying what it pulled
/// must not enqueue it for pushing back; a server applying what a client
/// pushed must, or no other machine ever hears about it.
#[derive(Debug, Clone, Copy)]
pub enum Recording<'a> {
    Off,
    As(&'a str),
}

/// Where a cursor is kept, by name.
///
/// Two of them, and they are not the same number. `PULLED` is the server's
/// `seq` this machine has taken everything up to. The push side needs no
/// cursor at all — the outbox is the queue, and an entry leaves it when the
/// server has it.
pub const PULLED: &str = "pulled";

impl Store {
    // -- identity ------------------------------------------------------------

    /// This installation's name in the log.
    pub fn machine(&self) -> String {
        self.setting("machine")
            .unwrap_or_default()
            .unwrap_or_default()
    }

    /// Name this installation, once, at startup.
    ///
    /// An id rather than a hostname: two machines can share a hostname, a
    /// hostname changes, and the only question this answers is "did I write
    /// this row" — which wants a value nothing else can accidentally take.
    pub fn set_machine(&self, id: &str) -> Result<()> {
        self.set_setting("machine", id)
    }

    // -- the small key/value table ------------------------------------------

    pub fn setting(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM sync_state WHERE name = ?1",
                [name],
                |row| row.get::<_, String>(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, name: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sync_state (name, value) VALUES (?1, ?2)
             ON CONFLICT (name) DO UPDATE SET value = excluded.value",
            [name, value],
        )?;
        Ok(())
    }

    pub fn cursor(&self, name: &str) -> i64 {
        self.setting(name)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    pub fn set_cursor(&self, name: &str, value: i64) -> Result<()> {
        self.set_setting(name, &value.to_string())
    }

    // -- the log ------------------------------------------------------------

    /// Turn the change log off, or back on.
    ///
    /// Read by every trigger in `Store::triggers`. Off for exactly two
    /// things: applying what was pulled, which must not be enqueued to go
    /// straight back up, and `Store::purge`, whose deletions are this
    /// machine's expiry rather than a statement that the data is gone.
    pub fn record(&self, on: bool) -> Result<()> {
        self.set_setting("recording", if on { "1" } else { "0" })
    }

    /// Whether writes are being logged. Only ever false inside a call that
    /// puts it back, and reset to true on every open, so a process that dies
    /// half way through one does not leave an installation that has silently
    /// stopped syncing.
    pub fn recording(&self) -> bool {
        self.setting("recording").ok().flatten().as_deref() != Some("0")
    }

    /// How much is waiting to go up, by scope.
    ///
    /// The number the sync page is about. Ordered by scope so the page does
    /// not reshuffle between passes.
    pub fn queued(&self) -> Result<Vec<(String, usize)>> {
        let mut select = self.connection.prepare(
            "SELECT scope, COUNT(*) FROM change GROUP BY scope ORDER BY COUNT(*) DESC, scope",
        )?;
        let rows = select
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The oldest thing waiting, if anything is.
    pub fn queued_since(&self) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT MIN(at) FROM change", [], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten())
    }

    // -- reading rows out ----------------------------------------------------

    /// The next batch to push, and the highest `seq` in it.
    ///
    /// A row whose change entry says it is still there but which is not — a
    /// purge took it, and purges are not logged — is dropped from the batch
    /// and its entry with it. So is a cached body over [`MAX_BODY`].
    pub fn outbox(&self, limit: usize) -> Result<(Parcel, i64)> {
        let mut select = self
            .connection
            .prepare("SELECT seq, scope, key, gone FROM change ORDER BY seq LIMIT ?1")?;
        let entries = select
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut parcel = Parcel::default();
        let mut through = 0;
        let mut carrying = 0;

        for (seq, scope_name, key_json, gone) in entries {
            // Full enough. The entries above this one stay in the queue and go
            // in the next batch — `through` is not advanced past them, which
            // is the whole of what stops a cached body wedging the pass.
            if carrying >= MAX_PARCEL && !parcel.rows.is_empty() {
                break;
            }
            through = seq;
            let Some(scope) = Scope::named(&scope_name) else {
                continue;
            };
            let Ok(key) = serde_json::from_str::<Vec<Value>>(&key_json) else {
                continue;
            };

            if gone {
                // A goal whose run is gone cannot be named on the wire, and a
                // local row id would name a different run on the machine that
                // received it.
                let Some(key) = self.outward_key(scope, &key)? else {
                    continue;
                };
                parcel.rows.push(Row {
                    scope: scope_name,
                    key,
                    fields: None,
                });
                continue;
            }

            let Some(fields) = self.read_row(scope, &key)? else {
                continue;
            };
            let Some(key) = self.outward_key(scope, &key)? else {
                continue;
            };
            let row = Row {
                scope: scope_name,
                key,
                fields: Some(fields),
            };
            carrying += crate::sync::weight(&row);
            parcel.rows.push(row);
        }

        Ok((parcel, through))
    }

    /// Forget everything the server has taken.
    pub fn drain(&self, through: i64) -> Result<usize> {
        Ok(self
            .connection
            .execute("DELETE FROM change WHERE seq <= ?1", [through])?)
    }

    /// The server's side of a pull: everything above `since` that `machine`
    /// did not write.
    pub fn log_since(&self, since: i64, machine: &str, limit: usize) -> Result<Pulled> {
        let mut select = self.connection.prepare(
            "SELECT seq, scope, key, gone FROM change
             WHERE seq > ?1 AND machine <> ?2
             ORDER BY seq LIMIT ?3",
        )?;
        let entries = select
            .query_map(rusqlite::params![since, machine, limit as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let taken = entries.len();
        let mut pulled = Pulled {
            cursor: since,
            ..Pulled::default()
        };
        let mut carrying = 0;
        let mut cut_short = false;

        for (seq, scope_name, key_json, gone) in entries {
            if carrying >= MAX_PARCEL && !pulled.parcel.rows.is_empty() {
                cut_short = true;
                break;
            }
            pulled.cursor = seq;
            let Some(scope) = Scope::named(&scope_name) else {
                continue;
            };
            let Ok(key) = serde_json::from_str::<Vec<Value>>(&key_json) else {
                continue;
            };
            if gone {
                let Some(key) = self.outward_key(scope, &key)? else {
                    continue;
                };
                pulled.parcel.rows.push(Row {
                    scope: scope_name,
                    key,
                    fields: None,
                });
                continue;
            }
            let Some(fields) = self.read_row(scope, &key)? else {
                continue;
            };
            let Some(key) = self.outward_key(scope, &key)? else {
                continue;
            };
            let row = Row {
                scope: scope_name,
                key,
                fields: Some(fields),
            };
            carrying += crate::sync::weight(&row);
            pulled.parcel.rows.push(row);
        }

        // `more` is about the log, not about the parcel: a batch that dropped
        // every row it read still moved the cursor, and a client that stopped
        // there would never reach what is above it. A batch cut short for size
        // is the same thing said a second way.
        pulled.more = cut_short || taken == limit;
        Ok(pulled)
    }

    /// The highest `seq` the log holds.
    pub fn high_water(&self) -> i64 {
        self.connection
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM change", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0)
    }

    /// Whether anything above `since` was written by somebody other than
    /// `machine`. The question `/wait` parks on.
    pub fn anything_since(&self, since: i64, machine: &str) -> bool {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM change WHERE seq > ?1 AND machine <> ?2)",
                rusqlite::params![since, machine],
                |row| row.get::<_, i64>(0),
            )
            .map(|held| held != 0)
            .unwrap_or(false)
    }

    /// The value columns of one row, in table order.
    fn read_row(&self, scope: Scope, key: &[Value]) -> Result<Option<Vec<Value>>> {
        let table = scope.table();
        if key.len() != table.key.len() {
            return Ok(None);
        }
        let columns: Vec<&str> = table.columns.iter().map(|column| column.name).collect();
        if columns.is_empty() {
            // `enrolment` is nothing but its key. Being there is the whole
            // record, so an empty field list is the row rather than a failure.
            let held: Option<i64> = self
                .connection
                .query_row(
                    &format!("SELECT 1 FROM {} WHERE {}", table.name, wheres(table)),
                    params_from_iter(key.iter().map(binding)),
                    |row| row.get(0),
                )
                .optional()?;
            return Ok(held.map(|_| Vec::new()));
        }

        // Asked before the body is read rather than after, so an auction dump
        // is never pulled into memory only to be dropped. The ceiling is on
        // the raw bytes, which is the number `MAX_BODY` is written in —
        // measuring the base64 instead means arithmetic that is off by one at
        // exactly the size nobody tests.
        if scope == Scope::Response {
            let size: Option<i64> = self
                .connection
                .query_row(
                    &format!(
                        "SELECT length(body) FROM {} WHERE {}",
                        table.name,
                        wheres(table)
                    ),
                    params_from_iter(key.iter().map(binding)),
                    |row| row.get(0),
                )
                .optional()?;
            if size.is_some_and(|size| size > MAX_BODY as i64) {
                return Ok(None);
            }
        }

        let sql = format!(
            "SELECT {} FROM {} WHERE {}",
            columns.join(", "),
            table.name,
            wheres(table)
        );
        let mut select = self.connection.prepare(&sql)?;
        let row = select
            .query_row(params_from_iter(key.iter().map(binding)), |row| {
                let mut out = Vec::with_capacity(columns.len());
                for (index, column) in table.columns.iter().enumerate() {
                    out.push(cell(row.get_ref(index)?, column.rule));
                }
                Ok(out)
            })
            .optional()?;

        Ok(row)
    }

    /// The key as another machine should see it.
    ///
    /// `Some(key)` unchanged for every table but `goal`, whose first key
    /// column is a local `run.id` and travels as the run's stable key.
    /// `None` when the run it names is gone, which makes the goal unsendable
    /// rather than sendable-and-wrong.
    fn outward_key(&self, scope: Scope, key: &[Value]) -> Result<Option<Vec<Value>>> {
        let table = scope.table();
        let Some((position, _)) = table.local_id else {
            return Ok(Some(key.to_vec()));
        };
        let Some(Value::Number(id)) = key.get(position) else {
            return Ok(None);
        };
        let held: Option<String> = self
            .connection
            .query_row(
                "SELECT key FROM run WHERE id = ?1",
                [id.as_i64().unwrap_or_default()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(run_key) = held.filter(|key| !key.is_empty()) else {
            return Ok(None);
        };
        let mut out = key.to_vec();
        out[position] = Value::String(run_key);
        Ok(Some(out))
    }

    /// The key as this machine holds it.
    fn inward_key(&self, scope: Scope, key: &[Value]) -> Result<Option<Vec<Value>>> {
        let table = scope.table();
        let Some((position, _)) = table.local_id else {
            return Ok(Some(key.to_vec()));
        };
        let Some(Value::String(run_key)) = key.get(position) else {
            return Ok(None);
        };
        let held: Option<i64> = self
            .connection
            .query_row("SELECT id FROM run WHERE key = ?1", [run_key], |row| {
                row.get(0)
            })
            .optional()?;
        let Some(id) = held else {
            return Ok(None);
        };
        let mut out = key.to_vec();
        out[position] = Value::Number(id.into());
        Ok(Some(out))
    }

    // -- writing rows in -----------------------------------------------------

    /// Apply a batch, in the order it arrived.
    ///
    /// Order matters for exactly one pair: a goal names its run, so the run
    /// has to land first. Both are written by `save_run` in that order and
    /// the log preserves it, so nothing here has to sort.
    pub fn apply(&mut self, parcel: &Parcel, recording: Recording<'_>) -> Result<Applied> {
        // Whose name the triggers write, and whether they write at all. On a
        // client this is off: a pulled row enqueued for pushing is a row two
        // machines hand each other forever. On the server it is on and under
        // the *pushing* machine's name, because the log's whole job there is
        // to tell every other machine, and to tell that one nothing.
        let mine = self.machine();
        match recording {
            Recording::Off => self.record(false)?,
            Recording::As(machine) => {
                self.record(true)?;
                self.set_setting("machine", machine)?;
            }
        }

        let mut applied = Applied::default();
        for row in &parcel.rows {
            match self.apply_row(row) {
                Ok(outcome) => match outcome {
                    Outcome::Written => applied.written += 1,
                    Outcome::Removed => applied.removed += 1,
                    Outcome::Kept => applied.kept += 1,
                    Outcome::Unreadable => applied.unreadable += 1,
                },
                // A single bad row must not lose the batch it arrived in.
                Err(_) => applied.unreadable += 1,
            }
        }

        self.record(true)?;
        self.set_setting("machine", &mine)?;
        Ok(applied)
    }

    fn apply_row(&self, row: &Row) -> Result<Outcome> {
        let Some(scope) = Scope::named(&row.scope) else {
            return Ok(Outcome::Unreadable);
        };
        let table = scope.table();

        let Some(key) = self.inward_key(scope, &row.key)? else {
            return Ok(Outcome::Unreadable);
        };
        if key.len() != table.key.len() {
            return Ok(Outcome::Unreadable);
        }

        let Some(fields) = &row.fields else {
            let removed = self.connection.execute(
                &format!("DELETE FROM {} WHERE {}", table.name, wheres(table)),
                params_from_iter(key.iter().map(binding)),
            )?;
            if removed == 0 {
                return Ok(Outcome::Kept);
            }
            return Ok(Outcome::Removed);
        };

        if fields.len() != table.columns.len() {
            return Ok(Outcome::Unreadable);
        }

        if matches!(table.guard, Guard::Newer(_)) && !self.is_newer(table, &key, fields)? {
            return Ok(Outcome::Kept);
        }

        let settled = self.settle(table, &key, fields)?;
        let written = self.upsert(table, &key, &settled)?;
        if written == 0 {
            return Ok(Outcome::Kept);
        }
        self.only_one_current_run(scope, &key)?;
        Ok(Outcome::Written)
    }

    /// A run arriving as the current one makes every other run not current.
    ///
    /// The one place a row landing has to touch a row beside it, and it is
    /// here because "the current run" is a singleton — `Store::current_run`
    /// takes the first it finds, so two of them makes which run Armory is
    /// about depend on the order SQLite happens to return. `save_run` keeps
    /// the same invariant locally with the same statement; this is that rule
    /// applied to a run that arrived rather than one that was started.
    fn only_one_current_run(&self, scope: Scope, key: &[Value]) -> Result<()> {
        if scope != Scope::Run {
            return Ok(());
        }
        let Some(Value::String(run_key)) = key.first() else {
            return Ok(());
        };
        let current: Option<i64> = self
            .connection
            .query_row(
                "SELECT is_current FROM run WHERE key = ?1",
                [run_key],
                |row| row.get(0),
            )
            .optional()?;
        if current != Some(1) {
            return Ok(());
        }
        self.connection.execute(
            "UPDATE run SET is_current = 0 WHERE key <> ?1 AND is_current <> 0",
            [run_key],
        )?;
        Ok(())
    }

    /// Whether an arriving row is at least as recent as the one held.
    ///
    /// Compared as text, which is right for these four: every one of them is
    /// an RFC 3339 stamp written in UTC by this same code, and those sort in
    /// the order they happened. A stamp that will not compare is treated as
    /// older than nothing, so the arriving row lands — the alternative is a
    /// row that can never be corrected.
    fn is_newer(&self, table: &Table, key: &[Value], fields: &[Value]) -> Result<bool> {
        let Guard::Newer(stamp) = table.guard else {
            return Ok(true);
        };
        let Some(position) = table.columns.iter().position(|c| c.name == stamp) else {
            return Ok(true);
        };
        let arriving = fields[position].as_str().unwrap_or_default().to_string();
        let held: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT {stamp} FROM {} WHERE {}", table.name, wheres(table)),
                params_from_iter(key.iter().map(binding)),
                |row| row.get(0),
            )
            .optional()?;
        Ok(match held {
            None => true,
            Some(held) => arriving >= held,
        })
    }

    /// Turn the arriving values into the ones that should be stored, given
    /// what is held: the larger of two counts, two halves of a collectible
    /// merged, a body decoded.
    fn settle(&self, table: &Table, key: &[Value], fields: &[Value]) -> Result<Vec<SqlValue>> {
        let needs_held = table
            .columns
            .iter()
            .any(|column| matches!(column.rule, Rule::Max | Rule::MergeJson));

        let held: Option<Vec<Value>> = if needs_held {
            let names: Vec<&str> = table.columns.iter().map(|column| column.name).collect();
            let sql = format!(
                "SELECT {} FROM {} WHERE {}",
                names.join(", "),
                table.name,
                wheres(table)
            );
            let mut select = self.connection.prepare(&sql)?;
            select
                .query_row(params_from_iter(key.iter().map(binding)), |row| {
                    let mut out = Vec::with_capacity(names.len());
                    for (index, column) in table.columns.iter().enumerate() {
                        out.push(cell(row.get_ref(index)?, column.rule));
                    }
                    Ok(out)
                })
                .optional()?
        } else {
            None
        };

        let mut out = Vec::with_capacity(table.columns.len());
        for (index, column) in table.columns.iter().enumerate() {
            let arriving = &fields[index];
            let mine = held.as_ref().and_then(|row| row.get(index));
            out.push(match column.rule {
                Rule::Take | Rule::Stamp => sql_value(arriving),
                Rule::Max => {
                    let theirs = arriving.as_i64().unwrap_or(0);
                    let ours = mine.and_then(Value::as_i64).unwrap_or(i64::MIN);
                    SqlValue::Integer(theirs.max(ours))
                }
                Rule::MergeJson => SqlValue::Text(merge_json(
                    arriving.as_str().unwrap_or("{}"),
                    mine.and_then(Value::as_str).unwrap_or("{}"),
                )),
                Rule::Blob => match arriving.as_str().and_then(crate::sync::decode_base64) {
                    Some(bytes) => SqlValue::Blob(bytes),
                    // Refusing beats writing half a body into the cache.
                    None => return Err(StoreError::Sqlite(rusqlite::Error::InvalidQuery)),
                },
            });
        }
        Ok(out)
    }

    /// Write the row, and report whether anything actually changed.
    ///
    /// The `WHERE` on the `DO UPDATE` is what makes that report true: without
    /// it an upsert that set every column to the value already there still
    /// counts as one row affected, every arriving row looks like news, and on
    /// the server every one of them goes into the log for every other machine
    /// to pull back. Two clients would then hand the same rows to each other
    /// forever.
    fn upsert(&self, table: &Table, key: &[Value], values: &[SqlValue]) -> Result<usize> {
        let names: Vec<&str> = table.all_columns().collect();
        let placeholders = (1..=names.len())
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(", ");

        let conflict = if table.columns.is_empty() {
            "DO NOTHING".to_string()
        } else {
            let sets = table
                .columns
                .iter()
                .map(|column| format!("{0} = excluded.{0}", column.name))
                .collect::<Vec<_>>()
                .join(", ");
            // Stamps are set but never argued from — the same rule the
            // triggers follow, so that "did anything change" has one answer
            // on both sides of the wire.
            let differs = table
                .columns
                .iter()
                .filter(|column| column.rule != Rule::Stamp)
                .map(|column| format!("{0}.{1} IS NOT excluded.{1}", table.name, column.name))
                .collect::<Vec<_>>();
            if differs.is_empty() {
                "DO NOTHING".to_string()
            } else {
                format!("DO UPDATE SET {sets} WHERE {}", differs.join(" OR "))
            }
        };

        let verb = if matches!(table.guard, Guard::Keep) {
            // An evening and a price at an instant are statements about a
            // moment that has passed. What is held is the same statement.
            "DO NOTHING".to_string()
        } else {
            conflict
        };

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({placeholders}) ON CONFLICT ({}) {verb}",
            table.name,
            names.join(", "),
            table.key.join(", "),
        );

        let bindings: Vec<SqlValue> = key
            .iter()
            .map(sql_value)
            .chain(values.iter().cloned())
            .collect();
        Ok(self
            .connection
            .execute(&sql, params_from_iter(bindings.iter()))?)
    }
}

/// What one pass moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub sent: usize,
    pub landed: usize,
    pub removed: usize,
    /// Rows one end or the other could not read. A number climbing here is
    /// what one machine running an older build looks like.
    pub unreadable: usize,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The next thing a pass should do.
///
/// A pass is a loop of these, and they are here rather than in the caller
/// because there are two callers: the GTK shell, which awaits between steps so
/// the window keeps drawing, and `sync-check`, which runs them straight
/// through. The loop is five lines either way and every decision inside it is
/// on this side, so the two cannot come to disagree about what a pass is.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Send this, then hand the answer to [`Store::absorb_push`].
    Push { parcel: Parcel, through: i64 },
    /// Entries whose rows are no longer here. A sweep took them, and sweeps
    /// are not logged — so they leave the queue without being sent, rather
    /// than being retried forever.
    Drain(i64),
    /// Ask for everything above this, then hand it to [`Store::absorb_pull`].
    Pull(i64),
}

impl Store {
    /// What to do next. **Pushes before pulls**, so a pass that dies half way
    /// has told the server about work that exists rather than only about work
    /// that is gone.
    pub fn next_step(&self, batch: usize) -> Result<Step> {
        let (parcel, through) = self.outbox(batch)?;
        if !parcel.rows.is_empty() {
            return Ok(Step::Push { parcel, through });
        }
        if through > 0 {
            return Ok(Step::Drain(through));
        }
        Ok(Step::Pull(self.cursor(PULLED)))
    }

    /// The server has it. Forget it.
    pub fn absorb_push(
        &self,
        through: i64,
        sent: usize,
        applied: &Applied,
        report: &mut Report,
    ) -> Result<()> {
        self.drain(through)?;
        report.sent += sent;
        report.unreadable += applied.unreadable;
        Ok(())
    }

    /// Write what arrived and move the cursor. Answers whether there is more.
    ///
    /// The cursor moves only once what it names is written, so a pass that
    /// dies between the two asks for the same batch again — which costs
    /// nothing, because every rule here is idempotent.
    pub fn absorb_pull(&mut self, pulled: &Pulled, report: &mut Report) -> Result<bool> {
        let applied = self.apply(&pulled.parcel, Recording::Off)?;
        self.set_cursor(PULLED, pulled.cursor)?;
        report.landed += applied.written;
        report.removed += applied.removed;
        report.unreadable += applied.unreadable;
        Ok(pulled.more)
    }
}

/// A whole pass, start to finish, blocking.
///
/// What `sync-check` drives and what a shell with no main loop would use. The
/// GTK shell runs the same steps with an `await` between them instead, because
/// a window that stops drawing for the length of a fifty-thousand-row first
/// sync is a window somebody force-quits.
pub fn pass(
    store: &mut Store,
    remote: &dyn crate::sync::Remote,
    batch: usize,
) -> std::result::Result<Report, crate::sync::SyncError> {
    let mut report = Report::default();
    let fail = |error: StoreError| crate::sync::SyncError(error.to_string());

    loop {
        match store.next_step(batch).map_err(fail)? {
            Step::Push { parcel, through } => {
                let sent = parcel.rows.len();
                let applied = remote.push(&parcel)?;
                store
                    .absorb_push(through, sent, &applied, &mut report)
                    .map_err(fail)?;
            }
            Step::Drain(through) => {
                store.drain(through).map_err(fail)?;
            }
            Step::Pull(since) => {
                let pulled = remote.pull(since, batch)?;
                if !store.absorb_pull(&pulled, &mut report).map_err(fail)? {
                    break;
                }
            }
        }
    }

    Ok(report)
}

enum Outcome {
    Written,
    Removed,
    Kept,
    Unreadable,
}

/// `WHERE k1 = ?1 AND k2 = ?2`, with the numbering the key columns get.
fn wheres(table: &Table) -> String {
    table
        .key
        .iter()
        .enumerate()
        .map(|(index, name)| format!("{name} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// One stored value as JSON.
fn cell(value: ValueRef<'_>, rule: Rule) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => Value::Number(number.into()),
        ValueRef::Real(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Blob(bytes) => match rule {
            Rule::Blob => Value::String(crate::sync::encode_base64(bytes)),
            // Nothing else in the schema is a blob; if something becomes one,
            // this says so rather than sending an empty string that reads as
            // an answer.
            _ => Value::Null,
        },
    }
}

fn sql_value(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Integer(i64::from(*flag)),
        Value::Number(number) => number
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| number.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Value::String(text) => SqlValue::Text(text.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn binding(value: &Value) -> ToSqlOutput<'static> {
    ToSqlOutput::Owned(sql_value(value))
}

/// The one column that is merged rather than taken.
///
/// The addon knows a collectible's source prose, its artwork and its faction
/// lock; the web API knows its name and its expansion. Whichever arrives
/// second must not flatten the other, which is the same rule
/// `Store::save_collectibles` already follows locally — and this is the
/// same `Collectible::merge` doing it, not a second copy of the idea.
fn merge_json(arriving: &str, held: &str) -> String {
    let Ok(mut merged) = serde_json::from_str::<Collectible>(arriving) else {
        return arriving.to_string();
    };
    if let Ok(mine) = serde_json::from_str::<Collectible>(held) {
        merged.merge(&mine);
    }
    serde_json::to_string(&merged).unwrap_or_else(|_| arriving.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::Scope;

    fn store(machine: &str) -> Store {
        let store = Store::in_memory().unwrap();
        store.set_machine(machine).unwrap();
        store
    }

    /// Push everything `from` has waiting straight into `to`, the way a pass
    /// would, and drain the outbox the way a pass would.
    fn carry(from: &Store, to: &mut Store) -> Applied {
        let (parcel, through) = from.outbox(10_000).unwrap();
        let applied = to.apply(&parcel, Recording::Off).unwrap();
        from.drain(through).unwrap();
        applied
    }

    #[test]
    fn a_write_is_logged_without_the_writer_knowing_it_exists() {
        // `watch_item` says nothing about syncing. The trigger does.
        let store = store("one");
        store.watch_item(4306, "Silk Cloth").unwrap();

        let (parcel, _) = store.outbox(10).unwrap();
        assert_eq!(parcel.rows.len(), 1);
        assert_eq!(parcel.rows[0].scope, "watched");
        assert_eq!(parcel.rows[0].key, vec![serde_json::json!(4306)]);
        assert_eq!(
            parcel.rows[0].fields,
            Some(vec![serde_json::json!("Silk Cloth")])
        );
    }

    #[test]
    fn writing_the_same_thing_again_is_not_news() {
        let store = store("one");
        store.watch_item(4306, "Silk Cloth").unwrap();
        store.drain(store.high_water()).unwrap();

        store.watch_item(4306, "Silk Cloth").unwrap();
        let (parcel, _) = store.outbox(10).unwrap();
        assert!(
            parcel.rows.is_empty(),
            "an upsert that changed nothing enqueued {:?}",
            parcel.rows
        );
    }

    #[test]
    fn a_row_written_twice_is_one_entry_at_the_later_place() {
        let store = store("one");
        store.watch_item(4306, "Silk").unwrap();
        store.watch_item(4306, "Silk Cloth").unwrap();

        let (parcel, _) = store.outbox(10).unwrap();
        assert_eq!(parcel.rows.len(), 1, "the log is the size of the data");
        assert_eq!(
            parcel.rows[0].fields,
            Some(vec![serde_json::json!("Silk Cloth")]),
            "and it carries what is there now, not what was there first"
        );
    }

    #[test]
    fn what_one_machine_wrote_lands_on_the_other() {
        let one = store("one");
        let mut two = store("two");
        one.watch_item(4306, "Silk Cloth").unwrap();

        let applied = carry(&one, &mut two);
        assert_eq!(applied.written, 1);
        assert_eq!(
            two.watched().unwrap(),
            vec![(4306, "Silk Cloth".to_string())]
        );
    }

    #[test]
    fn what_arrived_is_not_sent_straight_back() {
        // The failure this is here for: two machines handing each other the
        // same row forever, each pass looking like work.
        let one = store("one");
        let mut two = store("two");
        one.watch_item(4306, "Silk Cloth").unwrap();
        carry(&one, &mut two);

        let (parcel, _) = two.outbox(10).unwrap();
        assert!(parcel.rows.is_empty(), "{:?}", parcel.rows);
    }

    #[test]
    fn a_row_already_agreed_on_is_kept_rather_than_written() {
        let one = store("one");
        let mut two = store("two");
        one.watch_item(4306, "Silk Cloth").unwrap();
        two.watch_item(4306, "Silk Cloth").unwrap();

        let (parcel, _) = one.outbox(10).unwrap();
        let applied = two.apply(&parcel, Recording::Off).unwrap();
        assert_eq!(applied.written, 0);
        assert_eq!(applied.kept, 1);
    }

    #[test]
    fn a_deletion_travels() {
        let one = store("one");
        let mut two = store("two");
        one.watch_item(4306, "Silk Cloth").unwrap();
        carry(&one, &mut two);

        one.unwatch_item(4306).unwrap();
        let applied = carry(&one, &mut two);
        assert_eq!(applied.removed, 1);
        assert!(two.watched().unwrap().is_empty());
    }

    #[test]
    fn a_counter_never_goes_backwards_whichever_side_is_behind() {
        // The rule the tallies exist under: cumulative, nowhere to get them
        // back from, so a machine that has been away must not be able to take
        // them off one that is ahead.
        use crate::addon::collector::Collected;
        use crate::character::CharacterKey;
        use crate::tally::{Counting, Tally};

        let mut one = store("one");
        let mut two = store("two");
        let who = CharacterKey::new("emerald-dream", "Somechar");

        let flasks = |count| Tally {
            kind: Counting::Recipe,
            key: "371637".into(),
            label: "Flask of Alchemical Chaos".into(),
            count,
        };

        let mut ahead = Collected::default();
        ahead.tallies.insert(who.clone(), vec![flasks(412)]);
        two.save_collected(&ahead).unwrap();

        let mut behind = Collected::default();
        behind.tallies.insert(who.clone(), vec![flasks(1)]);
        one.save_collected(&behind).unwrap();

        carry(&one, &mut two);

        assert_eq!(
            two.tallies().unwrap()[&who][0].count,
            412,
            "the lower count arrived and took the higher one with it"
        );
    }

    #[test]
    fn a_sweep_is_this_machines_expiry_and_not_a_deletion_anybody_else_hears_about() {
        let store = store("one");
        store
            .store_response("https://example.test/a", b"body", None)
            .unwrap();
        store.drain(store.high_water()).unwrap();

        store.purge().unwrap();
        let (parcel, _) = store.outbox(10).unwrap();
        assert!(parcel.rows.is_empty(), "{:?}", parcel.rows);
        assert!(store.recording(), "and the flag is put back");
    }

    #[test]
    fn a_scope_this_build_does_not_know_is_counted_rather_than_guessed_at() {
        let mut store = store("one");
        let parcel = Parcel {
            rows: vec![Row {
                scope: "transmog".into(),
                key: vec![serde_json::json!(1)],
                fields: Some(vec![serde_json::json!("x")]),
            }],
        };
        let applied = store.apply(&parcel, Recording::Off).unwrap();
        assert_eq!(applied.unreadable, 1);
        assert_eq!(applied.written, 0);
    }

    #[test]
    fn a_row_of_the_wrong_shape_is_unreadable_rather_than_half_written() {
        let mut store = store("one");
        let parcel = Parcel {
            rows: vec![Row {
                scope: "watched".into(),
                key: vec![serde_json::json!(1)],
                fields: Some(vec![serde_json::json!("a"), serde_json::json!("b")]),
            }],
        };
        assert_eq!(store.apply(&parcel, Recording::Off).unwrap().unreadable, 1);
    }

    #[test]
    fn a_server_logs_what_a_client_pushed_under_that_clients_name() {
        let one = store("one");
        let mut server = store("server");
        one.watch_item(4306, "Silk Cloth").unwrap();

        let (parcel, _) = one.outbox(10).unwrap();
        server.apply(&parcel, Recording::As("one")).unwrap();

        // The machine that wrote it hears nothing back...
        let mine = server.log_since(0, "one", 100).unwrap();
        assert!(mine.parcel.rows.is_empty(), "{:?}", mine.parcel.rows);

        // ...and every other machine does.
        let theirs = server.log_since(0, "two", 100).unwrap();
        assert_eq!(theirs.parcel.rows.len(), 1);
        assert!(theirs.cursor > 0);

        // And the server's own name is back where it was.
        assert_eq!(server.machine(), "server");
    }

    #[test]
    fn a_cursor_only_moves_forward_and_says_when_there_is_more() {
        let one = store("one");
        let mut server = store("server");
        for item in 0..5u32 {
            one.watch_item(item, "thing").unwrap();
        }
        let (parcel, _) = one.outbox(100).unwrap();
        server.apply(&parcel, Recording::As("one")).unwrap();

        let first = server.log_since(0, "two", 2).unwrap();
        assert_eq!(first.parcel.rows.len(), 2);
        assert!(first.more);

        let second = server.log_since(first.cursor, "two", 2).unwrap();
        assert_eq!(second.parcel.rows.len(), 2);
        assert!(second.cursor > first.cursor);

        let third = server.log_since(second.cursor, "two", 2).unwrap();
        assert_eq!(third.parcel.rows.len(), 1);
        assert!(!third.more, "one short of a full batch is the end");
    }

    #[test]
    fn nothing_waiting_means_nothing_to_send() {
        let store = store("one");
        let (parcel, through) = store.outbox(100).unwrap();
        assert!(parcel.rows.is_empty());
        assert_eq!(through, 0);
        assert!(store.queued().unwrap().is_empty());
    }

    /// The rule the whole log stands on: **doing the same thing twice
    /// enqueues nothing the second time.**
    ///
    /// Every writer here used to delete its table and write it out again,
    /// which is invisible to a store nobody else reads and ruinous to a log.
    /// One addon read would enqueue every criterion the account has ever
    /// seen — tens of thousands of rows saying nothing had changed — and one
    /// auction sync would enqueue a realm.
    #[test]
    fn a_second_identical_write_enqueues_nothing() {
        use crate::addon::collector::Collected;
        use crate::character::{Character, CharacterKey, Faction, Roster};
        use crate::source::blizzard::auctions::Depth;
        use crate::tally::{Counting, Tally};

        let mut store = store("one");
        let who = CharacterKey::new("emerald-dream", "Somechar");

        let roster = Roster {
            characters: vec![Character {
                key: who.clone(),
                id: 1,
                realm_id: 1,
                display_name: "Somechar".into(),
                realm_name: "Emerald Dream".into(),
                level: 80,
                class: "Shaman".into(),
                race: "Orc".into(),
                faction: Faction::Horde,
                wow_account_id: 7,
            }],
        };

        let mut collected = Collected::default();
        collected.recipes.insert(
            who.clone(),
            vec![crate::market::Recipe {
                id: 371_637,
                name: "Flask of Alchemical Chaos".into(),
                output: 212_283,
                makes: 1,
                reagents: vec![crate::market::Reagent {
                    quantity: 3,
                    tiers: vec![210_796, 210_799],
                }],
            }],
        );
        collected.earned_by.insert(1234, who.clone());
        collected
            .criteria
            .insert(99, crate::achievement::CriterionKind::Quest(5));
        collected.warband_bank.insert(2589, 40);
        collected.pets_held.insert(42, 2);
        collected
            .currencies
            .insert(who.clone(), [(1602u32, 300u64)].into_iter().collect());
        collected.tallies.insert(
            who.clone(),
            vec![Tally {
                kind: Counting::Recipe,
                key: "371637".into(),
                label: "Flask".into(),
                count: 12,
            }],
        );

        let book = vec![Depth {
            item_id: 2589,
            variant: String::new(),
            cheapest: 100,
            quantity: 400,
            listings: 9,
            tenth: 110,
            median: 130,
        }];

        let run = crate::run::Run {
            name: "The Second Time".into(),
            baseline: crate::run::Baseline {
                taken_at: chrono::Utc::now(),
                collected: vec![6],
                completed: vec![],
            },
            cohort: crate::cohort::Cohort::from(vec![who.clone()]),
            goals: vec![crate::run::Goal {
                achievement_id: 1234,
                standing: crate::run::Standing::Unearned,
                bucket: crate::run::Bucket::Observable,
                attestation: None,
                nearest: None,
                evaluation: None,
            }],
        };

        let at = chrono::Utc::now();
        store.save_roster(&roster).unwrap();
        let run_id = store.save_run(None, &run).unwrap();
        store.save_collected(&collected).unwrap();
        store.record_snapshot(1, &book, at).unwrap();
        store
            .save_owned(
                crate::source::blizzard::collections::Kind::Mount,
                &[6u32].into_iter().collect(),
            )
            .unwrap();

        // Everything above is genuinely new, so it is genuinely queued.
        assert!(!store.queued().unwrap().is_empty());
        store.drain(store.high_water()).unwrap();

        // The same day's data arriving again is not news.
        store.save_roster(&roster).unwrap();
        store.save_run(Some(run_id), &run).unwrap();
        store.save_collected(&collected).unwrap();
        store.record_snapshot(1, &book, at).unwrap();
        store
            .save_owned(
                crate::source::blizzard::collections::Kind::Mount,
                &[6u32].into_iter().collect(),
            )
            .unwrap();

        let queued = store.queued().unwrap();
        assert!(
            queued.is_empty(),
            "a repeat write enqueued {queued:?} — the log is now the size of the history"
        );
    }

    #[test]
    fn a_run_and_its_goals_travel_and_stay_one_run() {
        // `run.id` is a local autoincrement, so the naive version of this
        // gives the two machines different ids for the same run and then
        // hangs one machine's goals off the other's run.
        let mut one = store("one");
        let mut two = store("two");

        // A run already on the receiving machine, so its ids are not the
        // sending machine's — which is what makes the translation load-bearing
        // rather than accidentally right.
        two.save_run(None, &a_run("Something Else", 100)).unwrap();
        two.drain(two.high_water()).unwrap();

        one.save_run(None, &a_run("The Second Time", 200)).unwrap();
        carry(&one, &mut two);

        let runs: i64 = two
            .connection
            .query_row("SELECT COUNT(*) FROM run", [], |row| row.get(0))
            .unwrap();
        assert_eq!(runs, 2, "the arriving run should be one more, not a copy");

        let (id, current) = two.current_run().unwrap().expect("a current run");
        assert_eq!(current.name, "The Second Time");
        assert_eq!(current.goals.len(), 1);
        assert_eq!(current.goals[0].achievement_id, 1234);

        let attached: i64 = two
            .connection
            .query_row("SELECT COUNT(*) FROM goal WHERE run_id = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(attached, 1, "the goal hung off the wrong run");

        // And sending it again changes nothing on either side.
        one.save_run(None, &a_run("The Second Time", 200)).unwrap();
        let (parcel, _) = one.outbox(100).unwrap();
        assert!(parcel.rows.is_empty(), "{:?}", parcel.rows);
    }

    fn a_run(name: &str, second: i64) -> crate::run::Run {
        crate::run::Run {
            name: name.into(),
            baseline: crate::run::Baseline {
                taken_at: chrono::DateTime::from_timestamp(second, 0).unwrap(),
                collected: vec![],
                completed: vec![],
            },
            cohort: crate::cohort::Cohort::default(),
            goals: vec![crate::run::Goal {
                achievement_id: 1234,
                standing: crate::run::Standing::Unearned,
                bucket: crate::run::Bucket::Observable,
                attestation: None,
                nearest: None,
                evaluation: None,
            }],
        }
    }

    #[test]
    fn a_goal_whose_run_never_arrived_is_dropped_rather_than_hung_off_another() {
        let mut two = store("two");
        two.save_run(None, &a_run("Mine", 100)).unwrap();

        let parcel = Parcel {
            rows: vec![Row {
                scope: "goal".into(),
                key: vec![
                    serde_json::json!("a-run-this-machine-has-never-seen"),
                    serde_json::json!(999),
                ],
                fields: Some(vec![
                    serde_json::json!("\"Unearned\""),
                    serde_json::json!("\"Observable\""),
                    serde_json::Value::Null,
                ]),
            }],
        };
        let applied = two.apply(&parcel, Recording::Off).unwrap();
        assert_eq!(applied.unreadable, 1);
        assert_eq!(applied.written, 0);

        let goals: i64 = two
            .connection
            .query_row("SELECT COUNT(*) FROM goal", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            goals, 1,
            "the orphan goal was attached to the run that is here"
        );
    }

    #[test]
    fn a_queue_full_of_cached_bodies_is_sent_in_pieces() {
        // The wedge this exists to prevent: a batch bounded only by a row
        // count is fine for the small rows and enormous for `response`, whose
        // rows carry a whole cached body. A client that built one past the
        // server's ceiling would rebuild the same one on every pass forever.
        let store = store("one");
        let body = vec![b'x'; 1024 * 1024];
        for index in 0..40 {
            store
                .store_response(&format!("https://example.test/{index}"), &body, None)
                .unwrap();
        }

        let (parcel, through) = store.outbox(10_000).unwrap();
        assert!(
            parcel.rows.len() < 40,
            "the whole queue went in one batch: {} rows",
            parcel.rows.len()
        );
        assert!(!parcel.rows.is_empty(), "and nothing went at all");
        assert!(through > 0);

        // And the rest is still queued, rather than having been skipped.
        store.drain(through).unwrap();
        let (rest, _) = store.outbox(10_000).unwrap();
        assert!(!rest.rows.is_empty(), "the rest of the queue was lost");
    }

    #[test]
    fn one_row_larger_than_a_batch_still_goes_on_its_own() {
        let store = store("one");
        store
            .store_response(
                "https://example.test/big",
                &vec![b'x'; 3 * 1024 * 1024],
                None,
            )
            .unwrap();
        let (parcel, _) = store.outbox(10_000).unwrap();
        assert_eq!(parcel.rows.len(), 1);
    }

    #[test]
    fn a_body_too_large_to_carry_is_left_where_it_is() {
        // Nothing depends on its presence — a cache miss is the ordinary case
        // — and a realm's auction dump is what this is about.
        let store = store("one");
        store
            .store_response(
                "https://example.test/auctions",
                &vec![b'x'; crate::sync::MAX_BODY + 1],
                None,
            )
            .unwrap();
        let (parcel, through) = store.outbox(10).unwrap();
        assert_eq!(parcel.rows.len(), 0);
        // But its entry still leaves the queue, or the pass retries it forever.
        assert!(through > 0);
    }

    /// A table added to the schema and forgotten here does not fail — it just
    /// silently never leaves the machine it was written on, and nothing about
    /// that looks wrong from either end.
    ///
    /// So the schema is asked, rather than trusted. The two exceptions are
    /// named individually and on purpose: `change` is the log, and putting the
    /// log on the wire would be a machine telling another machine what it had
    /// already told it. `sync_state` is the cursor and this installation's
    /// name, both of which mean something different on every machine.
    #[test]
    fn no_table_is_left_out_of_the_wire_by_accident() {
        let store = store("one");
        let held: std::collections::BTreeSet<String> = store
            .connection
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = \'table\' AND name NOT LIKE \'sqlite_%\'",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();

        let travelling: std::collections::BTreeSet<String> = crate::sync::TABLES
            .iter()
            .map(|table| table.name.to_string())
            .collect();
        let kept_back: std::collections::BTreeSet<String> = ["change", "sync_state"]
            .iter()
            .map(|n| n.to_string())
            .collect();

        let unaccounted: Vec<&String> = held
            .iter()
            .filter(|name| !travelling.contains(*name) && !kept_back.contains(*name))
            .collect();
        assert!(
            unaccounted.is_empty(),
            "these tables are in the schema and travel nowhere: {unaccounted:?}"
        );

        let phantom: Vec<&String> = travelling
            .iter()
            .filter(|name| !held.contains(*name))
            .collect();
        assert!(
            phantom.is_empty(),
            "these are described on the wire and are not in the schema: {phantom:?}"
        );
    }

    #[test]
    fn every_table_that_travels_has_its_triggers() {
        let store = store("one");
        let names: Vec<String> = store
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        for table in crate::sync::TABLES {
            for suffix in ["ins", "del"] {
                let wanted = format!("log_{}_{suffix}", table.name);
                assert!(names.contains(&wanted), "{wanted} is missing");
            }
            if !table.columns.is_empty() {
                let wanted = format!("log_{}_upd", table.name);
                assert!(names.contains(&wanted), "{wanted} is missing");
            }
        }
        let _ = Scope::Character;
    }
}
