//! Choosing what the Market page fetches.
//!
//! Two lists behind the same idea: everything here is opt-in, so both of these
//! exist to make the opting-in possible from the application rather than from
//! the database. Before them, `Store::watch_item` was the only way to put a row
//! on that page and it had no caller.
//!
//! Neither dialog fetches anything itself — a dialog under `ui/` that opened a
//! socket would put a second one in the tree — so both take the list they show
//! and hand back what was chosen.

use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use crate::model::source::blizzard::auctions::Realm;

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    pub struct WatchDialog {
        pub list: RefCell<Option<gtk::ListBox>>,
        pub entry: RefCell<Option<gtk::SearchEntry>>,
        pub status: RefCell<Option<adw::StatusPage>>,
        pub body: RefCell<Option<gtk::Stack>>,
        /// What to do with a chosen item. Held here rather than captured by the
        /// rows, because the rows are rebuilt on every search and the caller is
        /// given once.
        #[allow(clippy::type_complexity)]
        pub on_item: RefCell<Option<Rc<dyn Fn(u32, String)>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WatchDialog {
        const NAME: &'static str = "ArmoryWatchDialog";
        type Type = super::WatchDialog;
        type ParentType = adw::Dialog;
    }

    impl ObjectImpl for WatchDialog {}
    impl WidgetImpl for WatchDialog {}
    impl AdwDialogImpl for WatchDialog {}
}

