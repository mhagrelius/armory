//! One account, shared between one person's machines.
//!
//! It takes rows, applies them with exactly the rules a client would, keeps a
//! log of what landed, and hands each machine everything in that log it did
//! not write itself. That is the whole job.
//!
//! # What it is not
//!
//! It does not plan a run, evaluate a criterion, decide whether a goal is
//! poisoned, cost a craft, or write a journal entry. All of that is the
//! client's, where it already is and where it is already tested. A server that
//! starts answering "what is left to do" is a second Armory that can disagree
//! with the first.
//!
//! It is also never the only copy. Every machine keeps the whole account
//! locally and works with the NAS switched off; this is where the machines
//! meet, not where the account lives.
//!
//! # Why it is the same store
//!
//! Because the merges are the hard part. A tally takes the larger count, a
//! collectible merges field by field, an evening is written once, a run's
//! goals are reconciled rather than rewritten — twenty-seven tables of rules
//! that took evidence to arrive at. Re-deriving them here in another dialect
//! would give two definitions of `save_collected`'s merge and no way to notice
//! when they parted. So the server opens the same SQLite schema through the
//! same `armory-core`, and the only thing it adds is a socket and a lock.
//!
//! # The lock
//!
//! One `Mutex<Store>` held across a whole route. A push reads the log, writes
//! rows and appends to the log, and two of those interleaving would let one
//! machine's batch take a `seq` inside another's — which is the one way a
//! cursor can step over a row it never saw.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use armory_core::replica::Recording;
use armory_core::store::Store;
use armory_core::sync::{Applied, Parcel};
use serde::Serialize;

pub mod accounts;
pub mod http;

use http::{Request, Response};

/// The shortest shared secret this will start with.
///
/// The server refuses to run without one rather than inventing a default: a
/// shared secret nobody chose is a shared secret everybody has.
pub const MIN_TOKEN: usize = 32;

/// How many rows one pull hands over.
///
/// A first sync is an account's whole history and runs to tens of thousands of
/// rows, so it arrives in batches and `Pulled::more` says there are further
/// ones. Sized so a batch of cached bodies stays inside `http::MAX_BODY`.
pub const BATCH: usize = 2_000;

/// How long a parked `/wait` is held before it answers "nothing yet".
///
/// Long enough that an idle client is not reconnecting constantly, short
/// enough that no NAT table, proxy or sleeping Wi-Fi radio between here and a
/// gaming PC decides a silent connection is a dead one.
pub const MAX_WAIT: Duration = Duration::from_secs(50);

pub fn check_token(token: &str) -> Result<(), String> {
    if token.len() < MIN_TOKEN {
        return Err(format!(
            "the token must be at least {MIN_TOKEN} characters; this one is {}",
            token.len()
        ));
    }
    Ok(())
}

/// Woken when anything is written, so a parked `/wait` does not poll.
///
/// A counter rather than a flag: a write that lands between a client reading
/// the log and parking here would set a flag that had already been cleared,
/// and the client would sleep through its own news. Comparing counters cannot
/// miss one.
#[derive(Default)]
pub struct Changes {
    count: Mutex<u64>,
    woken: Condvar,
}

impl Changes {
    pub fn announce(&self) {
        if let Ok(mut count) = self.count.lock() {
            *count += 1;
            self.woken.notify_all();
        }
    }

    /// Where the counter is now. Taken *before* the caller looks at the log,
    /// never after — the whole point is to notice a write that happened while
    /// the caller was reading.
    pub fn mark(&self) -> u64 {
        self.count.lock().map(|count| *count).unwrap_or(0)
    }

    /// Park until the counter moves past `mark`, or until the wait is up.
    pub fn wait(&self, mark: u64, limit: Duration) -> bool {
        let Ok(count) = self.count.lock() else {
            return false;
        };
        let Ok((count, _)) = self
            .woken
            .wait_timeout_while(count, limit, |count| *count <= mark)
        else {
            return false;
        };
        *count > mark
    }
}

