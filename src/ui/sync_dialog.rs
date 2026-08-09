//! Where this account is shared to, and what is waiting to go there.
//!
//! A pass is silent when it works — sync is awareness, not applause — so this
//! is the place to come and ask. It is deliberately more than the sibling
//! applications' read-only status list, because Armory's sync has something
//! theirs does not: a **queue**, filled by an addon on a machine that is also
//! playing the game. "Did tonight's evening reach the server" is a real
//! question with a real answer, and a number beside each kind of thing is that
//! answer.
//!
//! Not a place in the sidebar. The ten places are about the account — a
//! roster, a run, a market — and this is about the program. It also reports
//! rather than acts, apart from the one button that says do it now.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

/// Told the server address and, when one was typed, a new token.
///
/// `None` for the token means "keep the one you have". The field cannot be
/// pre-filled without reading the secret back out to display it, so it is
/// empty on every launch — the same rule the Battle.net secret follows, and
/// the row's subtitle says whether one is held rather than leaving somebody
/// hunting for a value Armory is already holding.
type SaveHandler = Box<dyn Fn(String, Option<String>)>;

/// Told to run a pass now.
type PassHandler = Box<dyn Fn()>;

/// Everything the dialog draws, gathered by the application.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// `http://host:port`, or empty when sharing is off.
    pub server: String,
    /// Whether a token is held.
    pub token_held: bool,
    /// This installation's id in the log.
    pub machine: String,
    /// Whether a pass is in flight right now.
    pub passing: bool,
    /// What is waiting to go up, by table, largest first.
    pub queued: Vec<(String, usize)>,
    /// When the oldest thing waiting was written.
    pub queued_since: Option<String>,
    /// The last pass: when, what moved, and what went wrong.
    pub last: Option<Pass>,
    /// Consecutive failures. Three is where it stops being noise.
    pub failures: usize,
}

#[derive(Debug, Clone)]
pub struct Pass {
    pub when: String,
    pub sent: usize,
    pub landed: usize,
    pub removed: usize,
    pub unreadable: usize,
    pub failed: Option<String>,
}

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct SyncDialog {
        pub server: RefCell<Option<adw::EntryRow>>,
        pub token: RefCell<Option<adw::PasswordEntryRow>>,
        pub queue: RefCell<Option<adw::PreferencesGroup>>,
        pub queue_rows: RefCell<Vec<adw::ActionRow>>,
        pub state_rows: RefCell<Vec<adw::ActionRow>>,
        pub on_save: RefCell<Option<super::SaveHandler>>,
        pub on_pass: RefCell<Option<super::PassHandler>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SyncDialog {
        const NAME: &'static str = "ArmorySyncDialog";
        type Type = super::SyncDialog;
        type ParentType = adw::PreferencesDialog;
    }

    impl ObjectImpl for SyncDialog {}
    impl WidgetImpl for SyncDialog {}
    impl AdwDialogImpl for SyncDialog {}
    impl PreferencesDialogImpl for SyncDialog {}
}

