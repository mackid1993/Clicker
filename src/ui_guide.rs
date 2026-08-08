//! The guide grid.
//!
//! Channels down, time across, both scrolling in one surface with the channel
//! column and the time ruler pinned. Only the visible rows are drawn: a full
//! guide is over a thousand channels, and laying out every program in every
//! one of them each frame is not something to attempt.
//!
//! The interaction model keys off whether a program has started:
//!
//! * **Left click, already airing or aired** — watch it
//! * **Left click, not yet aired** — record it. Recording is the only thing
//!   that can be done with a program that has not happened, so it should not
//!   need a menu to reach
//! * **Right click, anything** — the full menu: watch, record this episode,
//!   record the whole series, adjust padding, or cancel what is scheduled

use eframe::egui;

use crate::guide::{Airing, GuideData, Row};
use crate::theme::{self, Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S};

/// Pixels per minute. Half an hour is 120px, which fits a readable title.
const PX_PER_MIN: f32 = 4.0;
/// Row height, and the gap drawn between rows. The gap is part of the row
/// rather than egui spacing, because `show_rows` needs every row to be exactly
/// one height and any additional item spacing is added on top of that, which is
/// what left the guide looking like a ladder.
const ROW_H: f32 = 64.0;
/// Space between rows and between cells. Generous on purpose: a wall of
/// touching rectangles is unreadable however good the type is, and the gap is
/// what lets the eye follow a single channel across the hour.
const ROW_GAP: f32 = 6.0;
const CELL_GAP: f32 = 4.0;
const CHANNEL_W: f32 = 176.0;
const RULER_H: f32 = 38.0;

/// What the guide is asking for.
pub enum GuideAction {
    None,
    Watch(String),
    /// Schedule this one airing.
    Record(Airing),
    /// Schedule the whole series.
    RecordSeries(Airing),
    /// Cancel a scheduled job by id.
    CancelJob(String, Airing),
    /// Cancel the series pass covering this airing.
    CancelSeries(Airing),
    /// Open the record dialog for an airing: padding, this episode, or the
    /// whole series.
    OpenRecord(Airing),
}

pub struct GuideState {
    pub collection: Option<String>,
    pub source: Option<String>,
    pub search: String,
    /// Horizontal scroll, in minutes from the window start.
    pub scroll_minutes: f32,
}

impl Default for GuideState {
    fn default() -> Self {
        Self {
            collection: None,
            source: None,
            search: String::new(),
            scroll_minutes: 0.0,
        }
    }
}

