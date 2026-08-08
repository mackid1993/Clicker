//! Fluent design tokens.
//!
//! These follow the WinUI 3 system rather than being invented: the neutral
//! ramp, the accent, the layer fills, the corner radii and the type ramp are
//! Fluent's, so the app sits naturally beside Settings and File Explorer.
//!
//! Layer fills are deliberately semi-transparent. The window paints its own
//! Mica-like material (see `backdrop`), and surfaces have to let it through or
//! the effect is lost and everything looks flat.

use egui::{Color32, Margin, Rounding, Shadow, Stroke, Visuals};

/// Build a color from ordinary RGBA, premultiplying at compile time.
///
/// `Color32::from_rgba_unmultiplied` is not a `const fn`, so theme constants
/// have to be written premultiplied — and writing them by hand is exactly how
/// every stroke in this file ended up wrong. In premultiplied alpha the color
/// channels must already be scaled by the alpha, so a white hairline at 7%
/// opacity is `(18, 18, 18, 18)`, not `(255, 255, 255, 18)`. The latter
/// describes an impossible color, clamps, and paints a hard white line.
///
/// Writing the numbers the intuitive way and doing the multiplication here
/// removes the trap entirely.
const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        (r as u32 * a as u32 / 255) as u8,
        (g as u32 * a as u32 / 255) as u8,
        (b as u32 * a as u32 / 255) as u8,
        a,
    )
}

pub struct Fluent;

impl Fluent {
    // ── Layers ──────────────────────────────────────────────────────────
    // Fluent builds depth from translucent layers over the backdrop rather
    // than opaque panels. Alpha is what lets the material read through.
    /// The window base. Fully transparent, so the painted backdrop shows: the
    /// panel is a pane of glass over the material, not a surface of its own.
    pub const LAYER_BASE: Color32 = Color32::TRANSPARENT;
    /// LayerFillColorDefault: cards and flyouts.
    pub const LAYER_CARD: Color32 = rgba(28, 30, 36, 210);
    /// ControlFillColorDefault.
    pub const CONTROL: Color32 = rgba(58, 62, 72, 180);
    pub const CONTROL_HOVER: Color32 = rgba(72, 77, 89, 200);
    pub const CONTROL_PRESSED: Color32 = rgba(46, 49, 58, 215);
    /// SolidBackgroundFillColorBase, for surfaces that must stay opaque.
    pub const SOLID: Color32 = Color32::from_rgb(20, 22, 27);

    // ── Text ────────────────────────────────────────────────────────────
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(255, 255, 255);
    pub const TEXT_SECONDARY: Color32 = rgba(200, 202, 208, 205);
    pub const TEXT_TERTIARY: Color32 = rgba(160, 163, 172, 175);

    // ── Accent ──────────────────────────────────────────────────────────
    // One accent, used for selection, focus and progress, so the eye always
    // knows what is actionable.
    pub const ACCENT: Color32 = Color32::from_rgb(96, 165, 250);
    pub const ACCENT_DARK: Color32 = Color32::from_rgb(70, 130, 210);
    pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(140, 190, 255);

    // ── Semantic ────────────────────────────────────────────────────────
    pub const LIVE: Color32 = Color32::from_rgb(255, 99, 99);
    pub const SUCCESS: Color32 = Color32::from_rgb(108, 203, 149);
    pub const CAUTION: Color32 = Color32::from_rgb(252, 191, 90);

    // ── Strokes ─────────────────────────────────────────────────────────
    // Fluent separates with a hairline that is lighter on top, giving surfaces
    // a lit edge rather than a drawn border. These are barely-there by design:
    // see `rgba` above for why they were previously anything but.
    pub const STROKE_SURFACE: Color32 = rgba(255, 255, 255, 18);
    pub const STROKE_CONTROL: Color32 = rgba(255, 255, 255, 26);
    pub const STROKE_DIVIDER: Color32 = rgba(255, 255, 255, 12);
}

/// Fluent's corner radii.
pub const RADIUS_CONTROL: f32 = 4.0;
pub const RADIUS_SURFACE: f32 = 8.0;

