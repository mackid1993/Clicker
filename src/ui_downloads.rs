//! The downloads screen.
//!
//! Downloads are started from a recording in the library, but they outlive the
//! row that started them and there is no reason anyone should have to find that
//! row again to see how one is getting on, or to change their mind. This is
//! that list: what is running, what is waiting, what finished, and a way to
//! stop or delete any of it.

use crate::downloads::{Downloads, Status};
use crate::library::Home;
use crate::theme::{self, Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S};

/// Everything here is local-only. Nothing on this screen can change what the
/// DVR holds: removing a download deletes a file from this machine, and the
/// recording remains on the server.
pub enum DownloadAction {
    None,
    /// Play a finished download, from the local file.
    Play(String),
    /// Stop a transfer, keeping what has arrived so it can be picked up again.
    Pause(String),
    /// Start one, or continue one that was stopped.
    Resume(String),
    /// Abandon a running download, or delete a finished one's local file.
    Remove(String),
    /// Delete the local files of everything that has finished or failed.
    ClearFinished,
}

pub fn downloads_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    images: &mut crate::images::Images,
    downloads: &Downloads,
) -> DownloadAction {
    let mut action = DownloadAction::None;

    let entries = downloads.entries();
    if entries.is_empty() {
        crate::ui::centered_message(
            ui,
            rect,
            "No downloads",
            "Download a recording from the library and it will appear here",
        );
        return action;
    }

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(SPACE_L * 3.0, SPACE_L)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut content, |ui| {
            ui.add_space(SPACE_L);

            let running = entries
                .iter()
                .filter(|(_, s)| matches!(s, Status::Active(_)))
                .count();
            let waiting = entries
                .iter()
                .filter(|(_, s)| matches!(s, Status::Queued))
                .count();
            let finished = entries.iter().filter(|(_, s)| s.is_finished()).count();

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Downloads")
                        .size(24.0)
                        .color(Fluent::TEXT_PRIMARY),
                );
                if finished > 0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Clear finished")
                            .on_hover_text(
                                "Deletes the local copies. The recordings stay on the DVR.",
                            )
                            .clicked()
                        {
                            action = DownloadAction::ClearFinished;
                        }
                    });
                }
            });

            // Two at a time is the limit, so saying how many are waiting is the
            // difference between "nothing is happening" and "your turn is
            // coming".
            let summary = match (running, waiting) {
                (0, 0) => format!("{finished} finished"),
                (r, 0) => format!("{r} downloading"),
                (r, w) => format!("{r} downloading, {w} waiting"),
            };
            ui.label(
                egui::RichText::new(summary)
                    .size(12.0)
                    .color(Fluent::TEXT_TERTIARY),
            );
            ui.add_space(SPACE_L);

            for (id, status) in &entries {
                if let Some(a) = row(ui, id, status, data, images) {
                    action = a;
                }
                ui.add_space(SPACE_S);
            }

            ui.add_space(SPACE_L * 2.0);
        });

    action
}

