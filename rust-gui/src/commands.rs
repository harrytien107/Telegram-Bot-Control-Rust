use std::path::PathBuf;
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
    let dir = PathBuf::from(download_dir);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("screenshot.png");
    let escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
         $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
         $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; \
         $g=[System.Drawing.Graphics]::FromImage($bmp); \
         $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); \
         $bmp.Save('{escaped}',[System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $bmp.Dispose()"
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("Screenshot command exited with {status}"));
    }
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