pub struct Server {
    /// Every account's store, opened when it is first asked for.
    ///
    /// One `Mutex` over the whole map rather than one per account: this
    /// serves one person's handful of machines, and a lock per account buys
    /// concurrency nobody is waiting on while making "list them" and "delete
    /// one" races that have to be reasoned about.
    stores: Mutex<HashMap<String, Store>>,
    root: PathBuf,
    token: String,
    changes: Arc<Changes>,
    /// How far the log reached, readable without the lock.
    ///
    /// `/health` must never queue behind a push. A first sync applies
    /// thousands of rows at a time while holding the store, and the container
    /// healthcheck gives up after three seconds — so a health route that took
    /// the lock would report the server unhealthy during precisely the
    /// operation it exists to survive, and would say nothing at all to anyone
    /// asking whether the sync was working.
    rows: AtomicI64,
}

/// The answer to `/wait`.
#[derive(Debug, Serialize)]
struct Waited {
    changed: bool,
    /// Where the log ends now, so a client that was told "nothing" still
    /// learns the cursor rather than asking again from where it was.
    cursor: i64,
}

/// The answer to `/health`. No token, so the container's healthcheck needs no
/// secret; it says how much is held so that "up" and "up and holding the
/// account" are different answers.
#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
    rows: i64,
}

/// One account the server holds.
#[derive(Debug, Serialize)]
struct Held {
    name: String,
    rows: i64,
}

/// What a deletion removed.
#[derive(Debug, Serialize)]
struct Forgotten {
    forgot: String,
}

/// What a push did, as the client's sync page shows it.
#[derive(Debug, Serialize)]
struct Report {
    written: usize,
    removed: usize,
    kept: usize,
    unreadable: usize,
    cursor: i64,
}

impl Server {
    pub fn new(root: PathBuf, token: String, changes: Arc<Changes>) -> Self {
        Self {
            stores: Mutex::new(HashMap::new()),
            root,
            token,
            changes,
            rows: AtomicI64::new(0),
        }
    }

    /// The account a request is about.
    ///
    /// A client that names none is talking about `default`, which is where a
    /// store from before accounts existed was adopted — so an older client
    /// keeps talking to its own data rather than starting a second, empty
    /// account beside it.
    fn account_of(request: &Request) -> String {
        let named = request
            .header("x-armory-account")
            .unwrap_or_default()
            .trim();
        if named.is_empty() {
            accounts::DEFAULT.to_string()
        } else {
            named.to_string()
        }
    }

    /// Do something with one account's store, opening it if this is the first
    /// time anyone has asked.
    ///
    /// `None` when the name is not one this server will turn into a path. The
    /// caller answers 400 — a refusal, rather than a store quietly made
    /// somewhere unintended.
    fn with_store<T>(&self, name: &str, work: impl FnOnce(&mut Store) -> T) -> Option<T> {
        let home = accounts::directory(&self.root, name)?;
        let mut stores = self.stores.lock().ok()?;
        if !stores.contains_key(name) {
            std::fs::create_dir_all(&home).ok()?;
            let store = Store::open(&home.join("armory.db")).ok()?;
            store.set_machine("server").ok()?;
            stores.insert(name.to_string(), store);
        }
        Some(work(stores.get_mut(name)?))
    }

    pub fn handle(&self, request: &Request) -> Response {
        // Listed as a guard rather than route by route, so that a route added
        // later is authenticated by default. Forgetting to add one to a list
        // should fail closed.
        if request.route() != "/health" && !authorised(request, &self.token) {
            // Deliberately says nothing about which part was wrong.
            return Response::text(401, "unauthorized");
        }

        match (request.method.as_str(), request.route()) {
            ("GET", "/health") => self.health(),
            ("POST", "/push") => self.push(request),
            ("GET", "/pull") => self.pull(request),
            ("GET", "/wait") => self.wait(request),
            ("GET", "/accounts") => self.accounts(),
            ("DELETE", path) if path.starts_with("/accounts/") => {
                self.forget(request, path.trim_start_matches("/accounts/"))
            }
            (_, "/health" | "/push" | "/pull" | "/wait" | "/accounts") => {
                Response::text(405, "wrong method for that route")
            }
            _ => Response::text(404, "no such route"),
        }
    }

