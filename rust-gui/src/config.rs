use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub telegram_token: String,
    pub allowed_user_id: String,
    pub download_dir: String,
    pub screenshot_dir: String,
    pub favorites: Vec<String>,
    pub hide_to_tray: bool,
    pub start_minimized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            telegram_token: String::new(),
            allowed_user_id: String::new(),
            download_dir: "D:\\Tele".to_owned(),
            screenshot_dir: ".\\screenshots".to_owned(),
            favorites: vec![
                "/screenshot".to_owned(),
                "/volume".to_owned(),
                "/ls".to_owned(),
                "/status".to_owned(),
            ],
            hide_to_tray: true,
            start_minimized: false,
        }
    }
}

impl Settings {
    pub fn load() -> Result<(Self, PathBuf), String> {
        let path = config_path();
        let mut settings = if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            toml::from_str(&raw).map_err(|error| error.to_string())?
        } else if let Some(legacy) = legacy_config_path().filter(|candidate| candidate.exists()) {
            let raw = fs::read_to_string(legacy).map_err(|error| error.to_string())?;
            let settings: Self = toml::from_str(&raw).map_err(|error| error.to_string())?;
            settings.save(&path)?;
            settings
        } else {
            Self::default()
        };

        // Backward compatibility with the Python version's .env convention.
        if settings.telegram_token.trim().is_empty() {
            if let Some(token) = load_legacy_env_token() {
                settings.telegram_token = token;
            }
        }
        Ok((settings, path))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let raw = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        fs::write(path, raw).map_err(|error| error.to_string())
    }
}

fn config_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("config.toml")
}

fn legacy_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("TelegramRemoteControl").join("config.toml"))
}

fn load_legacy_env_token() -> Option<String> {
    if let Ok(value) = std::env::var("TELEGRAM_BOT_TOKEN") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }

    for candidate in [PathBuf::from(".env"), PathBuf::from("..\\.env")] {
        let Ok(raw) = fs::read_to_string(candidate) else {
            continue;
        };
        for line in raw.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "TELEGRAM_BOT_TOKEN" {
                let value = value.trim().trim_matches(['\"', '\'']).to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}
