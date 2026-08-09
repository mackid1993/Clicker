//! Onboarding, and the settings screen.
//!
//! Both edit the same things, so they share the pieces. The difference is
//! framing: onboarding is a single centered card with one job, because someone
//! who has just installed this has no idea what a Channels DVR API is and
//! should only have to find the box's address.

use eframe::egui;

use crate::settings::{Server, Settings};
use crate::theme::{self, Fluent, RADIUS_SURFACE, SPACE_L, SPACE_M, SPACE_S};

/// What the setup screens are asking the application to do.
pub enum SetupAction {
    None,
    /// Try this address and, if it answers, add it.
    Probe(String),
    /// Switch to an already-configured server.
    Select(usize),
    Remove(usize),
    /// Persist whatever is currently in the settings struct.
    Save,
}

#[derive(Default)]
pub struct SetupState {
    /// What is currently typed in the address box.
    pub address: String,
    /// Set while a probe is in flight, so the button can say so and cannot be
    /// pressed twice.
    pub probing: bool,
    /// The result of the last probe, good or bad.
    pub message: Option<(String, bool)>,
    /// Why the configured folders were refused, if they were.
    ///
    /// Held rather than recomputed, because finding out means writing a file
    /// to the directory and a settings screen redraws sixty times a second.
    /// Checked when the field is left, which is when the answer can change.
    pub download_dir_error: Option<String>,
    pub buffer_dir_error: Option<String>,
}

/// First run: no server configured yet.
pub fn onboarding(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    settings: &mut Settings,
    state: &mut SetupState,
) -> SetupAction {
    let mut action = SetupAction::None;

    // Sized so the content has room to breathe rather than filling it edge to
    // edge. The previous height left the Connect button pressed against the
    // bottom border with no margin at all.
    let card = egui::Rect::from_center_size(rect.center(), egui::vec2(480.0, 424.0));

    // Fluent's elevation is a soft shadow and a translucent fill, not an
    // outline. A card with a drawn border reads as a web dialog.
    ui.painter().rect_filled(
        card.translate(egui::vec2(0.0, 10.0)),
        RADIUS_SURFACE + 4.0,
        egui::Color32::from_black_alpha(60),
    );
    ui.painter().rect_filled(card, RADIUS_SURFACE, Fluent::LAYER_CARD);
    ui.painter().rect_stroke(
        card,
        RADIUS_SURFACE,
        egui::Stroke::new(1.0, Fluent::STROKE_SURFACE),
    );

    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(card.shrink2(egui::vec2(SPACE_L * 2.0, SPACE_L * 1.75)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    inner.spacing_mut().item_spacing.y = 0.0;

    inner.label(
        egui::RichText::new(format!("Welcome to {}", crate::APP_NAME))
            .size(26.0)
            .color(Fluent::TEXT_PRIMARY),
    );
    inner.add_space(SPACE_S);
    inner.label(
        egui::RichText::new("Enter the address of your Channels DVR to get started.")
            .size(13.0)
            .color(Fluent::TEXT_SECONDARY),
    );
    inner.add_space(SPACE_L * 1.75);

    inner.label(
        egui::RichText::new("DVR ADDRESS")
            .size(11.0)
            .color(Fluent::TEXT_TERTIARY),
    );
    inner.add_space(SPACE_S);

    // No example address as placeholder text. Someone glancing at this cannot
    // tell a grayed-out suggestion from a value that is already filled in, and
    // a field that looks populated next to a disabled Connect button reads as
    // the application being broken.
    let entered = inner
        .add(
            egui::TextEdit::singleline(&mut state.address)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(SPACE_S, SPACE_S)),
        )
        .lost_focus()
        && inner.input(|i| i.key_pressed(egui::Key::Enter));

    inner.add_space(SPACE_S);
    inner.label(
        egui::RichText::new(
            "A hostname or IP. Add :port if yours is not on 8089, or paste a full URL.",
        )
        .size(11.0)
        .color(Fluent::TEXT_TERTIARY),
    );

    inner.add_space(SPACE_L * 1.5);
    inner.label(
        egui::RichText::new("THIS DEVICE")
            .size(11.0)
            .color(Fluent::TEXT_TERTIARY),
    );
    inner.add_space(SPACE_S);
    inner.add(
        egui::TextEdit::singleline(&mut settings.client_name)
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(SPACE_S, SPACE_S)),
    );
    inner.add_space(SPACE_S);
    inner.label(
        egui::RichText::new("How this machine appears in the DVR's client list.")
            .size(11.0)
            .color(Fluent::TEXT_TERTIARY),
    );

    inner.add_space(SPACE_L * 1.75);

    let can_connect = !state.address.trim().is_empty() && !state.probing;
    let pressed = inner
        .add_enabled(
            can_connect,
            egui::Button::new(if state.probing { "Connecting…" } else { "Connect" })
                .min_size(egui::vec2(120.0, 34.0)),
        )
        .clicked();

    if (pressed || entered) && can_connect {
        action = SetupAction::Probe(state.address.clone());
    }

    if let Some((message, ok)) = &state.message {
        inner.add_space(SPACE_M);
        inner.label(
            egui::RichText::new(message)
                .size(12.0)
                .color(if *ok { Fluent::SUCCESS } else { Fluent::LIVE }),
        );
    }

    action
}

