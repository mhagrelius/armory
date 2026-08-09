//! The Almanac: the vocabulary every page is drawn in.
//!
//! Three rules carry the whole thing, and they are worth stating once here
//! rather than being rediscovered per page:
//!
//! 1. **Every page is a main column and a right rail.** The standing, the
//!    filters, the caveats and the legends live in the rail; the main column is
//!    only ever the thing itself.
//! 2. **Gold means "you earned this."** One accent, spent only on work the run
//!    can claim. An inherited standing, an account-wide flag and an unmeasurable
//!    value are never gold, and that restraint is the reason the colour reads.
//! 3. **Numbers are monospaced, narrative is serif.** Counts, prices, durations
//!    and section labels in the mono face; the things written *about* the player
//!    in the serif; everything else the platform font.
//!
//! ## Where the colours live
//!
//! In Rust, in [`Palette`], and the stylesheet's `:root` block is *generated
//! from it* at load time. The alternative was the palette written twice — once
//! in CSS for the widgets and once in Rust for the Cairo draw functions — and
//! two copies of twenty-three colours is two copies that drift. A drawn widget
//! asks [`Palette::current`]; a styled widget reads `var(--al-gold)`; both are
//! the same literal.
//!
//! This is also why the scheme is swapped by replacing a provider rather than
//! by a `.dark` selector: libadwaita gives an application no CSS hook for the
//! colour scheme, so [`super::load_stylesheet`] watches `AdwStyleManager::dark`
//! and reloads the generated half.
//!
//! ## Fonts
//!
//! EB Garamond and IBM Plex Mono are the design's faces and both are OFL, so
//! they can be bundled. They are *named first* in every stack here and followed
//! by the system serif and the system monospace, which is what renders when they
//! are not installed. Nothing needs to change in this file if they later are.

use adw::prelude::*;
use gtk::glib;

/// A colour, as the four components Cairo and CSS both want.
///
/// Not `gdk::RGBA`, which has no const constructor — and a palette that cannot
/// be a `const` is a palette that gets built on every draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ink(pub f64, pub f64, pub f64, pub f64);

impl Ink {
    /// The same colour at a different opacity.
    ///
    /// The design gives several tokens as one hue at four alphas; writing the
    /// hue once and the alphas where they are used keeps them related.
    pub const fn at(self, alpha: f64) -> Self {
        Ink(self.0, self.1, self.2, alpha)
    }

    fn css(self) -> String {
        format!(
            "rgba({}, {}, {}, {:.3})",
            (self.0 * 255.0).round(),
            (self.1 * 255.0).round(),
            (self.2 * 255.0).round(),
            self.3
        )
    }

    /// Set this as a Cairo source.
    pub fn apply(self, context: &gtk::cairo::Context) {
        context.set_source_rgba(self.0, self.1, self.2, self.3);
    }
}

/// A hex literal, at full opacity. `const` so the palette can be one.
const fn hex(value: u32) -> Ink {
    Ink(
        ((value >> 16) & 0xff) as f64 / 255.0,
        ((value >> 8) & 0xff) as f64 / 255.0,
        (value & 0xff) as f64 / 255.0,
        1.0,
    )
}

/// White and black at an alpha, which is how most of the dark palette is given.
const fn white(alpha: f64) -> Ink {
    Ink(1.0, 1.0, 1.0, alpha)
}

const fn black(alpha: f64) -> Ink {
    Ink(0.0, 0.0, 0.0, alpha)
}

/// One scheme's worth of the design's tokens.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub ground: Ink,
    pub headerbar: Ink,
    pub sidebar: Ink,
    pub rail: Ink,
    pub card: Ink,
    pub card_alt: Ink,
    pub card_hover: Ink,
    pub hairline: Ink,
    pub card_border: Ink,
    pub text: Ink,
    /// The one accent. Earned progress, live figures, the active segment.
    pub gold: Ink,
    /// Gold as *text*, which needs more contrast than gold as a fill.
    pub gold_text: Ink,
    pub gold_tint: Ink,
    pub gold_tint_strong: Ink,
    pub gold_border: Ink,
    pub gold_border_strong: Ink,
    pub gold_halo: Ink,
    /// Gold held back — a momentum bar that is not tonight, a depth segment
    /// that is not the floor.
    pub gold_soft: Ink,
    pub positive: Ink,
    pub negative: Ink,
    pub positive_tint: Ink,
    pub negative_tint: Ink,
    /// Negative as *text*, which like gold needs lifting off its own tint.
    pub negative_text: Ink,
    /// The wash over a piece of key art, so a title can sit on it. Strong at
    /// the bottom where the words are, nearly clear at the top where the
    /// picture is.
    pub scrim_strong: Ink,
    pub scrim_weak: Ink,
    /// The unfilled part of any bar or ring.
    pub track: Ink,
    /// The account's standing behind a character's own work. Never gold: it is
    /// not this run's.
    pub pale: Ink,
    /// An older marker on a spine.
    pub dot: Ink,
    /// A day with no session, drawn as an absence rather than as a zero.
    pub absent: Ink,
}

impl Palette {
    pub const DARK: Palette = Palette {
        ground: hex(0x17161a),
        headerbar: hex(0x232227),
        sidebar: hex(0x232326),
        rail: hex(0x1c1b20),
        card: white(0.05),
        card_alt: white(0.03),
        card_hover: white(0.08),
        hairline: white(0.08),
        card_border: white(0.07),
        text: white(0.90),
        gold: hex(0xe0b34a),
        gold_text: hex(0xe6c273),
        gold_tint: Ink(0.878, 0.702, 0.290, 0.07),
        gold_tint_strong: Ink(0.878, 0.702, 0.290, 0.20),
        gold_border: Ink(0.878, 0.702, 0.290, 0.28),
        gold_border_strong: Ink(0.878, 0.702, 0.290, 0.45),
        gold_halo: Ink(0.878, 0.702, 0.290, 0.18),
        gold_soft: Ink(0.878, 0.702, 0.290, 0.30),
        positive: hex(0x7fa86b),
        negative: hex(0xe07c56),
        positive_tint: Ink(0.498, 0.659, 0.420, 0.16),
        negative_tint: Ink(0.878, 0.486, 0.337, 0.16),
        negative_text: hex(0xeda183),
        scrim_strong: Ink(0.090, 0.086, 0.102, 0.97),
        scrim_weak: Ink(0.090, 0.086, 0.102, 0.15),
        track: white(0.09),
        pale: white(0.16),
        dot: white(0.35),
        absent: white(0.16),
    };

