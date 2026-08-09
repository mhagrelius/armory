//! The loopback listener the sign-in redirect lands on.
//!
//! Battle.net rejects custom URI schemes — the redirect must begin with `http`
//! or `https` — and it matches the registered string exactly, with no RFC 8252
//! loopback port wildcard. So a real HTTP server on a fixed port on 127.0.0.1 is
//! not a shortcut, it is the only supported shape for a desktop client.
//!
//! `soup::Server` is what serves it. libsoup is already here for the API calls,
//! it is already in `org.gnome.Sdk`, and it runs on the GLib main loop — so the
//! listener costs no thread and no second dependency.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use soup::prelude::*;

use crate::model::source::blizzard::oauth;
use crate::model::source::Reason;

/// A one-shot listener for the authorization code.
pub struct Redirect {
    server: soup::Server,
}

impl Redirect {
    /// Start listening, and call `deliver` when the browser comes back.
    ///
    /// `state` is what was sent to the authorize endpoint. A redirect carrying
    /// anything else did not come from the flow we started and is refused —
    /// with no PKCE verifier to fall back on, this check is the only thing
    /// standing between the flow and a code somebody else planted.
    pub fn listen<F>(state: &str, deliver: F) -> Result<Redirect, glib::Error>
    where
        F: Fn(Result<String, Reason>) + 'static,
    {
        let server = soup::Server::builder().server_header("Armory").build();

        let expected = state.to_string();
        // The browser will happily request /favicon.ico alongside the callback,
        // and a retried or refreshed redirect delivers the same code twice.
        // Answering only once keeps the flow from being driven backwards.
        let spent = Rc::new(RefCell::new(false));

        server.add_handler(
            Some("/callback"),
            glib::clone!(
                #[strong]
                expected,
                #[strong]
                spent,
                move |_server: &soup::Server,
                      message: &soup::ServerMessage,
                      _path: &str,
                      _query: std::collections::HashMap<&str, &str>| {
                    // The parsed query map libsoup hands over is borrowed and
                    // already percent-decoded; the raw string is read back off
                    // the URI instead so that `oauth::parse_callback` — which is
                    // tested against real redirects — owns the decoding.
                    let uri = message.uri().map(|uri| uri.to_string()).unwrap_or_default();
                    let query = uri.split_once('?').map(|(_, query)| query).unwrap_or("");

                    if std::mem::replace(&mut *spent.borrow_mut(), true) {
                        respond(message, Landing::AlreadyDone);
                        return;
                    }

                    match oauth::parse_callback(query) {
                        Ok((code, state)) if state == expected => {
                            respond(message, Landing::SignedIn);
                            deliver(Ok(code));
                        }
                        Ok(_) => {
                            respond(message, Landing::Mismatch);
                            deliver(Err(Reason::Unauthorised(
                                "the sign-in did not come back from the request Armory started"
                                    .into(),
                            )));
                        }
                        Err(reason) => {
                            respond(message, Landing::Refused);
                            deliver(Err(reason));
                        }
                    }
                }
            ),
        );

        server.listen_local(
            oauth::REDIRECT_PORT as u32,
            soup::ServerListenOptions::IPV4_ONLY,
        )?;

        Ok(Redirect { server })
    }
}

impl Drop for Redirect {
    fn drop(&mut self) {
        // The port is fixed and registered, so leaving it bound would make the
        // next sign-in attempt fail with something that reads like a Blizzard
        // problem.
        soup::prelude::ServerExt::disconnect(&self.server);
    }
}

fn respond(message: &soup::ServerMessage, landing: Landing) {
    message.set_status(200, None);
    message.set_response(
        Some("text/html; charset=utf-8"),
        soup::MemoryUse::Copy,
        page(landing).as_bytes(),
    );
}

/// What the browser is told, in each of the four cases it can land in.
#[derive(Clone, Copy)]
enum Landing {
    SignedIn,
    Mismatch,
    Refused,
    AlreadyDone,
}

impl Landing {
    fn heading(self) -> &'static str {
        match self {
            Landing::SignedIn => "Signed in",
            Landing::Mismatch => "That did not come from Armory",
            Landing::Refused => "Sign-in cancelled",
            Landing::AlreadyDone => "Already done",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Landing::SignedIn => "You can close this tab and go back to Armory.",
            Landing::Mismatch => {
                "The sign-in response did not match the one Armory started, so it has \
                 been ignored and nothing has been saved. Start again from Armory."
            }
            Landing::Refused => {
                "Nothing has been saved. You can try again from Armory whenever you like."
            }
            Landing::AlreadyDone => "You can close this tab.",
        }
    }
}

/// Render one of them.
///
/// No external anything: no fonts, no scripts, no images. This is served to a
/// browser by a process that is not a web server and should not start behaving
/// like one — and a page that fetches something is a page that can hang on a
/// machine with no route out.
fn page(landing: Landing) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>Armory</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:4rem auto;max-width:30rem;\
         padding:0 1rem;line-height:1.5;color:#241f31;background:#fafafa}}\
         @media(prefers-color-scheme:dark){{body{{color:#deddda;background:#1d1d20}}}}\
         h1{{font-size:1.3rem}}</style><h1>{}</h1><p>{}</p>",
        landing.heading(),
        landing.body()
    )
}