/// The settings screen.
pub fn settings_screen(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    settings: &mut Settings,
    state: &mut SetupState,
) -> SetupAction {
    let mut action = SetupAction::None;

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(SPACE_L * 3.0, SPACE_L)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut content, |ui| {
            ui.add_space(SPACE_L);
            ui.label(
                egui::RichText::new("Settings")
                    .size(24.0)
                    .color(Fluent::TEXT_PRIMARY),
            );
            ui.add_space(SPACE_L * 1.5);

            // ── Servers ────────────────────────────────────────────────
            section(ui, "DVR servers", "Switch between them at any time.");

            let active = settings.active;
            let mut to_select = None;
            let mut to_remove = None;

            for (index, server) in settings.servers.iter().enumerate() {
                let selected = index == active;
                let (row, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().min(560.0), 58.0),
                    egui::Sense::click(),
                );

                let fill = if selected {
                    Fluent::CONTROL
                } else if response.hovered() {
                    Fluent::CONTROL_HOVER
                } else {
                    Fluent::LAYER_CARD
                };
                ui.painter().rect_filled(row, theme::RADIUS_CONTROL, fill);
                if selected {
                    ui.painter().rect_stroke(
                        row,
                        theme::RADIUS_CONTROL,
                        egui::Stroke::new(1.0, Fluent::ACCENT),
                    );
                }

                let name = if server.name.is_empty() { "Channels DVR" } else { &server.name };
                ui.painter().text(
                    egui::pos2(row.min.x + SPACE_M, row.min.y + 14.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    egui::FontId::proportional(14.0),
                    Fluent::TEXT_PRIMARY,
                );
                let detail = if server.version.is_empty() {
                    server.url.clone()
                } else {
                    format!("{}  ·  v{}", server.url, server.version)
                };
                ui.painter().text(
                    egui::pos2(row.min.x + SPACE_M, row.min.y + 33.0),
                    egui::Align2::LEFT_TOP,
                    detail,
                    egui::FontId::proportional(11.0),
                    Fluent::TEXT_TERTIARY,
                );

                if selected {
                    ui.painter().text(
                        egui::pos2(row.max.x - 78.0, row.center().y),
                        egui::Align2::LEFT_CENTER,
                        "In use",
                        egui::FontId::proportional(11.0),
                        Fluent::ACCENT,
                    );
                }

                // Remove sits at the right edge and is only offered on the ones
                // that are not currently in use, so there is no way to
                // disconnect yourself by accident.
                if !selected {
                    let remove = egui::Rect::from_center_size(
                        egui::pos2(row.max.x - 22.0, row.center().y),
                        egui::vec2(28.0, 28.0),
                    );
                    let remove_response =
                        ui.interact(remove, egui::Id::new(("rm", index)), egui::Sense::click());
                    if remove_response.hovered() {
                        ui.painter()
                            .rect_filled(remove, theme::RADIUS_CONTROL, Fluent::CONTROL_HOVER);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.painter().text(
                        remove.center(),
                        egui::Align2::CENTER_CENTER,
                        "\u{E74D}", // Delete
                        egui::FontId::new(13.0, egui::FontFamily::Name(theme::ICON_FONT.into())),
                        Fluent::TEXT_SECONDARY,
                    );
                    if remove_response.clicked() {
                        to_remove = Some(index);
                    } else if response.clicked() {
                        to_select = Some(index);
                    }
                }

                ui.add_space(SPACE_S);
            }

            if let Some(index) = to_remove {
                action = SetupAction::Remove(index);
            } else if let Some(index) = to_select {
                action = SetupAction::Select(index);
            }

            ui.add_space(SPACE_S);
            control_row(ui, |ui| {
                let entry = field(ui, &mut state.address, FIELD_W)
                    .hint_text("Add another DVR: host, host:port, or a full URL");
                ui.add(entry);
                let can = !state.address.trim().is_empty() && !state.probing;
                if ui
                    .add_enabled(
                        can,
                        egui::Button::new(if state.probing { "Checking…" } else { "Add" })
                            .min_size(egui::vec2(72.0, ROW_H)),
                    )
                    .clicked()
                {
                    action = SetupAction::Probe(state.address.clone());
                }
            });

            if let Some((message, ok)) = &state.message {
                ui.add_space(SPACE_S);
                ui.label(
                    egui::RichText::new(message)
                        .size(12.0)
                        .color(if *ok { Fluent::SUCCESS } else { Fluent::LIVE }),
                );
            }

            // ── This device ────────────────────────────────────────────
            ui.add_space(SPACE_L * 1.5);
            // Says "logs", not "client list", because that is the truth. A DVR
            // identifies a streaming client by its IP address and nothing
            // else — checked against a real server, which ignored both the
            // User-Agent and every plausible query parameter. The name reaches
            // the logs and stops there, and a setting that claims more than it
            // does is worse than one that claims less.
            section(
                ui,
                "This device",
                &format!(
                    "Identifies {} in the DVR's logs. Channels lists connected \
                     clients by IP address, so this will not change what appears there.",
                    crate::APP_NAME
                ),
            );
            control_row(ui, |ui| {
                let name = field(ui, &mut settings.client_name, FIELD_W);
                if ui.add(name).lost_focus() {
                    action = SetupAction::Save;
                }
                if ui
                    .add(
                        egui::Button::new("Use computer name")
                            .min_size(egui::vec2(0.0, ROW_H)),
                    )
                    .clicked()
                {
                    settings.client_name = crate::settings::hostname();
                    action = SetupAction::Save;
                }
            });

            // ── Quality ────────────────────────────────────────────────
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "Streaming quality",
                "Transcoding happens on the DVR, not here.",
            );

            let mut original = settings.original_quality;
            if ui
                .radio_value(&mut original, true, "Original — no re-encoding, best quality")
                .clicked()
            {
                settings.original_quality = true;
                action = SetupAction::Save;
            }
            if ui
                .radio_value(&mut original, false, "Transcoded — smaller, for a weak connection")
                .clicked()
            {
                settings.original_quality = false;
                action = SetupAction::Save;
            }

            if !settings.original_quality {
                ui.add_space(SPACE_S);
                control_row(ui, |ui| {
                    ui.add_space(SPACE_L);
                    ui.label(
                        egui::RichText::new("Height")
                            .size(12.0)
                            .color(Fluent::TEXT_SECONDARY),
                    );
                    for height in [1080u32, 720, 540, 360] {
                        if ui
                            .selectable_label(settings.transcode_height == height, format!("{height}p"))
                            .clicked()
                        {
                            settings.transcode_height = height;
                            // Bitrates that suit each size. Left adjustable,
                            // but a sensible default matters more than a
                            // number nobody wants to choose.
                            settings.transcode_kbps = match height {
                                1080 => 8000,
                                720 => 4000,
                                540 => 2500,
                                _ => 1200,
                            };
                            action = SetupAction::Save;
                        }
                    }
                });
            }

            // ── Playback ───────────────────────────────────────────────
            ui.add_space(SPACE_L * 1.5);
            section(ui, "Playback", "How far the skip buttons and arrow keys jump.");

            control_row(ui, |ui| {
                ui.label(
                    egui::RichText::new("Skip back")
                        .size(12.0)
                        .color(Fluent::TEXT_SECONDARY),
                );
                egui::ComboBox::from_id_salt("skip-back")
                    .selected_text(format!("{} seconds", settings.skip_back_secs))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for secs in [5u32, 10, 15, 20, 30, 60] {
                            if ui
                                .selectable_label(
                                    settings.skip_back_secs == secs,
                                    format!("{secs} seconds"),
                                )
                                .clicked()
                            {
                                settings.skip_back_secs = secs;
                                action = SetupAction::Save;
                            }
                        }
                    });

                ui.add_space(SPACE_L);
                ui.label(
                    egui::RichText::new("Skip forward")
                        .size(12.0)
                        .color(Fluent::TEXT_SECONDARY),
                );
                egui::ComboBox::from_id_salt("skip-forward")
                    .selected_text(format!("{} seconds", settings.skip_forward_secs))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for secs in [10u32, 15, 30, 60, 90, 120] {
                            if ui
                                .selectable_label(
                                    settings.skip_forward_secs == secs,
                                    format!("{secs} seconds"),
                                )
                                .clicked()
                            {
                                settings.skip_forward_secs = secs;
                                action = SetupAction::Save;
                            }
                        }
                    });
            });

            // ── Live buffer ────────────────────────────────────────────
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "Live buffer",
                "Original comes straight from the tuner, which cannot be rewound on \
                 its own. Buffering it to disk is what allows pause and rewind.",
            );
            control_row(ui, |ui| {
                ui.label(
                    egui::RichText::new("Keep")
                        .size(12.0)
                        .color(Fluent::TEXT_SECONDARY),
                );
                egui::ComboBox::from_id_salt("live-buffer")
                    .selected_text(match settings.live_buffer_gb {
                        0 => "Off — no rewind".to_string(),
                        gb => format!("{gb} GB"),
                    })
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        // Roughly two gigabytes an hour on a broadcast stream,
                        // so the sizes are shown with what they actually buy.
                        for (gb, label) in [
                            (0u32, "Off — no rewind".to_string()),
                            (2, "2 GB  ·  about 1 hour".to_string()),
                            (4, "4 GB  ·  about 2 hours".to_string()),
                            (8, "8 GB  ·  about 4 hours".to_string()),
                            (16, "16 GB  ·  about 8 hours".to_string()),
                            (32, "32 GB  ·  about 16 hours".to_string()),
                        ] {
                            if ui
                                .selectable_label(settings.live_buffer_gb == gb, label)
                                .clicked()
                            {
                                settings.live_buffer_gb = gb;
                                action = SetupAction::Save;
                            }
                        }
                    });
            });
            ui.label(
                egui::RichText::new(
                    "The buffer recycles rather than filling up: once it reaches this \
                     size the oldest part is released and recording continues, so a \
                     channel can be left on all day. It is deleted when playback stops.",
                )
                .size(11.0)
                .color(Fluent::TEXT_TERTIARY),
            );

            // ── Closing ────────────────────────────────────────────────
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "When the window is closed",
                &format!("Downloads only continue while {} is running.", crate::APP_NAME),
            );
            control_row(ui, |ui| {
                let mut to_tray = settings.minimize_to_tray;
                if ui
                    .checkbox(&mut to_tray, "Keep running in the notification area")
                    .on_hover_text(
                        "On: closing the window hides it instead, and downloads carry on. \
                         Quit from the tray icon.",
                    )
                    .changed()
                {
                    settings.minimize_to_tray = to_tray;
                    action = SetupAction::Save;
                }
            });

            // ── Where things are kept ──────────────────────────────────
            //
            // Both default to the user profile, which is on C: on almost every
            // machine, and neither is small: a download is an entire recording
            // and the live buffer can be 32GB. A second drive is exactly what
            // people have them for.
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "Where things are kept",
                "Leave either blank to keep it beside the application's own data. \
                 Changing one applies to what happens next; anything already \
                 downloaded stays where it is.",
            );

            for (label, value, error, hint) in [
                (
                    "Downloads",
                    &mut settings.download_dir,
                    &mut state.download_dir_error,
                    "e.g. M:\\Clicker\\Downloads",
                ),
                (
                    "Live buffer",
                    &mut settings.buffer_dir,
                    &mut state.buffer_dir_error,
                    "e.g. M:\\Clicker\\Buffer",
                ),
            ] {
                control_row(ui, |ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(12.0)
                            .color(Fluent::TEXT_SECONDARY),
                    );
                    let entry = field(ui, value, FIELD_W).hint_text(hint);
                    if ui.add(entry).lost_focus() {
                        // Checked by writing to it. A path can exist, be
                        // readable, and still refuse a write for reasons no
                        // attribute reports, and finding that out when a
                        // recording is half downloaded is too late.
                        *error = if value.trim().is_empty() {
                            None
                        } else {
                            crate::settings::writable(std::path::Path::new(value.trim()))
                                .err()
                                .map(|e| format!("{e:#}"))
                        };
                        action = SetupAction::Save;
                    }
                });
                if let Some(message) = error {
                    ui.label(
                        egui::RichText::new(message.as_str())
                            .size(11.0)
                            .color(Fluent::LIVE),
                    );
                }
            }

            // ── Keyboard ───────────────────────────────────────────────
            //
            // Documented here because an undocumented shortcut may as well not
            // exist: nobody discovers a key by pressing every key.
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "Keyboard",
                "The whole application can be driven without a mouse.",
            );
            for (keys, what) in crate::SHORTCUTS {
                ui.horizontal(|ui| {
                    ui.add_space(SPACE_S / 2.0);
                    ui.label(
                        egui::RichText::new(*keys)
                            .size(12.0)
                            .monospace()
                            .color(Fluent::TEXT_PRIMARY),
                    );
                    // A fixed column, so the descriptions line up into a list
                    // rather than stepping in and out with the key names.
                    let used = ui.min_rect().width();
                    ui.add_space((KEY_COLUMN - used).max(SPACE_S));
                    ui.label(
                        egui::RichText::new(*what)
                            .size(12.0)
                            .color(Fluent::TEXT_SECONDARY),
                    );
                });
            }

            // ── About ──────────────────────────────────────────────────
            //
            // The in-app third-party notice the LGPL asks for, alongside the
            // one in the installer and the one in the repository.
            ui.add_space(SPACE_L * 1.5);
            section(ui, "About", "");
            ui.label(
                egui::RichText::new(format!("{} {}", crate::APP_NAME, env!("CARGO_PKG_VERSION")))
                    .size(13.0)
                    .color(Fluent::TEXT_PRIMARY),
            );
            ui.label(
                egui::RichText::new(crate::player::Player::backend())
                    .size(12.0)
                    .color(Fluent::TEXT_TERTIARY),
            );

            // Who this is not.
            //
            // In the application and not only in the README, because the
            // person who needs to read it is the one about to ask Channels'
            // support why their DVR client is misbehaving — and they are
            // looking at this window, not at a repository.
            ui.add_space(SPACE_S);
            ui.label(
                egui::RichText::new(format!(
                    "{} is not affiliated with Fancy Bits, LLC and is an unofficial \
                     client to Channels DVR Server. It is not endorsed or supported \
                     by them, so please do not ask them about it — anything wrong \
                     with this program is wrong with this program.",
                    crate::APP_NAME
                ))
                .size(11.0)
                .color(Fluent::TEXT_TERTIARY),
            );

            ui.add_space(SPACE_S / 2.0);
            ui.label(
                egui::RichText::new(
                    "Media playback uses FFmpeg, licensed under the LGPL v2.1 or later. \
                     The FFmpeg libraries are separate files beside the application and \
                     may be replaced with any compatible build. License texts are in the \
                     installation folder.",
                )
                .size(11.0)
                .color(Fluent::TEXT_TERTIARY),
            );
            ui.hyperlink_to(
                egui::RichText::new("ffmpeg.org").size(11.0),
                "https://ffmpeg.org/",
            );

            // ── Reporting a problem ────────────────────────────────────
            //
            // The player writes what it is doing to a file, and this is how
            // someone finds it without being talked through a terminal. That
            // conversation is the reason a stutter reported from another
            // machine could not be diagnosed at all: the numbers that identify
            // the cause were produced at the moment it happened and written to
            // a console that a windowed build does not have.
            ui.add_space(SPACE_L * 1.5);
            section(
                ui,
                "Reporting a problem",
                "Playback writes a log. If something stutters or will not play, \
                 send this file with the report.",
            );
            control_row(ui, |ui| {
                if ui
                    .add(egui::Button::new("Open the log folder").min_size(egui::vec2(0.0, ROW_H)))
                    .clicked()
                {
                    if let Some(path) = crate::log::path() {
                        // Explorer selects the file rather than merely opening
                        // the directory, so there is no second step of finding
                        // it among the settings and the crash log.
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", path.display()))
                            .spawn();
                    }
                }
                if let Some(path) = crate::log::path() {
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .size(11.0)
                            .color(Fluent::TEXT_TERTIARY),
                    );
                }
            });

            // There is deliberately no "warn about unapproved server versions"
            // toggle. Everyone runs the beta, the warning is noise, and a
            // setting whose only sensible value is off is not a choice worth
            // presenting — it is a default nobody asked to make. The field
            // stays in the settings file, permanently suppressed.

            ui.add_space(SPACE_L * 2.0);
        });

    action
}

