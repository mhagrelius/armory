//! Everything known about one mount, pet or toy.
//!
//! The collections list answers "what is missing". This answers "what is it and
//! how do I get it", which is a different question and needs the room.
//!
//! Most of what is here exists only because the addon read it out of the game's
//! own journals. The web API gives an id, a name and — for mounts alone — one
//! word. It has no flavour text, no zone, no NPC, no faction restriction and no
//! icon. So a dialog built on the API would be a title and a link, and this one
//! is not.
//!
//! The picture is Blizzard's own render, addressed by the creature display id
//! the journal recorded. An earlier version of this file said there was no URL
//! for the art, on the strength of the icon being a FileDataID inside the
//! client's archives — true of the icon, and wrong about the model, which
//! `render.worldofwarcraft.com` has been serving all along.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::images::{Art, Images};
use crate::model::source::blizzard::collections::{Collectible, Kind, Source};

/// How big the render is at the top of the dialog.
///
/// The service serves 600 square. This is the largest that leaves room for the
/// provenance rows without the dialog needing a scroll to reach them.
const ART: i32 = 220;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CollectibleDialog;

    #[glib::object_subclass]
    impl ObjectSubclass for CollectibleDialog {
        const NAME: &'static str = "ArmoryCollectibleDialog";
        type Type = super::CollectibleDialog;
        type ParentType = adw::Dialog;
    }

    impl ObjectImpl for CollectibleDialog {}
    impl WidgetImpl for CollectibleDialog {}
    impl AdwDialogImpl for CollectibleDialog {}
}

glib::wrapper! {
    pub struct CollectibleDialog(ObjectSubclass<imp::CollectibleDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CollectibleDialog {
    /// Build a dialog for one entry.
    pub fn new(
        entry: &Collectible,
        owned: bool,
        images: Option<&Images>,
        art: Option<&str>,
    ) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.set_title(&entry.name);
        dialog.set_content_width(520);
        dialog.set_content_height(700);
        dialog.build(entry, owned, images, art);
        dialog
    }

    fn build(&self, entry: &Collectible, owned: bool, images: Option<&Images>, art: Option<&str>) {
        let content = Self::content(entry, owned, images, art);
        let view = adw::ToolbarView::builder().content(&content).build();
        view.add_top_bar(&adw::HeaderBar::new());
        self.set_child(Some(&view));
    }

    /// The dialog's body, on its own.
    ///
    /// Separate from the dialog so it can be laid out and painted without one.
    /// A dialog that has never been presented has no surface, so its child
    /// measures to nothing — which is how the first version of this rendered a
    /// blank page in the preview and would have been shipped unlooked-at.
    pub fn content(
        entry: &Collectible,
        owned: bool,
        images: Option<&Images>,
        art: Option<&str>,
    ) -> gtk::Widget {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(6)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();

        column.append(&Self::heading(entry, owned, images, art));
        column.append(&Self::provenance(entry));
        if let Some(group) = Self::flavour(entry) {
            column.append(&group);
        }
        column.append(&Self::links(entry));
        column.append(&Self::identifiers(entry));

        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build()
            .upcast()
    }

    /// The render, the name, and whether it is already had.
    fn heading(
        entry: &Collectible,
        owned: bool,
        images: Option<&Images>,
        art: Option<&str>,
    ) -> gtk::Widget {
        let box_ = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .build();

        // Bigger than the grid's thumbnail and fetched separately: the same URL
        // at two sizes is two textures, and a dialog that reused the ninety-six
        // pixel one would be showing a blurred thumbnail blown up.
        if let (Some(images), Some(url)) = (images, art) {
            let picture = Art::new(ART, placeholder_for(entry.kind));
            picture.set_halign(gtk::Align::Center);
            picture.add_css_class("art-large");
            picture.show(images, Some(url), ART);
            box_.append(&picture);
        }

        let title = gtk::Label::builder()
            .label(&entry.name)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        title.add_css_class("title-1");
        box_.append(&title);

        let status = gtk::Label::new(Some(if owned {
            "Collected"
        } else if entry.source == Source::Promotion {
            "Not collected — and no longer obtainable"
        } else {
            "Not collected"
        }));
        status.add_css_class("dimmed");
        box_.append(&status);

        // A faction lock is the difference between "you are missing this" and
        // "this was never yours to have", and only the game says so.
        if let Some(faction) = entry.faction {
            let label = gtk::Label::new(Some(&format!("{} only", faction.label())));
            label.add_css_class("dimmed");
            label.add_css_class("caption");
            box_.append(&label);
        }

        box_.upcast()
    }

    /// Where it comes from, as the game's own journal words it.
    fn provenance(entry: &Collectible) -> adw::PreferencesGroup {
        let text = entry.description.as_deref().filter(|text| !text.is_empty());

        let group = adw::PreferencesGroup::builder()
            .title("Where it comes from")
            .build();

        match text {
            Some(text) => {
                // The journal writes this as several labelled lines — "Drop:
                // Lord Aurius Rivendare", "Location: Stratholme" — so each
                // becomes its own row rather than one paragraph nobody reads.
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    let row = match line.split_once(':') {
                        Some((label, value)) if !value.trim().is_empty() => {
                            adw::ActionRow::builder()
                                .title(label.trim())
                                .subtitle(value.trim())
                                .build()
                        }
                        _ => adw::ActionRow::builder().title(line.trim()).build(),
                    };
                    row.add_css_class("property");
                    group.add(&row);
                }
            }
            None => {
                let row = adw::ActionRow::builder()
                    .title("Not recorded")
                    .subtitle(match entry.kind {
                        // Worth saying which, because "unknown" for a toy is a
                        // gap in the game's data rather than in ours.
                        Kind::Toy => {
                            "The toy box does not record where a toy came from. \
                             The Wowhead link below does."
                        }
                        Kind::Decor => {
                            "Decor comes from quests, achievements, reputations, \
                             professions and boss drops, and the catalogue does \
                             not always say which."
                        }
                        _ => {
                            "The journal has no source text for this one. The \
                             Wowhead link below usually does."
                        }
                    })
                    .build();
                row.add_css_class("property");
                group.add(&row);
            }
        }

        group
    }

    /// What it says about itself.
    fn flavour(entry: &Collectible) -> Option<adw::PreferencesGroup> {
        let text = entry.flavour.as_deref().filter(|text| !text.is_empty())?;

        let group = adw::PreferencesGroup::builder()
            .title("Description")
            .build();
        let label = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        label.add_css_class("body");
        group.add(&label);
        Some(group)
    }

    /// Somewhere to go for the pictures and the numbers.
    fn links(entry: &Collectible) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Read more")
            .description(
                "Armory fetches nothing from these — Wowhead's terms forbid automated \
                 access — but they are where the model previews, drop rates and \
                 comments live.",
            )
            .build();

        // Wowhead is offered only where the id it would be looked up by is
        // known. A toy and a piece of decor are addressed by the item they
        // wrap, and until a detail call supplies one there is nothing to link
        // — sending somebody to a real, unrelated item is worse than sending
        // them nowhere, because the wrong page reads as the right one.
        let mut links = Vec::new();
        if let Some(url) = entry.wowhead_url() {
            links.push(("Wowhead", "Drop rates, comments, and a 3D preview", url));
        }
        links.push((
            "Warcraft Wiki",
            "Community documentation and history",
            entry.wiki_url(),
        ));

        for (title, subtitle, url) in links {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));

            row.connect_activated(move |row| {
                let launcher = gtk::UriLauncher::new(&url);
                launcher.launch(
                    row.root().and_downcast_ref::<gtk::Window>(),
                    gtk::gio::Cancellable::NONE,
                    |_| {},
                );
            });
            group.add(&row);
        }

        group
    }

    /// The numbers, for anyone who wants to look something up themselves.
    fn identifiers(entry: &Collectible) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Identifiers")
            .description(
                "The model id is what addresses the picture above on Blizzard's render \
                 service. The icon id addresses a file inside the game client's own \
                 archives, which has no URL at all.",
            )
            .build();

        let mut rows = vec![
            (
                match entry.kind {
                    Kind::Mount => "Mount ID",
                    Kind::Pet => "Species ID",
                    Kind::Toy => "Toy ID",
                    Kind::Decor => "Decor ID",
                },
                entry.id.to_string(),
            ),
            (
                match entry.kind {
                    Kind::Mount => "Spell ID",
                    Kind::Pet => "Creature ID",
                    Kind::Toy | Kind::Decor => "Item ID",
                },
                entry.link_id.to_string(),
            ),
        ];
        if let Some(icon) = entry.icon {
            rows.push(("Icon file", icon.to_string()));
        }
        if let Some(display) = entry.display {
            rows.push(("Model", display.to_string()));
        }

        for (title, value) in rows {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(value)
                .subtitle_selectable(true)
                .build();
            row.add_css_class("property");
            group.add(&row);
        }

        group
    }
}

