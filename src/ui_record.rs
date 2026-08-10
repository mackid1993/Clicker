// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! The record dialog: padding, and season pass options.
//!
//! Channels does not pad a manually created job the way it pads one created by
//! a rule, so padding chosen here has to be folded into the job's own start and
//! duration before it is sent. That is invisible to the person setting it, who
//! simply expects "start two minutes early" to mean what it says — which is
//! exactly why the resulting window is shown as it is adjusted, rather than
//! leaving them to trust that it worked.

use eframe::egui;

use crate::guide::Airing;
use crate::theme::{Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S};

/// What the dialog decided.
pub enum RecordChoice {
    /// Still open.
    Pending,
    Canceled,
    /// Record this one airing, with these paddings in seconds.
    Once { start_pad: i64, end_pad: i64 },
    /// Record the series.
    Series {
        start_pad: i64,
        end_pad: i64,
        new_only: bool,
        keep: i64,
    },
}

pub struct RecordDialog {
    pub airing: Airing,
    pub start_pad_min: i64,
    pub end_pad_min: i64,
    pub new_only: bool,
    /// 0 means keep everything.
    pub keep: i64,
}

impl RecordDialog {
    pub fn new(airing: Airing, default_start: i64, default_end: i64) -> Self {
        Self {
            airing,
            start_pad_min: default_start / 60,
            end_pad_min: default_end / 60,
            new_only: true,
            keep: 0,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> RecordChoice {
        let mut choice = RecordChoice::Pending;
        let mut open = true;

        egui::Window::new("Record")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(&self.airing.title)
                        .size(17.0)
                        .color(Fluent::TEXT_PRIMARY),
                );
                let subtitle = self.airing.subtitle();
                if !subtitle.is_empty() {
                    ui.label(
                        egui::RichText::new(subtitle)
                            .size(12.0)
                            .color(Fluent::TEXT_SECONDARY),
                    );
                }
                ui.add_space(SPACE_M);
                ui.separator();
                ui.add_space(SPACE_M);

                ui.label(
                    egui::RichText::new("PADDING")
                        .size(11.0)
                        .color(Fluent::TEXT_TERTIARY),
                );
                ui.add_space(SPACE_S);

                padding_row(ui, "Start early", &mut self.start_pad_min);
                padding_row(ui, "End late", &mut self.end_pad_min);

                ui.add_space(SPACE_M);

                // The actual window that will be recorded, computed live. A
                // padding field with no visible consequence is a field people
                // set once and never trust again.
                let start = self.airing.start - self.start_pad_min * 60;
                let end = self.airing.end() + self.end_pad_min * 60;
                let minutes = (end - start) / 60;
                ui.label(
                    egui::RichText::new(format!(
                        "Records {} to {}  ·  {} minutes",
                        crate::ui_guide::time_only(start),
                        crate::ui_guide::time_only(end),
                        minutes
                    ))
                    .size(12.0)
                    .color(Fluent::ACCENT_LIGHT),
                );

                if !self.airing.series_id.is_empty() {
                    ui.add_space(SPACE_L);
                    ui.label(
                        egui::RichText::new("IF RECORDING THE SERIES")
                            .size(11.0)
                            .color(Fluent::TEXT_TERTIARY),
                    );
                    ui.add_space(SPACE_S);
                    ui.checkbox(&mut self.new_only, "New episodes only");
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Keep")
                                .size(12.0)
                                .color(Fluent::TEXT_SECONDARY),
                        );
                        for (label, value) in
                            [("All", 0i64), ("3", 3), ("5", 5), ("10", 10), ("25", 25)]
                        {
                            if ui.selectable_label(self.keep == value, label).clicked() {
                                self.keep = value;
                            }
                        }
                    });
                    if self.keep > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "Older episodes are deleted once there are more than {}.",
                                self.keep
                            ))
                            .size(11.0)
                            .color(Fluent::CAUTION),
                        );
                    }
                }

                ui.add_space(SPACE_L);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Record this episode").min_size(egui::vec2(160.0, 32.0)))
                        .clicked()
                    {
                        choice = RecordChoice::Once {
                            start_pad: self.start_pad_min * 60,
                            end_pad: self.end_pad_min * 60,
                        };
                    }
                    if !self.airing.series_id.is_empty()
                        && ui
                            .add(egui::Button::new("Record the series").min_size(egui::vec2(150.0, 32.0)))
                            .clicked()
                    {
                        choice = RecordChoice::Series {
                            start_pad: self.start_pad_min * 60,
                            end_pad: self.end_pad_min * 60,
                            new_only: self.new_only,
                            keep: self.keep,
                        };
                    }
                });
                ui.add_space(SPACE_S);
            });

        if !open {
            return RecordChoice::Canceled;
        }
        choice
    }
}

/// Minutes, with steppers. Negative start padding is allowed deliberately:
/// starting a minute *late* is how someone skips a station ident they never
/// want, and forbidding it would be arbitrary.
fn padding_row(ui: &mut egui::Ui, label: &str, minutes: &mut i64) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [90.0, 22.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .size(12.0)
                    .color(Fluent::TEXT_SECONDARY),
            ),
        );
        if ui.small_button("−").clicked() {
            *minutes -= 1;
        }
        ui.add_sized(
            [52.0, 22.0],
            egui::Label::new(
                egui::RichText::new(format!("{minutes} min"))
                    .size(13.0)
                    .color(Fluent::TEXT_PRIMARY),
            ),
        );
        if ui.small_button("+").clicked() {
            *minutes += 1;
        }
        ui.add_space(SPACE_S);
        for preset in [0i64, 1, 2, 5] {
            if ui.selectable_label(*minutes == preset, format!("{preset}")).clicked() {
                *minutes = preset;
            }
        }
    });
    // Padding beyond an hour is almost certainly a mis-click on the stepper.
    *minutes = (*minutes).clamp(-30, 60);
    let _ = RADIUS_SURFACE;
}