pub fn guide(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &GuideData,
    state: &mut GuideState,
    images: &mut crate::images::Images,
    settings: &mut crate::settings::Settings,
    now: i64,
    loading: bool,
) -> (GuideAction, bool) {
    let mut action = GuideAction::None;
    let mut settings_changed = false;

    if loading && data.rows.is_empty() {
        crate::ui::centered_message(ui, rect, "Loading the guide", "Reading listings from the DVR");
        return (action, settings_changed);
    }
    if data.rows.is_empty() {
        crate::ui::centered_message(ui, rect, "No listings", "The DVR returned no guide data");
        return (action, settings_changed);
    }

    // ── Filter bar ──────────────────────────────────────────────────────
    //
    // Every control is forced to exactly the same height. Asking nicely with
    // interact_size was not enough: a ComboBox sizes from button padding and a
    // TextEdit from its margin, and they disagreed by five pixels, which is
    // what kept these three off one another's baselines through two attempts.
    const BAR_H: f32 = 60.0;
    const CONTROL_H: f32 = 36.0;

    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), BAR_H));
    let mut bar_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(bar.min.x + SPACE_L, bar.center().y - CONTROL_H / 2.0),
                egui::vec2(bar.width() - SPACE_L * 2.0, CONTROL_H),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar_ui.spacing_mut().interact_size.y = CONTROL_H;
    bar_ui.spacing_mut().button_padding = egui::vec2(SPACE_M, 8.0);
    bar_ui.spacing_mut().item_spacing.x = SPACE_M;
    if filter_bar(&mut bar_ui, data, state, settings) {
        settings_changed = true;
    }

    let grid = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + BAR_H),
        rect.max,
    );

    let rows = crate::guide::filter(data, state.collection.as_deref(), state.source.as_deref(), &state.search);
    if rows.is_empty() {
        crate::ui::centered_message(ui, grid, "Nothing matches", "No channel is in both of those filters");
        return (action, settings_changed);
    }

    // ── Time ruler ──────────────────────────────────────────────────────
    let ruler = egui::Rect::from_min_size(
        egui::pos2(grid.min.x + CHANNEL_W, grid.min.y),
        egui::vec2(grid.width() - CHANNEL_W, RULER_H),
    );
    let body = egui::Rect::from_min_max(
        egui::pos2(grid.min.x, grid.min.y + RULER_H),
        grid.max,
    );

    // Horizontal position is driven by a scrollbar-free drag on the ruler plus
    // the mouse wheel with shift, because a full day is six hours of pixels and
    // a conventional scrollbar under a thousand rows is unusable.
    let ruler_response = ui.interact(ruler, egui::Id::new("guide-ruler"), egui::Sense::drag());
    if ruler_response.dragged() {
        state.scroll_minutes -= ruler_response.drag_delta().x / PX_PER_MIN;
    }
    let scroll = ui.input(|i| i.raw_scroll_delta);
    if body.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(egui::Pos2::ZERO))
        && scroll.x.abs() > 0.0
    {
        state.scroll_minutes -= scroll.x / PX_PER_MIN;
    }
    let max_minutes = data.minutes as f32 - (ruler.width() / PX_PER_MIN);
    state.scroll_minutes = state.scroll_minutes.clamp(0.0, max_minutes.max(0.0));

    draw_ruler(ui, ruler, data.start, state.scroll_minutes, now);

    // ── Rows ────────────────────────────────────────────────────────────
    let mut scroll_area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(&mut scroll_area, ROW_H, rows.len(), |ui, range| {
            // egui inserts item spacing between anything allocated in a
            // top-down layout, and `show_rows` has already reserved exactly
            // ROW_H per row. Leaving the default spacing in place adds another
            // eight pixels to each one, so the rows drift out of step with the
            // scroll positions egui computed and the whole grid gaps open.
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

            for index in range {
                let row = rows[index];
                let (allocated, _) =
                    ui.allocate_exact_size(egui::vec2(body.width(), ROW_H), egui::Sense::hover());
                let row_rect = egui::Rect::from_min_size(
                    allocated.min,
                    egui::vec2(body.width(), ROW_H - ROW_GAP),
                );

                if let Some(a) = draw_row(ui, row_rect, row, data, state, images, now) {
                    action = a;
                }
            }
        });

    // The now line continues faintly down through the grid, so the boundary
    // between "already on" and "still to come" is visible at every row rather
    // than only in the ruler.
    let now_min = (now - data.start) as f32 / 60.0;
    if now_min >= state.scroll_minutes {
        let x = body.min.x + CHANNEL_W + (now_min - state.scroll_minutes) * PX_PER_MIN;
        if x > body.min.x + CHANNEL_W && x <= body.max.x {
            ui.painter().with_clip_rect(body).line_segment(
                [egui::pos2(x, body.min.y), egui::pos2(x, body.max.y)],
                egui::Stroke::new(1.5, with_alpha(Fluent::LIVE, 70)),
            );
        }
    }

    (action, settings_changed)
}

