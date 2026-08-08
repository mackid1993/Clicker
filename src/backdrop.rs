//! The window's material.
//!
//! Fluent surfaces are translucent and need something behind them or the whole
//! interface reads as flat panels on a flat wall. On Windows 11 that something
//! is Mica, which the compositor draws behind the window — and asking for it
//! is what used to make this Windows 11 only.
//!
//! So the material is drawn here instead: a handful of wide, soft pools of
//! light on the dark base, computed onto a grid a dozen or so pixels across
//! and stretched over the whole window. That stretch is the blur — the
//! hardware interpolates between pixels that are each a hundred-odd pixels
//! apart on screen, so what reaches the glass is smooth at every scale a
//! window is ever seen at. A single corner-to-corner gradient was the first
//! attempt and it reads as a solid color with a slight lean: the eye needs
//! more than one place for the light to come from before it accepts a surface
//! as a material.
//!
//! There is deliberately no grain. Mica has one, and adding a speckle here
//! looked like television static: white noise on a base this dark is a large
//! relative lift on every pixel it touches, and tiling it at a fractional
//! scale beat against the pixel grid into a visible weave. Whatever it was
//! buying in texture, it was not worth what it cost in looking broken.
//!
//! What is left stays quiet on purpose. Everything in the interface is drawn
//! over this, so anything with real contrast in it competes with the content
//! instead of sitting behind it.

use eframe::egui;

use crate::theme::Fluent;

/// The grid the field is computed onto. Wide enough to hold several separate
/// pools of light, small enough that every one of them is spread over hundreds
/// of screen pixels and cannot resolve into a shape.
const FIELD_W: usize = 13;
const FIELD_H: usize = 8;

/// How far the brightest part of the field rises above the base, in 0-255
/// steps, and how far the darkest falls below it.
const LIFT: f32 = 20.0;
const FALL: f32 = 6.0;

/// How much of the accent color the lit parts take on. Mica picks up a cast
/// from whatever is behind it; this picks one from the interface's own blue,
/// which keeps the surface from reading as dead gray.
const TINT: f32 = 0.07;

/// One pool of light in the field: where it is, how wide, how strong.
///
/// Positions are fractions of the window, so the field stretches with it rather
/// than sliding about. Deliberately off-center and of different sizes — evenly
/// spaced lights of equal strength average out into the flat wash this exists
/// to avoid.
struct Pool {
    x: f32,
    y: f32,
    radius: f32,
    strength: f32,
}

const POOLS: [Pool; 4] = [
    // The main one, over the shoulder from the top left, where a window's
    // light conventionally comes from.
    Pool { x: 0.12, y: 0.06, radius: 0.62, strength: 1.00 },
    // A weaker one across the top right, so the upper edge is not simply a
    // ramp down from the corner.
    Pool { x: 0.88, y: 0.18, radius: 0.45, strength: 0.42 },
    // Low and central, lifting the bottom of the window slightly away from
    // the vignette the other two would otherwise leave there.
    Pool { x: 0.58, y: 0.92, radius: 0.50, strength: 0.30 },
    // A small, close one, purely to break the symmetry of the other three.
    Pool { x: 0.34, y: 0.44, radius: 0.28, strength: 0.22 },
];

pub struct Backdrop {
    field: egui::TextureHandle,
}

impl Backdrop {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            // Built once and never replaced. Nothing about it depends on
            // anything that changes, which is also what keeps it away from the
            // hazard of freeing a texture the last frame is still drawing with.
            field: ctx.load_texture("backdrop", field(), egui::TextureOptions::LINEAR),
        }
    }

    /// Fill `rect` with the material, behind everything else.
    ///
    /// Called before anything else paints, so every translucent surface in the
    /// theme has something to let through — which is what those surfaces were
    /// designed around when the material came from the system.
    pub fn paint(&self, ctx: &egui::Context, rect: egui::Rect) {
        ctx.layer_painter(egui::LayerId::background()).image(
            self.field.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// The pools of light, resolved onto the grid.
fn field() -> egui::ColorImage {
    let base = Fluent::SOLID;
    let accent = Fluent::ACCENT;

    let mut pixels = Vec::with_capacity(FIELD_W * FIELD_H);
    for row in 0..FIELD_H {
        for column in 0..FIELD_W {
            // Cell centers rather than corners, so the outermost cells sit
            // inside the window and the light does not appear to start off the
            // edge of it.
            let x = (column as f32 + 0.5) / FIELD_W as f32;
            let y = (row as f32 + 0.5) / FIELD_H as f32;

            // Every pool contributes, falling off smoothly with distance. A
            // squared falloff rather than a true gaussian: the difference is
            // invisible once this is spread across a window, and it is a
            // multiply instead of an exp.
            let mut light = 0.0;
            for pool in &POOLS {
                let (dx, dy) = (x - pool.x, y - pool.y);
                let distance = (dx * dx + dy * dy).sqrt() / pool.radius;
                if distance < 1.0 {
                    let falloff = 1.0 - distance * distance;
                    light += pool.strength * falloff * falloff;
                }
            }
            let light = light.min(1.0);

            let channel = |base: u8, accent: u8| {
                let lifted = base as f32 + LIFT * light - FALL * (1.0 - light);
                // The cast follows the light: lit areas pick it up, the
                // shadowed ones stay neutral.
                let tinted = lifted + (accent as f32 - lifted) * TINT * light;
                tinted.clamp(0.0, 255.0) as u8
            };
            pixels.push(egui::Color32::from_rgb(
                channel(base.r(), accent.r()),
                channel(base.g(), accent.g()),
                channel(base.b(), accent.b()),
            ));
        }
    }

    egui::ColorImage {
        size: [FIELD_W, FIELD_H],
        pixels,
    }
}

