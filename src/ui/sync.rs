//! Talking to `armory-server`, and nothing else.
//!
//! [`armory_core::sync::Remote`] is a trait so that the core never learns what
//! a socket is; this is libsoup-free, GLib-free plain HTTP answering it. Blunt
//! by design: connect, write a request, read a response, close.
//!
//! # Why not `ui::http`
//!
//! Because that client exists to obey Blizzard's quota. It is one token bucket
//! for the whole application, hung on the main loop, and putting a sync's
//! traffic through it would make a fifty-thousand-row first pass spend the
//! budget a roster sync needs — against a server on the tailnet that has no
//! quota at all. `ui::images` was separated from it for exactly this reason
//! and this is the same argument a second time.
//!
//! # Threading
//!
//! **Everything here blocks, and none of it touches the store.** It is handed
//! a parcel and gives back an answer; the application runs it on a worker
//! through `gio::spawn_blocking` and does every read and write on the thread
//! that owns the database. That is what keeps the change log honest: recording
//! is a flag in the database rather than a property of a connection, so a
//! second writer applying a pull could silence the first writer's changes
//! without either of them noticing. One writer, and the question does not
//! arise.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use armory_core::sync::{Applied, Parcel, Pulled, Remote, SyncError};

/// How long an ordinary call may take.
///
/// Generous, because a first push is megabytes over a tailnet rather than a
/// question and an answer.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long to hold a parked `/wait`.
///
/// Comfortably past the server's own fifty seconds: the server giving up is
/// the normal end of a quiet wait and must not look like the network failing.
const WAIT_TIMEOUT: Duration = Duration::from_secs(75);

/// A configured server.
#[derive(Debug, Clone)]
pub struct Service {
    host: String,
    token: String,
    machine: String,
}

impl Service {
    /// `http://host:port`, a token, and this installation's id.
    ///
    /// `https://` is refused rather than quietly downgraded. Accepting it and
    /// connecting in the clear would be the worst of the three possible
    /// behaviours: the address would say the traffic was encrypted and it
    /// would not be.
    pub fn new(url: &str, token: &str, machine: &str) -> Result<Service, SyncError> {
        let url = url.trim();
        if url.starts_with("https://") {
            return Err(SyncError(
                "this speaks plain HTTP; use http:// and a tailnet address".into(),
            ));
        }
        // The scheme comes off before the slashes do. The other order turns
        // `http://` into `http:`, which then looks like a host with a port.
        let host = url.strip_prefix("http://").unwrap_or(url).trim_matches('/');
        if host.is_empty() {
            return Err(SyncError("no server address".into()));
        }
        // A port is not optional here — nothing serves this on 80.
        let host = if host.contains(':') {
            host.to_string()
        } else {
            format!("{host}:8084")
        };

        Ok(Service {
            host,
            token: token.trim().to_string(),
            machine: machine.to_string(),
        })
    }

    pub fn address(&self) -> &str {
        &self.host
    }

    /// Ask whether it is there at all, without sending anything.
    ///
    /// What the sync page's "reachable" line is. It uses `/health`, which
    /// needs no token, so a server that is up and a token that is wrong are
    /// two different answers rather than one.
    pub fn reachable(&self) -> Result<(), SyncError> {
        self.send("GET", "/health", None, TIMEOUT).map(|_| ())
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Vec<u8>, SyncError> {
        let mut stream = TcpStream::connect(&self.host)
            .map_err(|error| SyncError(format!("could not reach {}: {error}", self.host)))?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| SyncError(error.to_string()))?;

        let body = body.unwrap_or_default();
        let head = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: {}\r\n\
             Authorization: Bearer {}\r\n\
             X-Armory-Machine: {}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            self.host,
            self.token,
            self.machine,
            body.len()
        );

        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(&body))
            .and_then(|()| stream.flush())
            .map_err(|error| SyncError(format!("could not send: {error}")))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|error| SyncError(format!("no answer: {error}")))?;

        split(&raw)
    }

    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<T, SyncError> {
        let answer = self.send(method, path, body, timeout)?;
        serde_json::from_slice(&answer)
            .map_err(|error| SyncError(format!("could not read the answer: {error}")))
    }
}