    /// The same layout and the same type. Gold is taken down to ink weight so
    /// it holds contrast on paper-white — a `#e0b34a` fill on `#faf8f4` is
    /// legible as a colour and not as a number.
    pub const LIGHT: Palette = Palette {
        ground: hex(0xfaf8f4),
        headerbar: hex(0xf4f2ed),
        sidebar: hex(0xf4f2ed),
        rail: hex(0xf2f0ea),
        card: hex(0xffffff),
        card_alt: hex(0xfbfaf7),
        card_hover: hex(0xfafafa),
        hairline: black(0.09),
        card_border: black(0.08),
        text: black(0.85),
        gold: hex(0xa97f18),
        gold_text: hex(0x8a6712),
        gold_tint: Ink(0.663, 0.498, 0.094, 0.14),
        gold_tint_strong: Ink(0.663, 0.498, 0.094, 0.22),
        gold_border: Ink(0.663, 0.498, 0.094, 0.30),
        gold_border_strong: Ink(0.663, 0.498, 0.094, 0.50),
        gold_halo: Ink(0.663, 0.498, 0.094, 0.20),
        gold_soft: Ink(0.663, 0.498, 0.094, 0.35),
        positive: hex(0x3a8a52),
        negative: hex(0xc45630),
        positive_tint: Ink(0.227, 0.541, 0.322, 0.14),
        negative_tint: Ink(0.769, 0.337, 0.188, 0.14),
        negative_text: hex(0x9d3f1e),
        scrim_strong: Ink(0.980, 0.973, 0.957, 0.97),
        scrim_weak: Ink(0.980, 0.973, 0.957, 0.15),
        track: black(0.10),
        pale: black(0.16),
        dot: black(0.35),
        absent: black(0.16),
    };

    /// Whichever scheme is in force right now.
    ///
    /// Read at draw time rather than held, so a widget built under one scheme
    /// and redrawn under another is drawn in the scheme it is being looked at
    /// in.
    pub fn current() -> Palette {
        if adw::StyleManager::default().is_dark() {
            Palette::DARK
        } else {
            Palette::LIGHT
        }
    }

    /// The generated half of the stylesheet: every token as a custom property.
    pub fn css(&self) -> String {
        let tokens = [
            ("ground", self.ground),
            ("headerbar", self.headerbar),
            ("sidebar", self.sidebar),
            ("rail", self.rail),
            ("card", self.card),
            ("card-alt", self.card_alt),
            ("card-hover", self.card_hover),
            ("hairline", self.hairline),
            ("card-border", self.card_border),
            ("text", self.text),
            ("gold", self.gold),
            ("gold-text", self.gold_text),
            ("gold-tint", self.gold_tint),
            ("gold-tint-strong", self.gold_tint_strong),
            ("gold-border", self.gold_border),
            ("gold-border-strong", self.gold_border_strong),
            ("gold-halo", self.gold_halo),
            ("gold-soft", self.gold_soft),
            ("positive", self.positive),
            ("negative", self.negative),
            ("positive-tint", self.positive_tint),
            ("negative-tint", self.negative_tint),
            ("negative-text", self.negative_text),
            ("scrim-strong", self.scrim_strong),
            ("scrim-weak", self.scrim_weak),
            ("track", self.track),
            ("pale", self.pale),
            ("dot", self.dot),
        ];

        let mut css = String::from(":root {\n");
        for (name, ink) in tokens {
            css.push_str(&format!("  --al-{name}: {};\n", ink.css()));
        }
        css.push_str("}\n");
        css
    }
}

// -- type ---------------------------------------------------------------------

/// A label in the platform font, with whatever style classes it needs.
pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .halign(gtk::Align::Start)
        .build();
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// Something written about the player. Serif, and wrapping.
pub fn serif(text: &str, class: &str) -> gtk::Label {
    let label = label(text, &["al-serif", class]);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label
}

/// How much taller than the type a line of narrative is set.
///
/// Enough that fifteen-point serif is prose rather than a wall, and no more.
/// It was 1.7, which is a page of poetry: over a three-paragraph journal entry
/// the leading alone was most of the card's height, and a reader scrolling past
/// an evening's writing to reach the evening's facts is a journal arranged
/// against itself.
const LEADING: f64 = 1.5;

/// What a blank line between two paragraphs is worth.
///
/// A fraction of a line rather than a whole one. The gap is what says the
/// paragraphs are two, so it stays — but at full leading a paragraph break
/// costs more vertical space than the two lines it separates, and an entry with
/// four of them spends half a screen saying nothing four times.
const BREATH: f64 = 0.55;

/// A paragraph of narrative, at the design's line height.
///
/// The line height is a Pango attribute rather than CSS because GTK's CSS has
/// no `line-height` property.
///
/// **The blank lines are set apart from the text.** The body arrives as the
/// model wrote it, paragraphs separated by an empty line, and that empty line
/// is a real line costing real height at whatever leading the prose is set in.
/// So the leading is applied to the writing and [`BREATH`] to the gaps, which
/// is the only way to have generous prose and a tight card at once.
pub fn prose(text: &str) -> gtk::Label {
    let label = serif(text, "al-prose");
    let attributes = gtk::pango::AttrList::new();
    attributes.insert(gtk::pango::AttrFloat::new_line_height(LEADING));

    for (start, end) in blank_lines(text) {
        let mut breath = gtk::pango::AttrFloat::new_line_height(BREATH);
        breath.set_start_index(start);
        breath.set_end_index(end);
        attributes.insert(breath);
    }

    label.set_attributes(Some(&attributes));
    label
}

