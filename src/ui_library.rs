//! The Library and Recordings screens.
//!
//! Library is what you own, arranged by series. Recordings is what the DVR is
//! going to do next, plus everything it has already done. They are separate
//! screens because they answer different questions: "what can I watch" and
//! "what is my DVR doing".

use eframe::egui;

use crate::images::Images;
use crate::library::{Home, Recording, Upcoming};
use crate::theme::{self, Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S, SPACE_XS};
use crate::ui::Action;

const POSTER_W: f32 = 168.0;
const POSTER_H: f32 = 236.0;

/// Which part of the library is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryTab {
    #[default]
    Shows,
    Movies,
}

#[derive(Default)]
pub struct LibraryState {
    /// The series being looked inside, if any.
    pub open_show: Option<String>,
    pub search: String,
    pub tab: LibraryTab,
}

/// The library: everything playable, recorded and imported alike, by series.
pub fn library_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    state: &mut LibraryState,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
    loading: bool,
) -> Action {
    let mut action = Action::None;

    if loading && data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Loading your library", "Reading recordings from the DVR");
        return action;
    }
    if data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Nothing recorded yet", "Anything you record will appear here");
        return action;
    }

    // Inside a series: the episode list.
    if let Some(show_id) = state.open_show.clone() {
        return show_detail(ui, rect, data, state, images, downloads, &show_id);
    }

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut content, |ui| {
            ui.add_space(SPACE_L);
            ui.horizontal(|ui| {
                ui.add_space(SPACE_L * 2.0);
                ui.label(
                    egui::RichText::new("Library")
                        .size(24.0)
                        .color(Fluent::TEXT_PRIMARY),
                );
                ui.add_space(SPACE_L);
                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text("Search")
                        .desired_width(260.0),
                );
            });
            ui.add_space(SPACE_S);

            // Movies are a flat list of individual films; TV is a grid of
            // series to open. Two different shapes, so two tabs rather than
            // one grid pretending both are the same thing.
            let movies: Vec<&Recording> =
                data.all.iter().filter(|r| r.is_movie()).collect();
            ui.horizontal(|ui| {
                ui.add_space(SPACE_L * 2.0);
                for (tab, label, count) in [
                    (LibraryTab::Shows, "TV", data.groups.len()),
                    (LibraryTab::Movies, "Movies", movies.len()),
                ] {
                    let selected = state.tab == tab;
                    if ui
                        .selectable_label(selected, format!("{label}  {count}"))
                        .clicked()
                    {
                        state.tab = tab;
                        state.open_show = None;
                    }
                }
            });
            ui.add_space(SPACE_M);

            let needle = state.search.trim().to_lowercase();

            if state.tab == LibraryTab::Movies {
                let matching: Vec<&Recording> = movies
                    .into_iter()
                    .filter(|m| needle.is_empty() || m.title.to_lowercase().contains(&needle))
                    .collect();
                if matching.is_empty() {
                    ui.add_space(SPACE_L);
                    empty_note(ui, "No movies match that");
                    return;
                }
                let available = ui.available_width() - SPACE_L * 4.0;
                let per_row =
                    ((available + SPACE_M) / (POSTER_W + SPACE_M)).floor().max(1.0) as usize;
                for chunk in matching.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.add_space(SPACE_L * 2.0);
                        for movie in chunk {
                            let year = movie
                                .original_air_date
                                .split('-')
                                .next()
                                .unwrap_or("")
                                .to_string();
                            if poster(ui, movie.art(), &movie.title, &year, 0, images) {
                                action = Action::Play((*movie).clone());
                            }
                            ui.add_space(SPACE_M);
                        }
                    });
                    ui.add_space(SPACE_M);
                }
                ui.add_space(SPACE_L * 2.0);
                return;
            }

            let shows: Vec<_> = data
                .groups
                .iter()
                .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
                .collect();

            if shows.is_empty() {
                ui.add_space(SPACE_L);
                empty_note(ui, "No series match that");
                return;
            }

            // A wrapping grid of posters, sized to whatever width there is.
            let available = ui.available_width() - SPACE_L * 4.0;
            let per_row = ((available + SPACE_M) / (POSTER_W + SPACE_M)).floor().max(1.0) as usize;

            for chunk in shows.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.add_space(SPACE_L * 2.0);
                    for group in chunk {
                        let episodes = data
                            .all
                            .iter()
                            .filter(|r| r.show_id == group.id || r.show_id == group.series_id)
                            .count();
                        if poster(
                            ui,
                            &group.image,
                            &group.name,
                            &format!(
                                "{episodes} recording{}",
                                if episodes == 1 { "" } else { "s" }
                            ),
                            group.unwatched,
                            images,
                        ) {
                            state.open_show = Some(if data
                                .all
                                .iter()
                                .any(|r| r.show_id == group.id)
                            {
                                group.id.clone()
                            } else {
                                group.series_id.clone()
                            });
                        }
                        ui.add_space(SPACE_M);
                    }
                });
                ui.add_space(SPACE_M);
            }

            ui.add_space(SPACE_L * 2.0);
        });

    action
}

