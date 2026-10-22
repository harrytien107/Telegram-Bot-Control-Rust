use crate::bot::{BotEvent, BotRuntime};
use crate::state::{AppState, LogLevel};
use crate::system::SystemMonitor;
use crate::tray::{AppTray, TrayAction};
use crate::ui::{self, Page};
use eframe::egui;

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::BLACK;
    visuals.window_fill = egui::Color32::BLACK;
    visuals.extreme_bg_color = egui::Color32::from_rgb(8, 8, 8);
    visuals.faint_bg_color = egui::Color32::from_rgb(16, 16, 16);
    visuals.code_bg_color = egui::Color32::from_rgb(14, 14, 14);
    visuals.selection.bg_fill = egui::Color32::from_rgb(215, 25, 33);
    visuals.hyperlink_color = egui::Color32::from_rgb(230, 230, 230);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(12, 12, 12);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(18, 18, 18);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(28, 28, 28);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(215, 25, 33);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

pub struct TelegramControlApp {
    state: AppState,
    page: Page,
    tray: Option<AppTray>,
    exit_requested: bool,
    system_monitor: SystemMonitor,
    last_system_refresh: std::time::Instant,
    bot: Option<BotRuntime>,
    initial_visibility_applied: bool,
}

impl TelegramControlApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);
        let mut state = AppState::default();
        if !state.telegram_token.trim().is_empty() {
            state.bot_status = crate::state::BotStatus::Starting;
            state.push_log(LogLevel::Info, "Bot", "Starting Telegram bot runtime");
        }
        let tray = match AppTray::new() {
            Ok(tray) => {
                state.push_log(LogLevel::Success, "Tray", "System tray icon created");
                Some(tray)
            }
            Err(error) => {
                state.push_log(
                    LogLevel::Warning,
                    "Tray",
                    format!("Tray unavailable: {error}"),
                );
                None
            }
        };
        Self {
            state,
            page: Page::Dashboard,
            tray,
            exit_requested: false,
            system_monitor: SystemMonitor::new(),
            last_system_refresh: std::time::Instant::now() - std::time::Duration::from_secs(5),
            bot: None,
            initial_visibility_applied: false,
        }
    }

    fn apply_initial_visibility(&mut self, ctx: &egui::Context) {
        if self.initial_visibility_applied {
            return;
        }
        self.initial_visibility_applied = true;
        if self.state.start_minimized && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn handle_bot_runtime(&mut self) {
        use crate::state::BotStatus;

        if self.state.bot_status == BotStatus::Starting && self.bot.is_none() {
            match BotRuntime::start(
                self.state.telegram_token.clone(),
                self.state.allowed_user_id.clone(),
                self.state.download_dir.clone(),
            ) {
                Ok(runtime) => {
                    self.bot = Some(runtime);
                    self.state
                        .push_log(LogLevel::Info, "Bot", "Connecting to Telegram");
                }
                Err(error) => {
                    self.state.bot_status = BotStatus::Offline;
                    self.state.push_log(LogLevel::Warning, "Bot", error);
                }
            }
        }

        if self.state.bot_status == BotStatus::Offline && self.bot.is_some() {
            if let Some(runtime) = self.bot.take() {
                runtime.stop();
            }
            self.state
                .push_log(LogLevel::Info, "Bot", "Telegram bot stopped");
        }

        let events = self.bot.as_ref().map(BotRuntime::poll).unwrap_or_default();
        let mut stopped = false;
        for event in events {
            match event {
                BotEvent::Online => {
                    self.state.bot_status = BotStatus::Online;
                    self.state
                        .push_log(LogLevel::Success, "Bot", "Connected to Telegram Bot API");
                }
                BotEvent::Message(message) => {
                    self.state.push_log(LogLevel::Info, "Telegram", message);
                }
                BotEvent::Error(error) => {
                    self.state.push_log(LogLevel::Warning, "Telegram", error);
                }
                BotEvent::Stopped => {
                    self.state.bot_status = BotStatus::Offline;
                    stopped = true;
                }
            }
        }
        if stopped {
            self.bot = None;
        }
    }

    fn refresh_system_metrics(&mut self) {
        if self.last_system_refresh.elapsed() >= std::time::Duration::from_secs(3) {
            self.state.system = self.system_monitor.snapshot();
            self.last_system_refresh = std::time::Instant::now();
        }
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        match self.tray.as_ref().and_then(AppTray::poll) {
            Some(TrayAction::Show) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.state
                    .push_log(LogLevel::Info, "Tray", "Window restored");
            }
            Some(TrayAction::Exit) => {
                self.exit_requested = true;
                if let Some(runtime) = self.bot.as_ref() {
                    runtime.stop();
                }
                self.state.save_settings();
                // A tray Exit action must terminate the process even when the main
                // viewport is hidden. Relying only on ViewportCommand::Close can be
                // swallowed by close-to-tray handling on some Windows event orders.
                std::process::exit(0);
            }
            None => {}
        }
    }

    fn handle_close_to_tray(&mut self, ctx: &egui::Context) {
        if self.exit_requested || !self.state.hide_to_tray || self.tray.is_none() {
            return;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.state
                .push_log(LogLevel::Info, "Tray", "Window hidden to tray");
        }
    }
}

impl eframe::App for TelegramControlApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_initial_visibility(ctx);
        self.refresh_system_metrics();
        self.handle_bot_runtime();
        self.handle_tray(ctx);
        self.handle_close_to_tray(ctx);
        ui::sidebar(ctx, &mut self.page);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ctx, |ui| match self.page {
                Page::Dashboard => ui::dashboard(ui, &mut self.state),
                Page::Commands => ui::commands(ui, &mut self.state),
                Page::Logs => ui::logs(ui, &mut self.state),
                Page::Settings => ui::settings(ui, &mut self.state),
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(1200));
    }
}
