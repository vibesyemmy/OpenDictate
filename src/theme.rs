//! Paper & Ink, both ways up.
//!
//! The kit ships one set of tokens and a dark twin that redefines them, so the
//! two themes are the same drawing with different values - nothing here
//! branches on the theme except the values themselves.

use eframe::egui::{self, Color32, FontFamily, FontId, RichText};
use std::sync::atomic::{AtomicBool, Ordering};

static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}
/// Switch the palette. Call `apply_style` afterwards so the widget styles
/// egui caches follow it.
pub fn set_light(light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
}

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// One token, dark value then light. Named in capitals because every call
/// site reads it where a constant used to be.
macro_rules! tone {
    ($(#[$m:meta])* $name:ident, $dark:expr, $light:expr) => {
        $(#[$m])*
        #[allow(non_snake_case)]
        pub fn $name() -> Color32 {
            if is_light() { $light } else { $dark }
        }
    };
}

tone!(PAPER, c(0x14, 0x14, 0x0F), c(0xF4, 0xF3, 0xEF));
tone!(INK, c(0xF4, 0xF3, 0xEF), c(0x14, 0x14, 0x0F));
tone!(SURFACE, c(0x1F, 0x1E, 0x18), c(0xFF, 0xFF, 0xFF));
tone!(BG, PAPER(), PAPER());
tone!(SIDEBAR, SURFACE(), SURFACE());
tone!(NAV_ACTIVE, INK(), INK());
tone!(CARD, SURFACE(), SURFACE());
tone!(CARD_DIM, PAPER(), PAPER());
tone!(PANEL, SURFACE(), SURFACE());
tone!(
    /// --hairline, flattened onto the paper behind it.
    LINE, c(0x22, 0x22, 0x1E), c(0xD9, 0xD8, 0xD5)
);
tone!(TEXT, INK(), INK());
tone!(
    /// Ink stepped back to the kit's 0.76, for secondary copy.
    MUTED, c(0xA8, 0xA7, 0xA2), c(0x4C, 0x4B, 0x46)
);
tone!(
    /// Small print, so it is lifted off the kit's 0.6 to clear 4.5:1 on the
    /// surface it sits on: 3.78 dark and 3.4 light at the kit's own value.
    FAINT, c(0x8B, 0x8A, 0x85), c(0x6E, 0x6D, 0x68)
);
tone!(
    /// --follow. The kit's blue reads as settled and working.
    ACCENT, c(0x8F, 0xB6, 0xEF), c(0x2B, 0x5A, 0x9B)
);
tone!(
    /// --green. Settled: set up and working. Added to the kit for this.
    GREEN, c(0x86, 0xD2, 0x9B), c(0x2E, 0x7D, 0x4F)
);
tone!(/// --red
    RED, c(0xE0, 0x49, 0x3D), c(0xC0, 0x34, 0x2A));
tone!(
    /// --y2. Yellow has noticed; something wants attention soon.
    AMBER, c(0xF2, 0xC3, 0x1A), c(0xF2, 0xC3, 0x1A)
);
tone!(/// --y1
    Y1, c(0xF8, 0xE0, 0x8A), c(0xF8, 0xE0, 0x8A));
tone!(/// --y3
    Y3, c(0xC9, 0x8F, 0x06), c(0xC9, 0x8F, 0x06));
tone!(
    /// What sits on a light fill in the dark theme and a dark one in the
    /// light theme: whatever paper is, as the kit's --on-red and --on-yellow
    /// do. White on the kit's blue is about 2.1:1, which is why it has no
    /// such token.
    ON_FILL, PAPER(), PAPER()
);
tone!(
    /// Paper stepped back, for secondary copy on that fill.
    ON_FILL_DIM, c(0x4A, 0x49, 0x44), c(0xBE, 0xBD, 0xB9)
);
tone!(
    /// --on-yellow. What goes on a yellow fill, in both themes.
    ///
    /// ON_FILL cannot do this job. It is paper, and paper flips with the
    /// theme while the yellows do not - they are one value in both. On a
    /// dark page paper is nearly black and reads on yellow at 11:1; on a
    /// light page it is nearly white and reads at 1.5:1, which is how the
    /// setup notice came to be white text on a yellow card. This is the
    /// dark value in both themes, so it changes nothing in the dark and
    /// gives the light theme the same 11:1.
    ON_YELLOW, c(0x14, 0x14, 0x0F), c(0x14, 0x14, 0x0F)
);
tone!(
    /// ON_YELLOW stepped back, for secondary copy on a yellow fill.
    ON_YELLOW_DIM, c(0x4A, 0x49, 0x44), c(0x4A, 0x49, 0x44)
);
tone!(
    /// Yellow where it is the mark itself rather than the fill behind it.
    ///
    /// AMBER is a fill colour and sits at the light end, so as a tick or a
    /// word it lands at 1.67:1 on a white card - the same yellow that reads
    /// at 10:1 on a dark one. The light theme takes it down to where it
    /// clears 4.5:1 on both paper and surface while still reading as yellow
    /// rather than brown.
    WARN, c(0xF2, 0xC3, 0x1A), c(0x8A, 0x62, 0x00)
);
tone!(
    /// --soft-shadow. Still hard edged, but translucent rather than full ink.
    /// The kit reserves the ink shadow for things you press; content cards
    /// get this, which is why they do not shout.
    SOFT_SHADOW,
    Color32::from_rgba_premultiplied(0, 0, 0, 115),
    Color32::from_rgba_unmultiplied(0x14, 0x14, 0x0F, 26)
);
tone!(
    /// The surface a pointer is resting on.
    HOVER, c(0x2B, 0x2A, 0x23), c(0xF0, 0xEF, 0xEB)
);
tone!(
    /// --track: the empty part of a meter, a heat cell with nothing in it,
    /// the well behind a small icon. Ink at the kit's 0.13/0.16, flattened.
    TRACK, c(0x31, 0x30, 0x2A), c(0xE2, 0xE1, 0xDD)
);

/// The page behind everything, let through a little. Cards, panels and the
/// side panel keep their own opaque fills, so only the paper between them
/// takes on what is behind the window.
#[allow(non_snake_case)]
pub fn BG_SHEER() -> Color32 {
    // With the AppKit backdrop in place the wash is painted there instead,
    // across the titlebar too, so painting it here as well would double it.
    if BACKDROP.load(Ordering::Relaxed) {
        return Color32::TRANSPARENT;
    }
    let p = PAPER();
    Color32::from_rgba_unmultiplied(p.r(), p.g(), p.b(), SHEER)
}

/// How much paper the page keeps over whatever is behind the window. 0.6.
pub const SHEER: u8 = 153;

static BACKDROP: AtomicBool = AtomicBool::new(false);

/// A container that only groups things - the side panel, the dotted field
/// behind the insights - rather than carrying text of its own. With the
/// backdrop in place these step aside and let it through; the cards sitting
/// on them keep their own surface.
#[allow(non_snake_case)]
pub fn PANEL_SHEER() -> Color32 {
    if BACKDROP.load(Ordering::Relaxed) { Color32::TRANSPARENT } else { SURFACE() }
}

/// Said once the blurred backdrop is carrying the wash.
pub fn set_backdrop(on: bool) {
    BACKDROP.store(on, Ordering::Relaxed);
}

/// The wash, as AppKit wants it: straight sRGB components, 0 to 1.
pub fn sheer_rgba() -> [f64; 4] {
    let p = PAPER();
    [p.r() as f64 / 255.0, p.g() as f64 / 255.0, p.b() as f64 / 255.0, SHEER as f64 / 255.0]
}

/// The streak ramp, coldest first. Dark climbs towards the kit's blue; light
/// climbs away from the paper towards its deep one.
#[allow(non_snake_case)]
pub fn HEAT() -> [Color32; 4] {
    if is_light() {
        [c(0xD7, 0xE4, 0xF7), c(0xA9, 0xC6, 0xEF), c(0x6D, 0x90, 0xCF), c(0x2B, 0x5A, 0x9B)]
    } else {
        [c(0x2B, 0x3A, 0x55), c(0x44, 0x63, 0x96), c(0x6D, 0x90, 0xCF), c(0x9A, 0xBE, 0xEF)]
    }
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}
/// Marsipan, the kit's rounded display face. Headings and figures only.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// A surface in the kit's hand: thick outline and a hard offset shadow.
///
/// The shadow is never blurred - blur 0 is what makes it read as ink on paper
/// rather than a drop shadow. Its colour is the choice that matters: ink for
/// something you press, soft for something you only read.
pub fn surface(fill: Color32, radius: u8, border: f32, lift: i8, shadow: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(border, INK()))
        .corner_radius(egui::CornerRadius::same(radius))
        .shadow(egui::epaint::Shadow { offset: [lift, lift + 1], blur: 0, spread: 0, color: shadow })
}
/// Content you read. Matches the kit's .card: 2.5px outline, 6/7 soft shadow.
pub fn ink_card() -> egui::Frame {
    surface(SURFACE(), 15, 2.5, 6, SOFT_SHADOW())
}
/// A larger reading surface. Matches .step-card and .menu: 3px, 7/8 soft.
pub fn ink_panel() -> egui::Frame {
    surface(SURFACE(), 18, 3.0, 7, SOFT_SHADOW())
}
/// Another surface you only read, at a size of its own choosing.
pub fn soft(fill: Color32, radius: u8, border: f32, lift: i8) -> egui::Frame {
    surface(fill, radius, border, lift, SOFT_SHADOW())
}
/// Something you press. Keeps the full ink shadow, as .inspect does.
pub fn ink(fill: Color32, radius: u8, border: f32, lift: i8) -> egui::Frame {
    surface(fill, radius, border, lift, INK())
}

pub fn t(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color)
}
pub fn b(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(FontId::new(size, bold())).color(color)
}
pub fn m(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(FontId::new(size, medium())).color(color)
}

