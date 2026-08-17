//! The half that knows a window exists.
//!
//! Widget trees are built in Rust — no `.ui` XML, no Blueprint, no GResource.
//! The structure of a page is then readable in the same file as the behaviour
//! that drives it, which for an application this size is worth more than a
//! designer could give back. The sibling apps are built the same way.
//!
//! Widgets report what a person did and nothing else. [`ArmoryApplication`] is
//! the only object here that asks a source anything or mutates state, and
//! `redirect` is the only module that listens on a socket. Three open one:
//! `http` for Blizzard, `images` for the render service, and `sync` for the
//! account's own server — each with its own client, because a rate gate that
//! exists for Blizzard's quota has no business slowing a scrolling grid or a
//! push to the NAS.

mod achievement_dialog;
pub mod almanac;
mod application;
pub mod character_page;
mod chronicle_page;
mod collectible_dialog;
mod collection_page;
mod collector;
mod http;
mod images;
mod journal_dialog;
mod keyring;
mod market_page;
mod onboarding;
mod redirect;
mod reputations_page;
mod roster_page;
pub mod run_page;
mod sync;
pub mod sync_dialog;
mod watch_dialog;
mod window;
pub mod zone_page;

pub use achievement_dialog::AchievementDialog;
pub use application::ArmoryApplication;
pub use character_page::CharacterPage;
pub use chronicle_page::ChroniclePage;
pub use collectible_dialog::CollectibleDialog;
pub use collection_page::CollectionPage;
pub use images::{Art, Images};
pub use journal_dialog::JournalDialog;
pub use market_page::{MarketPage, Quote};
pub use onboarding::Onboarding;
pub use reputations_page::ReputationsPage;
pub use roster_page::{RosterPage, Warband};
pub use run_page::RunPage;
pub use sync::Service;
pub use sync_dialog::SyncDialog;
pub use watch_dialog::WatchDialog;
pub use window::ArmoryWindow;
pub use zone_page::ZonePage;

/// The application stylesheet, compiled in.
pub const STYLE: &str = include_str!("style.css");

/// Load the stylesheet at application priority, above the theme and below the
/// user's own overrides.
///
/// Two providers rather than one. The first is [`STYLE`], which never changes.
/// The second is the `--al-*` colour tokens, generated from
/// [`almanac::Palette`] and *reloaded whenever the colour scheme changes* —
/// libadwaita gives an application no CSS selector for light or dark, so an
/// application with a palette of its own has to watch `AdwStyleManager::dark`
/// and swap the definitions itself.
pub fn load_stylesheet(display: &gtk::gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let tokens = gtk::CssProvider::new();
    tokens.load_from_string(&almanac::Palette::current().css());
    gtk::style_context_add_provider_for_display(
        display,
        &tokens,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    adw::StyleManager::default().connect_dark_notify(move |_| {
        tokens.load_from_string(&almanac::Palette::current().css());
    });
}