/// A Fluent filter chip: a pill showing the current selection with a chevron.
///
/// Hand-drawn rather than an egui ComboBox because the two stock widgets
/// derive their height from different things — button padding for one, text
/// margin for the other — and no amount of setting `interact_size` made them
/// agree. Drawing the pill means the height is simply the number given.
fn chip(ui: &mut egui::Ui, id: egui::Id, label: &str, active: bool, width: f32) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    let hover = ui
        .ctx()
        .animate_bool_with_time(id.with("h"), response.hovered(), theme::ANIM_FAST);
    let base = if active { Fluent::CONTROL } else { with_alpha(Fluent::LAYER_CARD, 150) };
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        height / 2.0,
        theme::mix(base, Fluent::CONTROL_HOVER, hover),
    );
    painter.rect_stroke(
        rect,
        height / 2.0,
        egui::Stroke::new(1.0, Fluent::STROKE_CONTROL),
    );

    painter.text(
        egui::pos2(rect.min.x + SPACE_M + 2.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        elide(label, width - 44.0),
        egui::FontId::proportional(13.0),
        if active { Fluent::TEXT_PRIMARY } else { Fluent::TEXT_SECONDARY },
    );
    painter.text(
        egui::pos2(rect.max.x - SPACE_M, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "\u{E70D}", // ChevronDown
        egui::FontId::new(9.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
        Fluent::TEXT_TERTIARY,
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// The collection and source pickers and the search box. Returns true when
/// something that lives in settings changed and needs saving.
fn filter_bar(
    ui: &mut egui::Ui,
    data: &GuideData,
    state: &mut GuideState,
    settings: &mut crate::settings::Settings,
) -> bool {
    let mut changed = false;

    // Collections and sources are deliberately two separate menus that both
    // apply. See the note in guide.rs.
    let label = state
        .collection
        .as_ref()
        .and_then(|slug| data.collections.iter().find(|c| &c.slug == slug))
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "All collections".into());

    // In the server's own order. There used to be a star on each row that
    // sorted favorites to the top, and it earned nothing: the guide already
    // reopens on the collection last picked, so the one that matters is
    // already selected when the menu is opened, and a second mechanism for
    // saying which collection matters was two ways to express one preference.
    let collection_id = egui::Id::new("chip-collection");
    let collection_chip = chip(ui, collection_id, &label, state.collection.is_some(), 196.0);
    if collection_chip.clicked() {
        ui.memory_mut(|m| m.toggle_popup(collection_id.with("popup")));
    }
    egui::popup::popup_below_widget(
        ui,
        collection_id.with("popup"),
        &collection_chip,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(268.0);
            if ui
                .selectable_label(state.collection.is_none(), "All collections")
                .clicked()
            {
                state.collection = None;
                settings.last_collection = None;
                changed = true;
            }
            ui.separator();
            for collection in &data.collections {
                let selected = state.collection.as_deref() == Some(collection.slug.as_str());
                if ui
                    .selectable_label(
                        selected,
                        format!("{}  ({})", collection.name, collection.items.len()),
                    )
                    .clicked()
                {
                    state.collection = Some(collection.slug.clone());
                    // Saved, which is what makes the guide reopen here.
                    settings.last_collection = Some(collection.slug.clone());
                    changed = true;
                }
            }
        },
    );

    let source_label = state.source.clone().unwrap_or_else(|| "All sources".into());
    let source_id = egui::Id::new("chip-source");
    let source_chip = chip(ui, source_id, &source_label, state.source.is_some(), 176.0);
    if source_chip.clicked() {
        ui.memory_mut(|m| m.toggle_popup(source_id.with("popup")));
    }
    egui::popup::popup_below_widget(
        ui,
        source_id.with("popup"),
        &source_chip,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(200.0);
            if ui.selectable_label(state.source.is_none(), "All sources").clicked() {
                state.source = None;
                settings.last_source = None;
                changed = true;
            }
            ui.separator();
            for source in &data.sources {
                let selected = state.source.as_deref() == Some(source.as_str());
                if ui.selectable_label(selected, source).clicked() {
                    state.source = Some(source.clone());
                    settings.last_source = Some(source.clone());
                    changed = true;
                }
            }
        },
    );

    // Search, drawn to the same pill geometry as the chips so the row reads as
    // one set of controls rather than three widgets that happen to be adjacent
    // — which is why this takes the chips' height rather than the standalone
    // one the library uses.
    let height = ui.spacing().interact_size.y;
    theme::search_field(ui, &mut state.search, 300.0, height);

    changed
}

fn draw_ruler(ui: &mut egui::Ui, rect: egui::Rect, start: i64, scroll_minutes: f32, now: i64) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, with_alpha(Fluent::SOLID, 200));

    // A mark every half hour, which is how listings are actually organized.
    let first = (scroll_minutes / 30.0).floor() * 30.0;
    let mut minutes = first;
    while minutes < scroll_minutes + rect.width() / PX_PER_MIN + 30.0 {
        let x = rect.min.x + (minutes - scroll_minutes) * PX_PER_MIN;
        if x > rect.min.x - 60.0 {
            painter.line_segment(
                [egui::pos2(x, rect.max.y - 8.0), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.0, Fluent::STROKE_CONTROL),
            );
            painter.text(
                egui::pos2(x + SPACE_S, rect.center().y - 2.0),
                egui::Align2::LEFT_CENTER,
                clock_label(start + (minutes as i64) * 60),
                egui::FontId::proportional(12.5),
                Fluent::TEXT_SECONDARY,
            );
        }
        minutes += 30.0;
    }

    // Where "now" falls: a line with a small cap in the ruler, the way every
    // calendar marks the current moment. The line itself is drawn by the rows
    // underneath; here it only needs its head.
    let now_minutes = (now - start) as f32 / 60.0;
    if now_minutes >= scroll_minutes {
        let x = rect.min.x + (now_minutes - scroll_minutes) * PX_PER_MIN;
        if x <= rect.max.x {
            painter.line_segment(
                [egui::pos2(x, rect.min.y + 6.0), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(2.0, Fluent::LIVE),
            );
            painter.circle_filled(egui::pos2(x, rect.min.y + 6.0), 3.5, Fluent::LIVE);
        }
    }
}