glib::wrapper! {
    pub struct WatchDialog(ObjectSubclass<imp::WatchDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl WatchDialog {
    fn shell(title: &str, placeholder: &str, empty: adw::StatusPage) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.set_title(title);
        dialog.set_content_width(460);
        dialog.set_content_height(560);

        let entry = gtk::SearchEntry::builder()
            .placeholder_text(placeholder)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .margin_top(6)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        list.add_css_class("boxed-list");

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let spinner = adw::Spinner::builder()
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        let body = gtk::Stack::new();
        body.add_named(&scroller, Some("list"));
        body.add_named(&empty, Some("empty"));
        body.add_named(&spinner, Some("busy"));
        body.set_visible_child_name("empty");

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        column.append(&entry);
        column.append(&body);

        let view = adw::ToolbarView::builder().content(&column).build();
        view.add_top_bar(&adw::HeaderBar::new());
        dialog.set_child(Some(&view));

        let imp = dialog.imp();
        *imp.list.borrow_mut() = Some(list);
        *imp.entry.borrow_mut() = Some(entry);
        *imp.status.borrow_mut() = Some(empty);
        *imp.body.borrow_mut() = Some(body);
        dialog
    }

    /// Pick a realm to fetch auctions from.
    ///
    /// `realms` is every realm in the region — one call, cached, and mostly
    /// unchanging. `suggested` are the slugs this account has characters on,
    /// which go to the top: with thirty-one characters across nine realms, the
    /// realm somebody wants is nearly always one of theirs.
    pub fn realms<F: Fn(Realm) + 'static>(
        realms: &[Realm],
        suggested: &[String],
        chosen: F,
    ) -> Self {
        let empty = adw::StatusPage::builder()
            .icon_name("network-server-symbolic")
            .title("No realms yet")
            .description(
                "Armory has not fetched the realm list. Sync once — it is a single \
                 call and the answer only changes when Blizzard opens or merges a \
                 realm.",
            )
            .build();

        let dialog = Self::shell("Add a Realm", "Search realms", empty);
        let chosen = Rc::new(chosen);

        // The account's own realms first, then the rest. Both alphabetical
        // within their half.
        let mut ordered: Vec<(bool, &Realm)> = realms
            .iter()
            .map(|realm| (!suggested.contains(&realm.slug), realm))
            .collect();
        ordered.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));

        let rows: Vec<(String, adw::ActionRow)> = ordered
            .into_iter()
            .map(|(other, realm)| {
                let row = adw::ActionRow::builder()
                    .title(&realm.name)
                    .subtitle(if other {
                        String::new()
                    } else {
                        "You have a character here".to_string()
                    })
                    .activatable(true)
                    .build();
                row.add_suffix(&gtk::Image::from_icon_name("list-add-symbolic"));

                let haystack = realm.name.to_lowercase();
                let realm = realm.clone();
                let chosen = Rc::clone(&chosen);
                let dialog = dialog.clone();
                row.connect_activated(move |_| {
                    chosen(realm.clone());
                    dialog.close();
                });

                (haystack, row)
            })
            .collect();

        // A held list, so the search box filters it rather than asking
        // Blizzard anything.
        dialog.fill(rows, true);
        dialog
    }

    /// Search for an item to watch.
    ///
    /// The rows arrive from the application as a search lands, rather than
    /// being filtered from a list held here: there are two hundred thousand
    /// items and no endpoint that lists them.
    pub fn items<S: Fn(String) + 'static, F: Fn(u32, String) + 'static>(
        search: S,
        chosen: F,
    ) -> Self {
        let empty = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("Search for an item")
            .description(
                "Type a name to look it up in Blizzard's catalogue. Commodities are \
                 priced region-wide; anything else is priced on the realms you have \
                 added.",
            )
            .build();

        let dialog = Self::shell("Watch an Item", "Search items by name", empty);
        *dialog.imp().on_item.borrow_mut() = Some(Rc::new(chosen));

        if let Some(entry) = dialog.imp().entry.borrow().as_ref() {
            let dialog_for_search = dialog.clone();
            // On activate rather than on every keystroke: this is a request to
            // Blizzard, and one per character typed would be a dozen searches
            // for one word.
            entry.connect_activate(move |entry| {
                let text = entry.text().trim().to_string();
                if text.len() < 2 {
                    return;
                }
                dialog_for_search.set_busy();
                search(text);
            });
        }
        dialog
    }

    /// Show what a search came back with.
    pub fn set_items(&self, items: &[(u32, String)]) {
        let chosen = self.imp().on_item.borrow().clone();

        let rows: Vec<(String, adw::ActionRow)> = items
            .iter()
            .map(|(id, name)| {
                let row = adw::ActionRow::builder()
                    .title(name)
                    .subtitle(format!("Item {id}"))
                    .activatable(true)
                    .build();
                row.add_suffix(&gtk::Image::from_icon_name("list-add-symbolic"));

                if let Some(chosen) = chosen.clone() {
                    let dialog = self.clone();
                    let id = *id;
                    let name = name.clone();
                    row.connect_activated(move |_| {
                        chosen(id, name.clone());
                        dialog.close();
                    });
                }
                (name.to_lowercase(), row)
            })
            .collect();

        if rows.is_empty() {
            if let Some(status) = self.imp().status.borrow().as_ref() {
                status.set_title("Nothing found");
                status.set_description(Some(
                    "No item in Blizzard's catalogue matches that. Names are matched \
                     from the start, so a fragment from the middle will not find one.",
                ));
            }
        }
        // Not filterable: Blizzard already matched the name, and filtering the
        // answer again on the way in would hide rows the search returned on
        // purpose.
        self.fill(rows, false);
    }

    /// Put rows into the list, and optionally let the box above filter them.
    fn fill(&self, rows: Vec<(String, adw::ActionRow)>, filterable: bool) {
        let imp = self.imp();
        let Some(list) = imp.list.borrow().clone() else {
            return;
        };
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        let empty = rows.is_empty();
        for (_, row) in &rows {
            list.append(row);
        }

        if let Some(body) = imp.body.borrow().as_ref() {
            body.set_visible_child_name(if empty { "empty" } else { "list" });
        }

        if !filterable {
            return;
        }
        if let Some(entry) = imp.entry.borrow().as_ref() {
            let held = rows;
            entry.connect_search_changed(move |entry| {
                let needle = entry.text().trim().to_lowercase();
                for (haystack, row) in &held {
                    row.set_visible(needle.is_empty() || haystack.contains(&needle));
                }
            });
        }
    }

    fn set_busy(&self) {
        if let Some(body) = self.imp().body.borrow().as_ref() {
            body.set_visible_child_name("busy");
        }
    }
}