    fn health(&self) -> Response {
        // Deliberately does not touch the store. See `Server::rows`.
        Response::json(
            200,
            &Health {
                ok: true,
                rows: self.rows.load(Ordering::Relaxed),
            },
        )
    }

    fn push(&self, request: &Request) -> Response {
        let machine = request.machine();
        if machine.is_empty() {
            // Without it the log cannot say who wrote a row, and every client
            // pulls back what it just sent. Refusing is much better than
            // accepting and being slow forever afterwards.
            return Response::text(400, "no X-Armory-Machine header");
        }

        let parcel: Parcel = match serde_json::from_slice(&request.body) {
            Ok(parcel) => parcel,
            Err(error) => return Response::text(400, &format!("that is not a parcel: {error}")),
        };

        let account = Self::account_of(request);
        let Some(outcome) = self.with_store(&account, |store| {
            let applied = store.apply(&parcel, Recording::As(machine))?;
            Ok::<_, armory_core::store::StoreError>((applied, store.high_water()))
        }) else {
            return Response::text(400, "that is not an account name this server will take");
        };
        let (applied, cursor) = match outcome {
            Ok(outcome) => outcome,
            Err(error) => return Response::text(500, &format!("could not apply: {error}")),
        };
        self.rows.store(cursor, Ordering::Relaxed);

        // Only when something actually landed. `Applied::kept` counts rows
        // both sides already agreed on, and waking every parked client for a
        // batch of those is a pass that wakes the others to tell them nothing.
        if applied.written + applied.removed > 0 {
            self.changes.announce();
        }
        Response::json(200, &report(applied, cursor))
    }

    fn pull(&self, request: &Request) -> Response {
        let since = number(request, "since", 0);
        let limit = number(request, "limit", BATCH as i64).clamp(1, BATCH as i64) as usize;

        let account = Self::account_of(request);
        let machine = request.machine().to_string();
        let Some(pulled) =
            self.with_store(&account, |store| store.log_since(since, &machine, limit))
        else {
            return Response::text(400, "that is not an account name this server will take");
        };
        match pulled {
            Ok(pulled) => Response::json(200, &pulled),
            Err(error) => Response::text(500, &format!("could not read the log: {error}")),
        }
    }

    /// What the server is holding, and how much of each.
    ///
    /// The list a client draws so that deleting one is a thing done to a named
    /// account with a size beside it, rather than to a path typed into a
    /// terminal.
    fn accounts(&self) -> Response {
        let mut held = Vec::new();
        for name in accounts::known(&self.root) {
            let rows = self
                .with_store(&name, |store| store.high_water())
                .unwrap_or(0);
            held.push(Held { name, rows });
        }
        Response::json(200, &held)
    }

    /// Remove an account and everything in it.
    ///
    /// Irreversible, and the only route here that destroys anything — so it
    /// takes the name in the path *and* again in `?confirm=`, and refuses
    /// unless they match. Not security: the token is that. It is so that a
    /// mistyped or half-built URL cannot delete an account, because there is
    /// no undo and the client's copy is the only one left afterwards.
    fn forget(&self, request: &Request, name: &str) -> Response {
        let Some(home) = accounts::directory(&self.root, name) else {
            return Response::text(400, "that is not an account name this server will take");
        };
        if request.query("confirm").as_deref() != Some(name) {
            return Response::text(400, "confirm the account by name to delete it");
        }
        if !home.exists() {
            return Response::text(404, "no such account");
        }

        // Out of the map first. A store left open on a directory that is being
        // removed would go on answering from a file nothing can reach, and the
        // next push would recreate it half way through the deletion.
        if let Ok(mut stores) = self.stores.lock() {
            stores.remove(name);
        }
        match std::fs::remove_dir_all(&home) {
            Ok(()) => {
                self.changes.announce();
                Response::json(
                    200,
                    &Forgotten {
                        forgot: name.into(),
                    },
                )
            }
            Err(error) => Response::text(500, &format!("could not delete it: {error}")),
        }
    }

