//! Armory: a World of Warcraft account, and what is left to do with it.
//!
//! Built for an account that has been played for a decade and is about to be
//! replayed. The interesting problem is not "what have I collected" — Blizzard
//! answers that — it is that the account remembers everything and the *run* has
//! done none of it. See [`model::run`] for what that costs and how much of it
//! can be avoided.
//!
//! Two halves. `armory-core` links no GTK and opens no socket: a source is a
//! pair of pure functions that build a [`model::source::Request`] and parse a
//! response body into an [`model::source::Outcome`], which is why `cargo test`
//! exercises every source, every failure shape and every classification with no
//! display and no network. `ui/` is the only half that knows a window exists,
//! and `ui::http` is the only file in the tree that performs a request to
//! Blizzard.
//!
//! Two deliberate exceptions to "pure": [`model::store`] talks to a local
//! SQLite file and [`model::addon`] reads Lua off disk. Both are deterministic,
//! neither goes near the network, and both are tested against real files. The
//! seam worth defending is the network one.
//!
//! The core is a crate of its own rather than a directory here for one reason:
//! `armory-server` is that same store with a socket in front of it, and it
//! cannot link libadwaita to get at the schema and the merge rules. It is
//! re-exported under the name it had when it was a directory, so every
//! `model::…` path in the application and its tests still means what it did.

pub use armory_core as model;

pub mod ui;

pub const APP_ID: &str = "us.hagreli.Armory";
