use crate::commands::{self, LocalCommand};
use crate::state::{AppState, BotStatus, LogLevel};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke};

const BG: Color32 = Color32::BLACK;
const SURFACE: Color32 = Color32::from_rgb(13, 13, 13);
const BORDER: Color32 = Color32::from_rgb(43, 43, 43);
const TEXT: Color32 = Color32::from_rgb(238, 238, 238);
const MUTED: Color32 = Color32::from_rgb(142, 142, 142);
const DIM: Color32 = Color32::from_rgb(92, 92, 92);
const RED: Color32 = Color32::from_rgb(215, 25, 33);
const GREEN: Color32 = Color32::from_rgb(114, 196, 139);
const AMBER: Color32 = Color32::from_rgb(214, 171, 91);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Dashboard,
    Commands,
    Logs,
    Settings,
}

pub fn sidebar(ctx: &egui::Context, current: &mut Page) {
    egui::TopBottomPanel::top("navigation")
        .exact_height(34.0)
        .frame(
            egui::Frame::new()
                .fill(BG)
                .stroke(Stroke::new(1.0_f32, BORDER))
                .inner_margin(egui::Margin::symmetric(10, 5)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").size(10.0).color(RED));
                ui.label(RichText::new("TRC").size(12.0).strong().color(TEXT));
                ui.add_space(6.0);
                nav_button(ui, current, Page::Dashboard, "STATUS");
                nav_button(ui, current, Page::Commands, "CMD");
                nav_button(ui, current, Page::Logs, "LOG");
                nav_button(ui, current, Page::Settings, "SET");
            });
        });
}

pub fn dashboard(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            mono_label(ui, "BOT STATUS");
            let (text, color) = match state.bot_status {
                BotStatus::Online => ("ONLINE", GREEN),
                BotStatus::Starting => ("STARTING", AMBER),
                BotStatus::Offline => ("OFFLINE", MUTED),
            };
            ui.label(RichText::new(text).size(22.0).strong().color(color));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match state.bot_status {
                BotStatus::Offline => {
                    if primary_button(ui, "START").clicked() {
                        state.bot_status = BotStatus::Starting;
                        state.push_log(LogLevel::Info, "Bot", "Start requested");
                    }
                }
                _ => {
                    if secondary_button(ui, "STOP").clicked() {
                        state.bot_status = BotStatus::Offline;
                        state.push_log(LogLevel::Warning, "Bot", "Bot stopped");
                    }
                }
            }
        });
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(8.0);
    ui.columns(3, |columns| {
        metric(
            &mut columns[0],
            "CPU",
            &format!("{:.0}%", state.system.cpu_percent),
        );
        metric(
            &mut columns[1],
            "RAM",
            &format!(
                "{:.1}/{:.1}G",
                state.system.memory_used_gib, state.system.memory_total_gib
            ),
        );
        metric(&mut columns[2], "BOT", state.bot_status.label());
    });

    ui.add_space(10.0);
    mono_label(ui, "RECENT ACTIVITY");
    ui.add_space(5.0);
    log_rows(ui, state, 6);
}

pub fn commands(ui: &mut egui::Ui, state: &mut AppState) {
    page_title(ui, "COMMANDS", "LOCAL QUICK ACTIONS");
    egui::Grid::new("quick_commands")
        .num_columns(2)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            for (index, command) in [
                LocalCommand::Screenshot,
                LocalCommand::LockPc,
                LocalCommand::MonitorOff,
                LocalCommand::Mute,
                LocalCommand::OpenBrowser,
                LocalCommand::OpenSpotify,
            ]
            .into_iter()
            .enumerate()
            {
                if command_button(ui, command.label()).clicked() {
                    state.push_log(
                        LogLevel::Info,
                        "GUI",
                        format!("Executing: {}", command.label()),
                    );
                    match commands::execute(command, &state.download_dir) {
                        Ok(message) => state.push_log(LogLevel::Success, "Command", message),
                        Err(error) => state.push_log(
                            LogLevel::Warning,
                            "Command",
                            format!("{} failed: {error}", command.label()),
                        ),
                    }
                }
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });

    ui.add_space(9.0);
    mono_label(ui, "POWER");
    ui.horizontal_wrapped(|ui| {
        for name in ["SLEEP", "HIBERNATE", "RESTART", "SHUTDOWN"] {
            if secondary_button(ui, name).clicked() {
                state.push_log(
                    LogLevel::Warning,
                    "GUI",
                    format!("Requested power action: {name}"),
                );
            }
        }
    });
}