/// Fluent's motion durations, in seconds. Fast controls, slower surfaces —
/// WinUI's own guidance. Nothing here should ever bounce or overshoot; motion
/// exists to explain a state change, not to decorate it.
pub const ANIM_FAST: f32 = 0.10;
pub const ANIM_NORMAL: f32 = 0.16;
pub const ANIM_SURFACE: f32 = 0.22;

/// Linear blend between two colors, for hover and selection animation.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

/// Fluent's 4px spacing grid.
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 8.0;
pub const SPACE_M: f32 = 12.0;
pub const SPACE_L: f32 = 16.0;

/// Custom caption height. WinUI uses 32px; a little more suits a media app.
pub const TITLEBAR_HEIGHT: f32 = 40.0;

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = Visuals::dark();

    // Transparent, so what shows through is the painted backdrop.
    visuals.panel_fill = Fluent::LAYER_BASE;
    visuals.window_fill = Fluent::LAYER_CARD;
    visuals.extreme_bg_color = Fluent::SOLID;
    visuals.faint_bg_color = Fluent::LAYER_CARD;

    visuals.override_text_color = Some(Fluent::TEXT_PRIMARY);
    visuals.hyperlink_color = Fluent::ACCENT;
    visuals.selection.bg_fill = Fluent::ACCENT_DARK;
    visuals.selection.stroke = Stroke::new(1.0, Fluent::ACCENT_LIGHT);

    let w = &mut visuals.widgets;

    w.noninteractive.bg_fill = Fluent::LAYER_CARD;
    w.noninteractive.weak_bg_fill = Fluent::LAYER_CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Fluent::STROKE_DIVIDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, Fluent::TEXT_SECONDARY);
    w.noninteractive.rounding = Rounding::same(RADIUS_CONTROL);

    w.inactive.bg_fill = Fluent::CONTROL;
    w.inactive.weak_bg_fill = Fluent::CONTROL;
    w.inactive.bg_stroke = Stroke::new(1.0, Fluent::STROKE_CONTROL);
    w.inactive.fg_stroke = Stroke::new(1.0, Fluent::TEXT_PRIMARY);
    w.inactive.rounding = Rounding::same(RADIUS_CONTROL);
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = Fluent::CONTROL_HOVER;
    w.hovered.weak_bg_fill = Fluent::CONTROL_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, Fluent::STROKE_CONTROL);
    w.hovered.fg_stroke = Stroke::new(1.0, Fluent::TEXT_PRIMARY);
    w.hovered.rounding = Rounding::same(RADIUS_CONTROL);
    // Fluent controls change fill on hover; they do not grow.
    w.hovered.expansion = 0.0;

    w.active.bg_fill = Fluent::CONTROL_PRESSED;
    w.active.weak_bg_fill = Fluent::CONTROL_PRESSED;
    w.active.bg_stroke = Stroke::new(1.0, Fluent::STROKE_CONTROL);
    w.active.fg_stroke = Stroke::new(1.0, Fluent::TEXT_PRIMARY);
    w.active.rounding = Rounding::same(RADIUS_CONTROL);
    w.active.expansion = 0.0;

    w.open.bg_fill = Fluent::CONTROL_PRESSED;
    w.open.weak_bg_fill = Fluent::CONTROL_PRESSED;
    w.open.bg_stroke = Stroke::new(1.0, Fluent::STROKE_CONTROL);
    w.open.rounding = Rounding::same(RADIUS_CONTROL);

    // Fluent shadows are soft and vertically offset, never harsh.
    visuals.popup_shadow = Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(110),
    };
    visuals.window_shadow = Shadow {
        offset: egui::vec2(0.0, 16.0),
        blur: 40.0,
        spread: 0.0,
        color: Color32::from_black_alpha(130),
    };
    visuals.window_rounding = Rounding::same(RADIUS_SURFACE);
    visuals.window_stroke = Stroke::new(1.0, Fluent::STROKE_SURFACE);
    visuals.menu_rounding = Rounding::same(RADIUS_SURFACE);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(SPACE_S, SPACE_S);
    style.spacing.button_padding = egui::vec2(SPACE_M, SPACE_S);
    style.spacing.menu_margin = Margin::same(SPACE_XS);
    style.spacing.window_margin = Margin::same(SPACE_L);
    style.spacing.indent = SPACE_L;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.interaction.selectable_labels = false;
    ctx.set_style(style);

    apply_type_ramp(ctx);
}

