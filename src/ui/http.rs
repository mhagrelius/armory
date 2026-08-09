//! The only file in the tree that performs a request.
//!
//! Everything under `model/source/` builds a [`Request`] and parses a body;
//! this drives them. Keeping that boundary in one file is what lets the whole
//! source layer be tested from recorded fixtures with no network.
//!
//! No threads. libsoup's async calls complete on the GLib main loop, so
//! twenty-three characters syncing at twenty-three different speeds need no
//! worker, no channel and no lock — each callback fires when its answer lands.
//!
//! Rate limiting is one token bucket for the whole application, because
//! Blizzard's limits are per client id rather than per endpoint or per IP:
//! 100 requests a second and 36,000 an hour, all sharing one budget. A request
//! that arrives too early is scheduled rather than dropped — a `glib::timeout`
//! on the main loop, never a sleeping thread.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gio;
use gtk::glib;
use soup::prelude::*;

use crate::model::source::{journal, Method, Outcome, Reason, Request};

/// Blizzard's per-second ceiling. The hourly one is two orders of magnitude
/// away from anything a person clicking around will reach, so the burst limit
/// is the one worth enforcing.
const MAX_PER_SECOND: usize = 100;

/// How long to wait on a request, in seconds.
///
/// Sized for an API that answers in milliseconds.
const TIMEOUT: u32 = 20;

/// Kept a little under the ceiling. A 429 costs a retry and a round trip;
/// spacing requests costs microseconds.
const MIN_INTERVAL: Duration = Duration::from_millis(1000 / MAX_PER_SECOND as u64 + 2);

/// How long to stand down after Blizzard says to slow down.
const BACKOFF: Duration = Duration::from_secs(5);

/// A body, and the stamp that makes the next request for it conditional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub body: Vec<u8>,
    pub last_modified: Option<String>,
}

#[derive(Clone)]
pub struct Http {
    session: soup::Session,
    gate: Rc<RefCell<Gate>>,
}

struct Gate {
    next_allowed: Instant,
    /// When each of the last requests went out, so the per-second ceiling is
    /// enforced over a window rather than only between neighbours.
    recent: VecDeque<Instant>,
}

impl Default for Http {
    fn default() -> Self {
        Http::new()
    }
}

impl Http {
    pub fn new() -> Self {
        Http::with_timeout(TIMEOUT)
    }

    /// A client that will wait longer than an API call has any business taking.
    ///
    /// For the one source here that is not a database lookup: writing four
    /// hundred words takes a language model tens of seconds, and twenty of them
    /// is not a timeout so much as a guarantee the feature never works. Its own
    /// client rather than a per-request setting because it also wants its own
    /// rate gate — Anthropic's limits are not Blizzard's, and one journal entry
    /// should not spend a sync's budget.
    pub fn with_timeout(seconds: u32) -> Self {
        Http {
            session: soup::Session::builder()
                .timeout(seconds)
                .user_agent(concat!("Armory/", env!("CARGO_PKG_VERSION")))
                .build(),
            gate: Rc::new(RefCell::new(Gate {
                next_allowed: Instant::now(),
                recent: VecDeque::new(),
            })),
        }
    }

    /// Perform a request.
    ///
    /// `deliver` is called exactly once, on the main loop, with the response or
    /// with the reason there is not one. It is never called with an exception
    /// and there is nothing to catch: a source refusing is an expected outcome
    /// of a sync, not an error in it.
    pub fn fetch<F>(&self, request: Request, deliver: F)
    where
        F: FnOnce(Outcome<Response>) + 'static,
    {
        let wait = self.reserve();
        let http = self.clone();
        if wait.is_zero() {
            http.send(request, deliver);
        } else {
            glib::timeout_add_local_once(wait, move || http.send(request, deliver));
        }
    }

    /// Claim the next slot, and say how long until it opens.
    fn reserve(&self) -> Duration {
        let mut gate = self.gate.borrow_mut();
        let now = Instant::now();

        // Drop anything that left the one-second window.
        while gate
            .recent
            .front()
            .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(1))
        {
            gate.recent.pop_front();
        }

        let mut at = gate.next_allowed.max(now);
        if gate.recent.len() >= MAX_PER_SECOND {
            if let Some(oldest) = gate.recent.front() {
                at = at.max(*oldest + Duration::from_secs(1));
            }
        }