fn draw_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    row: &Row,
    data: &GuideData,
    state: &GuideState,
    images: &mut crate::images::Images,
    now: i64,
) -> Option<GuideAction> {
    let mut action = None;

    // Channel cell, pinned to the left.
    let channel_rect = egui::Rect::from_min_size(rect.min, egui::vec2(CHANNEL_W, rect.height()));
    let programs = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + CHANNEL_W, rect.min.y),
        rect.max,
    );

    let painter = ui.painter().with_clip_rect(programs);

    for airing in &row.airings {
        let start_min = (airing.start - data.start) as f32 / 60.0;
        let end_min = (airing.end() - data.start) as f32 / 60.0;
        let x0 = programs.min.x + (start_min - state.scroll_minutes) * PX_PER_MIN;
        let x1 = programs.min.x + (end_min - state.scroll_minutes) * PX_PER_MIN;

        if x1 < programs.min.x || x0 > programs.max.x {
            continue;
        }

        // Clamped to the visible area, so a program already part way through
        // fills from the left edge instead of being drawn mostly off-screen.
        // The one pixel taken off the right is the gap between cells; taking it
        // off the top and bottom as well made every row look like it had a
        // border.
        let cell = egui::Rect::from_min_max(
            egui::pos2(x0.max(programs.min.x), rect.min.y),
            egui::pos2((x1 - CELL_GAP).min(programs.max.x), rect.max.y),
        );
        if cell.width() < 6.0 {
            continue;
        }

        let id = egui::Id::new(("guide-cell", &row.channel.number, airing.start));
        let response = ui.interact(cell, id, egui::Sense::click());

        let scheduled = data.schedule.job_for(airing);
        let passed = data.schedule.has_pass(airing);
        let live = airing.airing_now(now);

        // Hover eases in and out rather than snapping. The animation timer
        // only runs while a value is mid-flight, so several hundred cells cost
        // nothing when the pointer is still.
        let hover = ui
            .ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered(), theme::ANIM_FAST);
        let base = if live {
            with_alpha(Fluent::ACCENT_DARK, 70)
        } else {
            with_alpha(Fluent::LAYER_CARD, 190)
        };
        painter.rect_filled(cell, 6.0, theme::mix(base, Fluent::CONTROL_HOVER, hover));

        // A hairline only where the eye needs it: under the pointer, and on
        // what is airing now. Stroking every cell turns the grid into graph
        // paper.
        if hover > 0.01 {
            // The theme's own control hairline, eased in, rather than white at
            // an alpha chosen to match it. `with_alpha` multiplies in linear
            // space, which turns white at alpha 26 into bytes near
            // (90, 90, 90, 26) — several times the hairline every other
            // control wears, and a visible white outline against the backdrop.
            let stroke = theme::mix(egui::Color32::TRANSPARENT, Fluent::STROKE_CONTROL, hover);
            painter.rect_stroke(cell, 6.0, egui::Stroke::new(1.0, stroke));
        } else if live {
            painter.rect_stroke(
                cell,
                6.0,
                egui::Stroke::new(1.0, with_alpha(Fluent::ACCENT, 70)),
            );
        }

        // A recording is marked on the cell, not hidden in a menu, so a glance
        // at the guide answers "what am I already getting".
        if scheduled.is_some() || passed {
            let dot = egui::pos2(cell.min.x + SPACE_M + 2.0, cell.min.y + 18.0);
            painter.circle_filled(dot, 3.5, Fluent::LIVE);
            if passed {
                // A second, hollow ring means the whole series, not just this.
                painter.circle_stroke(dot, 6.5, egui::Stroke::new(1.2, Fluent::LIVE));
            }
        }

        // Room inside the cell. Text pressed against a rounded corner is what
        // made the old grid read as cramped however large the cells were.
        let pad = SPACE_M;
        let text_x = cell.min.x + if scheduled.is_some() || passed { pad + 14.0 } else { pad };
        let text_w = cell.max.x - text_x - pad;

        if text_w > 30.0 {
            // The badge sits on the title's own line, at the right edge, so
            // the title has to be elided to what is left after it rather than
            // to the whole cell. Eliding to the full width let a long title
            // run straight under "NEW" — the badge is drawn afterwards and
            // simply painted over whatever was there.
            const BADGE_W: f32 = 36.0;
            let badge = airing.is_new && text_w > 160.0;
            let title_w = if badge { text_w - BADGE_W } else { text_w };

            painter.text(
                egui::pos2(text_x, cell.min.y + 11.0),
                egui::Align2::LEFT_TOP,
                elide(&airing.title, title_w),
                egui::FontId::proportional(13.5),
                Fluent::TEXT_PRIMARY,
            );
            // The second line is dropped entirely on narrow cells rather than
            // squeezed to three characters and an ellipsis.
            let subtitle = airing.subtitle();
            if !subtitle.is_empty() && text_w > 100.0 {
                painter.text(
                    egui::pos2(text_x, cell.min.y + 33.0),
                    egui::Align2::LEFT_TOP,
                    elide(&subtitle, text_w),
                    egui::FontId::proportional(11.5),
                    Fluent::TEXT_TERTIARY,
                );
            }
            if badge {
                painter.text(
                    egui::pos2(cell.max.x - pad, cell.min.y + 12.0),
                    egui::Align2::RIGHT_TOP,
                    "NEW",
                    egui::FontId::proportional(10.0),
                    Fluent::SUCCESS,
                );
            }
        }

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Left click: watch what has started, and ask about what has not.
        //
        // It used to schedule a future programme outright. Recording is not a
        // preview — it claims a tuner, it may create a season pass, and the
        // padding is usually the thing worth changing — so a single click that
        // commits to all of that, with no confirmation and no undo beyond
        // finding the job again, is too much to infer from one click on a
        // grid cell. The dialog is one keystroke away from the same outcome
        // and does not guess.
        if response.clicked() {
            action = Some(if airing.in_future(now) {
                GuideAction::OpenRecord(airing.clone())
            } else {
                GuideAction::Watch(row.channel.number.clone())
            });
        }

        // Right click: everything, including the things a left click chose not
        // to guess at.
        response.context_menu(|ui| {
            ui.set_min_width(210.0);
            ui.label(
                egui::RichText::new(&airing.title)
                    .size(13.0)
                    .color(Fluent::TEXT_PRIMARY),
            );
            let subtitle = airing.subtitle();
            if !subtitle.is_empty() {
                ui.label(
                    egui::RichText::new(elide(&subtitle, 260.0))
                        .size(11.0)
                        .color(Fluent::TEXT_TERTIARY),
                );
            }
            ui.separator();

            if !airing.in_future(now) {
                if ui.button("▶  Watch").clicked() {
                    action = Some(GuideAction::Watch(row.channel.number.clone()));
                    ui.close_menu();
                }
            }

            match scheduled {
                Some(job) => {
                    if ui.button("✕  Cancel this recording").clicked() {
                        action = Some(GuideAction::CancelJob(job.clone(), airing.clone()));
                        ui.close_menu();
                    }
                }
                None => {
                    if ui.button("⏺  Record this episode").clicked() {
                        action = Some(GuideAction::Record(airing.clone()));
                        ui.close_menu();
                    }
                }
            }

            if !airing.series_id.is_empty() {
                if passed {
                    if ui.button("✕  Cancel the series pass").clicked() {
                        action = Some(GuideAction::CancelSeries(airing.clone()));
                        ui.close_menu();
                    }
                } else if ui.button("⧉  Record the whole series").clicked() {
                    action = Some(GuideAction::RecordSeries(airing.clone()));
                    ui.close_menu();
                }
            }

            ui.separator();
            if ui.button("⚙  Padding and options…").clicked() {
                action = Some(GuideAction::OpenRecord(airing.clone()));
                ui.close_menu();
            }
        });
    }

    // Drawn after the programs so it covers anything scrolled under it.
    draw_channel_cell(ui, channel_rect, row, images);

    action
}