/// Separate the status from the body, and turn a refusal into a sentence.
fn split(raw: &[u8]) -> Result<Vec<u8>, SyncError> {
    let head_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| SyncError("the answer had no headers".into()))?;
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let body = raw[head_end + 4..].to_vec();

    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| SyncError("the answer had no status".into()))?;

    match status {
        200 => Ok(body),
        // The one failure somebody can actually fix, so it says what to fix.
        // "unexpected status 401" sends people looking at the network.
        401 => Err(SyncError(
            "the server refused the token — check the sync token in Settings".into(),
        )),
        other => Err(SyncError(format!(
            "the server answered {other}: {}",
            String::from_utf8_lossy(&body).trim()
        ))),
    }
}

/// The answer to `/wait`.
#[derive(Debug, serde::Deserialize)]
struct Waited {
    changed: bool,
}

impl Remote for Service {
    fn push(&self, parcel: &Parcel) -> Result<Applied, SyncError> {
        let body = serde_json::to_vec(parcel)
            .map_err(|error| SyncError(format!("could not write the parcel: {error}")))?;
        let report: Report = self.call("POST", "/push", Some(body), TIMEOUT)?;
        Ok(Applied {
            written: report.written,
            removed: report.removed,
            kept: report.kept,
            unreadable: report.unreadable,
        })
    }

    fn pull(&self, since: i64, limit: usize) -> Result<Pulled, SyncError> {
        self.call(
            "GET",
            &format!("/pull?since={since}&limit={limit}"),
            None,
            TIMEOUT,
        )
    }

    fn wait(&self, since: i64) -> Result<bool, SyncError> {
        let waited: Waited =
            self.call("GET", &format!("/wait?since={since}"), None, WAIT_TIMEOUT)?;
        Ok(waited.changed)
    }
}

/// What the server says a push did.
///
/// The same four numbers as [`Applied`], read separately rather than by
/// deriving `Deserialize` on the core's type: the core owns what a merge
/// *means*, and this is a wire shape the shell is reading. It also carries a
/// cursor the client does not use — the log's own `seq`, which a pull asks
/// for.
#[derive(Debug, serde::Deserialize)]
struct Report {
    written: usize,
    removed: usize,
    kept: usize,
    unreadable: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_host_gets_the_servers_port() {
        let service = Service::new("http://nas.example:8084", "t", "m").expect("a service");
        assert_eq!(service.address(), "nas.example:8084");

        let bare = Service::new("http://nas.example", "t", "m").expect("a service");
        assert_eq!(bare.address(), "nas.example:8084");
    }

    #[test]
    fn a_trailing_slash_is_not_part_of_the_address() {
        let service = Service::new("http://nas:8084/", "t", "m").expect("a service");
        assert_eq!(service.address(), "nas:8084");
    }

    #[test]
    fn https_is_refused_rather_than_quietly_downgraded() {
        // Connecting in the clear to an address that says otherwise is the
        // worst of the three things this could do.
        let error = Service::new("https://nas:8084", "t", "m").expect_err("refused");
        assert!(error.0.contains("plain HTTP"), "{}", error.0);
    }

    #[test]
    fn an_empty_address_is_refused() {
        assert!(Service::new("", "t", "m").is_err());
        assert!(Service::new("http://", "t", "m").is_err());
    }

    #[test]
    fn a_refusal_names_the_thing_somebody_can_change() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\n\r\nunauthorized";
        let error = split(raw).expect_err("refused");
        assert!(error.0.contains("token"), "{}", error.0);
    }

    #[test]
    fn a_body_is_taken_from_after_the_blank_line() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{\"ok\":true}";
        assert_eq!(split(raw).expect("a body"), b"{\"ok\":true}");
    }

    #[test]
    fn another_status_carries_what_the_server_said() {
        let raw = b"HTTP/1.1 400 Bad Request\r\n\r\nno X-Armory-Machine header";
        let error = split(raw).expect_err("refused");
        assert!(error.0.contains("400"), "{}", error.0);
        assert!(error.0.contains("X-Armory-Machine"), "{}", error.0);
    }

    #[test]
    fn an_answer_that_is_not_http_is_an_error_rather_than_a_panic() {
        assert!(split(b"").is_err());
        assert!(split(b"garbage").is_err());
    }
}
