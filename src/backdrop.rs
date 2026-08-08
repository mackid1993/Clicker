//! The window's material.
//!
//! Fluent surfaces are translucent and need something behind them or the whole
//! interface reads as flat panels on a flat wall. On Windows 11 that something
//! is Mica, which the compositor draws behind the window — and asking for it
//! is what used to make this Windows 11 only.
//!
//! So the material is drawn here instead: a soft, dark, slightly blue-lifted
//! gradient, brightest at the top left and falling away across the window.
//! Four pixels, stretched across the whole window with linear filtering — the
//! hardware interpolates between them, which is a perfectly smooth gradient
//! for the cost of a four-pixel texture and no per-frame work at all.
//!
//! It has to stay quiet. Everything in the interface is drawn over it, so
//! anything with real contrast in it competes with the content instead of
//! sitting behind it.

use eframe::egui;

use crate::theme::Fluent;

/// How far the lit corner rises above the base, and how far the far corner
/// falls below it. Small numbers on purpose: this should be felt rather than
/// seen, and anything stronger turns into a visible vignette that fights the
/// cards drawn over it.
const LIFT: f32 = 13.0;
const FALL: f32 = 4.0;

/// How much of the accent is mixed into the lit corner. Mica takes a cast from
/// whatever is behind it; this takes one from the interface's own blue, which
/// keeps the surface from reading as dead gray.
const TINT: f32 = 0.05;

pub struct Backdrop {
    texture: egui::TextureHandle,
}

impl Backdrop {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            // Built once and never replaced. Nothing about it depends on
            // anything that changes, which is also what keeps it away from the
            // hazard of freeing a texture the last frame is still drawing with.
            texture: ctx.load_texture("backdrop", material(), egui::TextureOptions::LINEAR),
        }
    }

    /// Fill `rect` with the material, behind everything else.
    ///
    /// Called before anything else paints, so every translucent surface in the
    /// theme has something to let through — which is what those surfaces were
    /// designed around when the material came from the system.
    pub fn paint(&self, ctx: &egui::Context, rect: egui::Rect) {
        ctx.layer_painter(egui::LayerId::background()).image(
            self.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// The four corners, top-left brightest.
///
/// Linear filtering is not a detail here, it is the whole gradient: two pixels
/// across and two down, stretched over the window, and every pixel between them
/// is interpolated by the hardware. Nearest filtering would paint four
/// enormous squares.
fn material() -> egui::ColorImage {
    let base = Fluent::SOLID;
    let accent = Fluent::ACCENT;

    // `amount` is how lit this corner is, from 1.0 at the top left to 0.0 at
    // the bottom right.
    let corner = |amount: f32| {
        let channel = |base: u8, accent: u8| {
            let lifted = base as f32 + LIFT * amount - FALL * (1.0 - amount);
            // The tint follows the light: the lit corner picks up the cast,
            // the far one stays neutral.
            let tinted = lifted + (accent as f32 - lifted) * TINT * amount;
            tinted.clamp(0.0, 255.0) as u8
        };
        egui::Color32::from_rgb(
            channel(base.r(), accent.r()),
            channel(base.g(), accent.g()),
            channel(base.b(), accent.b()),
        )
    };

    egui::ColorImage {
        size: [2, 2],
        // Top-left, top-right, bottom-left, bottom-right. The diagonal falls
        // evenly, so the two middle corners share a value.
        pixels: vec![corner(1.0), corner(0.55), corner(0.55), corner(0.0)],
    }
}
