# Telegram Remote Control for Windows

A lightweight Windows remote-control utility written in **Rust**. It runs as a native Win32 desktop app, lives in the system tray, and lets an authorized Telegram user control common PC functions without keeping a terminal window open.

This repository now contains the Rust implementation only. The previous Python version has been retired.

## Highlights

- Native Win32 GUI with tray support and no console window
- Portable `config.toml` stored next to the executable
- Telegram user allow-list by user ID
- Command groups, favorites, quick actions, contextual back buttons
- Screenshot capture and Telegram preview actions
- File browser, upload, download, delete, and directory listing
- CPU, RAM, disk, uptime, battery, Wi-Fi, IP, hostname, user and process information
- Shutdown, restart, sleep, hibernate, lock and scheduled power actions
- Volume controls with current-volume reporting, mute/unmute and percentage control
- Mouse control and keyboard emulator with F1-F12, navigation keys and shortcuts
- Clipboard read/write and remote text typing
- Spotify/media controls
- Local Web Remote with touchpad, scrolling, double-click, current-volume slider, and keyboard controls

## Requirements

- Windows 10/11 x64
- Rust toolchain with the MSVC target
- Visual Studio Build Tools / MSVC with `rc.exe`
- A Telegram bot token from `@BotFather`
- [NirCmd](https://www.nirsoft.net/utils/nircmd.html) for volume, mute and monitor-off commands

No Python, Selenium, browser driver, or Python virtual environment is required.

## Build

Open an MSVC developer environment, then build the release executable:

```powershell
cd rust-gui
cargo build --release
```

The executable is created at:

```text
rust-gui\target\release\telegram-control-gui.exe
```

The release profile is configured for a small binary using size optimization, Thin LTO, a single codegen unit, symbol stripping, and `panic = "abort"`.

## NirCmd

Download NirCmd from NirSoft and place either `nircmdc.exe` or `nircmd.exe` in:

```text
nircmd-x64\nircmdc.exe
nircmd-x64\nircmd.exe
```

A portable folder can look like this:

```text
TelegramRemoteControl\
├─ telegram-control-gui.exe
├─ config.toml
└─ nircmd-x64\
   └─ nircmdc.exe
```

NirCmd itself is not bundled in this repository.

## First Run

1. Create a Telegram bot with `@BotFather` and copy its token.
2. Start `telegram-control-gui.exe`.
3. Enter the bot token in **BOT TOKEN**.
4. Enter your Telegram numeric user ID in **USER ID**.
5. Choose download and screenshot folders.
6. Click **SAVE**, then **START**.

Configuration is written to `config.toml` beside the executable, so the app remains portable.

Keep your bot token and `config.toml` private.

## Telegram Usage

Use `/menu` for the grouped control menu. Favorites appear as direct quick actions on the main menu.

| Command | Purpose |
| --- | --- |
| `/menu` | Open the grouped command menu |
| `/status` | CPU, RAM, disk, uptime and battery |
| `/screenshot` | Capture and send the screen |
| `/ls` | Browse files and folders |
| `/volume` | Volume controls and current level |
| `/controlmouse` | Telegram mouse controls |
| `/keyboardemulator` | Remote keyboard with special keys and shortcuts |
| `/clipboard` | Read the Windows clipboard |
| `/setclipboard` | Replace clipboard text |
| `/remote` | Start the local Web Remote |
| `/timers` | View scheduled power actions |

Run `/help` or **Hiện tất cả lệnh** in the menu to see the complete command set.

## Web Remote

Run `/remote` or tap **🌐 Web Remote** in the Telegram menu. The bot returns a tokenized local URL for a phone or another device on the same Wi-Fi/LAN.

The page provides touchpad, left/right/double click, wheel and two-finger scrolling, a volume slider initialized from the current Windows volume, text input, navigation keys, function keys and common shortcuts.

Run:

```text
/remote stop
```

when finished.

## Portable Data

Runtime data such as `config.toml`, screenshots, downloads and NirCmd binaries stay outside Git. Build output under `target/` and packaged executables under `dist/` are ignored as well.

## Security

This project can execute system-control actions on the Windows PC where it runs. Use a private bot token, configure your own Telegram user ID, and do not expose the Web Remote URL to untrusted users or networks.

## License

See [LICENSE](LICENSE).

## Author

**Lê Phi Anh**  
GitHub: [LePhiAnhDev](https://github.com/LePhiAnhDev)
