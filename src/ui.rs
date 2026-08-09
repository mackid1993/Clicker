//! Screens, and the chrome around them.
//!
//! Everything here draws with the Fluent tokens in `theme`, over the backdrop
//! the window paints for itself. The rule the whole interface follows: the
//! picture is the subject, and anything that is not the picture gets out of
//! the way.

use eframe::egui;

use crate::images::Images;
use crate::library::{Home, Recording};
use crate::theme::{self, Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S, SPACE_XS};

/// Which screen is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    Guide,
    Library,
    Recordings,
    Downloads,
    Settings,
}

impl Screen {
    pub const ALL: [Screen; 6] = [
        Screen::Home,
        Screen::Guide,
        Screen::Library,
        Screen::Recordings,
        Screen::Downloads,
        Screen::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Screen::Home => "Home",
            Screen::Guide => "Guide",
            Screen::Library => "Library",
            Screen::Recordings => "Recordings",
            Screen::Downloads => "Downloads",
            Screen::Settings => "Settings",
        }
    }

    /// Segoe Fluent Icons glyphs, so the navigation reads as Windows rather
    /// than as a web app that happens to run on it.
    pub fn glyph(self) -> &'static str {
        match self {
            Screen::Home => "\u{E80F}",       // Home
            Screen::Guide => "\u{E8BC}",      // GridView
            Screen::Library => "\u{E8F1}",    // Library
            Screen::Recordings => "\u{E7C8}", // Record
            Screen::Downloads => "\u{E896}",  // Download
            Screen::Settings => "\u{E713}",   // Settings
        }
    }
}