/// The byte ranges of the lines in `text` with nothing on them to read.
///
/// A range covers the newline that *ends* the empty line, because that
/// character is what the line is made of and what its height is measured from.
/// The last line is never one of these: there is no newline after it, so there
/// is no gap there to take.
fn blank_lines(text: &str) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    let mut offset = 0usize;
    let mut lines = text.split('\n').peekable();

    while let Some(line) = lines.next() {
        let start = offset;
        offset += line.len() + 1;
        // The final line carries no newline, so `offset` has run past the end.
        if lines.peek().is_none() {
            break;
        }
        if line.trim().is_empty() {
            let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(offset)) else {
                continue;
            };
            found.push((start, end));
        }
    }
    found
}

/// A number. Monospaced and tabular, so it does not shift as it changes.
pub fn mono(text: &str, classes: &[&str]) -> gtk::Label {
    let mut all = vec!["al-mono", "tabular"];
    all.extend_from_slice(classes);
    label(text, &all)
}

/// A figure: the number a card exists to say.
pub fn figure(text: &str) -> gtk::Label {
    mono(text, &["al-figure"])
}

/// "WITHIN REACH", "LAST NIGHT", "WHERE THEY COME FROM".
pub fn section(text: &str) -> gtk::Label {
    mono(text, &["al-section"])
}

/// "20:14 — 3H 41M · 12 QUESTS · 1 DEATH".
pub fn meta(text: &str) -> gtk::Label {
    mono(text, &["al-meta"])
}

/// A subtitle, caption or footnote in the platform font.
pub fn caption(text: &str) -> gtk::Label {
    let label = label(text, &["al-caption"]);
    label.set_wrap(true);
    label
}

/// A small rounded word: a route stop, a faction, a boss, a state.
pub fn chip(text: &str, tone: Tone) -> gtk::Label {
    let label = label(text, &["al-chip", tone.class()]);
    label.set_halign(gtk::Align::Start);
    label
}

/// What a chip, a card or a figure is saying about the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The default. Neither good news nor bad, and never gold.
    Plain,
    /// Work the run can claim.
    Gold,
    /// Income, a price rising.
    Positive,
    /// Spending, a death, a hostile faction, a price falling.
    Negative,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Tone::Plain => "al-plain",
            Tone::Gold => "al-gold",
            Tone::Positive => "al-positive",
            Tone::Negative => "al-negative",
        }
    }
}

// -- containers ---------------------------------------------------------------

/// The style class carrying a class's colour.
///
/// The colours themselves are in the stylesheet. They are the game's, not the
/// platform's, which is why they are the one place in this application with a
/// literal colour in it — the same reason a logo is not drawn in the accent
/// colour. They tint a ring around a portrait or a dot beside a name and never
/// any text, so no combination of them and a theme can produce something
/// unreadable.
pub fn class_style(class: &str) -> &'static str {
    match class {
        "Death Knight" => "class-death-knight",
        "Demon Hunter" => "class-demon-hunter",
        "Druid" => "class-druid",
        "Evoker" => "class-evoker",
        "Hunter" => "class-hunter",
        "Mage" => "class-mage",
        "Monk" => "class-monk",
        "Paladin" => "class-paladin",
        "Priest" => "class-priest",
        "Rogue" => "class-rogue",
        "Shaman" => "class-shaman",
        "Warlock" => "class-warlock",
        "Warrior" => "class-warrior",
        // A class Blizzard has not shipped yet, or a locale this build does not
        // speak. No ring beats a wrong one.
        _ => "class-unknown",
    }
}

/// A character's class, where there is no portrait to ring.
pub fn class_dot(class: &str) -> gtk::Box {
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("class-dot");
    dot.add_css_class(class_style(class));
    dot.set_valign(gtk::Align::Center);
    dot.set_size_request(7, 7);
    dot
}

/// A count and a noun that agrees with it.
///
/// "1 quests" in a journal entry reads as a bug in the journal rather than as
/// one quest, which is exactly the wrong thing for a page whose whole claim is
/// that it watched carefully.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// Group a number with thin spaces, so nine-figure gold totals can be read at
/// a glance instead of counted.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    out
}

/// A count, in words.
///
/// "Eleven closed this week" is a sentence and "11 closed this week" is a
/// readout. The headline of the Run page is the one place in the application
/// that is written rather than reported, so it spells its number — up to the
/// point where a word stops being easier to read than a figure.
pub fn spelled(count: usize) -> String {
    const WORDS: [&str; 21] = [
        "Nothing",
        "One",
        "Two",
        "Three",
        "Four",
        "Five",
        "Six",
        "Seven",
        "Eight",
        "Nine",
        "Ten",
        "Eleven",
        "Twelve",
        "Thirteen",
        "Fourteen",
        "Fifteen",
        "Sixteen",
        "Seventeen",
        "Eighteen",
        "Nineteen",
        "Twenty",
    ];
    WORDS
        .get(count)
        .map(|word| (*word).to_string())
        .unwrap_or_else(|| count.to_string())
}

/// A vertical box, which is most of what a page is made of.
pub fn column(spacing: i32) -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(spacing)
        .build()
}

/// A horizontal box.
pub fn row(spacing: i32) -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(spacing)
        .build()
}

/// A panel: the card everything on a page sits in.
pub fn card(spacing: i32) -> gtk::Box {
    let card = column(spacing);
    card.add_css_class("al-card");
    card
}

/// A card that is about work the run can claim, or that is the one being
/// recommended. Gold fill, gold border, and used sparingly enough that it still
/// means something when it appears.
pub fn earned_card(spacing: i32) -> gtk::Box {
    let card = card(spacing);
    card.add_css_class("al-earned");
    card
}

