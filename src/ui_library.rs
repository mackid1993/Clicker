// SPDX-License-Identifier: MIT
//
// Clicker - an unofficial, native Windows client for Channels DVR
// Copyright (c) 2026 David Brustein

//! The Library and Recordings screens.
//!
//! Library is what you own, arranged by series. Recordings is what the DVR is
//! going to do next, plus everything it has already done. They are separate
//! screens because they answer different questions: "what can I watch" and
//! "what is my DVR doing".

use eframe::egui;

use crate::images::Images;
use crate::library::{Home, Recording, Sort, Upcoming};
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
///
/// Returns what the user asked for, and whether the sort choice changed and
/// settings need saving — the same contract as the guide's filter bar.
pub fn library_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    state: &mut LibraryState,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
    settings: &mut crate::settings::Settings,
    loading: bool,
) -> (Action, bool) {
    let mut action = Action::None;
    let mut settings_changed = false;

    if loading && data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Loading your library", "Reading recordings from the DVR");
        return (action, settings_changed);
    }
    if data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Nothing recorded yet", "Anything you record will appear here");
        return (action, settings_changed);
    }

    // Inside a series: the episode list.
    if let Some(show_id) = state.open_show.clone() {
        return (
            show_detail(ui, rect, data, state, images, downloads, &show_id),
            settings_changed,
        );
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
                // The same pill the guide draws, at the same width, but full
                // height: this one stands beside a heading rather than in a row
                // of chips, and the chips' height on its own looks like a
                // control that failed to load.
                theme::search_field(ui, &mut state.search, 300.0, theme::SEARCH_H);
                ui.add_space(SPACE_M);

                // Which order the grid below is in. One menu serves both
                // tabs, each keeping its own choice and offering only the
                // orders that mean something for what it shows.
                let (sort, options) = match state.tab {
                    LibraryTab::Shows => (&mut settings.sort_shows, &Sort::SHOWS[..]),
                    LibraryTab::Movies => (&mut settings.sort_movies, &Sort::MOVIES[..]),
                };
                if crate::ui::sort_menu(ui, "library-sort", sort, options, Sort::NameAZ) {
                    settings_changed = true;
                }
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
                let mut matching: Vec<&Recording> = movies
                    .into_iter()
                    .filter(|m| needle.is_empty() || m.title.to_lowercase().contains(&needle))
                    .collect();
                settings.sort_movies.apply_recordings(&mut matching);
                if matching.is_empty() {
                    ui.add_space(SPACE_L);
                    empty_note(ui, "No movies match that");
                    return;
                }
                let available = ui.available_width() - SPACE_L * 4.0;
                let per_row =
                    ((available + SPACE_M) / (POSTER_W + SPACE_M)).floor().max(1.0) as usize;
                let (drawn_rows, skipped_rows) =
                    visible_rows(ui, &matching, per_row, |ui, chunk| {
                        for movie in chunk {
                            // The same year the year sorts use — release year
                            // first, air date second — so the label under a
                            // poster always agrees with where it sorted.
                            let year = movie
                                .year()
                                .map(|y| y.to_string())
                                .unwrap_or_default();
                            if poster(ui, movie.art(), &movie.title, &year, 0, images) {
                                action = Action::Play((*movie).clone());
                            }
                            ui.add_space(SPACE_M);
                        }
                    });
                grid_log("movies", matching.len(), per_row, drawn_rows, skipped_rows, images);
                ui.add_space(SPACE_L * 2.0);
                return;
            }

            // One pass over the library for every series' episode count and
            // newest recording. Two of the sorts order by it, and the count
            // under each tile reads from it, which replaces the scan of every
            // recording that used to run once per drawn row.
            let stats = data.series_stats();

            let mut shows: Vec<_> = data
                .groups
                .iter()
                .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
                .collect();
            settings.sort_shows.apply_shows(&mut shows, &stats);

            if shows.is_empty() {
                ui.add_space(SPACE_L);
                empty_note(ui, "No series match that");
                return;
            }

            // A wrapping grid of posters, sized to whatever width there is.
            let available = ui.available_width() - SPACE_L * 4.0;
            let per_row = ((available + SPACE_M) / (POSTER_W + SPACE_M)).floor().max(1.0) as usize;

            let (drawn_rows, skipped_rows) = visible_rows(ui, &shows, per_row, |ui, chunk| {
                for group in chunk {
                    let (episodes, _) = group.stats(&stats);
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
                        state.open_show = Some(if stats.contains_key(group.id.as_str()) {
                            group.id.clone()
                        } else {
                            group.series_id.clone()
                        });
                    }
                    ui.add_space(SPACE_M);
                }
            });
            grid_log("series", shows.len(), per_row, drawn_rows, skipped_rows, images);

            ui.add_space(SPACE_L * 2.0);

        });

    (action, settings_changed)
}