/// The navigation rail.
///
/// Collapsed it is a column of icons; expanded it shows labels beside them.
/// This is the Windows pattern — the same one Settings and the Store use — and
/// the hamburger toggles between the two rather than opening a menu, because a
/// rail that is already visible does not need to be summoned.
pub fn nav_rail(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    current: &mut Screen,
    expanded: &mut bool,
) -> bool {
    let mut changed = false;
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, with_alpha(Fluent::SOLID, 140));
    painter.line_segment(
        [
            egui::pos2(rect.max.x, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        ],
        egui::Stroke::new(1.0, Fluent::STROKE_DIVIDER),
    );

    let mut y = rect.min.y + SPACE_S;

    // The hamburger sits above the items, in the position Windows puts it.
    let toggle = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + SPACE_XS, y),
        egui::vec2(rect.width() - SPACE_XS * 2.0, 40.0),
    );
    let response = ui.interact(toggle, egui::Id::new("nav-toggle"), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(toggle, theme::RADIUS_CONTROL, Fluent::CONTROL_HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(
        egui::pos2(toggle.min.x + 20.0, toggle.center().y),
        egui::Align2::CENTER_CENTER,
        theme::icon::HAMBURGER,
        egui::FontId::new(15.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
        Fluent::TEXT_PRIMARY,
    );
    if response.clicked() {
        *expanded = !*expanded;
    }
    y += 48.0;

    for screen in Screen::ALL {
        let item = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + SPACE_XS, y),
            egui::vec2(rect.width() - SPACE_XS * 2.0, 40.0),
        );
        let id = egui::Id::new(("nav", screen.label()));
        let mut response = ui.interact(item, id, egui::Sense::click());
        // A collapsed rail is icons only, so the name rides on the tooltip.
        if !*expanded {
            response = response.on_hover_text(screen.label());
        }
        let selected = *current == screen;

        // Hover eases in and out; the selection bar grows from the middle.
        // Both are Fluent's own motions for this exact control.
        let hover = ui
            .ctx()
            .animate_bool_with_time(id.with("h"), response.hovered(), theme::ANIM_FAST);
        let select = ui
            .ctx()
            .animate_bool_with_time(id.with("s"), selected, theme::ANIM_NORMAL);

        if selected {
            ui.painter()
                .rect_filled(item, theme::RADIUS_CONTROL, Fluent::CONTROL);
        } else if hover > 0.01 {
            ui.painter().rect_filled(
                item,
                theme::RADIUS_CONTROL,
                theme::mix(egui::Color32::TRANSPARENT, Fluent::CONTROL_HOVER, hover),
            );
        }
        if select > 0.01 {
            let half = 8.0 * select;
            let bar = egui::Rect::from_min_max(
                egui::pos2(item.min.x + 2.0, item.center().y - half),
                egui::pos2(item.min.x + 5.0, item.center().y + half),
            );
            ui.painter().rect_filled(bar, 1.5, Fluent::ACCENT);
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        ui.painter().text(
            egui::pos2(item.min.x + 20.0, item.center().y),
            egui::Align2::CENTER_CENTER,
            screen.glyph(),
            egui::FontId::new(15.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            if selected { Fluent::ACCENT } else { Fluent::TEXT_PRIMARY },
        );

        // Labels appear once the animating rail is wide enough to hold them,
        // so they slide into existence with the surface instead of popping.
        if rect.width() > 96.0 {
            ui.painter().text(
                egui::pos2(item.min.x + 44.0, item.center().y),
                egui::Align2::LEFT_CENTER,
                screen.label(),
                egui::FontId::proportional(14.0),
                if selected { Fluent::TEXT_PRIMARY } else { Fluent::TEXT_SECONDARY },
            );
        }

        if response.clicked() && !selected {
            *current = screen;
            changed = true;
        }
        y += 44.0;
    }

    changed
}

/// What the home screen wants to do next.
pub enum Action {
    None,
    Play(Recording),
    WatchLive,
    /// Fetch this recording to local disk for offline playback.
    Download(Recording),
    /// Delete the local copy.
    RemoveDownload(String),
    /// Mark watched or unwatched on the server.
    SetWatched(String, bool),
    /// Delete the recording from the DVR. Destructive, and confirmed first.
    Delete(Recording),
}

/// How many of the unfinished recordings the hero is drawn from.
///
/// Not the whole list. Something abandoned four months ago is not what anyone
/// opened the application to carry on with, and putting it under the largest
/// image on the screen makes the home screen look stale rather than varied.
const HERO_POOL: usize = 5;

/// The home screen.
///
/// `launch` varies the hero. See where it is used.
pub fn home(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &Home,
    images: &mut Images,
    loading: bool,
    launch: u64,
) -> Action {
    let mut action = Action::None;

    if loading && data.continue_watching.is_empty() && data.recent.is_empty() {
        centered_message(ui, rect, "Loading your library", "Reading recordings from the DVR");
        return action;
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

            // The hero is a shuffle, and says so.
            //
            // It used to be whatever was least recently finished, which is
            // defensible and meant the same picture every launch for months at
            // a time. Then it rotated through the unfinished ones, which was
            // better and still looked like a broken "continue watching" —
            // because it sat where continue watching goes and showed something
            // that was not what you last watched.
            //
            // So it is drawn from the whole library and labelled as a pick.
            // Something surfaced from seven thousand recordings nobody was
            // going to scroll to is the point of it; pretending it is the next
            // thing to resume was the mistake.
            let pool: Vec<&Recording> = if data.all.is_empty() {
                data.continue_watching.iter().collect()
            } else {
                data.all.iter().collect()
            };
            if !pool.is_empty() {
                // The launch stamp, so it holds still for the session: a hero
                // that changed while the library finished loading would swap
                // out from under a click.
                let pick = (launch % pool.len() as u64) as usize;
                if let Some(a) = hero(ui, pool[pick], images) {
                    action = a;
                }
                ui.add_space(SPACE_L * 1.5);
            }

            // Everything unfinished, in order, with nothing held back for the
            // hero. The hero is no longer taken from this list, so the first
            // card here is the most recently watched — which is what a row
            // called Continue watching has to promise.
            let rest: Vec<&Recording> = data.continue_watching.iter().collect();
            if !rest.is_empty() {
                if let Some(a) = row(ui, "Continue watching", &rest, images) {
                    action = a;
                }
            }
            if !data.up_next.is_empty() {
                if let Some(a) = row(ui, "Up next", &data.up_next.iter().collect::<Vec<_>>(), images)
                {
                    action = a;
                }
            }
            if !data.recent.is_empty() {
                if let Some(a) =
                    row(ui, "Recently recorded", &data.recent.iter().collect::<Vec<_>>(), images)
                {
                    action = a;
                }
            }

            ui.add_space(SPACE_L);
            ui.horizontal(|ui| {
                ui.add_space(SPACE_L * 2.0);
                ui.label(
                    egui::RichText::new(format!("{} recordings on this server", data.total_recordings))
                        .size(12.0)
                        .color(Fluent::TEXT_TERTIARY),
                );
            });
            ui.add_space(SPACE_L * 2.0);
        });

    action
}

/// The large panel at the top of the home screen.
fn hero(ui: &mut egui::Ui, item: &Recording, images: &mut Images) -> Option<Action> {
    let mut action = None;
    let width = ui.available_width() - SPACE_L * 4.0;
    let height = (width * 0.32).clamp(200.0, 340.0);

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let card = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + SPACE_L * 2.0, rect.min.y),
        egui::vec2(width, height),
    );

    // The artwork fills the card and the text sits on a gradient over it, so
    // the picture is the panel rather than an inset thumbnail beside a block
    // of text.
    // Asked for at the size it is drawn at. The API hands out artwork URLs
    // sized for a thumbnail, and this is drawn the width of the window.
    let art = crate::images::at_size(item.art(), crate::images::HERO_MAX, crate::images::HERO_MAX * 3 / 4);
    if let Some(texture) = images.get_at(&art, crate::images::HERO_MAX) {
        // Focused above center: a wide crop of a 4:3 still keeps the faces if
        // it favors the top, and loses them if it takes the middle.
        theme::image_cover(ui.painter(), card, RADIUS_SURFACE, texture, 0.25);
    } else {
        ui.painter().rect_filled(card, RADIUS_SURFACE, Fluent::LAYER_CARD);
    }

    // Darken the left half so the text is legible over any image. Drawn as a
    // handful of wide bands rather than a true gradient, which egui has no
    // primitive for; at this width the steps are not visible.
    //
    // The outermost bands carry the card's corner radius on their outer side.
    // Square bands painted over rounded artwork put the corners straight back
    // — which is exactly what they did, most visibly on the left where this is
    // nearly opaque.
    let bands = 24;
    for i in 0..bands {
        let t = i as f32 / bands as f32;
        let x0 = card.min.x + card.width() * t;
        let x1 = card.min.x + card.width() * (t + 1.0 / bands as f32);
        let alpha = ((1.0 - t).powf(1.6) * 225.0) as u8;
        let rounding = if i == 0 {
            egui::Rounding { nw: RADIUS_SURFACE, sw: RADIUS_SURFACE, ne: 0.0, se: 0.0 }
        } else if i == bands - 1 {
            egui::Rounding { ne: RADIUS_SURFACE, se: RADIUS_SURFACE, nw: 0.0, sw: 0.0 }
        } else {
            egui::Rounding::ZERO
        };
        ui.painter().rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, card.min.y), egui::pos2(x1, card.max.y)),
            rounding,
            egui::Color32::from_rgba_unmultiplied(10, 11, 14, alpha),
        );
    }
    ui.painter().rect_stroke(
        card,
        RADIUS_SURFACE,
        egui::Stroke::new(1.0, Fluent::STROKE_SURFACE),
    );

    let text_x = card.min.x + SPACE_L * 2.0;
    let mut y = card.min.y + SPACE_L * 1.5;

    // Said plainly, because a large picture where continue-watching used to be
    // reads as "this is what you were watching" unless it says otherwise. It
    // is a shuffle of the whole library, and the delight only works if the
    // label is honest about that.
    ui.painter().text(
        egui::pos2(text_x, y),
        egui::Align2::LEFT_TOP,
        "SOMETHING FROM YOUR LIBRARY",
        egui::FontId::proportional(11.0),
        Fluent::ACCENT_LIGHT,
    );
    y += 24.0;

    ui.painter().text(
        egui::pos2(text_x, y),
        egui::Align2::LEFT_TOP,
        &item.title,
        egui::FontId::proportional(30.0),
        Fluent::TEXT_PRIMARY,
    );
    y += 40.0;

    // The facts line: whichever of these the server actually knows. Joined
    // rather than laid out in columns, because a film has a year and no
    // episode number and a news bulletin has neither.
    let mut facts: Vec<String> = Vec::new();
    let episode = item.episode_label();
    if !episode.is_empty() {
        facts.push(episode);
    }
    if !item.episode_title.trim().is_empty() {
        facts.push(item.episode_title.clone());
    }
    if let Some(year) = item.year() {
        facts.push(year.to_string());
    }
    if item.duration > 0.0 {
        facts.push(format!("{} min", (item.duration / 60.0).round() as i64));
    }
    if !item.content_rating.trim().is_empty() {
        facts.push(item.content_rating.clone());
    }
    if !item.genres.is_empty() {
        facts.push(item.genres.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
    }
    if !facts.is_empty() {
        ui.painter().text(
            egui::pos2(text_x, y),
            egui::Align2::LEFT_TOP,
            facts.join("  ·  "),
            egui::FontId::proportional(13.0),
            Fluent::TEXT_SECONDARY,
        );
        y += 24.0;
    }

    // The description. Wrapped by hand against the room the picture leaves,
    // because this is painted rather than laid out and there is no widget here
    // to wrap it for us.
    let summary = if item.full_summary.trim().is_empty() {
        item.summary.trim()
    } else {
        item.full_summary.trim()
    };
    if !summary.is_empty() {
        let width = (card.width() * 0.52).max(280.0);
        for line in wrap(summary, width, 13.0).into_iter().take(3) {
            ui.painter().text(
                egui::pos2(text_x, y),
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::proportional(13.0),
                Fluent::TEXT_TERTIARY,
            );
            y += 19.0;
        }
        y += 4.0;
    }

    // Who made it and who is in it, when the server knows. Directors first:
    // it is the shorter list and the one that identifies a film.
    let mut credits: Vec<String> = Vec::new();
    if !item.directors.is_empty() {
        credits.push(format!(
            "Directed by {}",
            item.directors.iter().take(2).cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !item.cast.is_empty() {
        credits.push(item.cast.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    for line in credits {
        ui.painter().text(
            egui::pos2(text_x, y),
            egui::Align2::LEFT_TOP,
            truncate(&line, 78),
            egui::FontId::proportional(12.0),
            Fluent::TEXT_TERTIARY,
        );
        y += 20.0;
    }

    // Progress, with the time left beside it rather than a percentage: nobody
    // wants to know they are 63% through, they want to know whether there is
    // time to finish it. Only for something actually started — on a shuffled
    // pick it is usually zero, and an empty bar says nothing worth the space.
    if item.progress() > 0.0 {
        y += 6.0;
        let bar = egui::Rect::from_min_size(egui::pos2(text_x, y + 6.0), egui::vec2(260.0, 4.0));
        ui.painter().rect_filled(bar, 2.0, with_alpha(Fluent::TEXT_PRIMARY, 50));
        let filled = egui::Rect::from_min_size(
            bar.min,
            egui::vec2(bar.width() * item.progress(), bar.height()),
        );
        ui.painter().rect_filled(filled, 2.0, Fluent::ACCENT);
        ui.painter().text(
            egui::pos2(bar.max.x + SPACE_M, bar.center().y),
            egui::Align2::LEFT_CENTER,
            item.remaining(),
            egui::FontId::proportional(12.0),
            Fluent::TEXT_SECONDARY,
        );
        y += 30.0;
    } else {
        y += 10.0;
    }

    let button = egui::Rect::from_min_size(egui::pos2(text_x, y), egui::vec2(132.0, 38.0));
    let response = ui.interact(button, egui::Id::new(("hero", &item.id)), egui::Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        Fluent::ACCENT_DARK
    } else if response.hovered() {
        Fluent::ACCENT_LIGHT
    } else {
        Fluent::ACCENT
    };
    ui.painter().rect_filled(button, theme::RADIUS_CONTROL, fill);
    ui.painter().text(
        egui::pos2(button.min.x + 22.0, button.center().y),
        egui::Align2::CENTER_CENTER,
        theme::icon::PLAY,
        egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
        egui::Color32::from_rgb(12, 14, 18),
    );
    ui.painter().text(
        egui::pos2(button.min.x + 42.0, button.center().y),
        egui::Align2::LEFT_CENTER,
        "Resume",
        egui::FontId::proportional(14.0),
        egui::Color32::from_rgb(12, 14, 18),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        action = Some(Action::Play(item.clone()));
    }

    action
}

const CARD_W: f32 = 232.0;
const CARD_H: f32 = 130.0;

/// A titled, horizontally scrolling row of cards.
fn row(ui: &mut egui::Ui, title: &str, items: &[&Recording], images: &mut Images) -> Option<Action> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.add_space(SPACE_L * 2.0);
        ui.label(
            egui::RichText::new(title)
                .size(18.0)
                .color(Fluent::TEXT_PRIMARY),
        );
    });
    ui.add_space(SPACE_S);

    egui::ScrollArea::horizontal()
        .id_salt(title)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(SPACE_L * 2.0);
                for item in items {
                    if let Some(a) = card(ui, item, images) {
                        action = Some(a);
                    }
                    ui.add_space(SPACE_M);
                }
                ui.add_space(SPACE_L);
            });
        });

    ui.add_space(SPACE_L * 1.5);
    action
}

