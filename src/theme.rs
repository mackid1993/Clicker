// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! Fluent design tokens.
//!
//! These follow the Fluent system rather than being invented: the neutral
//! ramp, the accent, the layer fills, the corner radii and the type ramp are
//! Fluent's, so the app sits naturally beside Settings and File Explorer.
//!
//! Layer fills are deliberately semi-transparent. The window paints its own
//! Mica-like material (see `backdrop`), and surfaces have to let it through or
//! the effect is lost and everything looks flat.

use egui::{Color32, FontId, Margin, Rounding, Shadow, Stroke, Visuals};

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
/// Fluent's own guidance. Nothing here should ever bounce or overshoot; motion
/// exists to explain a state change, not to decorate it.
pub const ANIM_FAST: f32 = 0.10;
pub const ANIM_NORMAL: f32 = 0.16;
pub const ANIM_SURFACE: f32 = 0.22;

/// Draw artwork into a rounded rectangle, cropped to fill it.
///
/// `Painter::image` draws a square-cornered quad, and clipping it to the tile
/// does not round it either — a clip rectangle has no radius. So artwork drawn
/// that way sat square inside the rounded stroke around it, with the corners of
/// the picture poking out past the border on all four sides. The fix is to draw
/// the texture *as* the rounded rectangle: `RectShape` carries a texture and UV
/// alongside its rounding, so the rounding applies to the picture itself.
///
/// Cover rather than contain: the picture is scaled to fill the tile and the
/// overflow is cropped, because artwork letterboxed into a tile is mostly empty
/// box. `focus_y` picks what survives the crop vertically — 0.5 keeps the
/// middle, lower values keep more of the top, which is where faces usually are.
pub fn image_cover(
    painter: &egui::Painter,
    rect: egui::Rect,
    rounding: f32,
    texture: &egui::TextureHandle,
    focus_y: f32,
) {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }

    let scaled = size * (rect.width() / size.x).max(rect.height() / size.y);
    let width = (rect.width() / scaled.x).min(1.0);
    let height = (rect.height() / scaled.y).min(1.0);

    let mut shape = egui::epaint::RectShape::filled(
        rect,
        Rounding::same(rounding),
        Color32::WHITE,
    );
    shape.fill_texture_id = texture.id();
    shape.uv = egui::Rect::from_min_size(
        egui::pos2((1.0 - width) * 0.5, (1.0 - height) * focus_y),
        egui::vec2(width, height),
    );
    painter.add(shape);
}

