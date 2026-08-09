//! Setting Armory up: registering an API client, and signing in.
//!
//! This page exists because of one fact about Battle.net. There is no PKCE and
//! no public-client mode — Blizzard developer relations confirmed in October
//! 2024 that every registered client is confidential — so the authorization
//! code flow needs a client secret, and a secret shipped inside a distributed
//! binary is both a terms violation and a way to pool every user of the
//! application into one 36,000-per-hour quota.
//!
//! So the user registers their own client. That is a worse first run than a
//! sign-in button, and there is no version of this application where it is
//! avoidable. What can be done is to make the steps short, explain why in one
//! sentence rather than a manual, and put the exact string to paste on screen
//! with a copy button next to it.
//!
//! The portal is also unreliable, and has been since November 2025 — creating a
//! a client regularly answers 500. The usual cause is that client names must be
//! globally unique across every developer and the form does not say so, so this
//! page does.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use crate::model::source::blizzard::{oauth, Region};

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct Onboarding {
        pub client_id: RefCell<Option<adw::EntryRow>>,
        pub client_secret: RefCell<Option<adw::PasswordEntryRow>>,
        pub region: RefCell<Option<adw::ComboRow>>,
        pub sign_in: RefCell<Option<gtk::Button>>,
        pub status: RefCell<Option<gtk::Label>>,
        pub skip: RefCell<Option<gtk::Button>>,
        /// Whether the keyring is already holding a secret for this client.
        ///
        /// Part of "are there credentials", because a saved secret is a
        /// credential. Without it the sign-in button reads the empty password
        /// field, concludes there is nothing to sign in with, and stays
        /// insensitive while the row above it says the secret is saved.
        pub secret_held: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Onboarding {
        const NAME: &'static str = "ArmoryOnboarding";
        type Type = super::Onboarding;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for Onboarding {}
    impl WidgetImpl for Onboarding {}
    impl BinImpl for Onboarding {}
}

glib::wrapper! {
    pub struct Onboarding(ObjectSubclass<imp::Onboarding>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::new()
    }
}

impl Onboarding {
    pub fn new() -> Self {
        let page: Self = glib::Object::builder().build();
        page.build();
        page
    }

    fn build(&self) {
        let clamp = adw::Clamp::builder().maximum_size(680).build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();

        column.append(
            &adw::StatusPage::builder()
                .icon_name("dialog-password-symbolic")
                .title("Connect your Battle.net account")
                .description(
                    "Battle.net has no sign-in that works for desktop apps without a \
                     key of their own, so Armory uses one you create. It takes about \
                     a minute, and it means your data is fetched under your own \
                     allowance rather than shared with every other person using \
                     Armory.",
                )
                .build(),
        );

        column.append(&self.step_one());
        column.append(&self.step_two());
        column.append(&self.step_three());
        column.append(&self.without_it());

        clamp.set_child(Some(&column));
        self.set_child(Some(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&clamp)
                .build(),
        ));
    }

    /// Open the portal, and say what to fill in.
    fn step_one(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("1. Create an API client")
            .description(
                "Sign in at the Battle.net developer portal and create a client. \
                 Give it any name you like — but it has to be one nobody else has \
                 used, and the form will answer with a server error rather than \
                 saying so. If that happens, change the name and try again.",
            )
            .build();

        let open = adw::ActionRow::builder()
            .title("Battle.net developer portal")
            .subtitle("community.developer.battle.net")
            .activatable(true)
            .build();
        open.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
        open.connect_activated(|row| {
            let launcher =
                gtk::UriLauncher::new("https://community.developer.battle.net/application");
            launcher.launch(
                row.root().and_downcast_ref::<gtk::Window>(),
                gtk::gio::Cancellable::NONE,
                |_| {},
            );
        });
        group.add(&open);

        // The one string that has to be exactly right. Blizzard matches it
        // character for character, rejects custom schemes, and offers no
        // loopback port wildcard — so it is shown rather than described, with a
        // button that removes the chance of a typo.
        let redirect = adw::ActionRow::builder()
            .title("Redirect URI")
            .subtitle(oauth::redirect_uri())
            .subtitle_selectable(true)
            .build();
        redirect.add_css_class("property");

        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy the redirect URI")
            .valign(gtk::Align::Center)
            .build();
        copy.add_css_class("flat");
        copy.connect_clicked(|button| {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&oauth::redirect_uri());
            }
            button.set_icon_name("object-select-symbolic");
            let button = button.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                button.set_icon_name("edit-copy-symbolic");
            });
        });
        redirect.add_suffix(&copy);
        group.add(&redirect);

        group
    }

    /// Take the credentials.
    fn step_two(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("2. Paste the client back here")
            .description(
                "The secret goes into your login keyring, not into a file next to \
                 Armory's settings.",
            )
            .build();

        let client_id = adw::EntryRow::builder().title("Client ID").build();
        let client_secret = adw::PasswordEntryRow::builder()
            .title("Client secret")
            .build();

        let region = adw::ComboRow::builder()
            .title("Region")
            .subtitle("Where your characters are")
            .model(&gtk::StringList::new(
                &Region::ALL.map(|region| region.label()),
            ))
            .build();

        group.add(&client_id);
        group.add(&client_secret);
        group.add(&region);

        *self.imp().client_id.borrow_mut() = Some(client_id);
        *self.imp().client_secret.borrow_mut() = Some(client_secret);
        *self.imp().region.borrow_mut() = Some(region);

        group
    }

    /// Say whether a secret is already in the keyring.
    ///
    /// A password field cannot be pre-filled from the keyring without reading
    /// the secret out to put it there, and a field of dots that came from
    /// storage is indistinguishable from one somebody typed. So the field stays
    /// empty and the row says why — because an empty field beside a saved
    /// client ID reads as "Armory forgot", and the natural response to that is
    /// to go and find the secret again.
    pub fn set_secret_held(&self, held: bool) {
        self.imp().secret_held.set(held);
        // The button's sensitivity is recomputed from the fields whenever one
        // changes, and this changes neither — so it has to be asked again by
        // hand or the button stays as it was.
        self.refresh_sign_in();

        let Some(entry) = self.imp().client_secret.borrow().clone() else {
            return;
        };
        entry.set_show_apply_button(false);
        if held {
            entry.set_title("Client secret — already saved");
            entry.set_tooltip_text(Some(
                "A secret for this client is in your login keyring. Leave this blank \
                 to sign in with it, or type a new one to replace it.",
            ));
        } else {
            entry.set_title("Client secret");
            entry.set_tooltip_text(None);
        }
    }

    /// Sign in.
    fn step_three(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("3. Sign in")
            .description(
                "Armory opens Battle.net in your browser. It never sees your \
                 password — Battle.net sends back a token, and that is all Armory \
                 holds.",
            )
            .build();

        let sign_in = gtk::Button::builder()
            .label("Sign in with Battle.net")
            .halign(gtk::Align::Center)
            .margin_top(6)
            .sensitive(false)
            .build();
        sign_in.add_css_class("suggested-action");
        sign_in.add_css_class("pill");

        let status = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .margin_top(6)
            .visible(false)
            .build();
        status.add_css_class("dimmed");

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        column.append(&sign_in);
        column.append(&status);
        group.add(&column);

        *self.imp().sign_in.borrow_mut() = Some(sign_in);

        // Nothing can be signed in with until there is an id and a secret, and
        // a button that does nothing when pressed teaches people to distrust
        // buttons. The secret may be one already in the keyring rather than one
        // in the field, which is why this asks `has_credentials` rather than
        // reading the two widgets.
        for entry in [
            self.imp()
                .client_id
                .borrow()
                .clone()
                .map(|e| e.upcast::<gtk::Widget>()),
            self.imp()
                .client_secret
                .borrow()
                .clone()
                .map(|e| e.upcast::<gtk::Widget>()),
        ]
        .into_iter()
        .flatten()
        {
            let page = self.clone();
            if let Some(row) = entry.downcast_ref::<adw::EntryRow>() {
                row.connect_changed(move |_| page.refresh_sign_in());
            } else if let Some(row) = entry.downcast_ref::<adw::PasswordEntryRow>() {
                row.connect_changed(move |_| page.refresh_sign_in());
            }
        }
        self.refresh_sign_in();
        *self.imp().status.borrow_mut() = Some(status);

        group
    }

    /// The way out, for when Blizzard's portal will not cooperate.
    ///
    /// It has been answering 500 to client creation since late 2025, and an
    /// application that cannot be used without it is an application that cannot
    /// be used. The addon covers everything except the auction house and any
    /// character you have not logged in on — and for collections it is
    /// *better*, because the in-game journals give a sentence where the web API
    /// gives one word.
    fn without_it(&self) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Or skip all of this")
            .description(
                "Armory works without a Battle.net client at all, using the collector \
                 addon instead. You lose auction prices and any character you have not \
                 logged in on — and you gain better sources for mounts, pets and toys, \
                 because the in-game journals name the boss and the raid where the web \
                 API only says \u{201c}Drop\u{201d}.",
            )
            .build();

        let skip = gtk::Button::builder()
            .label("Use the addon only")
            .halign(gtk::Align::Center)
            .margin_top(6)
            .build();
        skip.add_css_class("pill");

        group.add(&skip);
        *self.imp().skip.borrow_mut() = Some(skip);
        group
    }

    pub fn connect_skipped<F: Fn() + 'static>(&self, handler: F) {
        if let Some(button) = self.imp().skip.borrow().as_ref() {
            button.connect_clicked(move |_| handler());
        }
    }

    /// Enable the sign-in button if there is anything to sign in with.
    fn refresh_sign_in(&self) {
        let can = self.has_credentials();
        if let Some(button) = self.imp().sign_in.borrow().as_ref() {
            button.set_sensitive(can);
        }
    }

    fn has_credentials(&self) -> bool {
        !self.client_id().is_empty()
            && (!self.client_secret().is_empty() || self.imp().secret_held.get())
    }

    pub fn client_id(&self) -> String {
        self.imp()
            .client_id
            .borrow()
            .as_ref()
            .map(|entry| entry.text().trim().to_string())
            .unwrap_or_default()
    }

    pub fn client_secret(&self) -> String {
        self.imp()
            .client_secret
            .borrow()
            .as_ref()
            .map(|entry| entry.text().trim().to_string())
            .unwrap_or_default()
    }

    pub fn region(&self) -> Region {
        let index = self
            .imp()
            .region
            .borrow()
            .as_ref()
            .map(|row| row.selected() as usize)
            .unwrap_or(0);
        Region::ALL.get(index).copied().unwrap_or(Region::Us)
    }

    /// Prefill from settings, for someone coming back to change something.
    pub fn set_client_id(&self, id: &str) {
        if let Some(entry) = self.imp().client_id.borrow().as_ref() {
            entry.set_text(id);
        }
    }

    pub fn set_region(&self, region: Region) {
        if let Some(row) = self.imp().region.borrow().as_ref() {
            let index = Region::ALL
                .iter()
                .position(|candidate| *candidate == region)
                .unwrap_or(0);
            row.set_selected(index as u32);
        }
    }

    /// Say what is happening, or what went wrong.
    pub fn report(&self, message: Option<&str>) {
        if let Some(label) = self.imp().status.borrow().as_ref() {
            match message {
                Some(text) => {
                    label.set_text(text);
                    label.set_visible(true);
                }
                None => label.set_visible(false),
            }
        }
    }

    /// Whether the sign-in button can be pressed. Held down while a sign-in is
    /// already in flight, because the redirect listener binds a fixed port and
    /// a second attempt would fail to bind it.
    pub fn set_busy(&self, busy: bool) {
        if let Some(button) = self.imp().sign_in.borrow().as_ref() {
            button.set_sensitive(!busy && self.has_credentials());
            button.set_label(if busy {
                "Waiting for your browser…"
            } else {
                "Sign in with Battle.net"
            });
        }
    }

    pub fn connect_sign_in<F: Fn() + 'static>(&self, handler: F) {
        if let Some(button) = self.imp().sign_in.borrow().as_ref() {
            button.connect_clicked(move |_| handler());
        }
    }
}