/// A one-pixel divider.
pub fn hairline() -> gtk::Separator {
    let line = gtk::Separator::new(gtk::Orientation::Horizontal);
    line.add_css_class("al-hairline");
    line
}

/// The main column of a page: scrolling, and padded to the design's inset.
pub fn main_column(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .child(child)
        .build();
    scroller.add_css_class("al-main");
    scroller
}

/// The right-hand pane: the page's standing, filters, caveats and legends.
pub fn rail_pane(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build();
    scroller.add_css_class("al-rail");
    scroller
}

/// A rail's contents: a padded column with the page's asides in it.
pub fn rail_column() -> gtk::Box {
    let column = column(16);
    column.add_css_class("al-rail-column");
    column
}

/// The two-pane content area every Almanac page is.
///
/// `AdwOverlaySplitView` rather than a `GtkBox` of two children, because the
/// rail has to fold away on a narrow window and this gives that for free: below
/// the breakpoint it becomes an overlay reached from the header rather than a
/// pane squeezing the thing the page is about.
pub fn split(
    main: &impl IsA<gtk::Widget>,
    rail: &impl IsA<gtk::Widget>,
    width: f64,
) -> adw::OverlaySplitView {
    let split = adw::OverlaySplitView::builder()
        .sidebar_position(gtk::PackType::End)
        .content(main)
        .sidebar(rail)
        // Pinned rather than proportional: the rail carries a price book and a
        // legend at a size somebody read them at, and a fraction of the window
        // makes those reflow every time it is resized.
        .min_sidebar_width(width)
        .max_sidebar_width(width)
        .sidebar_width_fraction(0.4)
        .build();
    RAILS.with(|rails| rails.borrow_mut().push(split.downgrade()));
    // A rail built after the window has already gone narrow — a page redrawing
    // itself, or one opened for the first time — has to arrive folded. Without
    // this it appears expanded on a window that has no room for it, and stays
    // that way until the next resize.
    let collapsed = COLLAPSED.with(std::cell::Cell::get);
    split.set_collapsed(collapsed);
    split.set_show_sidebar(!collapsed);
    split
}

thread_local! {
    /// Every rail on every page, weakly.
    ///
    /// Whether the rail folds away is a fact about the *window* — it is the
    /// same breakpoint for all seven pages, and it has to hold for a page that
    /// has not been looked at yet as much as for the one on screen. The
    /// alternative was a `rail()` accessor on each page and a match in
    /// `ArmoryWindow` listing all of them, which is the same knowledge written
    /// eight times and wrong the first time somebody adds a page.
    ///
    /// Weak references, so a page that is dropped does not keep its rail alive;
    /// [`rails`] sweeps the dead ones as it goes.
    static RAILS: std::cell::RefCell<Vec<glib::WeakRef<adw::OverlaySplitView>>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Whether the window is currently too narrow to keep a rail beside the
    /// content. Held here rather than only on the widgets, so a rail built
    /// after the breakpoint fired can be born in the right state.
    static COLLAPSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Every rail still alive.
pub fn rails() -> Vec<adw::OverlaySplitView> {
    RAILS.with(|rails| {
        let mut rails = rails.borrow_mut();
        rails.retain(|rail| rail.upgrade().is_some());
        rails.iter().filter_map(glib::WeakRef::upgrade).collect()
    })
}

/// Fold every rail away, or bring them all back.
pub fn collapse_rails(collapsed: bool) {
    COLLAPSED.with(|state| state.set(collapsed));
    for rail in rails() {
        rail.set_collapsed(collapsed);
        // Expanded, the pane is simply there. Folded, it starts out of the way
        // — an overlay covering half the thing the page is about is not a
        // sensible resting state for a narrow window.
        rail.set_show_sidebar(!collapsed);
    }
}

/// Whether the folded rails are currently showing.
pub fn rails_showing() -> bool {
    rails().first().is_some_and(|rail| rail.shows_sidebar())
}

/// Show or hide the folded rails.
pub fn show_rails(showing: bool) {
    for rail in rails() {
        rail.set_show_sidebar(showing);
    }
}

// -- drawn widgets ------------------------------------------------------------

/// How long a bar, a ring or a strip takes to say what it is worth.
///
/// Every one of these goes through `AdwTimedAnimation`, which honours
/// `gtk-enable-animations` on its own — with animations off the target is
/// called once with the final value, which is exactly the required behaviour
/// and is why none of this is conditional here.
const EASE: adw::Easing = adw::Easing::EaseOutCubic;

fn animate(widget: &impl IsA<gtk::Widget>, from: f64, to: f64, ms: u32, delay: u32) {
    let area = widget.clone().upcast::<gtk::Widget>();
    let target = adw::CallbackAnimationTarget::new(glib::clone!(
        #[weak]
        area,
        move |value| {
            unsafe { area.set_data("al-value", value) };
            area.queue_draw();
        }
    ));
    let animation = adw::TimedAnimation::builder()
        .widget(&area)
        .value_from(from)
        .value_to(to)
        .duration(ms)
        .easing(EASE)
        .target(&target)
        .build();
    if delay == 0 {
        animation.play();
    } else {
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(u64::from(delay)),
            move || {
                animation.play();
            },
        );
    }
}

/// How far through an animation a drawn widget is, 0 to 1.
fn phase(widget: &gtk::Widget) -> f64 {
    unsafe { widget.data::<f64>("al-value").map(|value| *value.as_ref()) }.unwrap_or(0.0)
}

/// The run's standing, as a ring.
///
/// A `GtkDrawingArea` and a Cairo arc. The figures in the middle are real
/// labels in a `GtkOverlay` rather than drawn text, so they take the mono face
/// and the gold from the stylesheet like every other number on the page.
pub struct Ring {
    pub widget: gtk::Overlay,
    area: gtk::DrawingArea,
    headline: gtk::Label,
    detail: gtk::Label,
    fraction: std::cell::Cell<f64>,
}