/// Lay out a grid in rows of `per_row`, building only the rows on screen.
///
/// Both library grids come through here — that matters, because the last time
/// this logic existed for one grid only, the other kept the bug. Drawing every
/// card is the whole cause of artwork flickering on a large library: each card
/// asks for its poster whether or not it is visible, so five hundred cards
/// asked for five hundred pictures every frame, and no cache of any size can
/// hold a working set that large. Forty of them are visible.
///
/// A row is measured and skipped rather than not laid out, so the scroll bar
/// and the scroll position stay exactly what they were. The height reserved
/// for a skipped row is measured from the last row actually drawn rather than
/// computed from the poster size and guessed text metrics; until one has been
/// drawn, an estimate stands in.
///
/// Returns how many rows were built and how many were skipped, for the log.
fn visible_rows<T>(
    ui: &mut egui::Ui,
    items: &[T],
    per_row: usize,
    mut row: impl FnMut(&mut egui::Ui, &[T]),
) -> (usize, usize) {
    let mut row_h = POSTER_H + SPACE_M * 3.0 + 34.0;
    let mut drawn_rows = 0usize;
    let mut skipped_rows = 0usize;
    for chunk in items.chunks(per_row) {
        let space = ui.available_rect_before_wrap();
        let space = egui::Rect::from_min_size(space.min, egui::vec2(space.width(), row_h));
        if !ui.is_rect_visible(space) {
            skipped_rows += 1;
            ui.allocate_space(egui::vec2(space.width(), row_h));
            continue;
        }
        drawn_rows += 1;
        let drawn = ui.horizontal(|ui| {
            ui.add_space(SPACE_L * 2.0);
            row(ui, chunk);
        });
        row_h = drawn.response.rect.height() + SPACE_M;
        ui.add_space(SPACE_M);
    }
    (drawn_rows, skipped_rows)
}