        gate.recent.push_back(at);
        gate.next_allowed = at + MIN_INTERVAL;
        at.saturating_duration_since(now)
    }

    /// Push the next slot out after being told to slow down.
    fn penalise(&self) {
        let mut gate = self.gate.borrow_mut();
        gate.next_allowed = Instant::now() + BACKOFF;
    }

    fn send<F>(&self, request: Request, deliver: F)
    where
        F: FnOnce(Outcome<Response>) + 'static,
    {
        let Ok(message) = soup::Message::new(request.method.as_str(), &request.url) else {
            deliver(Outcome::Unusable(Reason::Network(format!(
                "{} is not a URL",
                request.url
            ))));
            return;
        };

        if let Some(headers) = message.request_headers() {
            for (name, value) in &request.headers {
                headers.replace(name, value);
            }
        }

        if request.method == Method::Post {
            if let Some(body) = &request.body {
                // Taken from the request rather than assumed. The token
                // endpoint posts a form and the Messages API posts JSON, and a
                // JSON body announced as form-encoded is rejected outright.
                let content_type = request
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("application/x-www-form-urlencoded");
                message.set_request_body_from_bytes(
                    Some(content_type),
                    Some(&glib::Bytes::from(body.as_bytes())),
                );
            }
        }

        let source = request.source;
        let http = self.clone();
        let sent = message.clone();
        self.session.send_and_read_async(
            &message,
            glib::Priority::DEFAULT,
            gio::Cancellable::NONE,
            move |result| {
                let status = sent.status_code() as u16;
                let last_modified = sent
                    .response_headers()
                    .and_then(|headers| headers.one("Last-Modified"))
                    .map(|value| value.to_string());

                let outcome = match result {
                    Err(error) => {
                        // libsoup reports a timeout as an ordinary I/O error, so
                        // the distinction is made on the message rather than
                        // lost. A timed-out sync and an unreachable one read
                        // differently to a person.
                        let text = error.to_string();
                        if text.contains("Timeout") || text.contains("timed out") {
                            Outcome::Unusable(Reason::Timeout)
                        } else {
                            Outcome::Unusable(Reason::Network(text))
                        }
                    }
                    // The whole point of the conditional request: our copy is
                    // still good, and this cost one round trip and no bytes.
                    Ok(_) if status == 304 => Outcome::Unchanged,
                    Ok(_) if status == 429 || status == 503 => {
                        http.penalise();
                        Outcome::Unusable(Reason::RateLimited)
                    }
                    // Everything below this line about 401, 403 and 404 is
                    // true of Blizzard and false of anywhere else. A 403 is a
                    // privacy checkbox *there*; the Messages API answers one
                    // for an ordinary permission problem, and reporting that as
                    // "your Battle.net privacy settings" would be a sentence
                    // about the wrong account entirely.
                    //
                    // So the one non-Blizzard source reads its own statuses,
                    // and reads the body while it is at it: these are the
                    // errors a person can actually act on — a mistyped key, an
                    // empty balance — and the service says which in words.
                    Ok(bytes) if !source.is_blizzard() && !(200..300).contains(&status) => {
                        let detail = journal::parse_error(&bytes)
                            .unwrap_or_else(|| format!("HTTP {status}"));
                        if status == 401 {
                            Outcome::Unusable(Reason::Unauthorised(detail))
                        } else {
                            Outcome::Unusable(Reason::Declined(detail))
                        }
                    }
                    Ok(_) if status == 401 => {
                        Outcome::Unusable(Reason::Unauthorised("the sign-in has expired".into()))
                    }
                    // Blizzard answers 403 when the account has third-party
                    // data sharing switched off. That is a checkbox on their
                    // privacy settings, not a fault here, and saying "HTTP 403"
                    // would send someone hunting for a bug.
                    Ok(_) if status == 403 => Outcome::Unusable(Reason::SharingDisabled),
                    // A character who has never logged in since the API started
                    // tracking them 404s. That is an answer about the
                    // character, not a failure of the request.
                    Ok(_) if status == 404 => Outcome::Empty,
                    Ok(_) if !(200..300).contains(&status) => {
                        Outcome::Unusable(Reason::Http(status))
                    }
                    Ok(bytes) if bytes.is_empty() => Outcome::Empty,
                    Ok(bytes) => Outcome::Found(Response {
                        body: bytes.to_vec(),
                        last_modified,
                    }),
                };
                deliver(outcome);
            },
        );
    }
}