impl Ring {
    pub fn new(diameter: i32) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(diameter)
            .content_height(diameter)
            .build();

        area.set_draw_func(|area, context, width, height| {
            let palette = Palette::current();
            let stroke = 9.0;
            let radius = (f64::from(width.min(height)) - stroke) / 2.0;
            let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);

            context.set_line_width(stroke);
            palette.track.apply(context);
            context.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
            let _ = context.stroke();

            let swept = phase(area.upcast_ref());
            if swept <= 0.0 {
                return;
            }
            context.set_line_cap(gtk::cairo::LineCap::Round);
            palette.gold.apply(context);
            let start = -std::f64::consts::FRAC_PI_2;
            context.arc(cx, cy, radius, start, start + swept * std::f64::consts::TAU);
            let _ = context.stroke();
        });

        let headline = mono("", &["al-ring-figure"]);
        headline.set_halign(gtk::Align::Center);
        let detail = mono("", &["al-ring-detail"]);
        detail.set_halign(gtk::Align::Center);

        let centre = column(0);
        centre.set_halign(gtk::Align::Center);
        centre.set_valign(gtk::Align::Center);
        centre.append(&headline);
        centre.append(&detail);

        let widget = gtk::Overlay::builder().child(&area).build();
        widget.add_overlay(&centre);
        widget.set_halign(gtk::Align::Start);
        widget.set_valign(gtk::Align::Center);

        Ring {
            widget,
            area,
            headline,
            detail,
            fraction: std::cell::Cell::new(0.0),
        }
    }

    /// Sweep to a new standing.
    ///
    /// From wherever it already was, not from zero: ticking a goal off re-plans
    /// the run and redraws this, and starting the sweep again each time would
    /// make a one-goal change look like the run beginning.
    pub fn set(&self, fraction: f64, headline: &str, detail: &str) {
        let from = self.fraction.replace(fraction);
        self.headline.set_label(headline);
        self.detail.set_label(detail);
        animate(&self.area, from, fraction.clamp(0.0, 1.0), 1400, 250);
    }
}

/// The last fourteen evenings, as bars.
///
/// A day with no session is not a zero-height bar — it is a dotted rule where a
/// bar would be. Somebody who did not play on Tuesday did not play a very
/// little on Tuesday.
pub struct Momentum {
    pub widget: gtk::DrawingArea,
    days: std::rc::Rc<std::cell::RefCell<Vec<Option<f64>>>>,
}

/// One bar's width, its gap, and the shortest a real evening is drawn.
const BAR: f64 = 15.0;
const GAP: f64 = 5.0;
const FLOOR: f64 = 0.12;
/// Each bar's rise, and how far behind its neighbour it starts.
const RISE_MS: f64 = 500.0;
const STAGGER_MS: f64 = 40.0;

impl Momentum {
    pub fn new() -> Self {
        let days: std::rc::Rc<std::cell::RefCell<Vec<Option<f64>>>> = Default::default();
        let widget = gtk::DrawingArea::builder()
            .content_height(40)
            .content_width(((BAR + GAP) * 14.0 - GAP) as i32)
            .valign(gtk::Align::End)
            .build();

        let held = days.clone();
        widget.set_draw_func(move |area, context, _, height| {
            let palette = Palette::current();
            let days = held.borrow();
            if days.is_empty() {
                return;
            }

            let total = RISE_MS + STAGGER_MS * (days.len().saturating_sub(1)) as f64;
            let elapsed = phase(area.upcast_ref()) * total;
            let bottom = f64::from(height);
            let last = days.iter().rposition(Option::is_some);

            for (index, day) in days.iter().enumerate() {
                let x = (BAR + GAP) * index as f64;
                let Some(value) = day else {
                    // An absence. Two pixels of dotted rule where the evening
                    // would have been.
                    palette.absent.apply(context);
                    context.set_line_width(2.0);
                    context.set_dash(&[2.0, 2.0], 0.0);
                    context.move_to(x, bottom - 1.0);
                    context.line_to(x + BAR, bottom - 1.0);
                    let _ = context.stroke();
                    context.set_dash(&[], 0.0);
                    continue;
                };

                let local = ((elapsed - STAGGER_MS * index as f64) / RISE_MS).clamp(0.0, 1.0);
                let eased = 1.0 - (1.0 - local).powi(3);
                let full = value.clamp(FLOOR, 1.0) * bottom;
                let drawn = full * eased;
                if drawn <= 0.0 {
                    continue;
                }

                if Some(index) == last {
                    palette.gold.apply(context);
                } else {
                    palette.gold_soft.apply(context);
                }
                context.rectangle(x, bottom - drawn, BAR, drawn);
                let _ = context.fill();
            }
        });

        Momentum { widget, days }
    }

    /// Fourteen days, oldest first. `None` is a day with no session.
    pub fn set(&self, days: Vec<Option<f64>>) {
        *self.days.borrow_mut() = days;
        animate(&self.widget, 0.0, 1.0, 900, 0);
    }
}

impl Default for Momentum {
    fn default() -> Self {
        Self::new()
    }
}

/// A bar, with up to two readings on it.
///
/// The second reading is the whole argument of the Reputations page: a pale
/// fill at where the *account* stands, and a gold fill at what the character in
/// front of you actually earned. Only the gold one animates — the pale one is
/// not this run's work and drawing attention to it would say it was.
pub struct Bar {
    pub widget: gtk::DrawingArea,
    state: std::rc::Rc<std::cell::Cell<(f64, f64, Tone)>>,
    fraction: std::cell::Cell<f64>,
}

