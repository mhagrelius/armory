//! Setting the journal up: where the model is, and whether to use it unasked.
//!
//! Shorter than it was. Entries used to go to a hosted API, which meant a key
//! in the keyring, a field that could not be pre-filled, a bill to warn about
//! and a switch that had to default to off. They go to a `llama-server` on this
//! machine now, so all of that collapses to one address and one toggle.
//!
//! The address is not a credential. It travels in plain sight, it lives in the
//! settings file with the region and the client id, and there is nothing here
//! for the keyring to hold.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use crate::model::source::journal::DEFAULT_SERVER;

/// Told the server address and whether to write automatically.
type SaveHandler = Box<dyn Fn(String, bool)>;
/// Told to check whether anything is answering.
type TestHandler = Box<dyn Fn(String)>;

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct JournalDialog {
        pub server: RefCell<Option<adw::EntryRow>>,
        pub automatic: RefCell<Option<adw::SwitchRow>>,
        pub status: RefCell<Option<adw::ActionRow>>,
        pub on_save: RefCell<Option<super::SaveHandler>>,
        pub on_test: RefCell<Option<super::TestHandler>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for JournalDialog {
        const NAME: &'static str = "ArmoryJournalDialog";
        type Type = super::JournalDialog;
        type ParentType = adw::PreferencesDialog;
    }

    impl ObjectImpl for JournalDialog {}
    impl WidgetImpl for JournalDialog {}
    impl AdwDialogImpl for JournalDialog {}
    impl PreferencesDialogImpl for JournalDialog {}
}

glib::wrapper! {
    pub struct JournalDialog(ObjectSubclass<imp::JournalDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl JournalDialog {
    pub fn new(server: &str, automatic: bool) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.build(server, automatic);
        dialog
    }

    fn build(&self, server: &str, automatic: bool) {
        self.set_title("Journal");

        let page = adw::PreferencesPage::builder()
            .title("Journal")
            .icon_name("document-edit-symbolic")
            .build();

        let about = adw::PreferencesGroup::builder()
            .title("Writing entries")
            .description(
                "The Chronicle records every session on its own and needs nothing set up \
                 to do it. This is only for turning one into prose, which a llama-server \
                 on this machine does — the same one Familiar talks to. Nothing is sent \
                 anywhere and nothing is billed.",
            )
            .build();
        page.add(&about);

        let where_group = adw::PreferencesGroup::builder()
            .title("Server")
            .description("llama.cpp's OpenAI-compatible endpoint.")
            .build();

        let address = adw::EntryRow::builder()
            .title("Address")
            .text(server)
            .build();

        let status = adw::ActionRow::builder()
            .title("Not checked yet")
            .subtitle("Test the address to see which model answers.")
            .build();

        let test = gtk::Button::builder()
            .label("Test")
            .valign(gtk::Align::Center)
            .build();
        test.add_css_class("flat");
        let dialog = self.clone();
        test.connect_clicked(move |_| {
            let address = dialog.address();
            dialog.set_status("Checking…", &address);
            if let Some(check) = dialog.imp().on_test.borrow().as_ref() {
                check(address);
            }
        });
        status.add_suffix(&test);

        where_group.add(&address);
        where_group.add(&status);
        page.add(&where_group);

        let behaviour = adw::PreferencesGroup::builder()
            .title("When to write")
            .build();
        let auto = adw::SwitchRow::builder()
            .title("Write entries automatically")
            .subtitle(
                "Write up each new evening as soon as Armory reads it. On by default — \
                 it costs nothing but a few seconds of the machine, and a journal you \
                 have to remember to write is one that does not get written.",
            )
            .active(automatic)
            .build();
        behaviour.add(&auto);
        page.add(&behaviour);

        let save = adw::PreferencesGroup::new();
        let button = adw::ButtonRow::builder().title("Save").build();
        button.add_css_class("suggested-action");
        let dialog = self.clone();
        button.connect_activated(move |_| dialog.save());
        save.add(&button);
        page.add(&save);

        let imp = self.imp();
        *imp.server.borrow_mut() = Some(address);
        *imp.automatic.borrow_mut() = Some(auto);
        *imp.status.borrow_mut() = Some(status);

        self.add(&page);
    }

    /// What is in the address field, falling back to the default when emptied.
    fn address(&self) -> String {
        self.imp()
            .server
            .borrow()
            .as_ref()
            .map(|row| row.text().trim().to_string())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| DEFAULT_SERVER.to_string())
    }

    /// Say what the server answered, or did not.
    pub fn set_status(&self, title: &str, detail: &str) {
        if let Some(row) = self.imp().status.borrow().as_ref() {
            row.set_title(title);
            row.set_subtitle(detail);
        }
    }

    pub fn connect_save<F: Fn(String, bool) + 'static>(&self, handler: F) {
        *self.imp().on_save.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_test<F: Fn(String) + 'static>(&self, handler: F) {
        *self.imp().on_test.borrow_mut() = Some(Box::new(handler));
    }

    fn save(&self) {
        let imp = self.imp();
        let automatic = imp
            .automatic
            .borrow()
            .as_ref()
            .is_some_and(|switch| switch.is_active());

        if let Some(save) = imp.on_save.borrow().as_ref() {
            save(self.address(), automatic);
        }
        self.close();
    }
}