/// Inside one series.
fn show_detail(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    state: &mut LibraryState,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
    show_id: &str,
) -> Action {
    let mut action = Action::None;
    let episodes = data.episodes_of(show_id);
    let title = episodes
        .first()
        .map(|r| r.title.clone())
        .unwrap_or_else(|| "Series".into());

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut content, |ui| {
            ui.add_space(SPACE_L);
            ui.horizontal(|ui| {
                ui.add_space(SPACE_L * 2.0);
                if ui.button("←  Library").clicked() {
                    state.open_show = None;
                }
                ui.add_space(SPACE_M);
                ui.label(
                    egui::RichText::new(&title)
                        .size(22.0)
                        .color(Fluent::TEXT_PRIMARY),
                );
            });
            ui.add_space(SPACE_M);

            for episode in &episodes {
                if let Some(a) = episode_row(ui, episode, images, downloads) {
                    action = a;
                }
            }
            ui.add_space(SPACE_L * 2.0);
        });

    action
}

/// One episode, as a wide row: thumbnail, title, summary, progress, and the
/// download state at the right edge.
fn episode_row(
    ui: &mut egui::Ui,
    item: &Recording,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
) -> Option<Action> {
    let mut action = None;
    let width = (ui.available_width() - SPACE_L * 4.0).min(820.0);
    let height = 96.0;

    ui.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

        if response.hovered() {
            ui.painter()
                .rect_filled(rect, theme::RADIUS_CONTROL, Fluent::CONTROL_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let art = egui::Rect::from_min_size(
            rect.min + egui::vec2(SPACE_S, SPACE_S),
            egui::vec2(140.0, height - SPACE_S * 2.0),
        );
        let url = item.art().to_string();
        if let Some(texture) = images.get(&url) {
            let size = texture.size_vec2();
            let scale = (art.width() / size.x).max(art.height() / size.y);
            let scaled = size * scale;
            let uv = egui::Rect::from_min_size(
                egui::pos2(
                    (1.0 - (art.width() / scaled.x).min(1.0)) * 0.5,
                    (1.0 - (art.height() / scaled.y).min(1.0)) * 0.5,
                ),
                egui::vec2(
                    (art.width() / scaled.x).min(1.0),
                    (art.height() / scaled.y).min(1.0),
                ),
            );
            ui.painter()
                .with_clip_rect(art)
                .image(texture.id(), art, uv, egui::Color32::WHITE);
        } else {
            ui.painter().rect_filled(art, 4.0, Fluent::LAYER_CARD);
        }

        if item.progress() > 0.0 {
            let track = egui::Rect::from_min_size(
                egui::pos2(art.min.x, art.max.y - 3.0),
                egui::vec2(art.width(), 3.0),
            );
            ui.painter()
                .rect_filled(track, 0.0, egui::Color32::from_black_alpha(150));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * item.progress(), 3.0),
                ),
                0.0,
                Fluent::ACCENT,
            );
        }

        let text_x = art.max.x + SPACE_M;
        let mut y = rect.min.y + SPACE_M;

        let heading = {
            let label = item.episode_label();
            if label.is_empty() {
                item.title.clone()
            } else if item.episode_title.is_empty() {
                label
            } else {
                format!("{label}  ·  {}", item.episode_title)
            }
        };
        ui.painter().text(
            egui::pos2(text_x, y),
            egui::Align2::LEFT_TOP,
            heading,
            egui::FontId::proportional(14.0),
            Fluent::TEXT_PRIMARY,
        );
        y += 22.0;

        if !item.summary.is_empty() {
            ui.painter().text(
                egui::pos2(text_x, y),
                egui::Align2::LEFT_TOP,
                elide(&item.summary, rect.max.x - text_x - SPACE_M),
                egui::FontId::proportional(11.0),
                Fluent::TEXT_SECONDARY,
            );
            y += 20.0;
        }

        let status = if item.watched {
            "Watched".to_string()
        } else if item.progress() > 0.0 {
            item.remaining()
        } else {
            format!("{:.0} min", item.duration / 60.0)
        };
        ui.painter().text(
            egui::pos2(text_x, y),
            egui::Align2::LEFT_TOP,
            status,
            egui::FontId::proportional(11.0),
            if item.watched { Fluent::TEXT_TERTIARY } else { Fluent::ACCENT_LIGHT },
        );

        // Download state, at the right edge where every row keeps it. This is
        // the laptop-first feature: one press copies the recording to local
        // disk so it plays on a plane exactly as it does at home.
        let corner = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 30.0, rect.center().y),
            egui::vec2(34.0, 34.0),
        );
        match downloads.status(&item.id) {
            Some(crate::downloads::Status::Done(_)) => {
                let done = ui.interact(
                    corner,
                    egui::Id::new(("dl", &item.id)),
                    egui::Sense::click(),
                );
                ui.painter().text(
                    corner.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{E73E}", // CheckMark
                    egui::FontId::new(14.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
                    Fluent::SUCCESS,
                );
                let done = done.on_hover_text("Downloaded — click to remove the local copy");
                if done.clicked() {
                    action = Some(Action::RemoveDownload(item.id.clone()));
                }
            }
            // Two run at a time; the rest wait their turn. Both states are
            // clickable, because changing your mind about a download you have
            // only just started is the commonest reason to touch it at all.
            Some(crate::downloads::Status::Queued) => {
                let queued = ui.interact(
                    corner,
                    egui::Id::new(("dl", &item.id)),
                    egui::Sense::click(),
                );
                ui.painter().text(
                    corner.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{E712}", // More — waiting for a slot
                    egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
                    Fluent::TEXT_TERTIARY,
                );
                if queued.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if queued
                    .on_hover_text("Waiting to download — click to cancel")
                    .clicked()
                {
                    action = Some(Action::RemoveDownload(item.id.clone()));
                }
            }
            Some(crate::downloads::Status::Active(fraction)) => {
                let running = ui.interact(
                    corner,
                    egui::Id::new(("dl", &item.id)),
                    egui::Sense::click(),
                );
                let label = if fraction >= 0.0 {
                    format!("{:.0}%", fraction * 100.0)
                } else {
                    "…".to_string()
                };
                ui.painter().text(
                    corner.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(11.0),
                    Fluent::ACCENT_LIGHT,
                );
                if running.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if running
                    .on_hover_text("Downloading — click to cancel")
                    .clicked()
                {
                    action = Some(Action::RemoveDownload(item.id.clone()));
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            }
            // Paused looks like "not downloaded" here on purpose: the button
            // starts it, and `start` continues from what is on disk rather
            // than beginning again. The Downloads screen is where a paused
            // transfer is managed as one.
            Some(crate::downloads::Status::Paused(_))
            | Some(crate::downloads::Status::Failed(_))
            | None => {
                let dl = ui.interact(
                    corner,
                    egui::Id::new(("dl", &item.id)),
                    egui::Sense::click(),
                );
                if dl.hovered() {
                    ui.painter()
                        .rect_filled(corner, theme::RADIUS_CONTROL, Fluent::CONTROL_HOVER);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                ui.painter().text(
                    corner.center(),
                    egui::Align2::CENTER_CENTER,
                    "\u{E896}", // Download
                    egui::FontId::new(14.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
                    Fluent::TEXT_SECONDARY,
                );
                let dl = dl.on_hover_text("Download for offline");
                if dl.clicked() {
                    action = Some(Action::Download(item.clone()));
                }
            }
        }

        // The row itself plays, unless the download corner took the click.
        if response.clicked() && action.is_none() {
            action = Some(Action::Play(item.clone()));
        }

        // Right click for everything else. Keeping delete behind a menu, and
        // then behind a confirmation, is deliberate: it is the one action here
        // that cannot be undone.
        response.context_menu(|ui| {
            ui.set_min_width(190.0);
            if ui.button("▶  Play").clicked() {
                action = Some(Action::Play(item.clone()));
                ui.close_menu();
            }
            let label = if item.watched {
                "Mark unwatched"
            } else {
                "Mark watched"
            };
            if ui.button(label).clicked() {
                action = Some(Action::SetWatched(item.id.clone(), !item.watched));
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("Delete recording…").color(Fluent::LIVE))
                .clicked()
            {
                action = Some(Action::Delete(item.clone()));
                ui.close_menu();
            }
        });
    });
    ui.add_space(SPACE_S);

    action
}

fn poster(
    ui: &mut egui::Ui,
    url: &str,
    title: &str,
    detail: &str,
    unwatched: u32,
    images: &mut Images,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(POSTER_W, POSTER_H + 44.0),
        egui::Sense::click(),
    );
    let art = egui::Rect::from_min_size(rect.min, egui::vec2(POSTER_W, POSTER_H));

    let owned = url.to_string();
    if let Some(texture) = images.get(&owned) {
        let size = texture.size_vec2();
        let scale = (art.width() / size.x).max(art.height() / size.y);
        let scaled = size * scale;
        let uv = egui::Rect::from_min_size(
            egui::pos2(
                (1.0 - (art.width() / scaled.x).min(1.0)) * 0.5,
                (1.0 - (art.height() / scaled.y).min(1.0)) * 0.5,
            ),
            egui::vec2(
                (art.width() / scaled.x).min(1.0),
                (art.height() / scaled.y).min(1.0),
            ),
        );
        ui.painter()
            .with_clip_rect(art)
            .image(texture.id(), art, uv, egui::Color32::WHITE);
    } else {
        ui.painter().rect_filled(art, RADIUS_SURFACE, Fluent::LAYER_CARD);
    }

    ui.painter().rect_stroke(
        art,
        RADIUS_SURFACE,
        egui::Stroke::new(
            1.0,
            if response.hovered() { Fluent::STROKE_CONTROL } else { Fluent::STROKE_SURFACE },
        ),
    );

    // How many are still to watch, as a badge. The count is the reason to open
    // a series, so it belongs on the tile rather than inside it.
    if unwatched > 0 {
        let badge = egui::Rect::from_min_size(
            egui::pos2(art.max.x - 34.0, art.min.y + 8.0),
            egui::vec2(26.0, 20.0),
        );
        ui.painter().rect_filled(badge, 10.0, Fluent::ACCENT);
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            unwatched.to_string(),
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(12, 14, 18),
        );
    }

    if response.hovered() {
        ui.painter()
            .rect_filled(art, RADIUS_SURFACE, egui::Color32::from_black_alpha(70));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    ui.painter().text(
        egui::pos2(rect.min.x, art.max.y + SPACE_S),
        egui::Align2::LEFT_TOP,
        elide(title, POSTER_W),
        egui::FontId::proportional(13.0),
        Fluent::TEXT_PRIMARY,
    );
    ui.painter().text(
        egui::pos2(rect.min.x, art.max.y + SPACE_S + 19.0),
        egui::Align2::LEFT_TOP,
        detail,
        egui::FontId::proportional(11.0),
        Fluent::TEXT_TERTIARY,
    );

    response.clicked()
}