/// What stands in for a picture that is not there.
fn placeholder_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Mount => "starred-symbolic",
        Kind::Pet => "emblem-favorite-symbolic",
        Kind::Toy => "applications-games-symbolic",
        Kind::Decor => "user-home-symbolic",
    }
}

/// Present a dialog for one entry.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    entry: &Collectible,
    owned: bool,
    images: Option<&Images>,
    art: Option<&str>,
) {
    CollectibleDialog::new(entry, owned, images, art).present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::character::Faction;

    fn entry() -> Collectible {
        Collectible {
            kind: Kind::Mount,
            id: 337,
            name: "Rivendare's Deathcharger".into(),
            source: Source::Drop,
            description: Some("Drop: Lord Aurius Rivendare\nLocation: Stratholme".into()),
            flavour: Some("A skeletal steed.".into()),
            icon: Some(132250),
            display: Some(10995),
            faction: None,
            link_id: 17481,
            tradeable: None,
        }
    }

    #[test]
    fn a_mount_links_by_its_spell() {
        assert_eq!(
            entry().wowhead_url().as_deref(),
            Some("https://www.wowhead.com/spell=17481")
        );
    }

    #[test]
    fn the_wiki_link_searches_by_name_with_the_apostrophe_encoded() {
        // A raw apostrophe in a query string is a link that lands nowhere.
        let url = entry().wiki_url();
        assert!(url.contains("Rivendare%27s%20Deathcharger"), "{url}");
    }

    #[test]
    fn a_faction_locked_mount_is_not_missing_from_the_other_factions_collection() {
        // Counting it as missing overstates the backlog by a few hundred, and
        // only the game knows about the restriction at all.
        let mut horde_only = entry();
        horde_only.faction = Some(Faction::Horde);

        assert!(horde_only.obtainable_by(Faction::Horde));
        assert!(!horde_only.obtainable_by(Faction::Alliance));
        // No restriction means anyone.
        assert!(entry().obtainable_by(Faction::Alliance));
    }

    #[test]
    fn a_promotional_mount_is_obtainable_by_nobody() {
        let mut promotion = entry();
        promotion.source = Source::Promotion;
        assert!(!promotion.obtainable_by(Faction::Horde));
    }
}