impl Bar {
    pub fn new(height: i32) -> Self {
        let state = std::rc::Rc::new(std::cell::Cell::new((0.0_f64, 0.0_f64, Tone::Gold)));
        let widget = gtk::DrawingArea::builder()
            .content_height(height)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();

        let held = state.clone();
        widget.set_draw_func(move |area, context, width, height| {
            let palette = Palette::current();
            let (fraction, ghost, tone) = held.get();
            let full = f64::from(width);
            let h = f64::from(height);
            let radius = h / 2.0;

            let rounded = |context: &gtk::cairo::Context, w: f64| {
                if w <= 0.0 {
                    return;
                }
                let w = w.max(h.min(full));
                context.new_sub_path();
                context.arc(
                    radius,
                    radius,
                    radius,
                    std::f64::consts::FRAC_PI_2,
                    -std::f64::consts::FRAC_PI_2,
                );
                context.arc(
                    w - radius,
                    radius,
                    radius,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                );
                context.close_path();
            };

            palette.track.apply(context);
            rounded(context, full);
            let _ = context.fill();

            if ghost > 0.0 {
                palette.pale.apply(context);
                rounded(context, full * ghost.clamp(0.0, 1.0));
                let _ = context.fill();
            }

            let swept = phase(area.upcast_ref()) * fraction;
            if swept > 0.0 {
                match tone {
                    Tone::Gold => palette.gold,
                    Tone::Positive => palette.positive,
                    Tone::Negative => palette.negative,
                    Tone::Plain => palette.pale,
                }
                .apply(context);
                rounded(context, full * swept.clamp(0.0, 1.0));
                let _ = context.fill();
            }
        });

        Bar {
            widget,
            state,
            fraction: std::cell::Cell::new(0.0),
        }
    }

    /// Fill to `fraction`. `delay` staggers a list of them down the page.
    pub fn set(&self, fraction: f64, delay: u32) {
        self.set_full(fraction, 0.0, Tone::Gold, delay);
    }

    /// Fill to `fraction`, with the account's own standing behind it.
    pub fn set_full(&self, fraction: f64, ghost: f64, tone: Tone, delay: u32) {
        let previous = self.fraction.replace(fraction);
        self.state.set((fraction, ghost, tone));
        let from = if previous > 0.0 {
            previous / fraction.max(1e-9)
        } else {
            0.0
        };
        animate(&self.widget, from.clamp(0.0, 1.0), 1.0, 1000, delay);
    }
}

/// A price over the window it has been watched for.
///
/// Fewer than two readings draws nothing at all, and the caller says so in
/// words instead: two readings is not a trend, and a flat line claiming a
/// stable market is a lie a chart tells very convincingly.
pub fn spark(prices: Vec<f64>, width: i32, height: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(width)
        .content_height(height)
        .valign(gtk::Align::Center)
        .build();

    area.set_draw_func(move |_, context, width, height| {
        if prices.len() < 2 {
            return;
        }
        let palette = Palette::current();
        // A series that has fallen over the window it was watched for is drawn
        // held back rather than in a second colour: the line is still the
        // price, and a red line would read as a warning about the item.
        let ink = if prices[prices.len() - 1] < prices[0] {
            palette.gold.at(0.6)
        } else {
            palette.gold
        };

        let low = prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let high = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let flat = high <= low;

        let inset = 4.0;
        let usable = f64::from(height) - inset * 2.0;
        let step = (f64::from(width) - 1.0) / (prices.len() - 1) as f64;
        let at = |index: usize| {
            let x = step * index as f64;
            let y = if flat {
                f64::from(height) / 2.0
            } else {
                inset + (1.0 - (prices[index] - low) / (high - low)) * usable
            };
            (x, y)
        };

        for index in 0..prices.len() {
            let (x, y) = at(index);
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        context.line_to(f64::from(width) - 1.0, f64::from(height));
        context.line_to(0.0, f64::from(height));
        context.close_path();
        ink.at(0.12).apply(context);
        let _ = context.fill();

        for index in 0..prices.len() {
            let (x, y) = at(index);
            if index == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        ink.apply(context);
        context.set_line_width(1.8);
        context.set_line_join(gtk::cairo::LineJoin::Round);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        let _ = context.stroke();
    });

    area
}

/// How big a spine's marker is, whichever kind it is. See [`spine_dot`].
pub const DOT: i32 = 20;

/// A spine down the left of a list of dated things.
///
/// The gradient is the point: the run is brightest at tonight and fades into
/// what came before it.
pub fn spine() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(2)
        .vexpand(true)
        .build();
    area.set_size_request(2, -1);
    area.set_halign(gtk::Align::Center);
    area.set_draw_func(|_, context, width, height| {
        let palette = Palette::current();
        let gradient = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, f64::from(height));
        gradient.add_color_stop_rgba(
            0.0,
            palette.gold.0,
            palette.gold.1,
            palette.gold.2,
            palette.gold.3,
        );
        gradient.add_color_stop_rgba(1.0, palette.gold.0, palette.gold.1, palette.gold.2, 0.12);
        let _ = context.set_source(&gradient);
        context.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
        let _ = context.fill();
    });
    area
}

/// The marker one entry on a spine hangs off.
///
/// The newest is gold with a halo around it; everything older is a smaller grey
/// dot. There is exactly one newest, and it is the evening somebody is here to
/// read about.
///
/// **Both are the same size of widget**, and only what is drawn inside differs.
/// The dots share a grid column with the spine, so the column is as wide as the
/// widest thing in it and each child is centred in that — two different widget
/// sizes centre at two different places once anything else in the column
/// changes width, and the markers come off the line they are meant to sit on.
/// It also means a caller aligning a dot vertically has one offset to tune
/// rather than one per state.
pub fn spine_dot(newest: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(DOT)
        .content_height(DOT)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .build();
    area.set_draw_func(move |_, context, width, height| {
        let palette = Palette::current();
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        if newest {
            palette.gold_halo.apply(context);
            context.arc(cx, cy, 10.0, 0.0, std::f64::consts::TAU);
            let _ = context.fill();
            palette.gold.apply(context);
            context.arc(cx, cy, 6.0, 0.0, std::f64::consts::TAU);
            let _ = context.fill();
        } else {
            palette.dot.apply(context);
            context.arc(cx, cy, 4.5, 0.0, std::f64::consts::TAU);
            let _ = context.fill();
        }
    });
    area
}