    fn wait(&self, request: &Request) -> Response {
        let since = number(request, "since", 0);
        let machine = request.machine().to_string();

        // The mark comes first, before the log is read. The other order is a
        // race with a name: a write that lands between the read and the park
        // would be slept through for the whole fifty seconds, and the client
        // would look like it had stopped syncing.
        let mark = self.changes.mark();

        let account = Self::account_of(request);
        let Some((already, cursor)) = self.with_store(&account, |store| {
            (store.anything_since(since, &machine), store.high_water())
        }) else {
            return Response::text(400, "that is not an account name this server will take");
        };
        if already {
            return Response::json(
                200,
                &Waited {
                    changed: true,
                    cursor,
                },
            );
        }

        // Nothing is held while parked — not the lock, not a store handle.
        self.changes.wait(mark, MAX_WAIT);

        let Some(waited) = self.with_store(&account, |store| Waited {
            changed: store.anything_since(since, &machine),
            cursor: store.high_water(),
        }) else {
            return Response::text(400, "that is not an account name this server will take");
        };
        Response::json(200, &waited)
    }
}

fn report(applied: Applied, cursor: i64) -> Report {
    Report {
        written: applied.written,
        removed: applied.removed,
        kept: applied.kept,
        unreadable: applied.unreadable,
        cursor,
    }
}