/// What the Recordings screen is asking for.
pub enum RecordingsAction {
    None,
    /// Cancel a scheduled job.
    Cancel(String),
    /// Anything a recording row can ask for — play, download, delete and the
    /// rest. Forwarded whole rather than re-listed here, so adding a row
    /// action does not mean threading it through a second enum.
    Item(Action),
}

/// Which half of the Recordings screen is showing.
///
/// Two tabs rather than one long page. They answer different questions —
/// "what is my DVR about to do" and "what has it already got" — and stacking
/// them meant scrolling past fifty scheduled jobs to reach a recording.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingsTab {
    /// First, and the default. What has been recorded is what people come here
    /// to watch; what is scheduled is what they came here to check.
    #[default]
    Recorded,
    Scheduled,
}

/// A Fluent tab strip: label, count, and an accent underline on the selected
/// one. Returns true when the selection changed.
fn tab_strip(ui: &mut egui::Ui, current: &mut RecordingsTab, counts: (usize, usize)) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        for (tab, label, count) in [
            (RecordingsTab::Recorded, "Recorded", counts.1),
            (RecordingsTab::Scheduled, "Scheduled", counts.0),
        ] {
            let selected = *current == tab;
            let text = format!("{label}  {count}");
            let galley = ui.painter().layout_no_wrap(
                text.clone(),
                egui::FontId::proportional(15.0),
                Fluent::TEXT_PRIMARY,
            );
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(galley.size().x + SPACE_L, 40.0),
                egui::Sense::click(),
            );

            if response.hovered() && !selected {
                ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, Fluent::CONTROL_HOVER);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &text,
                egui::FontId::proportional(15.0),
                if selected { Fluent::TEXT_PRIMARY } else { Fluent::TEXT_TERTIARY },
            );

            if selected {
                // Fluent underlines the selected tab with a short accent bar
                // inset from the edges, not a full-width rule.
                let bar = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + SPACE_S, rect.max.y - 3.0),
                    egui::vec2(rect.width() - SPACE_S * 2.0, 3.0),
                );
                ui.painter().rect_filled(bar, 1.5, Fluent::ACCENT);
            }

            if response.clicked() && !selected {
                *current = tab;
                changed = true;
            }
        }
    });
    changed
}