/// A ledger, as one bar: income from the left, spending from the right, and
/// whatever survived the evening as track in the middle.
pub fn ledger(income: Vec<(f64, bool)>, spending: Vec<(f64, bool)>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(8)
        .hexpand(true)
        .build();
    area.set_draw_func(move |_, context, width, height| {
        let palette = Palette::current();
        let full = f64::from(width);
        let h = f64::from(height);

        palette.track.apply(context);
        context.rectangle(0.0, 0.0, full, h);
        let _ = context.fill();

        let mut x = 0.0;
        for (share, first) in &income {
            let w = full * share.clamp(0.0, 1.0);
            if *first {
                palette.positive
            } else {
                palette.positive.at(0.5)
            }
            .apply(context);
            context.rectangle(x, 0.0, w, h);
            let _ = context.fill();
            x += w;
        }

        let mut x = full;
        for (share, first) in &spending {
            let w = full * share.clamp(0.0, 1.0);
            if *first {
                palette.negative
            } else {
                palette.negative.at(0.55)
            }
            .apply(context);
            context.rectangle(x - w, 0.0, w, h);
            let _ = context.fill();
            x -= w;
        }
    });
    area
}

/// A three-segment depth bar: the floor, the cheap tenth, and the rest.
pub fn depth(shares: [f64; 3]) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(9)
        .hexpand(true)
        .build();
    area.set_draw_func(move |_, context, width, height| {
        let palette = Palette::current();
        let inks = [palette.gold, palette.gold.at(0.45), palette.track];
        let full = f64::from(width);
        let mut x = 0.0;
        for (share, ink) in shares.iter().zip(inks) {
            let w = full * share.clamp(0.0, 1.0);
            ink.apply(context);
            context.rectangle(x, 0.0, w, f64::from(height));
            let _ = context.fill();
            x += w;
        }
    });
    area
}

/// A proportional bar beside a name — what keeps killing you, where a
/// collection comes from. Not a progress bar: it is one row's share of a list's
/// largest, and it does not animate.
pub fn tally_bar(share: f64, width: i32, tone: Tone) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(width)
        .content_height(5)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |_, context, width, height| {
        let palette = Palette::current();
        let full = f64::from(width);
        let h = f64::from(height);
        palette.track.apply(context);
        context.rectangle(0.0, 0.0, full, h);
        let _ = context.fill();
        match tone {
            Tone::Gold => palette.gold,
            Tone::Positive => palette.positive,
            Tone::Negative => palette.negative,
            Tone::Plain => palette.pale,
        }
        .apply(context);
        context.rectangle(0.0, 0.0, full * share.clamp(0.0, 1.0), h);
        let _ = context.fill();
    });
    area
}

// -- assembled pieces ---------------------------------------------------------

/// A caption, a figure and a footnote, as one small card.
///
/// The Run page's three numbers and the Chronicle card's are the same widget at
/// two sizes, because they are the same three facts about the same evening.
pub fn stat(caption_text: &str, value: &str, footnote: &str, small: bool) -> gtk::Box {
    let tile = card(2);
    tile.add_css_class("al-stat");
    if small {
        tile.add_css_class("al-stat-small");
    }
    tile.set_hexpand(true);

    // Every line ellipsizes. Three of these sit homogeneously in a row and a
    // caption that refuses to shrink is what decides the minimum width of a
    // whole page — which is how a rail gets pushed off the edge of the window.
    let caption = label(caption_text, &["al-caption"]);
    caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
    tile.append(&caption);
    let value = mono(value, &["al-figure"]);
    if small {
        value.add_css_class("al-figure-small");
    }
    tile.append(&value);
    if !footnote.is_empty() {
        let note = label(footnote, &["al-caption"]);
        note.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tile.append(&note);
    }
    tile
}

/// A stat, with a say in whether its figure is gold.
///
/// [`stat`] beside this draws a card; these are meant to sit in a row of them
/// over a hairline background so the row reads as one object, and the tone is
/// the whole reason it is a separate function. The character page puts three
/// of the character's own figures beside one of the *account's*, and the
/// account's is the one that must not be gold — a strip where everything is
/// gold says everything was earned here, which is precisely the claim the
/// application exists to refuse.
pub fn stat_tile(caption_text: &str, value: &str, footnote: &str, tone: Tone) -> gtk::Box {
    let tile = column(7);
    // `al-strip-cell`, not `al-tile`: a tile is a collection cell and draws its
    // own border and corners, which in a row of four is four boxes rather than
    // one object. The cells here are separated by the strip's background
    // showing through a one-pixel gap and nothing else.
    tile.add_css_class("al-strip-cell");
    tile.set_hexpand(true);

    // Every line ellipsizes. Four of these sit homogeneously in a row and a
    // caption that refuses to shrink decides the minimum width of the page —
    // see `tests/width.rs`.
    let caption = mono(caption_text, &["al-caption"]);
    caption.set_xalign(0.0);
    caption.set_ellipsize(gtk::pango::EllipsizeMode::End);
    tile.append(&caption);

    let figure = mono(value, &["al-figure", tone.class()]);
    figure.set_xalign(0.0);
    figure.set_ellipsize(gtk::pango::EllipsizeMode::End);
    tile.append(&figure);

    if !footnote.is_empty() {
        let note = label(footnote, &["al-caption"]);
        note.set_xalign(0.0);
        note.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tile.append(&note);
    }
    tile
}

/// A section: a mono label, and whatever the section is.
pub fn titled(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let column = column(9);
    column.append(&section(title));
    column.append(child);
    column
}