/// The Fluent search field: a pill with a magnifier in it.
///
/// Shared rather than drawn per screen. The guide had this and the library had
/// a stock `TextEdit`, so the same control appeared as a rounded pill on one
/// screen and a hard-edged box on the next — the kind of inconsistency that
/// reads as two different applications stitched together.
///
/// Returns the field's response so a caller can tell when it was interacted
/// with.
///
/// The height is the caller's because it depends on what the field sits beside:
/// see [`SEARCH_H`].
pub fn search_field(
    ui: &mut egui::Ui,
    text: &mut String,
    width: f32,
    height: f32,
) -> egui::Response {
    let (field, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    ui.painter().rect_filled(
        field,
        height / 2.0,
        Color32::from_rgba_unmultiplied(
            Fluent::LAYER_CARD.r(),
            Fluent::LAYER_CARD.g(),
            Fluent::LAYER_CARD.b(),
            150,
        ),
    );
    ui.painter().rect_stroke(
        field,
        height / 2.0,
        Stroke::new(1.0, Fluent::STROKE_CONTROL),
    );
    ui.painter().text(
        egui::pos2(field.min.x + SPACE_M + 2.0, field.center().y),
        egui::Align2::LEFT_CENTER,
        icon::SEARCH,
        FontId::new(12.0, egui::FontFamily::Name(ICON_FONT.into())),
        Fluent::TEXT_TERTIARY,
    );

    // The text gets a rect exactly one line tall, centered on the pill.
    //
    // Handing the TextEdit the whole 36px pill is what left the text sitting
    // against the top edge, out of line with the magnifier beside it: a
    // TextEdit lays its galley out from the top of whatever rect it is given,
    // and stretching it to the full height on the cross axis leaves spare room
    // for it to sit at the top of. Giving it only the height it needs means
    // there is none.
    let font = FontId::proportional(13.0);
    let line = ui.fonts(|f| f.row_height(&font));
    let inner = egui::Rect::from_min_max(
        egui::pos2(field.min.x + 34.0, field.center().y - line / 2.0),
        egui::pos2(field.max.x - SPACE_M, field.center().y + line / 2.0),
    );
    let mut text_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    text_ui.add(
        egui::TextEdit::singleline(text)
            .hint_text("Search")
            .font(font)
            .desired_width(inner.width())
            .text_color(Fluent::TEXT_PRIMARY)
            .frame(false)
            .margin(Margin::ZERO),
    );

    response
}

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

/// The height of a search field that stands on its own, rather than in a row
/// of chips sized to match it.
///
/// egui's default `interact_size` is 18px. That is right in the guide, where
/// the pill is one of a row of controls that all use it and the row reads as a
/// set — and wrong on its own beside a heading, where an 18px control next to
/// 24pt type looks like it failed to load. 32 is the Fluent standard.
pub const SEARCH_H: f32 = 32.0;

/// Custom caption height. Fluent uses 32px; a little more suits a media app.
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

    // The platform's own interface face, ahead of egui's bundled one. Missing
    // is survivable — egui's face is legible — but the system's is what makes
    // the window read as belonging on the desktop it is on.
    if let Some(bytes) = crate::platform::text_font() {
        fonts
            .font_data
            .insert("system".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system".into());
    }

    // Caption and control glyphs. Substituting lookalike Unicode characters is
    // what makes custom chrome read as wrong: the shapes, weights and optical
    // sizes do not match the real thing.
    if let Some(bytes) = crate::platform::icon_font() {
        fonts
            .font_data
            .insert("icons".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .insert(egui::FontFamily::Name(ICON_FONT.into()), vec!["icons".into()]);
    }

    ctx.set_fonts(fonts);
}

/// Family name for the icon font.
pub const ICON_FONT: &str = "icons";

/// The chrome and control glyphs, by name.
///
/// Two codepoint tables for one set of names: Segoe Fluent Icons on Windows,
/// read from the system, and Microsoft's Fluent UI System Icons — the same
/// design language, MIT-licensed — bundled everywhere else. Every glyph the
/// interface draws must come from this table; an inline `\u{E7xx}` literal is
/// a hole in the port, because it names a Segoe codepoint the bundled font
/// does not have.
macro_rules! icons {
    ($($(#[$doc:meta])* $name:ident: $segoe:literal, $fluent:literal;)*) => {
        $(
            $(#[$doc])*
            #[cfg(windows)]
            pub const $name: &str = $segoe;
            $(#[$doc])*
            #[cfg(not(windows))]
            pub const $name: &str = $fluent;
        )*
    };
}

pub mod icon {
    icons! {
        MINIMIZE: "\u{E921}", "\u{EBD1}";
        MAXIMIZE: "\u{E922}", "\u{E7EC}";
        RESTORE: "\u{E923}", "\u{F78C}";
        CLOSE: "\u{E8BB}", "\u{F36A}";
        /// Back, for leaving the player.
        ///
        /// Deliberately not CLOSE. The window's own close button is an X in the
        /// top-right of the same window, and an X on the player was being read as
        /// "quit the program" rather than "leave this show".
        BACK: "\u{E72B}", "\u{F15C}";
        PLAY: "\u{E768}", "\u{F606}";
        PAUSE: "\u{E769}", "\u{F5A2}";
        /// The curved undo/redo arrows, which is what every media player uses
        /// for a fixed-interval skip. E7A6 alone was being used for "back",
        /// but it is Redo: a clockwise arrow, pointing the wrong way.
        SKIP_BACK: "\u{E7A7}", "\u{F19A}";
        SKIP_FORWARD: "\u{E7A6}", "\u{F16F}";
        VOLUME: "\u{E767}", "\u{EB43}";
        MUTE: "\u{E74F}", "\u{EB4B}";
        MORE: "\u{E712}", "\u{E825}";
        RECORD: "\u{E7C8}", "\u{F662}";
        HAMBURGER: "\u{E700}", "\u{F561}";
        /// FastForward, for skipping a commercial break.
        SKIP_BREAK: "\u{EB9D}", "\u{F3FF}";
        FULLSCREEN: "\u{E740}", "\u{E685}";
        EXIT_FULLSCREEN: "\u{E73F}", "\u{E688}";
        // The rest of the interface: navigation, rows, and states.
        HOME: "\u{E80F}", "\u{F481}";
        LIBRARY: "\u{E8F1}", "\u{F4D3}";
        GRID: "\u{E8BC}", "\u{F463}";
        VIDEO: "\u{E7F4}", "\u{F850}";
        DOWNLOAD: "\u{E896}", "\u{F151}";
        DELETE: "\u{E74D}", "\u{F34D}";
        CHECK: "\u{E73E}", "\u{F295}";
        FORWARD: "\u{E72A}", "\u{F182}";
        SEARCH: "\u{E721}", "\u{F690}";
        SETTINGS: "\u{E713}", "\u{F6AA}";
        CANCEL: "\u{E711}", "\u{F36A}";
        CHEVRON_DOWN: "\u{E70D}", "\u{F2A4}";
    }
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