glib::wrapper! {
    pub struct SyncDialog(ObjectSubclass<imp::SyncDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for SyncDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncDialog {
    pub fn new() -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.build();
        dialog
    }

    fn build(&self) {
        self.set_title("Sharing");

        let page = adw::PreferencesPage::builder()
            .title("Sharing")
            .icon_name("folder-remote-symbolic")
            .build();

        let about = adw::PreferencesGroup::builder()
            .title("One account, several machines")
            .description(
                "Each machine keeps the whole account and works with the server switched \
                 off. This is where they meet: what the addon recorded here goes up, and \
                 what the other machines recorded comes down. Leave the address empty and \
                 Armory keeps to itself.",
            )
            .build();
        page.add(&about);

        // -- where -----------------------------------------------------------

        let where_group = adw::PreferencesGroup::builder()
            .title("Server")
            .description("An armory-server on your own network. Plain HTTP over the tailnet.")
            .build();

        let address = adw::EntryRow::builder().title("Address").build();
        let token = adw::PasswordEntryRow::builder().title("Token").build();

        where_group.add(&address);
        where_group.add(&token);
        page.add(&where_group);

        // -- what is waiting --------------------------------------------------

        let queue = adw::PreferencesGroup::builder()
            .title("Waiting to go up")
            .description("What this machine has recorded and the server has not taken yet.")
            .build();
        page.add(&queue);

        // -- how it is going ---------------------------------------------------

        let state = adw::PreferencesGroup::builder().title("State").build();
        let machine = adw::ActionRow::builder().title("This machine").build();
        let last = adw::ActionRow::builder().title("Last pass").build();
        state.add(&machine);
        state.add(&last);

        let now = gtk::Button::builder()
            .label("Sync Now")
            .valign(gtk::Align::Center)
            .build();
        now.add_css_class("flat");
        let dialog = self.clone();
        now.connect_clicked(move |_| {
            if let Some(pass) = dialog.imp().on_pass.borrow().as_ref() {
                pass();
            }
        });
        last.add_suffix(&now);
        page.add(&state);

        // -- saving -------------------------------------------------------------

        let save_group = adw::PreferencesGroup::new();
        let button = adw::ButtonRow::builder().title("Save").build();
        button.add_css_class("suggested-action");
        let dialog = self.clone();
        button.connect_activated(move |_| dialog.save());
        save_group.add(&button);
        page.add(&save_group);

        let imp = self.imp();
        *imp.server.borrow_mut() = Some(address);
        *imp.token.borrow_mut() = Some(token);
        *imp.queue.borrow_mut() = Some(queue);
        *imp.state_rows.borrow_mut() = vec![machine, last];

        self.add(&page);
    }

    /// Redraw everything the application knows.
    pub fn show_state(&self, state: &State) {
        let imp = self.imp();

        if let Some(row) = imp.server.borrow().as_ref() {
            // Only when it differs, or setting the text moves the cursor out
            // from under somebody who is typing into it.
            if row.text() != state.server {
                row.set_text(&state.server);
            }
        }
        if let Some(row) = imp.token.borrow().as_ref() {
            row.set_title(if state.token_held {
                "Token — one is held; type to replace it"
            } else {
                "Token"
            });
        }

        let rows = imp.state_rows.borrow();
        if let Some(machine) = rows.first() {
            machine.set_subtitle(if state.machine.is_empty() {
                "not named yet"
            } else {
                &state.machine
            });
        }
        if let Some(last) = rows.get(1) {
            last.set_subtitle(&describe(state));
        }
        drop(rows);

        self.show_queue(state);
    }

    fn show_queue(&self, state: &State) {
        let imp = self.imp();
        let Some(group) = imp.queue.borrow().clone() else {
            return;
        };

        for row in imp.queue_rows.borrow_mut().drain(..) {
            group.remove(&row);
        }

        if state.server.is_empty() {
            let row = adw::ActionRow::builder()
                .title("Not sharing")
                .subtitle("Set an address and a token above to share this account.")
                .build();
            group.add(&row);
            imp.queue_rows.borrow_mut().push(row);
            return;
        }

        if state.queued.is_empty() {
            let row = adw::ActionRow::builder()
                .title("Nothing waiting")
                .subtitle("Everything this machine has recorded is on the server.")
                .build();
            group.add(&row);
            imp.queue_rows.borrow_mut().push(row);
            return;
        }

        for (scope, count) in &state.queued {
            let row = adw::ActionRow::builder()
                .title(pretty(scope))
                .subtitle(format!("{count} {}", plural(*count, "row", "rows")))
                .build();
            // Monospaced, because it is a number. The almanac's rule, applied
            // to the one place in this dialog that has one.
            let count = gtk::Label::builder()
                .label(count.to_string())
                .valign(gtk::Align::Center)
                .build();
            count.add_css_class("al-figure");
            row.add_suffix(&count);
            group.add(&row);
            imp.queue_rows.borrow_mut().push(row);
        }

        if let Some(since) = &state.queued_since {
            let row = adw::ActionRow::builder()
                .title("Oldest")
                .subtitle(since.clone())
                .build();
            group.add(&row);
            imp.queue_rows.borrow_mut().push(row);
        }
    }

    pub fn connect_save<F: Fn(String, Option<String>) + 'static>(&self, handler: F) {
        *self.imp().on_save.borrow_mut() = Some(Box::new(handler));
    }

    pub fn connect_pass<F: Fn() + 'static>(&self, handler: F) {
        *self.imp().on_pass.borrow_mut() = Some(Box::new(handler));
    }

    fn save(&self) {
        let imp = self.imp();
        let address = imp
            .server
            .borrow()
            .as_ref()
            .map(|row| row.text().trim().to_string())
            .unwrap_or_default();

        // An empty field means keep what is held, never "clear it". Clearing
        // is what emptying the *address* does, and that is the deliberate act.
        let token = imp
            .token
            .borrow()
            .as_ref()
            .map(|row| row.text().trim().to_string())
            .filter(|text| !text.is_empty());

        if let Some(save) = imp.on_save.borrow().as_ref() {
            save(address, token);
        }
        self.close();
    }
}