/// The height every control in a settings row is forced to.
const ROW_H: f32 = 34.0;

/// How far in the description column of the keyboard list starts.
const KEY_COLUMN: f32 = 150.0;

/// How wide a single-line setting field is.
///
/// The old 320 left the address field narrower than the text it was asking for
/// — "Add another DVR: host, host:port, or a full URL" ran to the very edge of
/// it — which made the field read as too small for its own purpose.
const FIELD_W: f32 = 420.0;

/// One row of controls, all the same height and centered on one line.
///
/// egui derives a TextEdit's height from its text margin, a Button's from its
/// padding and a ComboBox's from neither, so controls meant to read as a single
/// row settle at three different heights and, in a `horizontal` layout, three
/// different baselines. That is what put the Add button below its field, the
/// "Use computer name" button below its field, and the skip-forward dropdown a
/// dozen pixels below skip-back. Fixing the height here means a row cannot come
/// out uneven, whatever it is built from.
fn control_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::hover(),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().interact_size.y = ROW_H;
    // Buttons and ComboBoxes both take their height from this.
    row.spacing_mut().button_padding = egui::vec2(SPACE_M, pad_for(&row));
    row.spacing_mut().item_spacing.x = SPACE_S;
    add(&mut row)
}

/// The vertical padding that brings one line of text up to `ROW_H`.
fn pad_for(ui: &egui::Ui) -> f32 {
    let line = ui.text_style_height(&egui::TextStyle::Body);
    ((ROW_H - line) / 2.0).max(2.0)
}

/// A single-line field sized to match `control_row`.
fn field<'a>(ui: &egui::Ui, text: &'a mut String, width: f32) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .desired_width(width)
        .margin(egui::Margin::symmetric(SPACE_S, pad_for(ui)))
}

fn section(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(15.0)
            .color(Fluent::TEXT_PRIMARY),
    );
    if !detail.is_empty() {
        ui.label(
            egui::RichText::new(detail)
                .size(11.0)
                .color(Fluent::TEXT_TERTIARY),
        );
    }
    ui.add_space(SPACE_S);
}

/// Build a `Server` from a probe result.
pub fn server_from_probe(url: String, info: crate::settings::ServerInfo) -> Server {
    Server {
        url,
        name: info.name,
        version: info.version,
    }
}
