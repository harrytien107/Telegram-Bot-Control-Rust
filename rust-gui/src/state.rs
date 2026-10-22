use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Settings;
use crate::system::SystemSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotStatus {
    Offline,
    Starting,
    Online,
}

impl BotStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offline => "Offline",
            Self::Starting => "Starting",
            Self::Online => "Online",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub source: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub struct AppState {
    pub bot_status: BotStatus,
    pub hide_to_tray: bool,
    pub start_minimized: bool,
    pub telegram_token: String,
    pub allowed_user_id: String,
    pub download_dir: String,
    pub logs: VecDeque<LogEntry>,
    pub config_path: PathBuf,
    pub system: SystemSnapshot,
}

impl Default for AppState {
    fn default() -> Self {
        let (settings, config_path) = Settings::load()
            .unwrap_or_else(|_| (Settings::default(), PathBuf::from("config.toml")));
        let mut state = Self {
            bot_status: BotStatus::Offline,
            hide_to_tray: settings.hide_to_tray,
            start_minimized: settings.start_minimized,
            telegram_token: settings.telegram_token,
            allowed_user_id: settings.allowed_user_id,
            download_dir: settings.download_dir,
            logs: VecDeque::with_capacity(256),
            config_path,
            system: SystemSnapshot::default(),
        };
        state.push_log(LogLevel::Info, "App", "Desktop controller initialized");
        state
    }
}

impl AppState {
    pub fn push_log(&mut self, level: LogLevel, source: &'static str, message: impl Into<String>) {
        if self.logs.len() >= 500 {
            self.logs.pop_front();
        }
        self.logs.push_back(LogEntry {
            timestamp: clock_text(),
            level,
            source,
            message: message.into(),
        });
    }

    pub fn settings(&self) -> Settings {
        Settings {
            telegram_token: self.telegram_token.clone(),
            allowed_user_id: self.allowed_user_id.clone(),
            download_dir: self.download_dir.clone(),
            hide_to_tray: self.hide_to_tray,
            start_minimized: self.start_minimized,
        }
    }

    pub fn save_settings(&mut self) {
        match self.settings().save(&self.config_path) {
            Ok(()) => self.push_log(
                LogLevel::Success,
                "Settings",
                format!("Saved to {}", self.config_path.display()),
            ),
            Err(error) => self.push_log(
                LogLevel::Warning,
                "Settings",
                format!("Save failed: {error}"),
            ),
        }
    }
}

fn clock_text() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let day_seconds = seconds % 86_400;
    let hours = day_seconds / 3_600;
    let minutes = (day_seconds % 3_600) / 60;
    let secs = day_seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02} UTC")
}