pub fn logs(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| page_title(ui, "ACTIVITY", "EVENT STREAM"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if secondary_button(ui, "CLEAR").clicked() {
                state.logs.clear();
                state.push_log(LogLevel::Info, "App", "Log cleared");
            }
        });
    });
    log_rows(ui, state, usize::MAX);
}

pub fn settings(ui: &mut egui::Ui, state: &mut AppState) {
    page_title(ui, "SETTINGS", "PORTABLE CONFIG");
    egui::ScrollArea::vertical().show(ui, |ui| {
        settings_field(ui, "BOT TOKEN", &mut state.telegram_token, true);
        settings_field(ui, "ALLOWED USER ID", &mut state.allowed_user_id, false);
        settings_field(ui, "DOWNLOAD DIR", &mut state.download_dir, false);
        ui.add_space(4.0);
        ui.checkbox(&mut state.hide_to_tray, "Close window to tray");
        ui.checkbox(&mut state.start_minimized, "Start minimized");
        ui.add_space(8.0);
        if primary_button(ui, "SAVE").clicked() {
            state.save_settings();
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(state.config_path.display().to_string())
                .monospace()
                .size(10.0)
                .color(DIM),
        );
    });
}

fn nav_button(ui: &mut egui::Ui, current: &mut Page, page: Page, label: &str) {
    let selected = *current == page;
    let text = if selected { TEXT } else { MUTED };
    let response = ui.add(
        egui::Button::new(RichText::new(label).monospace().size(11.0).color(text))
            .fill(if selected {
                SURFACE
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::NONE)
            .corner_radius(4),
    );
    if response.clicked() {
        *current = page;
    }
}

fn page_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(17.0).strong().color(TEXT));
    ui.label(RichText::new(subtitle).monospace().size(10.0).color(MUTED));
    ui.add_space(10.0);
}

fn mono_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).monospace().size(10.0).color(MUTED));
}

fn metric(ui: &mut egui::Ui, label: &str, value: &str) {
    mono_label(ui, label);
    ui.label(RichText::new(value).size(16.0).color(TEXT));
}

fn log_rows(ui: &mut egui::Ui, state: &AppState, max_entries: usize) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for entry in state.logs.iter().rev().take(max_entries) {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&entry.timestamp)
                        .monospace()
                        .size(9.0)
                        .color(DIM),
                );
                let color = match entry.level {
                    LogLevel::Info => MUTED,
                    LogLevel::Success => GREEN,
                    LogLevel::Warning => AMBER,
                };
                ui.label(
                    RichText::new(entry.source.to_uppercase())
                        .monospace()
                        .size(9.0)
                        .color(color),
                );
                ui.label(RichText::new(&entry.message).size(11.0).color(TEXT));
            });
            ui.add_space(3.0);
        }
    });
}

fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .monospace()
                .size(11.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(RED)
        .stroke(Stroke::NONE)
        .corner_radius(4)
        .min_size(egui::vec2(66.0, 26.0)),
    )
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).monospace().size(10.0).color(TEXT))
            .fill(SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(4)
            .min_size(egui::vec2(66.0, 25.0)),
    )
}

fn command_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add_sized(
        [185.0, 30.0],
        egui::Button::new(
            RichText::new(label.to_uppercase())
                .monospace()
                .size(11.0)
                .color(TEXT),
        )
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(4),
    )
}

fn settings_field(ui: &mut egui::Ui, label: &str, value: &mut String, secret: bool) {
    mono_label(ui, label);
    let edit = egui::TextEdit::singleline(value)
        .password(secret)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(8, 6));
    ui.add(edit);
    ui.add_space(6.0);
}