fn number(request: &Request, name: &str, fallback: i64) -> i64 {
    request
        .query(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

/// Compared byte by byte in constant time.
///
/// A shared secret compared with `==` leaks its prefix through how long the
/// comparison takes, and the fix is four lines.
fn authorised(request: &Request, token: &str) -> bool {
    let Some(offered) = request.bearer() else {
        return false;
    };
    constant_eq(offered.as_bytes(), token.as_bytes())
}

fn constant_eq(one: &[u8], two: &[u8]) -> bool {
    if one.len() != two.len() {
        return false;
    }
    one.iter()
        .zip(two)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use armory_core::sync::Row;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn server() -> Server {
        // A real directory, because accounts are directories now and the
        // route that deletes one deletes a directory.
        let home = tempfile::tempdir().expect("a directory");
        let root = home.path().to_path_buf();
        // Kept alive for the test's length; the store is opened underneath it.
        std::mem::forget(home);
        Server::new(root, TOKEN.to_string(), Arc::new(Changes::default()))
    }

    fn get(path: &str, machine: &str, token: Option<&str>) -> Request {
        request("GET", path, machine, token, Vec::new())
    }

    fn request(
        method: &str,
        path: &str,
        machine: &str,
        token: Option<&str>,
        body: Vec<u8>,
    ) -> Request {
        let mut headers = std::collections::HashMap::new();
        headers.insert("x-armory-machine".into(), machine.to_string());
        if let Some(token) = token {
            headers.insert("authorization".into(), format!("Bearer {token}"));
        }
        Request {
            method: method.into(),
            path: path.into(),
            headers,
            body,
        }
    }

    fn watched(item: u32, name: &str) -> Parcel {
        Parcel {
            rows: vec![Row {
                scope: "watched".into(),
                key: vec![serde_json::json!(item)],
                fields: Some(vec![serde_json::json!(name)]),
            }],
        }
    }

    fn push(server: &Server, machine: &str, parcel: &Parcel) -> Response {
        server.handle(&request(
            "POST",
            "/push",
            machine,
            Some(TOKEN),
            serde_json::to_vec(parcel).unwrap(),
        ))
    }

    #[test]
    fn health_needs_no_token_because_the_container_asks_it() {
        let response = server().handle(&get("/health", "", None));
        assert_eq!(response.status, 200);
    }

    #[test]
    fn health_answers_while_the_store_is_busy() {
        // The failure this exists for: a first sync holds the store for the
        // length of a two-thousand-row apply, the container healthcheck gives
        // up after three seconds, and the server is reported unhealthy exactly
        // when it is working hardest.
        let server = server();
        // Hold the map the way a long push does.
        let held = server.stores.lock().expect("the stores");

        let response = server.handle(&get("/health", "", None));
        assert_eq!(response.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["ok"], true);

        drop(held);
    }

    #[test]
    fn health_reports_what_the_last_push_left_behind() {
        let server = server();
        assert_eq!(rows_of(&server), 0);
        push(&server, "one", &watched(4306, "Silk Cloth"));
        assert!(rows_of(&server) > 0);
    }

    fn rows_of(server: &Server) -> i64 {
        let response = server.handle(&get("/health", "", None));
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        body["rows"].as_i64().unwrap()
    }

    #[test]
    fn every_other_route_needs_one() {
        let server = server();
        for route in ["/pull", "/wait"] {
            assert_eq!(
                server.handle(&get(route, "one", None)).status,
                401,
                "{route}"
            );
        }
        assert_eq!(
            server
                .handle(&request("POST", "/push", "one", None, b"{}".to_vec()))
                .status,
            401
        );
    }

    #[test]
    fn a_wrong_token_says_nothing_about_which_part_was_wrong() {
        let response = server().handle(&get("/pull", "one", Some("wrong")));
        assert_eq!(response.status, 401);
        assert_eq!(response.body, b"unauthorized");
    }

    #[test]
    fn a_route_nobody_serves_is_a_404_rather_than_a_401() {
        // Only once the token is right — an unauthenticated caller learns
        // nothing about what routes exist.
        let server = server();
        assert_eq!(
            server.handle(&get("/notes", "one", Some(TOKEN))).status,
            404
        );
        assert_eq!(server.handle(&get("/notes", "one", None)).status, 401);
    }

    #[test]
    fn the_wrong_method_on_a_real_route_says_so() {
        assert_eq!(
            server().handle(&get("/push", "one", Some(TOKEN))).status,
            405
        );
    }

    #[test]
    fn a_push_with_no_machine_is_refused() {
        // A row with nobody's name on it comes straight back to whoever sent
        // it, forever.
        let response = push(&server(), "", &watched(4306, "Silk Cloth"));
        assert_eq!(response.status, 400);
    }

    #[test]
    fn what_one_machine_pushes_another_pulls_and_the_first_does_not() {
        let server = server();
        assert_eq!(
            push(&server, "one", &watched(4306, "Silk Cloth")).status,
            200
        );

        let mine = server.handle(&get("/pull?since=0", "one", Some(TOKEN)));
        let mine: serde_json::Value = serde_json::from_slice(&mine.body).unwrap();
        assert_eq!(mine["parcel"]["rows"].as_array().unwrap().len(), 0);

        let theirs = server.handle(&get("/pull?since=0", "two", Some(TOKEN)));
        let theirs: serde_json::Value = serde_json::from_slice(&theirs.body).unwrap();
        assert_eq!(theirs["parcel"]["rows"].as_array().unwrap().len(), 1);
        assert!(theirs["cursor"].as_i64().unwrap() > 0);
    }

    #[test]
    fn pushing_the_same_rows_twice_writes_them_once() {
        let server = server();
        let parcel = watched(4306, "Silk Cloth");

        let first = push(&server, "one", &parcel);
        let first: serde_json::Value = serde_json::from_slice(&first.body).unwrap();
        assert_eq!(first["written"], 1);

        let second = push(&server, "one", &parcel);
        let second: serde_json::Value = serde_json::from_slice(&second.body).unwrap();
        assert_eq!(second["written"], 0);
        assert_eq!(second["kept"], 1);
    }

    #[test]
    fn a_body_that_is_not_a_parcel_is_a_400_rather_than_a_500() {
        let response = server().handle(&request(
            "POST",
            "/push",
            "one",
            Some(TOKEN),
            b"not json".to_vec(),
        ));
        assert_eq!(response.status, 400);
    }

    #[test]
    fn waiting_answers_at_once_when_there_is_already_something() {
        let server = server();
        push(&server, "one", &watched(4306, "Silk Cloth"));

        // If this parked it would take fifty seconds, which is the failure.
        let response = server.handle(&get("/wait?since=0", "two", Some(TOKEN)));
        let waited: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(waited["changed"], true);
    }

    #[test]
    fn waiting_says_nothing_changed_to_the_machine_that_changed_it() {
        let server = server();
        push(&server, "one", &watched(4306, "Silk Cloth"));

        let mark = server.changes.mark();
        let held = server
            .with_store(accounts::DEFAULT, |store| store.anything_since(0, "one"))
            .expect("the store");
        assert!(!held, "a machine must not be woken by its own writes");
        let _ = mark;
    }

    #[test]
    fn a_counter_cannot_miss_a_change_that_lands_while_the_caller_is_reading() {
        let changes = Changes::default();
        let mark = changes.mark();
        // The write arrives before anybody parks.
        changes.announce();
        // And the park returns at once rather than sleeping through it.
        assert!(changes.wait(mark, Duration::from_millis(50)));
    }

    fn with_account(method: &str, path: &str, account: &str, body: Vec<u8>) -> Request {
        let mut request = request(method, path, "one", Some(TOKEN), body);
        request
            .headers
            .insert("x-armory-account".into(), account.to_string());
        request
    }

    fn listed(server: &Server) -> serde_json::Value {
        let response = server.handle(&get("/accounts", "one", Some(TOKEN)));
        assert_eq!(response.status, 200);
        serde_json::from_slice(&response.body).unwrap()
    }

    #[test]
    fn two_accounts_do_not_see_each_other() {
        // The whole reason accounts are separate files. Every merge rule in
        // the store is written to fold two views of *one* account together; a
        // shared store would fold two accounts together and call it agreement.
        let server = server();
        let parcel = serde_json::to_vec(&watched(4306, "Silk Cloth")).unwrap();
        server.handle(&with_account("POST", "/push", "first", parcel));

        let mine = server.handle(&with_account("GET", "/pull?since=0", "first", Vec::new()));
        let mine: serde_json::Value = serde_json::from_slice(&mine.body).unwrap();
        assert_eq!(
            mine["parcel"]["rows"].as_array().unwrap().len(),
            0,
            "own rows"
        );

        let others = server.handle(&{
            let mut r = with_account("GET", "/pull?since=0", "first", Vec::new());
            r.headers.insert("x-armory-machine".into(), "two".into());
            r
        });
        let others: serde_json::Value = serde_json::from_slice(&others.body).unwrap();
        assert_eq!(others["parcel"]["rows"].as_array().unwrap().len(), 1);

        // And the second account has never heard of any of it.
        let elsewhere = server.handle(&{
            let mut r = with_account("GET", "/pull?since=0", "second", Vec::new());
            r.headers.insert("x-armory-machine".into(), "two".into());
            r
        });
        let elsewhere: serde_json::Value = serde_json::from_slice(&elsewhere.body).unwrap();
        assert_eq!(elsewhere["parcel"]["rows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn a_client_that_names_no_account_gets_the_default_one() {
        let server = server();
        push(&server, "one", &watched(1, "thing"));
        let held = listed(&server);
        assert_eq!(held[0]["name"], accounts::DEFAULT);
    }

    #[test]
    fn accounts_are_listed_with_how_much_each_holds() {
        let server = server();
        for name in ["alpha", "beta"] {
            let parcel = serde_json::to_vec(&watched(1, "thing")).unwrap();
            server.handle(&with_account("POST", "/push", name, parcel));
        }
        let held = listed(&server);
        let names: Vec<&str> = held
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["alpha", "beta"]);
        assert!(held[0]["rows"].as_i64().unwrap() > 0);
    }

    #[test]
    fn deleting_an_account_needs_it_named_twice() {
        // No undo, and the client's copy is the only one left afterwards. The
        // token is the security; this is against a mistyped or half-built URL.
        let server = server();
        let parcel = serde_json::to_vec(&watched(1, "thing")).unwrap();
        server.handle(&with_account("POST", "/push", "alpha", parcel));

        let bare = server.handle(&with_account(
            "DELETE",
            "/accounts/alpha",
            "alpha",
            Vec::new(),
        ));
        assert_eq!(bare.status, 400);

        let wrong = server.handle(&with_account(
            "DELETE",
            "/accounts/alpha?confirm=beta",
            "alpha",
            Vec::new(),
        ));
        assert_eq!(wrong.status, 400);
        assert_eq!(listed(&server).as_array().unwrap().len(), 1, "still there");

        let right = server.handle(&with_account(
            "DELETE",
            "/accounts/alpha?confirm=alpha",
            "alpha",
            Vec::new(),
        ));
        assert_eq!(right.status, 200);
        assert!(listed(&server).as_array().unwrap().is_empty());
    }

    #[test]
    fn deleting_one_account_leaves_the_others_alone() {
        let server = server();
        for name in ["alpha", "beta"] {
            let parcel = serde_json::to_vec(&watched(1, "thing")).unwrap();
            server.handle(&with_account("POST", "/push", name, parcel));
        }
        server.handle(&with_account(
            "DELETE",
            "/accounts/alpha?confirm=alpha",
            "alpha",
            Vec::new(),
        ));
        let held = listed(&server);
        assert_eq!(held.as_array().unwrap().len(), 1);
        assert_eq!(held[0]["name"], "beta");
    }

    #[test]
    fn an_account_name_that_is_a_path_is_refused_everywhere() {
        // The one string off the network that becomes a filesystem path.
        let server = server();
        for bad in ["../escape", "a/b", ".."] {
            let parcel = serde_json::to_vec(&watched(1, "thing")).unwrap();
            let pushed = server.handle(&with_account("POST", "/push", bad, parcel));
            assert_eq!(pushed.status, 400, "push accepted {bad:?}");
        }
        assert!(listed(&server).as_array().unwrap().is_empty());
    }

    #[test]
    fn deleting_an_account_that_is_not_there_is_a_404_rather_than_a_success() {
        let server = server();
        let response = server.handle(&with_account(
            "DELETE",
            "/accounts/ghost?confirm=ghost",
            "ghost",
            Vec::new(),
        ));
        assert_eq!(response.status, 404);
    }

    #[test]
    fn listing_and_deleting_need_the_token() {
        let server = server();
        assert_eq!(server.handle(&get("/accounts", "one", None)).status, 401);
        assert_eq!(
            server
                .handle(&request(
                    "DELETE",
                    "/accounts/a?confirm=a",
                    "one",
                    None,
                    Vec::new()
                ))
                .status,
            401
        );
    }

    #[test]
    fn a_token_shorter_than_the_floor_is_refused_at_startup() {
        assert!(check_token("short").is_err());
        assert!(check_token(TOKEN).is_ok());
    }

    #[test]
    fn a_token_is_compared_without_leaking_where_it_stopped_matching() {
        assert!(constant_eq(b"abcd", b"abcd"));
        assert!(!constant_eq(b"abcd", b"abce"));
        assert!(!constant_eq(b"abcd", b"abcde"));
    }

    #[test]
    fn a_pull_hands_over_a_batch_at_a_time_and_says_there_is_more() {
        let server = server();
        for item in 0..5u32 {
            push(&server, "one", &watched(item, "thing"));
        }

        let response = server.handle(&get("/pull?since=0&limit=2", "two", Some(TOKEN)));
        let pulled: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(pulled["parcel"]["rows"].as_array().unwrap().len(), 2);
        assert_eq!(pulled["more"], true);
    }

    #[test]
    fn a_limit_beyond_the_batch_is_clamped_rather_than_obeyed() {
        let server = server();
        push(&server, "one", &watched(1, "thing"));
        let response = server.handle(&get("/pull?since=0&limit=999999", "two", Some(TOKEN)));
        assert_eq!(response.status, 200);
    }
}