fn row(
    ui: &mut egui::Ui,
    id: &str,
    status: &Status,
    data: &Home,
    images: &mut crate::images::Images,
) -> Option<DownloadAction> {
    let mut action = None;

    // Everything shown comes from the library's copy of the recording. A
    // download whose recording has since been deleted from the server still
    // has to say something, and its id is the only thing left that is
    // certainly true.
    let recording = data.all.iter().find(|r| r.id == *id);

    let title = recording
        .map(|r| r.title.clone())
        .unwrap_or_else(|| format!("Recording {id}"));

    // Season, episode, episode title, length, air date — whichever of them the
    // DVR actually knows. Joined rather than laid out in fixed columns because
    // a film has none of them and a news bulletin has half.
    let mut facts: Vec<String> = Vec::new();
    if let Some(r) = recording {
        if r.season_number > 0 && r.episode_number > 0 {
            facts.push(format!("S{}E{}", r.season_number, r.episode_number));
        }
        if !r.episode_title.is_empty() {
            facts.push(r.episode_title.clone());
        }
        if r.duration > 0.0 {
            facts.push(format!("{} min", (r.duration / 60.0).round() as i64));
        }
        // Just the year: the full ISO date is noise at this size, and the year
        // is the part that tells you whether this is a repeat.
        if r.original_air_date.len() >= 4 {
            facts.push(r.original_air_date[..4].to_string());
        }
    }
    let episode = facts.join("  ·  ");

    let art = recording
        .map(|r| r.art().to_string())
        .filter(|url| !url.is_empty())
        .and_then(|url| images.get(&url).cloned());

    let width = ui.available_width().min(720.0);
    // A finished download is something you can watch, so the row is the button.
    // One still arriving is not, and lighting up under the pointer would only
    // invite a click that does nothing.
    let ready = matches!(status, Status::Done(_));
    let sense = if ready { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, row_response) = ui.allocate_exact_size(egui::vec2(width, 96.0), sense);

    let hover = ui.ctx().animate_bool_with_time(
        egui::Id::new(("dl-row", id)),
        ready && row_response.hovered(),
        theme::ANIM_FAST,
    );
    ui.painter().rect_filled(
        rect,
        RADIUS_SURFACE,
        theme::mix(Fluent::LAYER_CARD, Fluent::CONTROL_HOVER, hover),
    );
    ui.painter().rect_stroke(
        rect,
        RADIUS_SURFACE,
        egui::Stroke::new(1.0, Fluent::STROKE_SURFACE),
    );

    let thumb = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + SPACE_S, rect.min.y + SPACE_S),
        egui::vec2(128.0, rect.height() - SPACE_S * 2.0),
    );
    ui.painter()
        .rect_filled(thumb, theme::RADIUS_CONTROL, with_alpha(Fluent::CONTROL, 90));
    if let Some(texture) = art {
        theme::image_cover(ui.painter(), thumb, theme::RADIUS_CONTROL, &texture, 0.5);
    }

    // A play glyph over the artwork, so "this one is watchable" is visible
    // without hovering.
    if ready {
        ui.painter().circle_filled(
            thumb.center(),
            15.0,
            with_alpha(egui::Color32::BLACK, (110.0 + hover * 90.0) as u8),
        );
        ui.painter().text(
            thumb.center(),
            egui::Align2::CENTER_CENTER,
            "\u{E768}", // Play
            egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            Fluent::TEXT_PRIMARY,
        );
        if row_response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if row_response.clicked() {
            action = Some(DownloadAction::Play(id.to_string()));
        }
    }

    // Four lines, top down: what it is, which episode, what happens in it, and
    // how the download is getting on. Laid out from the top rather than around
    // the centre so a row with no summary does not shuffle the other three.
    let text_x = thumb.max.x + SPACE_M;
    let text_w = rect.max.x - text_x - 52.0;
    let mut line_y = rect.min.y + 12.0;

    ui.painter().text(
        egui::pos2(text_x, line_y),
        egui::Align2::LEFT_TOP,
        elide(&title, text_w),
        egui::FontId::proportional(13.5),
        Fluent::TEXT_PRIMARY,
    );
    line_y += 20.0;

    if !episode.is_empty() {
        ui.painter().text(
            egui::pos2(text_x, line_y),
            egui::Align2::LEFT_TOP,
            elide(&episode, text_w),
            egui::FontId::proportional(11.5),
            Fluent::TEXT_SECONDARY,
        );
        line_y += 17.0;
    }

    let summary = recording.map(|r| r.summary.as_str()).unwrap_or_default();
    if !summary.is_empty() {
        ui.painter().text(
            egui::pos2(text_x, line_y),
            egui::Align2::LEFT_TOP,
            elide(summary, text_w),
            egui::FontId::proportional(11.0),
            Fluent::TEXT_TERTIARY,
        );
        line_y += 17.0;
    }

    let (detail, tint) = match status {
        Status::Queued => ("Waiting".to_string(), Fluent::TEXT_TERTIARY),
        Status::Active(fraction) => {
            let text = if *fraction >= 0.0 {
                format!("{:.0}%", fraction * 100.0)
            } else {
                "Starting…".to_string()
            };
            (text, Fluent::ACCENT)
        }
        // A negative fraction is one recovered from disk at startup: the bytes
        // are there but nothing has asked the server how big the whole file is
        // since the process restarted, so claiming a percentage would be
        // inventing one.
        Status::Paused(fraction) => {
            let text = if *fraction >= 0.0 {
                format!("Paused at {:.0}%", fraction * 100.0)
            } else {
                "Paused — resumes where it stopped".to_string()
            };
            (text, Fluent::CAUTION)
        }
        Status::Done(_) => ("Downloaded".to_string(), Fluent::SUCCESS),
        Status::Failed(e) => (format!("{e} — resumes where it stopped"), Fluent::LIVE),
    };
    ui.painter().text(
        egui::pos2(text_x, line_y),
        egui::Align2::LEFT_TOP,
        elide(&detail, text_w),
        egui::FontId::proportional(11.5),
        tint,
    );

    // A progress bar only for the one thing that has progress to report.
    if let Status::Active(fraction) = status {
        if *fraction >= 0.0 {
            let track = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.max.y - 3.0),
                egui::vec2(rect.width(), 3.0),
            );
            ui.painter()
                .rect_filled(track, 0.0, with_alpha(Fluent::CONTROL, 120));
            let filled = egui::Rect::from_min_size(
                track.min,
                egui::vec2(track.width() * fraction.clamp(0.0, 1.0), track.height()),
            );
            ui.painter().rect_filled(filled, 0.0, Fluent::ACCENT);
        }
    }

    // Pause or resume, for anything still on its way. Separate from remove,
    // because these two are opposites and one deletes: stopping for now and
    // giving up entirely must not be a single ambiguous button.
    let stoppable = matches!(status, Status::Active(_) | Status::Queued);
    if stoppable || status.is_resumable() {
        let toggle = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 64.0, rect.center().y),
            egui::vec2(34.0, 34.0),
        );
        let response = ui.interact(toggle, egui::Id::new(("dl-hold", id)), egui::Sense::click());
        let hover = ui.ctx().animate_bool_with_time(
            egui::Id::new(("dl-hold-h", id)),
            response.hovered(),
            theme::ANIM_FAST,
        );
        if hover > 0.0 {
            ui.painter().rect_filled(
                toggle,
                theme::RADIUS_CONTROL,
                with_alpha(Fluent::CONTROL_HOVER, (hover * 200.0) as u8),
            );
        }
        ui.painter().text(
            toggle.center(),
            egui::Align2::CENTER_CENTER,
            if stoppable { "\u{E769}" } else { "\u{E768}" }, // Pause / Play
            egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            Fluent::TEXT_SECONDARY,
        );
        let response = response.on_hover_text(if stoppable {
            "Pause — what has arrived is kept"
        } else {
            "Resume where it stopped"
        });
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            action = Some(if stoppable {
                DownloadAction::Pause(id.to_string())
            } else {
                DownloadAction::Resume(id.to_string())
            });
        }
    }

    // Remove: abandons a transfer, or deletes a finished file.
    let button = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 26.0, rect.center().y),
        egui::vec2(34.0, 34.0),
    );
    let response = ui.interact(button, egui::Id::new(("dl-remove", id)), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        egui::Id::new(("dl-remove-h", id)),
        response.hovered(),
        theme::ANIM_FAST,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            button,
            theme::RADIUS_CONTROL,
            with_alpha(Fluent::CONTROL_HOVER, (hover * 200.0) as u8),
        );
    }
    ui.painter().text(
        button.center(),
        egui::Align2::CENTER_CENTER,
        if status.is_finished() {
            "\u{E74D}" // Delete
        } else {
            "\u{E711}" // Cancel
        },
        egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
        if response.hovered() {
            Fluent::LIVE
        } else {
            Fluent::TEXT_SECONDARY
        },
    );
    // Says "local copy" deliberately. This button deletes a file off this
    // machine and never touches the DVR — the recording stays exactly where it
    // is, and can be downloaded again. A delete button next to a recording's
    // title that might mean either would be a genuinely dangerous ambiguity.
    let response = response.on_hover_text(if status.is_finished() {
        "Delete the local copy — the recording stays on the DVR"
    } else {
        "Stop this download"
    });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        action = Some(DownloadAction::Remove(id.to_string()));
    }

    action
}

/// Roughly six pixels a character at these sizes. Measuring properly would mean
/// laying every row's text out twice.
fn elide(text: &str, width: f32) -> String {
    let max = (width / 6.2).max(3.0) as usize;
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}
