//! Everything known about one goal.
//!
//! The run page answers "what should I do next". This answers "why is this on
//! the list at all", which for a soft reset is the question people actually
//! have — an achievement your account finished in 2016 appearing in a backlog
//! needs to explain itself.
//!
//! So the standing is stated in words rather than left implicit: who earned it,
//! when, and why that means the completion flag is no use. That reasoning is
//! the whole application, and burying it in a struct nobody sees would be
//! hiding the one thing that makes this different from every other tracker.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use crate::model::run::{Bucket, Exclusion, Goal, Standing};
use crate::model::source::blizzard::gamedata::Achievement;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AchievementDialog;

    #[glib::object_subclass]
    impl ObjectSubclass for AchievementDialog {
        const NAME: &'static str = "ArmoryAchievementDialog";
        type Type = super::AchievementDialog;
        type ParentType = adw::Dialog;
    }

    impl ObjectImpl for AchievementDialog {}
    impl WidgetImpl for AchievementDialog {}
    impl AdwDialogImpl for AchievementDialog {}
}

glib::wrapper! {
    pub struct AchievementDialog(ObjectSubclass<imp::AchievementDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AchievementDialog {
    pub fn new(goal: &Goal, achievement: Option<&Achievement>) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.set_title(
            achievement
                .map(|a| a.name.as_str())
                .unwrap_or("Achievement"),
        );
        dialog.set_content_width(520);
        dialog.set_content_height(640);

        let view = adw::ToolbarView::builder()
            .content(&Self::content(goal, achievement))
            .build();
        view.add_top_bar(&adw::HeaderBar::new());
        dialog.set_child(Some(&view));
        dialog
    }

    /// The dialog's body, on its own.
    ///
    /// Separate from the dialog so it can be laid out and painted without one —
    /// a dialog that has never been presented has no surface, so its child
    /// measures to nothing.
    pub fn content(goal: &Goal, achievement: Option<&Achievement>) -> gtk::Widget {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(6)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();

        column.append(&Self::heading(goal, achievement));
        column.append(&Self::standing(goal));
        column.append(&Self::tracking(goal));
        column.append(&Self::links(goal, achievement));

        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build()
            .upcast()
    }

    fn heading(goal: &Goal, achievement: Option<&Achievement>) -> gtk::Widget {
        let box_ = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .build();

        let title = gtk::Label::builder()
            .label(
                achievement
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| format!("Achievement {}", goal.achievement_id)),
            )
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        title.add_css_class("title-1");
        box_.append(&title);

        if let Some(achievement) = achievement {
            let mut parts = Vec::new();
            if !achievement.category.is_empty() {
                parts.push(achievement.category.clone());
            }
            if achievement.points > 0 {
                parts.push(format!("{} points", achievement.points));
            }
            if !parts.is_empty() {
                let label = gtk::Label::new(Some(&parts.join("  ·  ")));
                label.add_css_class("dimmed");
                box_.append(&label);
            }

            if !achievement.description.is_empty() {
                let label = gtk::Label::builder()
                    .label(&achievement.description)
                    .wrap(true)
                    .justify(gtk::Justification::Center)
                    .margin_top(6)
                    .build();
                label.add_css_class("body");
                box_.append(&label);
            }
        }

        box_.upcast()
    }

    /// Why this is on the list, in words.
    fn standing(goal: &Goal) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Where this stands")
            .build();

        let (title, detail) = match &goal.standing {
            Standing::Unearned => (
                "Nobody on the account has this",
                "So the game's own completion flag works normally. When an \
                 enrolled character earns it, it lights up and Armory reads it \
                 like any other tracker would."
                    .to_string(),
            ),
            Standing::EarnedDuringRun { at } => (
                "Earned during this run",
                format!(
                    "Finished on {}. Anything completed after the baseline \
                     belongs to the run — nobody else is playing this account.",
                    at.format("%-d %B %Y")
                ),
            ),
            Standing::EarnedByCohort { by } => (
                "An enrolled character earned this",
                format!(
                    "{name} has it, and {name} is in this run. Nothing further \
                     needs computing.",
                    name = by.display_name()
                ),
            ),
            Standing::Poisoned { by: Some(by) } => (
                "Earned before the run, by someone outside it",
                format!(
                    "{} earned this before the baseline was taken. The \
                     completion flag was set then and will never move again — a \
                     second character finishing the same content produces no \
                     signal at all. So Armory ignores the flag and works from \
                     each enrolled character's own data instead.",
                    by.display_name()
                ),
            ),
            Standing::Poisoned { by: None } => (
                "Earned before the run, by an unknown character",
                "The account had this before the baseline and nothing records \
                 who earned it. Logging in on more of your characters fills \
                 this in: the game tells the collector addon which achievements \
                 the character you are playing earned, so each login attributes \
                 a few hundred more."
                    .to_string(),
            ),
        };

        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(detail)
            .build();
        row.set_subtitle_lines(0);
        group.add(&row);
        group
    }

    /// How it is being measured, and what that costs.
    fn tracking(goal: &Goal) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("How it is tracked")
            .build();

        if !goal.standing.is_poisoned() {
            let row = adw::ActionRow::builder()
                .title("By the game's own flag")
                .subtitle("Nothing here needs recomputing.")
                .build();
            row.set_subtitle_lines(0);
            group.add(&row);
            return group;
        }

        let (title, detail) = match &goal.bucket {
            Bucket::Observable => {
                let progress = goal
                    .evaluation
                    .as_ref()
                    .map(|e| format!("{} of {} so far. ", e.progress, e.required))
                    .unwrap_or_default();
                (
                    "Measured from your characters' own data",
                    format!(
                        "{progress}Every one of this achievement's criteria maps \
                         to something recorded per character — quests completed, \
                         encounters cleared — so progress is computed rather than \
                         guessed."
                    ),
                )
            }
            Bucket::Attestable => (
                "Only you can say",
                "At least one of this achievement's criteria is something WoW \
                 records account-wide only — a creature killed, an area explored, \
                 a spell cast. There is no per-character record to measure \
                 against, so rather than draw a progress bar over a number that \
                 means something else, Armory asks you."
                    .to_string(),
            ),
            Bucket::Excluded(why) => (
                "Left out of this run",
                match why {
                    Exclusion::AlreadyOwned => {
                        "The account already has what this awards, and it cannot \
                         be collected twice."
                    }
                    Exclusion::Unrepeatable => {
                        "A Feat of Strength or legacy achievement. Nobody can earn \
                         this again, so leaving it in the backlog would leave a row \
                         that can only ever read zero."
                    }
                    Exclusion::Unmeasurable => {
                        "Nothing measures it and nobody could honestly attest to it."
                    }
                    Exclusion::ByHand => "You took this out of the run.",
                }
                .to_string(),
            ),
        };

        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(detail)
            .build();
        row.set_subtitle_lines(0);
        group.add(&row);

        if let Some(evaluation) = &goal.evaluation {
            if evaluation.inherited {
                let row = adw::ActionRow::builder()
                    .title("Some of this was inherited")
                    .subtitle(
                        "Part of the progress comes from a reputation The War Within \
                         made account-wide, which an unenrolled character may well \
                         have earned. It is shown but never counted.",
                    )
                    .build();
                row.set_subtitle_lines(0);
                row.add_css_class("inherited");
                group.add(&row);
            }
        }

        group
    }

    fn links(goal: &Goal, achievement: Option<&Achievement>) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder()
            .title("Read more")
            .description(
                "Armory fetches nothing from these — Wowhead's terms forbid automated \
                 access — but they are where the criteria, comments and guides live.",
            )
            .build();

        let id = goal.achievement_id;
        for (title, subtitle, url) in [
            (
                "Wowhead",
                "Criteria, comments and guides".to_string(),
                format!("https://www.wowhead.com/achievement={id}"),
            ),
            (
                "Warcraft Wiki",
                "Community documentation".to_string(),
                match achievement {
                    Some(achievement) => format!(
                        "https://warcraft.wiki.gg/wiki/Special:Search?search={}",
                        crate::model::source::blizzard::encode(&achievement.name)
                    ),
                    None => format!("https://warcraft.wiki.gg/wiki/Special:Search?search={id}"),
                },
            ),
        ] {
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
}

/// Present a dialog for one goal.
pub fn present(parent: &impl IsA<gtk::Widget>, goal: &Goal, achievement: Option<&Achievement>) {
    AchievementDialog::new(goal, achievement).present(Some(parent));
}