/// A label and a figure on one line, the figure to the right.
pub fn stat_line(name: &str, value: &str, tone: Tone) -> gtk::Box {
    let line = row(8);
    let name = label(name, &["al-caption"]);
    name.set_hexpand(true);
    line.append(&name);
    let value = mono(value, &["al-stat-figure", tone.class()]);
    value.set_halign(gtk::Align::End);
    line.append(&value);
    line
}

/// A three-segment toggle: All / Unwritten / Written, Missing / Collected / All.
///
/// One `GtkToggleButton` group in a `.linked`-free track, styled as the design's
/// segments rather than as Adwaita's — the active segment is gold-tinted, which
/// no stock style offers.
pub fn segments<F: Fn(usize) + 'static>(options: &[&str], active: usize, on_change: F) -> gtk::Box {
    let track = row(3);
    track.add_css_class("al-segments");
    track.set_halign(gtk::Align::Start);

    let handler = std::rc::Rc::new(on_change);
    let mut first: Option<gtk::ToggleButton> = None;

    for (index, option) in options.iter().enumerate() {
        let button = gtk::ToggleButton::builder()
            .label(*option)
            .active(index == active)
            .build();
        button.add_css_class("al-segment");
        if let Some(first) = &first {
            button.set_group(Some(first));
            button.set_active(index == active);
        } else {
            first = Some(button.clone());
        }

        let handler = handler.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                handler(index);
            }
        });
        track.append(&button);
    }
    track
}

/// A switch at the design's size, with its label.
///
/// Both the track and the knob are fixed: a switch in a horizontal box with a
/// long title next to it gets squashed into an oval otherwise, which was a real
/// bug in the mocks and is a real bug in GTK for the same reason.
pub fn switch_row(title: &str, subtitle: &str, active: bool) -> (gtk::Box, gtk::Switch) {
    let line = row(12);
    line.add_css_class("al-switch-row");

    let text = column(2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let title = label(title, &["al-row-title"]);
    title.set_wrap(true);
    text.append(&title);
    if !subtitle.is_empty() {
        let subtitle = label(subtitle, &["al-caption"]);
        subtitle.set_wrap(true);
        text.append(&subtitle);
    }
    line.append(&text);

    let switch = gtk::Switch::builder()
        .active(active)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::End)
        .hexpand(false)
        .build();
    switch.add_css_class("al-switch");
    // Both the track and the knob pinned, not merely given a minimum. A switch
    // beside a title that wraps is otherwise stretched into an oval by the box
    // it is in, which is a real GTK behaviour and was a real bug in the mocks.
    switch.set_size_request(36, 21);
    line.append(&switch);
    (line, switch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_survives_the_trip_to_css() {
        assert_eq!(hex(0xe0b34a).css(), "rgba(224, 179, 74, 1.000)");
        assert_eq!(white(0.05).css(), "rgba(255, 255, 255, 0.050)");
    }

    #[test]
    fn every_token_reaches_the_stylesheet() {
        // A token added to the struct and forgotten in `css` is a `var()` that
        // resolves to nothing, which GTK renders as transparent rather than as
        // an error — so it is worth counting.
        let css = Palette::DARK.css();
        for name in [
            "--al-ground",
            "--al-rail",
            "--al-card",
            "--al-gold",
            "--al-gold-text",
            "--al-positive",
            "--al-negative",
            "--al-track",
            "--al-pale",
        ] {
            assert!(css.contains(name), "{name} is missing from the stylesheet");
        }
    }

    #[test]
    fn gold_totals_are_grouped_so_they_can_be_read_rather_than_counted() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1\u{202f}000");
        assert_eq!(thousands(12_345_678), "12\u{202f}345\u{202f}678");
    }

    #[test]
    fn a_count_of_one_takes_the_singular() {
        // "1 quests" reads as a bug in the journal rather than as one quest.
        assert_eq!(plural(1, "quest", "quests"), "1 quest");
        assert_eq!(plural(0, "quest", "quests"), "0 quests");
        assert_eq!(plural(12, "quest", "quests"), "12 quests");
    }

    #[test]
    fn a_headline_spells_its_number_until_a_figure_is_easier() {
        assert_eq!(spelled(0), "Nothing");
        assert_eq!(spelled(11), "Eleven");
        assert_eq!(spelled(20), "Twenty");
        assert_eq!(spelled(21), "21");
    }

    #[test]
    fn an_alpha_is_the_same_hue() {
        let gold = hex(0xe0b34a);
        let soft = gold.at(0.3);
        assert_eq!((gold.0, gold.1, gold.2), (soft.0, soft.1, soft.2));
        assert_eq!(soft.3, 0.3);
    }

    #[test]
    fn a_paragraph_break_is_the_newline_that_ends_the_empty_line() {
        // "a\n\nb": the blank line is the second newline, at byte 2. Taking the
        // first would shrink the line the prose is on.
        assert_eq!(blank_lines("a\n\nb"), [(2, 3)]);
    }

    #[test]
    fn text_with_no_break_in_it_has_no_gap_to_take() {
        assert_eq!(blank_lines("one line only"), Vec::new());
        assert_eq!(blank_lines("two\nlines"), Vec::new());
    }

    #[test]
    fn a_line_of_spaces_is_a_blank_line() {
        // A model that writes "   " between paragraphs has still written a
        // paragraph break, and it costs the same full line as an empty one.
        assert_eq!(blank_lines("a\n   \nb"), [(2, 6)]);
    }

    #[test]
    fn a_trailing_newline_is_not_a_gap_between_anything() {
        // Nothing follows it, so shrinking it would take height off the bottom
        // of the card rather than out from between two paragraphs.
        assert_eq!(blank_lines("a\n"), Vec::new());
    }

    #[test]
    fn every_break_in_a_long_entry_is_found() {
        let entry = "First.\n\nSecond.\n\nThird.";
        assert_eq!(blank_lines(entry), [(7, 8), (16, 17)]);
    }
}