fn draw_channel_cell(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    row: &Row,
    images: &mut crate::images::Images,
) {
    // The logo is fetched before the painter is borrowed; both need the ui.
    let logo = if row.channel.logo.is_empty() {
        None
    } else {
        images.get(&row.channel.logo).cloned()
    };
    let dark_ink = !row.channel.logo.is_empty() && images.is_dark(&row.channel.logo);

    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, with_alpha(Fluent::SOLID, 225));
    painter.line_segment(
        [
            egui::pos2(rect.max.x, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        ],
        egui::Stroke::new(1.0, Fluent::STROKE_DIVIDER),
    );

    // Logo, name over number. The logo box is a fixed size whether or not the
    // artwork has arrived, so the text never shifts as images load in.
    let logo_box = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + SPACE_M + 24.0, rect.center().y),
        egui::vec2(48.0, 32.0),
    );
    if let Some(texture) = logo {
        let size = texture.size_vec2();
        if size.x > 0.0 && size.y > 0.0 {
            // Contain, not cover: a station mark must never be cropped.
            let scale = (logo_box.width() / size.x).min(logo_box.height() / size.y);
            let target = egui::Rect::from_center_size(logo_box.center(), size * scale);
            // A dark mark on this column is a logo-shaped hole, so give those
            // — and only those — something light to sit on. Slightly off
            // white: pure white next to the material reads as a cut-out.
            if dark_ink {
                painter.rect_filled(
                    target.expand(3.0),
                    4.0,
                    egui::Color32::from_rgba_unmultiplied(244, 245, 247, 236),
                );
            }
            painter.image(
                texture.id(),
                target,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    } else {
        painter.rect_filled(logo_box, 4.0, with_alpha(Fluent::CONTROL, 90));
    }

    let text_x = logo_box.max.x + SPACE_M;
    painter.text(
        egui::pos2(text_x, rect.center().y - 10.0),
        egui::Align2::LEFT_CENTER,
        elide(&row.channel.name, rect.max.x - text_x - SPACE_S),
        egui::FontId::proportional(13.5),
        Fluent::TEXT_PRIMARY,
    );
    painter.text(
        egui::pos2(text_x, rect.center().y + 11.0),
        egui::Align2::LEFT_CENTER,
        &row.channel.number,
        egui::FontId::proportional(11.5),
        Fluent::TEXT_TERTIARY,
    );
}

/// "in 12 min", "Today 8:00 PM", "Tomorrow 9:30 PM", or a plain time.
///
/// Relative for anything imminent, because "in 12 minutes" is what someone
/// wants to know about a recording that is about to start, and absolute
/// further out where a countdown stops being useful.
pub fn when_label(start: i64, now: i64) -> String {
    let delta = start - now;
    if delta < 0 {
        return clock_label(start);
    }
    if delta < 3600 {
        let minutes = (delta / 60).max(1);
        return format!("in {minutes} min");
    }

    let local_day = |t: i64| (t + local_offset_seconds()).div_euclid(86_400);
    let days = local_day(start) - local_day(now);
    match days {
        0 => format!("Today {}", clock_label(start)),
        1 => format!("Tomorrow {}", clock_label(start)),
        d if d < 7 => format!("{} {}", weekday(start), clock_label(start)),
        _ => clock_label(start),
    }
}

/// Weekday name for a timestamp. 1 Jan 1970 was a Thursday, which anchors it.
fn weekday(unix: i64) -> &'static str {
    const NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let day = (unix + local_offset_seconds()).div_euclid(86_400);
    NAMES[day.rem_euclid(7) as usize]
}