/// Two tabs: what is going to be recorded, and what already has been.
pub fn recordings_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    tab: &mut RecordingsTab,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
    now: i64,
    loading: bool,
) -> RecordingsAction {
    let mut action = RecordingsAction::None;

    if loading && data.upcoming.is_empty() && data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Loading", "Reading the schedule from the DVR");
        return action;
    }

    // The header and tabs are fixed; only the list below them scrolls, so the
    // tabs cannot scroll out of reach.
    let mut header = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                rect.min,
                egui::vec2(rect.width(), 108.0),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    header.add_space(SPACE_L);
    header.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        ui.label(
            egui::RichText::new("Recordings")
                .size(24.0)
                .color(Fluent::TEXT_PRIMARY),
        );
    });
    header.add_space(SPACE_S);
    tab_strip(&mut header, tab, (data.upcoming.len(), data.recorded.len()));

    // A hairline under the strip, which is what makes it read as a tab bar
    // rather than as two words floating above a list.
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x + SPACE_L * 2.0, rect.min.y + 106.0),
            egui::pos2(rect.max.x - SPACE_L, rect.min.y + 106.0),
        ],
        egui::Stroke::new(1.0, Fluent::STROKE_DIVIDER),
    );

    let list = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + 112.0),
        rect.max,
    );
    let mut body = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .id_salt(match tab {
            RecordingsTab::Scheduled => "rec-scheduled",
            RecordingsTab::Recorded => "rec-recorded",
        })
        .auto_shrink([false, false])
        .show(&mut body, |ui| {
            ui.add_space(SPACE_M);
            match tab {
                RecordingsTab::Scheduled => {
                    if data.upcoming.is_empty() {
                        empty_note(ui, "Nothing scheduled. Record something from the guide.");
                    }
                    for job in &data.upcoming {
                        if let Some(a) = upcoming_row(ui, job, images, now) {
                            action = a;
                        }
                    }
                }
                RecordingsTab::Recorded => {
                    // Only what this DVR recorded off a tuner. Imported
                    // external media lives in the Library and does not belong
                    // here: on a real server that is 303 recordings against
                    // 7,233 imports, and mixing them makes this meaningless.
                    if data.recorded.is_empty() && !loading {
                        empty_note(ui, "Nothing recorded yet.");
                    }
                    for item in data.recorded.iter().take(300) {
                        if let Some(a) = episode_row(ui, item, images, downloads) {
                            action = RecordingsAction::Item(a);
                        }
                    }
                }
            }
            ui.add_space(SPACE_L * 2.0);
        });

    action
}