/// What the library page is doing, occasionally.
///
/// Every few hundred builds rather than every frame: this exists to answer
/// "why is the artwork flickering" from a user's log, and what answers it is
/// how many rows were built against how many exist, and how much artwork is
/// resident against how much is on screen. A line per frame would drown the
/// log it is meant to help read.
///
/// Both grids report through here and say which they are. The Movies tab
/// going unlogged is how its flickering went undiagnosed across two releases:
/// the log said the library was healthy, and it was — the half of it that
/// could speak.
fn grid_log(
    kind: &str,
    total: usize,
    per_row: usize,
    drawn: usize,
    skipped: usize,
    images: &Images,
) {
    static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 600 == 0 {
        let (resident, bytes) = images.resident();
        crate::log::line(&format!(
            "[library] {total} {kind}, {per_row} per row, {drawn} drawn, {skipped} skipped, artwork {resident} images {} MB",
            bytes / (1024 * 1024),
        ));
    }
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

                // Shuffle, which for a series with a hundred episodes is the
                // only sensible way to start one. Unwatched first, because
                // "play me something" from a series someone is working through
                // means something they have not seen; it falls back to the
                // whole run once there is nothing new, which is what makes it
                // useful on a comedy somebody has finished twice.
                if episodes.len() > 1 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(SPACE_L * 2.0);
                        if ui
                            .button("🔀  Shuffle")
                            .on_hover_text("Play a random episode")
                            .clicked()
                        {
                            let unseen: Vec<&&Recording> =
                                episodes.iter().filter(|e| !e.watched).collect();
                            let pool: &[&&Recording] = if unseen.is_empty() {
                                // Nothing unwatched left, so anything goes.
                                &[]
                            } else {
                                &unseen
                            };
                            // The clock as the die. There is no random number
                            // generator in this program and one crate is not
                            // worth adding to pick an episode.
                            let roll = crate::ui::scatter(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos() as u64)
                                    .unwrap_or(0),
                            );
                            let chosen = if pool.is_empty() {
                                episodes.get((roll % episodes.len() as u64) as usize).copied()
                            } else {
                                pool.get((roll % pool.len() as u64) as usize).map(|e| **e)
                            };
                            if let Some(episode) = chosen {
                                action = Action::Play(episode.clone());
                            }
                        }
                    });
                }
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
            theme::image_cover(ui.painter(), art, 4.0, texture, 0.5);
        } else {
            ui.painter().rect_filled(art, 4.0, Fluent::LAYER_CARD);
        }

        if item.progress() > 0.0 {
            // Inset, so a full-width bar does not poke out through the rounded
            // corners of the artwork it is drawn on.
            let track = egui::Rect::from_min_size(
                egui::pos2(art.min.x + 1.5, art.max.y - 3.0),
                egui::vec2(art.width() - 3.0, 3.0),
            );
            ui.painter()
                .rect_filled(track, 1.5, egui::Color32::from_black_alpha(150));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * item.progress(), 3.0),
                ),
                1.5,
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
                    theme::icon::CHECK,
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
                    // More — waiting for a slot.
                    theme::icon::MORE,
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
                    theme::icon::DOWNLOAD,
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
        theme::image_cover(ui.painter(), art, RADIUS_SURFACE, texture, 0.5);
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
///
/// Returns what the user asked for, and whether the sort choice changed and
/// settings need saving.
pub fn recordings_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    tab: &mut RecordingsTab,
    images: &mut Images,
    downloads: &crate::downloads::Downloads,
    settings: &mut crate::settings::Settings,
    now: i64,
    loading: bool,
) -> (RecordingsAction, bool) {
    let mut action = RecordingsAction::None;
    let mut settings_changed = false;

    if loading && data.upcoming.is_empty() && data.all.is_empty() {
        crate::ui::centered_message(ui, rect, "Loading", "Reading the schedule from the DVR");
        return (action, settings_changed);
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
        // The library's sort menu, on the tab where it means something. The
        // schedule stays chronological: a list of what happens next sorted by
        // anything but when is not a schedule.
        if *tab == RecordingsTab::Recorded {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(SPACE_L * 2.0);
                if crate::ui::sort_menu(
                    ui,
                    "recordings-sort",
                    &mut settings.sort_recordings,
                    &Sort::RECORDED,
                    Sort::Added,
                ) {
                    settings_changed = true;
                }
            });
        }
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
                    // Sorted before the cap, so "A to Z" means the whole
                    // tab's worth alphabetically, not the newest three
                    // hundred rearranged.
                    let mut recorded: Vec<&Recording> = data.recorded.iter().collect();
                    settings.sort_recordings.apply_recordings(&mut recorded);
                    for item in recorded.into_iter().take(300) {
                        if let Some(a) = episode_row(ui, item, images, downloads) {
                            action = RecordingsAction::Item(a);
                        }
                    }
                }
            }
            ui.add_space(SPACE_L * 2.0);

        });

    (action, settings_changed)
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
            theme::image_cover(ui.painter(), thumb, theme::RADIUS_CONTROL, &texture, 0.5);
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
            theme::icon::CANCEL,
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