/// Display type: Marsipan, for titles and figures. The kit uses it at 40 for a
/// big title, 27 for a figure and 15 on buttons; body copy stays Nunito.
pub fn d(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).font(FontId::new(size, display())).color(color)
}

/// Body copy that runs to more than one line.
///
/// egui spaces rows by the font's own metrics, which is about 1.2x the size:
/// fine for a label, too tight to read a paragraph in. Anything that wraps
/// should use this instead of `t`.
pub fn para(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    t(s, size, color).line_height(Some(size * 1.5))
}

/// Paper & Ink's faces: Marsipan for display, Nunito for everything else.
///
/// Nunito is a variable font, so the kit's three body weights come from one
/// file at different points on its wght axis. Lucide sits behind all of them
/// so icon codepoints keep rendering inside ordinary strings.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts.font_data.insert(
        "lucide".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/lucide.ttf"))),
    );
    fonts.font_data.insert(
        "marsipan".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/Marsipan-Regular.ttf"))),
    );
    let nunito = include_bytes!("../assets/Nunito-Variable.ttf");
    let mut add = |name: &str, family: FontFamily, weight: f32| {
        let mut tweak = egui::FontTweak::default();
        tweak.coords = egui::epaint::text::VariationCoords::new([("wght", weight)]);
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(nunito).tweak(tweak)),
        );
        let mut list = vec![name.to_owned(), "lucide".to_owned()];
        list.extend(fallback.iter().cloned());
        fonts.families.insert(family, list);
    };
    // The kit's body weights: 600 copy, 700 emphasis, 800 buttons and labels.
    add("nunito-600", FontFamily::Proportional, 600.0);
    add("nunito-700", medium(), 700.0);
    add("nunito-800", bold(), 800.0);
    let mut disp = vec!["marsipan".to_owned(), "nunito-800".to_owned(), "lucide".to_owned()];
    disp.extend(fallback.iter().cloned());
    fonts.families.insert(display(), disp);
    ctx.set_fonts(fonts);
    apply_style(ctx);
}