/// Local wall time as "7:30 PM", for callers outside this module.
pub fn time_only(unix: i64) -> String {
    clock_label(unix)
}

/// Local wall time as "7:30 PM".
fn clock_label(unix: i64) -> String {
    // Deliberately arithmetic rather than a date library. The only thing needed
    // is a clock face, and pulling in a full calendar implementation to render
    // "7:30 PM" would be the largest dependency in the project.
    let local = unix + local_offset_seconds();
    let minutes_of_day = ((local % 86_400) + 86_400) % 86_400 / 60;
    let hour24 = minutes_of_day / 60;
    let minute = minutes_of_day % 60;
    let suffix = if hour24 < 12 { "AM" } else { "PM" };
    let hour12 = match hour24 % 12 {
        0 => 12,
        h => h,
    };
    format!("{hour12}:{minute:02} {suffix}")
}

/// Offset from UTC, in seconds.
///
/// Measured once, by asking the C runtime to format the same instant both ways
/// and taking the difference. That avoids a dependency on a date library for a
/// single number, and avoids the Win32 time zone API, whose bias fields are
/// signed the opposite way round to how everyone expects and are a reliable
/// source of off-by-an-hour bugs.
fn local_offset_seconds() -> i64 {
    use std::sync::OnceLock;
    static OFFSET: OnceLock<i64> = OnceLock::new();

    *OFFSET.get_or_init(|| {
        // SystemTime has no notion of local time, so the offset is derived
        // from what the platform reports for "now" in both zones. The value
        // does not change while the app is running, daylight saving included:
        // a transition mid-session is a cosmetic hour on a guide, not worth a
        // dependency to handle.
        let now = std::time::SystemTime::now();
        let utc = now
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        #[cfg(windows)]
        {
            // GetLocalTime and GetSystemTime differ by exactly the offset.
            use windows::Win32::Foundation::SYSTEMTIME;
            use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

            let (local, system): (SYSTEMTIME, SYSTEMTIME) =
                unsafe { (GetLocalTime(), GetSystemTime()) };

            let minutes = |t: &SYSTEMTIME| {
                t.wHour as i64 * 60 + t.wMinute as i64 + t.wDay as i64 * 24 * 60
            };
            let mut delta = (minutes(&local) - minutes(&system)) * 60;
            // Guard against a month boundary making the day component wrap.
            if delta > 15 * 3600 {
                delta -= 24 * 3600;
            } else if delta < -15 * 3600 {
                delta += 24 * 3600;
            }
            let _ = utc;
            delta
        }
        #[cfg(not(windows))]
        {
            let _ = utc;
            0
        }
    })
}

fn elide(text: &str, width: f32) -> String {
    // Roughly six pixels a character at these sizes. Measuring properly would
    // mean laying the text out twice for every cell on screen.
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

/// Unused imports kept meaningful: these are referenced by the padding dialog
/// that the guide opens.
#[allow(unused)]
use theme::RADIUS_CONTROL;
#[allow(unused)]
use RADIUS_SURFACE as _SURFACE;