fn card(ui: &mut egui::Ui, item: &Recording, images: &mut Images) -> Option<Action> {
    let mut action = None;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(CARD_W, CARD_H + 52.0), egui::Sense::click());
    let art_rect = egui::Rect::from_min_size(rect.min, egui::vec2(CARD_W, CARD_H));

    // Fluent's reveal: the card lifts slightly and brightens under the
    // pointer, easing both ways. Small enough not to shove the row around,
    // large enough to say which one is under the cursor.
    let hover = ui.ctx().animate_bool_with_time(
        egui::Id::new(("card", &item.id)),
        response.hovered(),
        theme::ANIM_FAST,
    );
    let art_rect = art_rect.translate(egui::vec2(0.0, -2.0 * hover));

    let art = item.art().to_string();
    if let Some(texture) = images.get(&art) {
        theme::image_cover(ui.painter(), art_rect, RADIUS_SURFACE, texture, 0.5);
    } else {
        // A placeholder that is a surface, not an empty hole, so a row does
        // not visibly assemble itself as images arrive.
        ui.painter()
            .rect_filled(art_rect, RADIUS_SURFACE, Fluent::LAYER_CARD);
        ui.painter().text(
            art_rect.center(),
            egui::Align2::CENTER_CENTER,
            "\u{E7F4}", // Video
            egui::FontId::new(22.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            with_alpha(Fluent::TEXT_PRIMARY, 40),
        );
    }

    ui.painter().rect_stroke(
        art_rect,
        RADIUS_SURFACE,
        egui::Stroke::new(
            1.0,
            if response.hovered() { Fluent::STROKE_CONTROL } else { Fluent::STROKE_SURFACE },
        ),
    );

    // How far in, drawn on the artwork itself.
    //
    // Inset by the width of the corner arc at this height, so a full-width bar
    // does not poke out through the rounded corners it sits between. Two pixels
    // at an 8px radius: invisible as a bar, and the difference between a clean
    // corner and a black nub in it.
    if item.progress() > 0.0 {
        let track = egui::Rect::from_min_size(
            egui::pos2(art_rect.min.x + 2.0, art_rect.max.y - 3.0),
            egui::vec2(art_rect.width() - 4.0, 3.0),
        );
        ui.painter().rect_filled(track, 1.5, with_alpha(egui::Color32::BLACK, 150));
        ui.painter().rect_filled(
            egui::Rect::from_min_size(track.min, egui::vec2(track.width() * item.progress(), 3.0)),
            1.5,
            Fluent::ACCENT,
        );
    }

    if response.hovered() {
        ui.painter().rect_filled(art_rect, RADIUS_SURFACE, with_alpha(egui::Color32::BLACK, 90));
        ui.painter().circle_filled(art_rect.center(), 21.0, with_alpha(Fluent::SOLID, 220));
        ui.painter().text(
            art_rect.center(),
            egui::Align2::CENTER_CENTER,
            theme::icon::PLAY,
            egui::FontId::new(15.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
            Fluent::TEXT_PRIMARY,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mut y = art_rect.max.y + SPACE_S;
    ui.painter().text(
        egui::pos2(rect.min.x, y),
        egui::Align2::LEFT_TOP,
        truncate(&item.title, 30),
        egui::FontId::proportional(13.0),
        Fluent::TEXT_PRIMARY,
    );
    y += 19.0;

    let secondary = if item.progress() > 0.0 {
        item.remaining()
    } else {
        item.subtitle()
    };
    ui.painter().text(
        egui::pos2(rect.min.x, y),
        egui::Align2::LEFT_TOP,
        truncate(&secondary, 34),
        egui::FontId::proportional(11.0),
        Fluent::TEXT_TERTIARY,
    );

    if response.clicked() {
        action = Some(Action::Play(item.clone()));
    }
    action
}

pub fn centered_message(ui: &mut egui::Ui, rect: egui::Rect, title: &str, detail: &str) {
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y - 10.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(16.0),
        Fluent::TEXT_PRIMARY,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y + 14.0),
        egui::Align2::CENTER_CENTER,
        detail,
        egui::FontId::proportional(12.0),
        Fluent::TEXT_TERTIARY,
    );
}

/// Break text into lines that fit a width, on word boundaries.
///
/// Roughly six pixels a character at these sizes, the same estimate the guide
/// and the downloads list use. Measuring properly would mean laying the text
/// out twice for a paragraph that is redrawn every frame.
fn wrap(text: &str, width: f32, size: f32) -> Vec<String> {
    let per_line = ((width / (size * 0.48)).floor() as usize).max(16);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > per_line {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn truncate(text: &str, max: usize) -> String {
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