/// Segoe UI Variable is the Windows 11 face; plain Segoe UI keeps it native on
/// Windows 10. Both are tried, in that order, so one build looks right on
/// either — which is the whole approach this program takes to the two.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    for candidate in [
        r"C:\Windows\Fonts\SegoeUIVariableStatic-Display.ttf",
        r"C:\Windows\Fonts\SegoeUIVariableStatic-Regular.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
    ] {
        let Ok(bytes) = std::fs::read(candidate) else { continue };
        fonts
            .font_data
            .insert("system".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system".into());
        break;
    }

    // Caption and control glyphs come from the Windows icon font. Substituting
    // lookalike Unicode characters is what makes custom chrome read as wrong:
    // the shapes, weights and optical sizes do not match the real thing.
    for candidate in [
        r"C:\Windows\Fonts\SegoeIcons.ttf",   // Segoe Fluent Icons (Windows 11)
        r"C:\Windows\Fonts\segmdl2.ttf",      // Segoe MDL2 Assets (Windows 10)
    ] {
        let Ok(bytes) = std::fs::read(candidate) else { continue };
        fonts
            .font_data
            .insert("icons".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .insert(egui::FontFamily::Name(ICON_FONT.into()), vec!["icons".into()]);
        break;
    }

    ctx.set_fonts(fonts);
}

/// Family name for the Windows icon font.
pub const ICON_FONT: &str = "icons";

/// Segoe Fluent Icons codepoints for the pieces of chrome we draw ourselves.
pub mod icon {
    pub const MINIMIZE: &str = "\u{E921}";
    pub const MAXIMIZE: &str = "\u{E922}";
    pub const RESTORE: &str = "\u{E923}";
    pub const CLOSE: &str = "\u{E8BB}";
    /// Back, for leaving the player.
    ///
    /// Deliberately not CLOSE. The window's own close button is an X in the
    /// top-right of the same window, and an X on the player was being read as
    /// "quit the program" rather than "leave this show".
    pub const BACK: &str = "\u{E72B}";
    pub const PLAY: &str = "\u{E768}";
    pub const PAUSE: &str = "\u{E769}";
    // The curved undo/redo arrows, which is what every media player uses for a
    // fixed-interval skip. E7A6 alone was being used for "back", but it is
    // Redo: a clockwise arrow, pointing the wrong way.
    pub const SKIP_BACK: &str = "\u{E7A7}";
    pub const SKIP_FORWARD: &str = "\u{E7A6}";
    pub const VOLUME: &str = "\u{E767}";
    pub const MUTE: &str = "\u{E74F}";
    pub const MORE: &str = "\u{E712}";
    pub const RECORD: &str = "\u{E7C8}";
    pub const HAMBURGER: &str = "\u{E700}";
    /// FastForward, for skipping a commercial break.
    pub const SKIP_BREAK: &str = "\u{EB9D}";
    pub const FULLSCREEN: &str = "\u{E740}";
    pub const EXIT_FULLSCREEN: &str = "\u{E73F}";
}

/// A label in the icon font at a given size.
pub fn icon_text(glyph: &str, size: f32) -> egui::RichText {
    egui::RichText::new(glyph)
        .family(egui::FontFamily::Name(ICON_FONT.into()))
        .size(size)
}

/// The Fluent type ramp. Named sizes rather than arbitrary ones are what keep
/// an interface looking designed instead of assembled.
fn apply_type_ramp(ctx: &egui::Context) {
    use egui::{FontFamily::Proportional, FontId, TextStyle};

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, egui::FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
}