/// The widget styles egui caches. Re-run whenever the palette flips.
pub fn apply_style(ctx: &egui::Context) {
    let mut style = egui::Style::default();
    style.visuals = egui::Visuals::dark();
    let v = &mut style.visuals;
    v.panel_fill = BG();
    v.window_fill = Color32::from_rgb(0x34, 0x34, 0x34);
    v.extreme_bg_color = PAPER();
    v.override_text_color = Some(TEXT());
    // Anything chosen is filled with ink and carries paper on it - the nav
    // row, the segmented control, a choice card, and now a dropdown row and
    // selected text. egui takes the text colour from selection.stroke, so
    // both the highlight and what sits on it come from this one pair.
    v.selection.bg_fill = INK();
    v.selection.stroke = egui::Stroke::new(1.0, PAPER());
    v.hyperlink_color = ACCENT();
    // Controls carry the outline too. The kit presses a button by moving it
    // onto its own shadow, so pressed state is the ink fill with paper on top.
    v.widgets.inactive.weak_bg_fill = SURFACE();
    v.widgets.inactive.bg_fill = SURFACE();
    v.widgets.inactive.bg_stroke = egui::Stroke::new(2.5, INK());
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, INK());
    v.widgets.hovered.weak_bg_fill = HOVER();
    v.widgets.hovered.bg_fill = HOVER();
    v.widgets.hovered.bg_stroke = egui::Stroke::new(2.5, INK());
    v.widgets.active.weak_bg_fill = INK();
    v.widgets.active.bg_fill = INK();
    v.widgets.active.bg_stroke = egui::Stroke::new(2.5, INK());
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, PAPER());
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.corner_radius = egui::CornerRadius::same(11);
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, LINE());

    // Popups and tooltips follow .menu: an ink edge over a hard shadow. The
    // kit's .tip is yellow, but egui draws every popup from one set of values,
    // so a yellow tooltip would drag the dropdown lists with it.
    v.window_fill = SURFACE();
    v.window_stroke = egui::Stroke::new(2.5, INK());
    v.window_corner_radius = egui::CornerRadius::same(14);
    v.menu_corner_radius = egui::CornerRadius::same(14);
    let hard = egui::epaint::Shadow { offset: [3, 4], blur: 0, spread: 0, color: SOFT_SHADOW() };
    v.popup_shadow = hard;
    v.window_shadow = hard;

    // Scrollbar: a slim ink handle that keeps out of the way until wanted.
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_width = 9.0;
    style.spacing.scroll.floating_width = 5.0;
    style.spacing.scroll.foreground_color = true;
    style.spacing.scroll.dormant_handle_opacity = 0.25;
    style.spacing.scroll.active_handle_opacity = 0.6;
    style.spacing.scroll.interact_handle_opacity = 0.9;
    style.spacing.scroll.dormant_background_opacity = 0.0;
    style.spacing.scroll.active_background_opacity = 0.0;
    // The kit's slider is a plain track with an ink thumb, with nothing
    // filled in behind it.
    style.visuals.slider_trailing_fill = false;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(13.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    for (ts, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 12.0),
        (egui::TextStyle::Heading, 22.0),
    ] {
        if let Some(f) = style.text_styles.get_mut(&ts) {
            f.size = size;
        }
    }
    ctx.set_global_style(style);
}
