use std::process::{Command, Stdio};

use crate::remote;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalCommand {
    Screenshot,
    LockPc,
    MonitorOff,
    Mute,
    OpenBrowser,
    OpenSpotify,
}

impl LocalCommand {
    pub fn label(self) -> &'static str {
        match self {
            Self::Screenshot => "Screenshot",
            Self::LockPc => "Lock PC",
            Self::MonitorOff => "Monitor off",
            Self::Mute => "Mute",
            Self::OpenBrowser => "Open browser",
            Self::OpenSpotify => "Open Spotify",
        }
    }
}

pub fn execute(command: LocalCommand, download_dir: &str) -> Result<String, String> {
    match command {
        LocalCommand::Screenshot => screenshot(download_dir),
        LocalCommand::LockPc => {
            spawn("rundll32.exe", &["user32.dll,LockWorkStation"])?;
            Ok("PC lock requested".to_owned())
        }
        LocalCommand::MonitorOff => {
            remote::monitor_off()?;
            Ok("Monitor-off requested".to_owned())
        }
        LocalCommand::Mute => {
            remote::mute(true)?;
            Ok("System audio muted".to_owned())
        }
        LocalCommand::OpenBrowser => {
            spawn(
                "cmd.exe",
                &["/d", "/c", "start", "", "https://www.google.com"],
            )?;
            Ok("Default browser opened".to_owned())
        }
        LocalCommand::OpenSpotify => {
            spawn("cmd.exe", &["/d", "/c", "start", "", "spotify:"])?;
            Ok("Spotify launch requested".to_owned())
        }
    }
}

fn screenshot(download_dir: &str) -> Result<String, String> {
    let path = remote::screenshot(download_dir)?;
    Ok(format!("Screenshot saved to {}", path.display()))
}

fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
