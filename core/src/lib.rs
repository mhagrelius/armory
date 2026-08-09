//! The half that links no GTK and opens no socket.
//!
//! Everything below is a pure function over data, with two deliberate
//! exceptions: [`store`] talks to a local SQLite file and [`addon`] reads Lua
//! off disk. Neither touches the network, both are deterministic, and both are
//! tested against real files — the seam worth defending here is the network
//! one, not the disk.
//!
//! Read in this order: [`character`] for who is on the account and [`cohort`]
//! for which of them the application is actually about, [`source`] for how
//! anything is asked, [`achievement`] for the criteria trees, [`run`] for the
//! reason all of it exists, and [`plan`] for where the three come together.

pub mod achievement;
pub mod addon;
pub mod adventure;
pub mod character;
pub mod chronicle;
pub mod cohort;
pub mod hunt;
pub mod market;
pub mod place;
pub mod plan;
pub mod provenance;
pub mod rarity;
pub mod replica;
pub mod run;
pub mod settings;
pub mod source;
pub mod store;
pub mod sync;
pub mod tally;