fn empty_note(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        ui.label(
            egui::RichText::new(text)
                .size(12.0)
                .color(Fluent::TEXT_TERTIARY),
        );
    });
}

fn upcoming_row(
    ui: &mut egui::Ui,
    job: &Upcoming,
    images: &mut Images,
    now: i64,
) -> Option<RecordingsAction> {
    let mut action = None;
    let width = (ui.available_width() - SPACE_L * 4.0).min(820.0);

    // Fetched before the painter borrows the ui; both need it.
    let art = if job.image.is_empty() {
        None
    } else {
        images.get(&job.image).cloned()
    };

    ui.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 60.0), egui::Sense::hover());

        let recording_now = job.start <= now && now < job.start + job.duration;
        ui.painter().rect_filled(
            rect,
            theme::RADIUS_CONTROL,
            if response.hovered() { Fluent::CONTROL_HOVER } else { Fluent::LAYER_CARD },
        );

        // A live red dot for something being recorded right now.
        let dot = egui::pos2(rect.min.x + SPACE_M + 4.0, rect.center().y);
        ui.painter().circle_filled(
            dot,
            4.0,
            if recording_now { Fluent::LIVE } else { Fluent::TEXT_TERTIARY },
        );

        // Artwork, the same as the library's rows carry. A schedule of nothing
        // but titles is the hardest kind of list to scan.
        let thumb = egui::Rect::from_min_size(
            egui::pos2(dot.x + SPACE_M, rect.min.y + SPACE_XS),
            egui::vec2(88.0, rect.height() - SPACE_XS * 2.0),
        );
        ui.painter()
            .rect_filled(thumb, theme::RADIUS_CONTROL, Fluent::CONTROL);
        if let Some(texture) = art {
            let size = texture.size_vec2();
            if size.x > 0.0 && size.y > 0.0 {
                // Cover, then clip: a poster letterboxed into a wide thumbnail
                // is mostly empty box.
                let scale = (thumb.width() / size.x).max(thumb.height() / size.y);
                let target = egui::Rect::from_center_size(thumb.center(), size * scale);
                ui.painter().with_clip_rect(thumb).image(
                    texture.id(),
                    target,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }

        let text_x = thumb.max.x + SPACE_M;
        ui.painter().text(
            egui::pos2(text_x, rect.min.y + 12.0),
            egui::Align2::LEFT_TOP,
            &job.title,
            egui::FontId::proportional(14.0),
            Fluent::TEXT_PRIMARY,
        );

        let when = if recording_now {
            "Recording now".to_string()
        } else {
            format!(
                "{}  ·  ch {}  ·  {} min",
                crate::ui_guide::when_label(job.start, now),
                job.channel,
                job.duration / 60
            )
        };
        ui.painter().text(
            egui::pos2(text_x, rect.min.y + 33.0),
            egui::Align2::LEFT_TOP,
            when,
            egui::FontId::proportional(11.0),
            if recording_now { Fluent::LIVE } else { Fluent::TEXT_TERTIARY },
        );

        if !job.rule_id.is_empty() {
            ui.painter().text(
                egui::pos2(rect.max.x - 92.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "Series",
                egui::FontId::proportional(11.0),
                Fluent::TEXT_TERTIARY,
            );
        }

        let cancel = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 28.0, rect.center().y),
            egui::vec2(30.0, 30.0),
        );
        let cancel_response =
            ui.interact(cancel, egui::Id::new(("cancel-job", &job.id)), egui::Sense::click());
        if cancel_response.hovered() {
            ui.painter()
                .rect_filled(cancel, theme::RADIUS_CONTROL, Fluent::CONTROL_PRESSED);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(
            cancel.center(),
            egui::Align2::CENTER_CENTER,
            "\u{E711}", // Cancel
            egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            if cancel_response.hovered() { Fluent::LIVE } else { Fluent::TEXT_SECONDARY },
        );
        if cancel_response.clicked() {
            action = Some(RecordingsAction::Cancel(job.id.clone()));
        }
    });
    ui.add_space(SPACE_S / 2.0);

    action
}

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