/// The last pass, in a sentence.
fn describe(state: &State) -> String {
    if state.server.is_empty() {
        return "Sharing is off.".into();
    }
    if state.passing {
        return "Running now…".into();
    }
    let Some(last) = &state.last else {
        return "Not yet.".into();
    };
    if let Some(error) = &last.failed {
        // The count is the thing worth knowing. One failed pass is a NAS
        // asleep or a machine between networks; five in a row is a problem.
        return format!(
            "{} — failed {} in a row: {error}",
            last.when,
            plural(state.failures, "time", "times")
        );
    }
    if last.sent + last.landed + last.removed == 0 {
        return format!("{} — nothing to do.", last.when);
    }

    let mut parts = Vec::new();
    if last.sent > 0 {
        parts.push(format!("{} up", last.sent));
    }
    if last.landed > 0 {
        parts.push(format!("{} down", last.landed));
    }
    if last.removed > 0 {
        parts.push(format!("{} removed", last.removed));
    }
    if last.unreadable > 0 {
        // Worth its own clause rather than a silent drop: a number here is
        // what one machine running an older build looks like.
        parts.push(format!("{} not understood", last.unreadable));
    }
    format!("{} — {}.", last.when, parts.join(", "))
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

/// A table's name as somebody would say it.
///
/// The wire names are the SQL ones because one name is better than two, and
/// `earned_reputation` is not what a person calls it.
fn pretty(scope: &str) -> String {
    match scope {
        "character" => "Characters",
        "enrolment" => "Who is in the run",
        "detail" => "Character detail",
        "attribution" => "Who earned what",
        "currency" => "Currencies",
        "earned_reputation" => "Reputation earned",
        "earned_currency" => "Currency earned",
        "tally" => "Lifetime counters",
        "recipe" => "Recipes",
        "recipe_reagent" => "Recipe reagents",
        "instance" => "Dungeons and raids",
        "encounter" => "Bosses",
        "criterion" => "Achievement criteria",
        "warband_item" => "Warband bank",
        "pet_held" => "Pets held",
        "run" => "The run",
        "goal" => "Goals",
        "collectible" => "Collections",
        "achievement" => "Achievements",
        "price" => "Price history",
        "snapshot" => "The auction house",
        "item" => "Item names",
        "watched" => "Watched items",
        "watched_realm" => "Watched realms",
        "session" => "Evenings",
        "entry" => "Journal entries",
        "forgotten" => "Evenings you threw away",
        "response" => "Cached replies",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sharing() -> State {
        State {
            server: "nas:8084".into(),
            ..State::default()
        }
    }

    #[test]
    fn every_table_that_travels_has_a_name_somebody_would_say() {
        // A new table in `sync::TABLES` with no entry here shows up in the
        // dialog as `earned_reputation`, which is the failure this catches.
        for table in armory_core::sync::TABLES {
            assert_ne!(
                pretty(table.name),
                table.name,
                "{} has no readable name",
                table.name
            );
        }
    }

    #[test]
    fn a_pass_that_did_nothing_says_so_rather_than_listing_three_zeroes() {
        let state = State {
            last: Some(Pass {
                when: "just now".into(),
                sent: 0,
                landed: 0,
                removed: 0,
                unreadable: 0,
                failed: None,
            }),
            ..sharing()
        };
        assert_eq!(describe(&state), "just now — nothing to do.");
    }

    #[test]
    fn a_pass_that_moved_things_names_which_way_they_went() {
        let state = State {
            last: Some(Pass {
                when: "just now".into(),
                sent: 12,
                landed: 3,
                removed: 1,
                unreadable: 0,
                failed: None,
            }),
            ..sharing()
        };
        assert_eq!(describe(&state), "just now — 12 up, 3 down, 1 removed.");
    }

    #[test]
    fn rows_the_other_end_could_not_read_are_said_out_loud() {
        let state = State {
            last: Some(Pass {
                when: "just now".into(),
                sent: 0,
                landed: 4,
                removed: 0,
                unreadable: 2,
                failed: None,
            }),
            ..sharing()
        };
        assert!(describe(&state).contains("2 not understood"));
    }

    #[test]
    fn a_failure_carries_how_many_in_a_row_because_one_is_not_news() {
        let state = State {
            failures: 4,
            last: Some(Pass {
                when: "an hour ago".into(),
                sent: 0,
                landed: 0,
                removed: 0,
                unreadable: 0,
                failed: Some("could not reach nas:8084".into()),
            }),
            ..sharing()
        };
        let said = describe(&state);
        assert!(said.contains("failed 4 times in a row"), "{said}");
        assert!(said.contains("could not reach"), "{said}");
    }

    #[test]
    fn with_no_server_it_says_sharing_is_off_rather_than_never() {
        assert_eq!(describe(&State::default()), "Sharing is off.");
    }
}
