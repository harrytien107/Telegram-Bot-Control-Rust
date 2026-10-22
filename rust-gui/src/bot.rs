use crate::{config::Settings, remote, remote_web};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum BotEvent {
    Online,
    Message(String),
    Error(String),
    Stopped,
}

pub struct BotRuntime {
    stop: Arc<AtomicBool>,
    events: Receiver<BotEvent>,
}

impl BotRuntime {
    pub fn start(
        token: String,
        allowed_user_id: String,
        download_dir: String,
        screenshot_dir: String,
    ) -> Result<Self, String> {
        if token.trim().is_empty() {
            return Err("Telegram bot token is empty. Save it in Settings first.".to_owned());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("telegram-bot".to_owned())
            .spawn(move || run_bot(token, allowed_user_id, download_dir, screenshot_dir, worker_stop, tx))
            .map_err(|e| e.to_string())?;
        Ok(Self { stop, events: rx })
    }
    pub fn poll(&self) -> Vec<BotEvent> {
        self.events.try_iter().collect()
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for BotRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Default)]
struct RuntimeState {
    pending: HashMap<String, PendingAction>,
    keyboard_users: HashSet<String>,
    shutdown_deadline: Option<SystemTime>,
    hibernate_deadline: Option<(SystemTime, String)>,
    browser_paths: HashMap<u64, PathBuf>,
    next_browser_id: u64,
    browser_root: PathBuf,
    screenshot_dir: String,
    favorites: Vec<String>,
}

#[derive(Clone)]
enum PendingAction {
    Shutdown,
    Restart,
    Hibernate,
    Sleep,
    Lockdown,
    DeleteFile(String),
    TypeText(String),
    MonitorOff,
    CloseWindow(String),
    KillName(String),
    KillPid(u32),
}

fn run_bot(
    token: String,
    allowed_user_id: String,
    download_dir: String,
    screenshot_dir: String,
    stop: Arc<AtomicBool>,
    tx: Sender<BotEvent>,
) {
    let client = match Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .user_agent("telegram-control-gui/0.2")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(BotEvent::Error(format!("HTTP client failed: {e}")));
            let _ = tx.send(BotEvent::Stopped);
            return;
        }
    };

    if let Err(e) = telegram_get_me(&client, &token) {
        let _ = tx.send(BotEvent::Error(format!("Telegram connection failed: {e}")));
        let _ = tx.send(BotEvent::Stopped);
        return;
    }
    let _ = set_commands(&client, &token);
    let _ = tx.send(BotEvent::Online);
    let mut offset = 0_i64;
    let mut state = RuntimeState {
        screenshot_dir,
        browser_root: PathBuf::from(&download_dir),
        favorites: Settings::load()
            .map(|(settings, _)| settings.favorites)
            .unwrap_or_default(),
        ..RuntimeState::default()
    };

    while !stop.load(Ordering::Relaxed) {
        run_due_timers(&client, &token, &mut state);
        match get_updates(&client, &token, offset) {
            Ok(updates) => {
                for update in updates {
                    offset = offset.max(update.update_id + 1);
                    if let Some(query) = update.callback_query {
                        let user_id = query.from.id.to_string();
                        if !allowed(&allowed_user_id, &user_id) {
                            let _ = answer_callback(&client, &token, &query.id);
                            continue;
                        }
                        let _ = handle_callback(&client, &token, query, &download_dir, &mut state, &tx);
                        continue;
                    }
                    let Some(message) = update.message else {
                        continue;
                    };
                    let user_id = message
                        .from
                        .as_ref()
                        .map(|u| u.id.to_string())
                        .unwrap_or_default();
                    if !allowed(&allowed_user_id, &user_id) {
                        let _ = send_message(
                            &client,
                            &token,
                            &message.chat.id.to_string(),
                            "This Telegram user is not allowed to control this PC.",
                        );
                        continue;
                    }
                    let chat_id = message.chat.id.to_string();
                    if let Some(name) = attachment_name(&message) {
                        let _ = tx.send(BotEvent::Message(format!(
                            "Telegram {user_id}: uploaded {name}"
                        )));
                        if let Err(e) = save_attachment(&client, &token, &message, &download_dir) {
                            let _ = send_message(
                                &client,
                                &token,
                                &chat_id,
                                &format!("Upload failed: {e}"),
                            );
                        }
                    }
                    if let Some(text) = message.text.as_deref() {
                        let _ = tx.send(BotEvent::Message(format!("Telegram {user_id}: {text}")));
                        if let Err(e) = handle_text(
                            &client,
                            &token,
                            &chat_id,
                            &user_id,
                            text,
                            &download_dir,
                            &mut state,
                        ) {
                            let _ = send_message(
                                &client,
                                &token,
                                &chat_id,
                                &format!("Command failed: {e}"),
                            );
                        }
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(BotEvent::Error(format!("Telegram polling error: {e}")));
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
    let _ = tx.send(BotEvent::Stopped);
}

fn allowed(allowed_user_id: &str, user_id: &str) -> bool {
    allowed_user_id.trim().is_empty() || allowed_user_id.trim() == user_id.trim()
}

fn handle_text(
    client: &Client,
    token: &str,
    chat_id: &str,
    user_id: &str,
    input: &str,
    download_dir: &str,
    state: &mut RuntimeState,
) -> Result<(), String> {
    if !input.starts_with('/') {
        if state.keyboard_users.contains(user_id) {
            remote::key_press(input)?;
        }
        return Ok(());
    }
    let mut parts = input.trim().split_whitespace();
    let raw = parts.next().unwrap_or("");
    let mut command = raw.split('@').next().unwrap_or("").to_ascii_lowercase();
    let mut args = parts.map(str::to_owned).collect::<Vec<_>>();
    if let Some(value) = command.strip_prefix("/volume")
        && !value.is_empty()
        && value.trim_end_matches('%').chars().all(|c| c.is_ascii_digit())
    {
        args.insert(0, value.trim_end_matches('%').to_owned());
        command = "/volume".to_owned();
    }
    command = canonical_command(&command).to_owned();
    let arg_text = args.join(" ");

    match command.as_str() {
        "/start" | "/menu" => send_menu(client, token, chat_id, state),
        "/remote" => {
            if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("stop")) {
                let stopped = remote_web::stop();
                send_message(client, token, chat_id, if stopped { "Web remote đã dừng." } else { "Web remote chưa chạy." })
            } else {
                let url = remote_web::start_or_get()?;
                send_message(client, token, chat_id, &format!("🌐 Web Remote\n\nMở link này trên điện thoại cùng Wi-Fi:\n{url}\n\nCó: 🖱 touchpad · 🎚 volume slider · ⌨️ keyboard\nDùng /remote stop khi xong."))
            }
        }
        "/favorite" => {
            let Some(name) = args.first() else {
                return send_message(client, token, chat_id, "Ví dụ: /favorite screenshot");
            };
            let command = normalize_favorite_name(name)?;
            if !state.favorites.iter().any(|item| item == &command) {
                state.favorites.push(command.clone());
                save_favorites(&state.favorites)?;
            }
            send_message(client, token, chat_id, &format!("⭐ Đã thêm {command} vào yêu thích."))
        }
        "/unfavorite" => {
            let Some(name) = args.first() else {
                return send_message(client, token, chat_id, "Ví dụ: /unfavorite screenshot");
            };
            let command = normalize_favorite_name(name)?;
            state.favorites.retain(|item| item != &command);
            save_favorites(&state.favorites)?;
            send_message(client, token, chat_id, &format!("☆ Đã xóa {command} khỏi yêu thích."))
        }
        "/introduce" => send_message(client, token, chat_id, INTRODUCE),
        "/help" => {
            if let Some(arg) = args.first() {
                let key = if arg.starts_with('/') {
                    arg.to_owned()
                } else {
                    format!("/{arg}")
                };
                let desc = command_description(&key).unwrap_or("Không tìm thấy lệnh.");
                send_message(client, token, chat_id, &format!("{key}\n{desc}"))
            } else {
                send_message(
                    client,
                    token,
                    chat_id,
                    "Dùng /help <command>. Ví dụ: /help status\nDùng /menu để xem nhóm lệnh.",
                )
            }
        }
        "/status" => reply_result(client, token, chat_id, remote::status_text()),
        "/battery" => reply_result(client, token, chat_id, remote::battery_text()),
        "/uptime" => reply_result(client, token, chat_id, remote::uptime_text()),
        "/publicip" => {
            let text = client
                .get("https://api.ipify.org")
                .send()
                .map_err(|e| e.to_string())?
                .text()
                .map_err(|e| e.to_string())?;
            send_message(
                client,
                token,
                chat_id,
                &format!("Public IP: {}", text.trim()),
            )
        }
        "/wifi" => send_command_output(client, token, chat_id, "wifi"),
        "/timers" => send_message(client, token, chat_id, &timers_text(state)),
        "/canceltimer" | "/cancel" => {
            let _ = remote::cancel_shutdown();
            state.shutdown_deadline = None;
            state.hibernate_deadline = None;
            state.pending.remove(user_id);
            send_message(
                client,
                token,
                chat_id,
                "Đã hủy timer shutdown/hibernate và pending action.",
            )
        }
        "/shutdown" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::Shutdown,
            "Tắt máy",
        ),
        "/restart" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::Restart,
            "Khởi động lại máy",
        ),
        "/hibernate" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::Hibernate,
            "Ngủ đông",
        ),
        "/sleep" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::Sleep,
            "Sleep",
        ),
        "/lockdown" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::Lockdown,
            "Khóa máy",
        ),
        "/shutdownafter" => {
            let Some(raw) = args.first() else {
                return send_message(client, token, chat_id, "Ví dụ: /shutdownafter 30m");
            };
            let secs = parse_duration(raw)
                .ok_or_else(|| "Thời gian không hợp lệ; dùng s/m/h/d".to_owned())?;
            remote::shutdown_after(secs)?;
            state.shutdown_deadline = Some(SystemTime::now() + Duration::from_secs(secs));
            send_message(
                client,
                token,
                chat_id,
                &format!("Đã hẹn tắt máy sau {}.", format_duration(secs)),
            )
        }
        "/hibernateafter" => {
            let Some(raw) = args.first() else {
                return send_message(client, token, chat_id, "Ví dụ: /hibernateafter 1h");
            };
            let secs = parse_duration(raw)
                .ok_or_else(|| "Thời gian không hợp lệ; dùng s/m/h/d".to_owned())?;
            state.hibernate_deadline = Some((
                SystemTime::now() + Duration::from_secs(secs),
                chat_id.to_owned(),
            ));
            send_message(
                client,
                token,
                chat_id,
                &format!("Đã hẹn ngủ đông sau {}.", format_duration(secs)),
            )
        }
        "/screenshot" => {
            let path = remote::screenshot(download_dir)?;
            send_photo_with_markup(client, token, chat_id, &path, screenshot_keyboard())?;
            let _ = std::fs::remove_file(&path);
            Ok(())
        }
        "/uploadfile" => send_message(
            client,
            token,
            chat_id,
            &format!("Hãy gửi file. File sẽ được lưu vào {download_dir}"),
        ),
        "/downloadfile" => {
            if arg_text.is_empty() {
                send_message(
                    client,
                    token,
                    chat_id,
                    "Ví dụ: /downloadfile D:/example.txt",
                )
            } else if Path::new(&arg_text).is_file() {
                send_document(client, token, chat_id, Path::new(&arg_text))
            } else {
                send_message(
                    client,
                    token,
                    chat_id,
                    &format!("Không tìm thấy file: {arg_text}"),
                )
            }
        }
        "/deletefile" => {
            if arg_text.is_empty() {
                send_message(client, token, chat_id, "Ví dụ: /deletefile D:/example.txt")
            } else {
                ask_confirmation(
                    client,
                    token,
                    chat_id,
                    user_id,
                    state,
                    PendingAction::DeleteFile(arg_text.clone()),
                    &format!("Xóa file: {arg_text}"),
                )
            }
        }
        "/ls" => {
            let target = if arg_text.is_empty() {
                download_dir
            } else {
                &arg_text
            };
            send_file_browser(client, token, chat_id, None, Path::new(target), 0, state)
        }
        "/clipboard" => {
            let text = remote::clipboard_get()?;
            send_message(
                client,
                token,
                chat_id,
                if text.is_empty() {
                    "Clipboard trống."
                } else {
                    &text[..text.len().min(3900)]
                },
            )
        }
        "/setclipboard" => {
            if arg_text.is_empty() {
                send_message(client, token, chat_id, "Ví dụ: /setclipboard hello")
            } else {
                remote::clipboard_set(&arg_text)?;
                send_message(client, token, chat_id, "Đã đặt clipboard.")
            }
        }
        "/type" => {
            if arg_text.is_empty() {
                send_message(client, token, chat_id, "Ví dụ: /type hello")
            } else {
                ask_confirmation(
                    client,
                    token,
                    chat_id,
                    user_id,
                    state,
                    PendingAction::TypeText(arg_text.clone()),
                    "Gõ text vào máy",
                )
            }
        }
        "/tasklist" | "/systeminfo" | "/ipconfig" | "/release" | "/renew" | "/netuser"
        | "/whoami" | "/hostname" => send_command_output(client, token, chat_id, &command[1..]),
        "/playvideo" => {
            if arg_text.is_empty() {
                send_message(
                    client,
                    token,
                    chat_id,
                    "Ví dụ: /playvideo https://youtube.com/...",
                )
            } else {
                remote::open_video(&arg_text)?;
                send_with_markup(
                    client,
                    token,
                    chat_id,
                    "Đang phát video. Chọn hành động:",
                    video_keyboard(),
                )
            }
        }
        "/customvolume" => send_with_markup(
            client,
            token,
            chat_id,
            &volume_status_text("Chọn hành động để điều chỉnh âm lượng:"),
            volume_keyboard(),
        ),
        "/mute" => {
            remote::mute(true)?;
            send_message(
                client,
                token,
                chat_id,
                &volume_status_text("Đã tắt tiếng."),
            )
        }
        "/unmute" => {
            remote::mute(false)?;
            send_message(
                client,
                token,
                chat_id,
                &volume_status_text("Đã bật tiếng."),
            )
        }
        "/volume" => {
            let Some(raw) = args.first() else {
                return send_with_markup(
                    client,
                    token,
                    chat_id,
                    &volume_status_text("🎚 Điều khiển âm lượng"),
                    volume_keyboard(),
                );
            };
            let p = raw.trim_end_matches('%').parse::<u32>().map_err(|_| "Ví dụ: /volume 50".to_owned())?;
            remote::set_volume(p.min(100))?;
            send_message(
                client,
                token,
                chat_id,
                &volume_status_text(&format!("Đã đặt âm lượng {}%.", p.min(100))),
            )
        }
        "/brightness" => {
            let p = args
                .first()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| "Ví dụ: /brightness 70".to_owned())?;
            remote::brightness(p.min(100))?;
            send_message(
                client,
                token,
                chat_id,
                &format!("Đã thử đặt độ sáng {}%.", p.min(100)),
            )
        }
        "/monitoroff" => ask_confirmation(
            client,
            token,
            chat_id,
            user_id,
            state,
            PendingAction::MonitorOff,
            "Tắt màn hình",
        ),
        "/controlmouse" => send_with_markup(
            client,
            token,
            chat_id,
            "Chọn thao tác điều khiển chuột:",
            mouse_keyboard(),
        ),
        "/keyboardemulator" => {
            if args.first().is_some_and(|arg| arg.eq_ignore_ascii_case("stop")) {
                state.keyboard_users.remove(user_id);
                return send_message(client, token, chat_id, "⌨️ Keyboard emulator đã tắt.");
            }
            state.keyboard_users.insert(user_id.to_owned());
            send_with_markup(
                client,
                token,
                chat_id,
                "⌨️ Keyboard emulator\n\nGửi text thường để gõ nguyên văn, hoặc bấm phím/shortcut bên dưới. /type vẫn là shortcut để gõ nhanh một chuỗi mà không cần bật keyboard emulator.",
                keyboard_emulator_keyboard(),
            )
        }
        "/windows" => send_text_chunks(client, token, chat_id, &remote::windows_list()?),
        "/closewindow" => {
            if arg_text.is_empty() {
                send_message(client, token, chat_id, "Ví dụ: /closewindow Notepad")
            } else {
                ask_confirmation(
                    client,
                    token,
                    chat_id,
                    user_id,
                    state,
                    PendingAction::CloseWindow(arg_text.clone()),
                    &format!("Đóng cửa sổ: {arg_text}"),
                )
            }
        }
        "/openapp" => {
            let Some(alias) = args.first() else {
                return send_message(
                    client,
                    token,
                    chat_id,
                    &format!("Alias hỗ trợ: {}", remote::aliases()),
                );
            };
            remote::open_app(alias)?;
            send_message(client, token, chat_id, &format!("Đã mở app: {alias}"))
        }
        "/openurl" | "/openweb" => {
            if arg_text.is_empty() {
                send_message(client, token, chat_id, "Ví dụ: /openurl https://google.com")
            } else {
                remote::open_url(&arg_text)?;
                send_message(client, token, chat_id, &format!("Đã mở URL: {arg_text}"))
            }
        }
        "/spotify" => {
            let _ = remote::open_app("spotify");
            send_with_markup(
                client,
                token,
                chat_id,
                &volume_status_text("Điều khiển Spotify:"),
                spotify_keyboard(),
            )
        }
        "/openspotify" => {
            remote::open_app("spotify")?;
            send_with_markup(
                client,
                token,
                chat_id,
                &volume_status_text("Spotify đã mở. Điều khiển:"),
                spotify_keyboard(),
            )
        }
        "/kill" => handle_kill_command(client, token, chat_id, &arg_text),
        _ => send_message(
            client,
            token,
            chat_id,
            &format!("Unknown command: {command}\nDùng /menu để xem toàn bộ lệnh."),
        ),
    }
}

fn handle_callback(
    client: &Client,
    token: &str,
    query: CallbackQuery,
    download_dir: &str,
    state: &mut RuntimeState,
    tx: &Sender<BotEvent>,
) -> Result<(), String> {
    answer_callback(client, token, &query.id)?;
    let Some(message) = query.message else {
        return Ok(());
    };
    let chat_id = message.chat.id.to_string();
    let message_id = message.message_id;
    let user_id = query.from.id.to_string();
    let data = query.data.unwrap_or_default();
    let _ = tx.send(BotEvent::Message(format!(
        "Telegram {user_id}: callback {data}"
    )));
    match data.as_str() {
        "confirm" | "danger_confirm" => {
            if let Some(action) = state.pending.remove(&user_id) {
                let text = execute_pending(action)?;
                edit_message(client, token, &chat_id, message_id, &text, None)
            } else {
                edit_message(
                    client,
                    token,
                    &chat_id,
                    message_id,
                    "Không có hành động chờ xác nhận.",
                    None,
                )
            }
        }
        "cancel" | "danger_cancel" | "cancel_kill" => {
            state.pending.remove(&user_id);
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Hành động đã bị hủy.",
                None,
            )
        }
        s if s.starts_with("menu_group_") => {
            let group = &s[11..];
            let text = menu_group_text(group);
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &text,
                Some(menu_group_keyboard(group, &state.favorites)),
            )
        }
        "menu_all_commands" => {
            let text = all_commands_text();
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &text,
                Some(menu_back_keyboard()),
            )
        }
        "menu_back" => {
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                main_menu_text(),
                Some(main_menu_keyboard(&state.favorites)),
            )
        }
        "quick_remote" => {
            let url = remote_web::start_or_get()?;
            send_message(client, token, &chat_id, &format!("🌐 Web Remote\n\nMở link này trên điện thoại cùng Wi-Fi:\n{url}\n\nCó: 🖱 touchpad + scroll + double-click · 🎚 volume · ⌨️ keyboard\nDùng /remote stop khi xong."))
        }
        s if s.starts_with("kbd:") => {
            let key = s.trim_start_matches("kbd:");
            remote::key_press(key)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("⌨️ Keyboard emulator\nĐã gửi: {key}\n\nGửi text thường để gõ nguyên văn."),
                Some(keyboard_emulator_keyboard()),
            )
        }
        "kbd_close" => {
            state.keyboard_users.remove(&user_id);
            edit_message(client, token, &chat_id, message_id, "⌨️ Keyboard emulator đã tắt.", Some(menu_back_keyboard()))
        }
        s if s.starts_with("cmd_detail:") => {
            let payload = s.trim_start_matches("cmd_detail:");
            let (group, key) = payload.split_once(':').unwrap_or(("", payload));
            let command = format!("/{}", key.trim_start_matches('/'));
            let command = canonical_command(&command).to_owned();
            let Some(description) = command_description(&command) else {
                return edit_message(client, token, &chat_id, message_id, "Không tìm thấy lệnh.", Some(menu_back_keyboard()));
            };
            let favorite = state.favorites.iter().any(|item| item == &command);
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("{}\n\n{}", command, description),
                Some(command_detail_keyboard(&command, favorite, group)),
            )
        }
        s if s.starts_with("fav_add:") => {
            let payload = s.trim_start_matches("fav_add:");
            let (group, key) = payload.split_once(':').unwrap_or(("", payload));
            let command = format!("/{}", key.trim_start_matches('/'));
            let command = canonical_command(&command).to_owned();
            if command_description(&command).is_none() {
                return send_message(client, token, &chat_id, "Lệnh không hợp lệ.");
            }
            if !state.favorites.iter().any(|item| item == &command) {
                state.favorites.push(command.clone());
                save_favorites(&state.favorites)?;
            }
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("{}\n\n{}", command, command_description(&command).unwrap_or("")),
                Some(command_detail_keyboard(&command, true, group)),
            )
        }
        s if s.starts_with("fav_remove:") => {
            let payload = s.trim_start_matches("fav_remove:");
            let (group, key) = payload.split_once(':').unwrap_or(("", payload));
            let command = format!("/{}", key.trim_start_matches('/'));
            let command = canonical_command(&command).to_owned();
            state.favorites.retain(|item| item != &command);
            save_favorites(&state.favorites)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("{}\n\n{}", command, command_description(&command).unwrap_or("")),
                Some(command_detail_keyboard(&command, false, group)),
            )
        }
        s if s.starts_with("menu_back_group:") => {
            let group = s.trim_start_matches("menu_back_group:");
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &menu_group_text(group),
                Some(menu_group_keyboard(group, &state.favorites)),
            )
        }
        s if s.starts_with("cmd_run:") => {
            let command = format!("/{}", s.trim_start_matches("cmd_run:").trim_start_matches('/'));
            handle_text(client, token, &chat_id, &user_id, &command, download_dir, state)
        }
        s if s.starts_with("quick_fav:") => {
            let command = format!("/{}", s.trim_start_matches("quick_fav:").trim_start_matches('/'));
            let command = canonical_command(&command).to_owned();
            if !state.favorites.iter().any(|item| item == &command) {
                return edit_message(
                    client,
                    token,
                    &chat_id,
                    message_id,
                    main_menu_text(),
                    Some(main_menu_keyboard(&state.favorites)),
                );
            }
            handle_text(client, token, &chat_id, &user_id, &command, download_dir, state)
        }
        "quick_screenshot" | "shot_again" => {
            let path = remote::screenshot(download_dir)?;
            let result = send_photo_with_markup(client, token, &chat_id, &path, screenshot_keyboard());
            let _ = std::fs::remove_file(&path);
            result
        }
        "quick_volume" => edit_message(
            client,
            token,
            &chat_id,
            message_id,
            &volume_status_text("🎚 Điều khiển âm lượng"),
            Some(volume_keyboard()),
        ),
        "quick_files" => send_file_browser(
            client,
            token,
            &chat_id,
            Some(message_id),
            Path::new(download_dir),
            0,
            state,
        ),
        "quick_status" => reply_result(client, token, &chat_id, remote::status_text()),
        "volume_mute" => {
            remote::mute(true)?;
            edit_message(client, token, &chat_id, message_id, &volume_status_text("🔇 Đã tắt tiếng."), Some(volume_keyboard()))
        }
        "volume_unmute" => {
            remote::mute(false)?;
            edit_message(client, token, &chat_id, message_id, &volume_status_text("🔊 Đã bật tiếng."), Some(volume_keyboard()))
        }
        s if s.starts_with("volume_set_") => {
            let p = s.trim_start_matches("volume_set_").parse::<u32>().map_err(|e| e.to_string())?.min(100);
            remote::set_volume(p)?;
            edit_message(client, token, &chat_id, message_id, &volume_status_text(&format!("Đã đặt âm lượng {p}%.")), Some(volume_keyboard()))
        }
        s if s.starts_with("volume_delta_") => {
            let delta = s.trim_start_matches("volume_delta_").parse::<i32>().map_err(|e| e.to_string())?;
            let current = remote::current_volume_percent()? as i32;
            let target = (current + delta).clamp(0, 100) as u32;
            remote::set_volume(target)?;
            edit_message(client, token, &chat_id, message_id, &volume_status_text(&format!("Đã chỉnh âm lượng {delta:+}%.")), Some(volume_keyboard()))
        }
        "shot_save" => {
            let path = remote::screenshot(&state.screenshot_dir)?;
            send_message(client, token, &chat_id, &format!("💾 Đã lưu screenshot:\n{}", path.display()))
        }
        "shot_folder" => send_message(
            client,
            token,
            &chat_id,
            &format!("📁 Thư mục lưu screenshot hiện tại:\n{}\n\nBạn có thể đổi đường dẫn ở ô SCREENSHOT trong GUI rồi bấm SAVE.", state.screenshot_dir),
        ),
        s if s.starts_with("fbdir:") => {
            let mut parts = s.split(':');
            let _ = parts.next();
            let id = parts.next().and_then(|x| x.parse::<u64>().ok()).ok_or_else(|| "File browser id không hợp lệ".to_owned())?;
            let page = parts.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(0);
            let path = state.browser_paths.get(&id).cloned().ok_or_else(|| "File browser entry đã hết hạn. Chạy /ls lại.".to_owned())?;
            send_file_browser(client, token, &chat_id, Some(message_id), &path, page, state)
        }
        s if s.starts_with("fbfile:") => {
            let id = s.trim_start_matches("fbfile:").parse::<u64>().map_err(|e| e.to_string())?;
            let path = state.browser_paths.get(&id).cloned().ok_or_else(|| "File browser entry đã hết hạn. Chạy /ls lại.".to_owned())?;
            if path.is_file() && within_browser_root(&state.browser_root, &path) {
                send_document(client, token, &chat_id, &path)
            } else {
                send_message(client, token, &chat_id, "File không còn tồn tại.")
            }
        }
        "noop" => Ok(()),
        "decrease_volume" => {
            remote::change_volume(-3277)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &volume_status_text("Đã giảm âm lượng."),
                Some(volume_keyboard()),
            )
        }
        "increase_volume" => {
            remote::change_volume(3277)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &volume_status_text("Đã tăng âm lượng."),
                Some(volume_keyboard()),
            )
        }
        "up" | "down" | "left" | "right" | "left_click" | "right_click" => {
            remote::mouse_action(&data)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("Đã thực hiện: {data}"),
                Some(mouse_keyboard()),
            )
        }
        "spotify_previous" => {
            remote::media_key(0xB1)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "⏮ Đã chuyển về bài trước",
                Some(spotify_keyboard()),
            )
        }
        "spotify_playpause" => {
            remote::media_key(0xB3)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "⏯ Đã play/pause",
                Some(spotify_keyboard()),
            )
        }
        "spotify_next" => {
            remote::media_key(0xB0)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "⏭ Đã chuyển bài",
                Some(spotify_keyboard()),
            )
        }
        "spotify_volume_down" => {
            remote::change_volume(-6550)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &volume_status_text("🔉 Đã giảm âm lượng 10%"),
                Some(spotify_keyboard()),
            )
        }
        "spotify_volume_up" => {
            remote::change_volume(6550)?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &volume_status_text("🔊 Đã tăng âm lượng 10%"),
                Some(spotify_keyboard()),
            )
        }
        "play_pause" => {
            remote::video_key("k")?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Đã play/pause video.",
                Some(video_keyboard()),
            )
        }
        "rewind" => {
            remote::video_key("j")?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Đã tua lại khoảng 10 giây.",
                Some(video_keyboard()),
            )
        }
        "forward" => {
            remote::video_key("l")?;
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Đã tua tới khoảng 10 giây.",
                Some(video_keyboard()),
            )
        }
        "change_video" => edit_message(
            client,
            token,
            &chat_id,
            message_id,
            "Gửi /playvideo <link> để đổi video.",
            Some(video_keyboard()),
        ),
        "close_all" => {
            let _ = remote::close_thorium();
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Đã đóng Thorium.",
                None,
            )
        }
        "close_thorium_and_play" => {
            let _ = remote::close_thorium();
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                "Đã đóng Thorium. Chạy lại /playvideo <link>.",
                None,
            )
        }
        "cancel_playvideo" => edit_message(
            client,
            token,
            &chat_id,
            message_id,
            "Đã hủy /playvideo.",
            None,
        ),
        "search_process" => edit_message(
            client,
            token,
            &chat_id,
            message_id,
            "Dùng /kill <tên tiến trình>. Ví dụ: /kill chrome",
            None,
        ),
        "show_all_processes" => show_process_buttons(client, token, &chat_id, message_id, None),
        s if s.starts_with("kill_name_") => {
            let name = s.trim_start_matches("kill_name_").to_owned();
            state
                .pending
                .insert(user_id.clone(), PendingAction::KillName(name.clone()));
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("Xác nhận kết thúc tất cả {name}?"),
                Some(confirm_keyboard()),
            )
        }
        s if s.starts_with("kill_") => {
            let pid = s
                .trim_start_matches("kill_")
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            state
                .pending
                .insert(user_id.clone(), PendingAction::KillPid(pid));
            edit_message(
                client,
                token,
                &chat_id,
                message_id,
                &format!("Xác nhận kết thúc PID {pid}?"),
                Some(confirm_keyboard()),
            )
        }
        _ => send_message(
            client,
            token,
            &chat_id,
            &format!("Callback chưa hỗ trợ: {data}"),
        ),
    }
}

fn ask_confirmation(
    client: &Client,
    token: &str,
    chat_id: &str,
    user_id: &str,
    state: &mut RuntimeState,
    action: PendingAction,
    description: &str,
) -> Result<(), String> {
    state.pending.insert(user_id.to_owned(), action);
    send_with_markup(
        client,
        token,
        chat_id,
        &format!("⚠️ {description}\nBạn có chắc chắn muốn thực hiện?"),
        confirm_keyboard(),
    )
}

fn execute_pending(action: PendingAction) -> Result<String, String> {
    match action {
        PendingAction::Shutdown => {
            remote::shutdown_now()?;
            Ok("Máy sẽ tắt sau 3 giây.".into())
        }
        PendingAction::Restart => {
            remote::restart_now()?;
            Ok("Máy sẽ khởi động lại sau 3 giây.".into())
        }
        PendingAction::Hibernate => {
            remote::hibernate_now()?;
            Ok("Đã gửi lệnh ngủ đông.".into())
        }
        PendingAction::Sleep => {
            remote::sleep_now()?;
            Ok("Đã gửi lệnh sleep.".into())
        }
        PendingAction::Lockdown => {
            remote::lock_pc()?;
            Ok("Đã khóa máy.".into())
        }
        PendingAction::DeleteFile(p) => {
            if Path::new(&p).is_file() {
                std::fs::remove_file(&p).map_err(|e| e.to_string())?;
                Ok(format!("Đã xóa file: {p}"))
            } else {
                Ok(format!("Không tìm thấy file: {p}"))
            }
        }
        PendingAction::TypeText(t) => {
            remote::type_text(&t)?;
            Ok("Đã gõ text.".into())
        }
        PendingAction::MonitorOff => {
            remote::monitor_off()?;
            Ok("Đã tắt màn hình.".into())
        }
        PendingAction::CloseWindow(t) => {
            remote::close_window(&t)?;
            Ok(format!("Đã đóng cửa sổ chứa title: {t}"))
        }
        PendingAction::KillName(n) => {
            remote::kill_name(&n)?;
            Ok(format!("Đã kết thúc tiến trình: {n}"))
        }
        PendingAction::KillPid(p) => {
            remote::kill_pid(p)?;
            Ok(format!("Đã kết thúc PID: {p}"))
        }
    }
}

fn run_due_timers(client: &Client, token: &str, state: &mut RuntimeState) {
    if let Some((deadline, chat)) = &state.hibernate_deadline {
        if SystemTime::now() >= *deadline {
            let chat = chat.clone();
            state.hibernate_deadline = None;
            let _ = send_message(client, token, &chat, "Đã đến giờ ngủ đông.");
            let _ = remote::hibernate_now();
        }
    }
    if state
        .shutdown_deadline
        .is_some_and(|d| SystemTime::now() >= d)
    {
        state.shutdown_deadline = None;
    }
}
fn timers_text(state: &RuntimeState) -> String {
    format!(
        "Shutdown: {}\nHibernate: {}",
        deadline_text(state.shutdown_deadline),
        deadline_text(state.hibernate_deadline.as_ref().map(|x| x.0))
    )
}
fn deadline_text(d: Option<SystemTime>) -> String {
    match d.and_then(|x| x.duration_since(SystemTime::now()).ok()) {
        Some(v) => format!("còn {}", format_duration(v.as_secs())),
        None => "không có".into(),
    }
}
fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let (unit, num) = match s.chars().last().unwrap() {
        's' | 'm' | 'h' | 'd' => (&s[s.len() - 1..], &s[..s.len() - 1]),
        _ => ("s", s.as_str()),
    };
    let v = num.parse::<u64>().ok()?;
    let mul = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => 1,
    };
    (v > 0).then_some(v * mul)
}
fn format_duration(mut s: u64) -> String {
    let d = s / 86400;
    s %= 86400;
    let h = s / 3600;
    s %= 3600;
    let m = s / 60;
    let sec = s % 60;
    let mut p = Vec::new();
    if d > 0 {
        p.push(format!("{d} ngày"))
    }
    if h > 0 {
        p.push(format!("{h} giờ"))
    }
    if m > 0 {
        p.push(format!("{m} phút"))
    }
    if sec > 0 || p.is_empty() {
        p.push(format!("{sec} giây"))
    }
    p.join(" ")
}

fn handle_kill_command(
    client: &Client,
    token: &str,
    chat_id: &str,
    query: &str,
) -> Result<(), String> {
    if query.trim().is_empty() {
        return send_with_markup(
            client,
            token,
            chat_id,
            "Chọn tiến trình hoặc dùng /kill <tên>:",
            kill_menu_keyboard(),
        );
    }
    let q = query.to_ascii_lowercase();
    let p = remote::process_list()?;
    let matches = p
        .into_iter()
        .filter(|(n, _)| n.to_ascii_lowercase().contains(&q))
        .take(40)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return send_message(
            client,
            token,
            chat_id,
            &format!("Không tìm thấy tiến trình chứa: {query}"),
        );
    }
    let rows = matches
        .into_iter()
        .map(|(n, pid)| {
            vec![json!({"text":format!("🔴 {n} ({pid})"),"callback_data":format!("kill_{pid}")})]
        })
        .collect::<Vec<_>>();
    send_with_markup(
        client,
        token,
        chat_id,
        "Các tiến trình tìm thấy:",
        json!({"inline_keyboard":rows}),
    )
}
fn show_process_buttons(
    client: &Client,
    token: &str,
    chat_id: &str,
    message_id: i64,
    filter: Option<&str>,
) -> Result<(), String> {
    let p = remote::process_list()?;
    let f = filter.unwrap_or("").to_ascii_lowercase();
    let rows = p
        .into_iter()
        .filter(|(n, _)| f.is_empty() || n.to_ascii_lowercase().contains(&f))
        .take(80)
        .map(|(n, pid)| {
            vec![json!({"text":format!("{n} ({pid})"),"callback_data":format!("kill_{pid}")})]
        })
        .collect::<Vec<_>>();
    edit_message(
        client,
        token,
        chat_id,
        message_id,
        "Chọn tiến trình muốn kết thúc:",
        Some(json!({"inline_keyboard":rows})),
    )
}

fn send_command_output(
    client: &Client,
    token: &str,
    chat_id: &str,
    name: &str,
) -> Result<(), String> {
    let text = remote::system_command(name)?;
    send_text_chunks(client, token, chat_id, &format!("⚡️ /{name}\n{text}"))
}

fn send_text_chunks(client: &Client, token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    let text = if text.trim().is_empty() {
        "Không có dữ liệu."
    } else {
        text
    };
    let mut chunk = String::new();
    for line in text.lines() {
        let needed = line.chars().count() + usize::from(!chunk.is_empty());
        if chunk.chars().count() + needed > 3800 && !chunk.is_empty() {
            send_message(client, token, chat_id, &chunk)?;
            chunk.clear();
        }

        if line.chars().count() > 3800 {
            if !chunk.is_empty() {
                send_message(client, token, chat_id, &chunk)?;
                chunk.clear();
            }
            let chars = line.chars().collect::<Vec<_>>();
            for part in chars.chunks(3800) {
                let part = part.iter().collect::<String>();
                send_message(client, token, chat_id, &part)?;
            }
            continue;
        }

        if !chunk.is_empty() {
            chunk.push('\n');
        }
        chunk.push_str(line);
    }
    if !chunk.is_empty() {
        send_message(client, token, chat_id, &chunk)?;
    }
    Ok(())
}

fn save_attachment(
    client: &Client,
    token: &str,
    message: &TelegramMessage,
    download_dir: &str,
) -> Result<(), String> {
    let (file_id, name) = if let Some(d) = &message.document {
        (
            d.file_id.clone(),
            d.file_name
                .clone()
                .unwrap_or_else(|| format!("file_{}", unix_seconds())),
        )
    } else if let Some(v) = &message.video {
        (
            v.file_id.clone(),
            v.file_name
                .clone()
                .unwrap_or_else(|| format!("video_{}.mp4", unix_seconds())),
        )
    } else if let Some(p) = message.photo.as_ref().and_then(|p| p.last()) {
        (p.file_id.clone(), format!("photo_{}.jpg", unix_seconds()))
    } else {
        return Ok(());
    };
    let safe = Path::new(&name)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let path = Path::new(download_dir).join(safe);
    std::fs::create_dir_all(download_dir).map_err(|e| e.to_string())?;
    download_telegram_file(client, token, &file_id, &path)?;
    send_message(
        client,
        token,
        &message.chat.id.to_string(),
        &format!("File đã được lưu: {}", path.display()),
    )
}
fn attachment_name(m: &TelegramMessage) -> Option<String> {
    m.document
        .as_ref()
        .and_then(|x| x.file_name.clone())
        .or_else(|| m.video.as_ref().and_then(|x| x.file_name.clone()))
        .or_else(|| m.photo.as_ref().map(|_| "photo".into()))
}

fn telegram_get_me(client: &Client, token: &str) -> Result<(), String> {
    let r: ApiResponse<Value> = client
        .get(api_url(token, "getMe"))
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn get_updates(client: &Client, token: &str, offset: i64) -> Result<Vec<TelegramUpdate>, String> {
    let r: ApiResponse<Vec<TelegramUpdate>> = client
        .get(api_url(token, "getUpdates"))
        .query(&[
            ("timeout", "3".to_owned()),
            ("offset", offset.to_string()),
            (
                "allowed_updates",
                "[\"message\",\"callback_query\"]".to_owned(),
            ),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r)
}
fn set_commands(client: &Client, token: &str) -> Result<(), String> {
    let commands = COMMANDS
        .iter()
        .map(|(c, d)| json!({"command":c.trim_start_matches('/'),"description":d}))
        .collect::<Vec<_>>();
    let r: ApiResponse<bool> = client
        .post(api_url(token, "setMyCommands"))
        .json(&json!({"commands":commands}))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn send_message(client: &Client, token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    let r: ApiResponse<Value> = client
        .post(api_url(token, "sendMessage"))
        .form(&[("chat_id", chat_id), ("text", text)])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn send_with_markup(
    client: &Client,
    token: &str,
    chat_id: &str,
    text: &str,
    markup: Value,
) -> Result<(), String> {
    let m = markup.to_string();
    let r: ApiResponse<Value> = client
        .post(api_url(token, "sendMessage"))
        .form(&[
            ("chat_id", chat_id),
            ("text", text),
            ("reply_markup", m.as_str()),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn edit_message(
    client: &Client,
    token: &str,
    chat_id: &str,
    message_id: i64,
    text: &str,
    markup: Option<Value>,
) -> Result<(), String> {
    let mut form = vec![
        ("chat_id".to_owned(), chat_id.to_owned()),
        ("message_id".to_owned(), message_id.to_string()),
        ("text".to_owned(), text.to_owned()),
    ];
    if let Some(m) = markup {
        form.push(("reply_markup".into(), m.to_string()))
    }
    let r: ApiResponse<Value> = client
        .post(api_url(token, "editMessageText"))
        .form(&form)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn answer_callback(client: &Client, token: &str, id: &str) -> Result<(), String> {
    let r: ApiResponse<bool> = client
        .post(api_url(token, "answerCallbackQuery"))
        .form(&[("callback_query_id", id)])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn send_document(client: &Client, token: &str, chat_id: &str, path: &Path) -> Result<(), String> {
    let form = multipart::Form::new()
        .text("chat_id", chat_id.to_owned())
        .file("document", path)
        .map_err(|e| e.to_string())?;
    let r: ApiResponse<Value> = client
        .post(api_url(token, "sendDocument"))
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn send_photo(client: &Client, token: &str, chat_id: &str, path: &Path) -> Result<(), String> {
    let form = multipart::Form::new()
        .text("chat_id", chat_id.to_owned())
        .file("photo", path)
        .map_err(|e| e.to_string())?;
    let r: ApiResponse<Value> = client
        .post(api_url(token, "sendPhoto"))
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn send_photo_with_markup(
    client: &Client,
    token: &str,
    chat_id: &str,
    path: &Path,
    markup: Value,
) -> Result<(), String> {
    let form = multipart::Form::new()
        .text("chat_id", chat_id.to_owned())
        .text("reply_markup", markup.to_string())
        .file("photo", path)
        .map_err(|e| e.to_string())?;
    let r: ApiResponse<Value> = client
        .post(api_url(token, "sendPhoto"))
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    ensure_ok(r).map(|_| ())
}
fn download_telegram_file(
    client: &Client,
    token: &str,
    file_id: &str,
    dest: &Path,
) -> Result<(), String> {
    let r: ApiResponse<FileResult> = client
        .get(api_url(token, "getFile"))
        .query(&[("file_id", file_id)])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let f = ensure_ok(r)?;
    let bytes = client
        .get(format!(
            "https://api.telegram.org/file/bot{token}/{}",
            f.file_path
        ))
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;
    std::fs::write(dest, &bytes).map_err(|e| e.to_string())
}
fn reply_result(
    client: &Client,
    token: &str,
    chat_id: &str,
    result: Result<String, String>,
) -> Result<(), String> {
    match result {
        Ok(t) => send_message(client, token, chat_id, &t),
        Err(e) => send_message(client, token, chat_id, &format!("Có lỗi xảy ra: {e}")),
    }
}
fn ensure_ok<T>(r: ApiResponse<T>) -> Result<T, String> {
    if r.ok {
        r.result
            .ok_or_else(|| "Telegram API returned no result".to_owned())
    } else {
        Err(r
            .description
            .unwrap_or_else(|| "Telegram API returned ok=false".to_owned()))
    }
}
fn api_url(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn send_menu(
    client: &Client,
    token: &str,
    chat_id: &str,
    state: &RuntimeState,
) -> Result<(), String> {
    send_with_markup(
        client,
        token,
        chat_id,
        main_menu_text(),
        main_menu_keyboard(&state.favorites),
    )
}

fn normalize_favorite_name(name: &str) -> Result<String, String> {
    let normalized = if name.starts_with('/') {
        name.to_ascii_lowercase()
    } else {
        format!("/{}", name.to_ascii_lowercase())
    };
    let canonical = canonical_command(&normalized).to_owned();
    if command_description(&canonical).is_none() {
        Err(format!("Không tìm thấy lệnh: {name}"))
    } else {
        Ok(canonical)
    }
}

fn save_favorites(favorites: &[String]) -> Result<(), String> {
    let (mut settings, path) = Settings::load()?;
    settings.favorites = favorites.to_vec();
    settings.save(&path)
}
fn canonical_command(command: &str) -> &str {
    match command {
        "/ss" | "/shot" => "/screenshot",
        "/v" | "/vol" => "/volume",
        "/dir" | "/files" => "/ls",
        "/proc" | "/processes" => "/tasklist",
        "/spoti" => "/spotify",
        "/lock" => "/lockdown",
        "/cancelall" => "/cancel",
        _ => command,
    }
}

fn register_browser_path(state: &mut RuntimeState, path: PathBuf) -> u64 {
    if state.browser_paths.len() > 1024 {
        state.browser_paths.clear();
    }
    state.next_browser_id = state.next_browser_id.wrapping_add(1).max(1);
    let id = state.next_browser_id;
    state.browser_paths.insert(id, path);
    id
}

fn short_button_name(name: &str) -> String {
    let mut chars = name.chars();
    let short = chars.by_ref().take(28).collect::<String>();
    if chars.next().is_some() { format!("{short}…") } else { short }
}

fn within_browser_root(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = root.canonicalize() else { return false; };
    let Ok(candidate) = candidate.canonicalize() else { return false; };
    candidate.starts_with(root)
}

fn send_file_browser(
    client: &Client,
    token: &str,
    chat_id: &str,
    message_id: Option<i64>,
    path: &Path,
    page: usize,
    state: &mut RuntimeState,
) -> Result<(), String> {
    let root = state.browser_root.clone();
    let root = root.as_path();
    if !path.is_dir() || !within_browser_root(root, path) {
        return send_message(client, token, chat_id, "Không thể mở thư mục ngoài DOWNLOAD root.");
    }
    let mut entries = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (!entry.path().is_dir(), entry.file_name().to_string_lossy().to_ascii_lowercase()));

    const PAGE_SIZE: usize = 8;
    let pages = entries.len().div_ceil(PAGE_SIZE).max(1);
    let page = page.min(pages - 1);
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(entries.len());
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for entry in &entries[start..end] {
        let entry_path = entry.path();
        if !within_browser_root(root, &entry_path) { continue; }
        let id = register_browser_path(state, entry_path.clone());
        let name = short_button_name(&entry.file_name().to_string_lossy());
        if entry_path.is_dir() {
            rows.push(vec![json!({"text":format!("📁 {name}"),"callback_data":format!("fbdir:{id}:0")})]);
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            rows.push(vec![json!({"text":format!("📄 {name} ({size} B)"),"callback_data":format!("fbfile:{id}")})]);
        }
    }
    let mut nav = Vec::new();
    if page > 0 {
        let id = register_browser_path(state, path.to_path_buf());
        nav.push(json!({"text":"◀ Prev","callback_data":format!("fbdir:{id}:{}", page - 1)}));
    }
    nav.push(json!({"text":format!("{}/{}", page + 1, pages),"callback_data":"noop"}));
    if page + 1 < pages {
        let id = register_browser_path(state, path.to_path_buf());
        nav.push(json!({"text":"Next ▶","callback_data":format!("fbdir:{id}:{}", page + 1)}));
    }
    rows.push(nav);
    if path != root {
        if let Some(parent) = path.parent().filter(|p| within_browser_root(root, p)) {
            let id = register_browser_path(state, parent.to_path_buf());
            rows.push(vec![json!({"text":"⬆ Parent","callback_data":format!("fbdir:{id}:0")})]);
        }
    }
    rows.push(vec![json!({"text":"⬅️ Menu","callback_data":"menu_back"})]);
    let text = format!("📁 {}\n{} mục · trang {}/{}", path.display(), entries.len(), page + 1, pages);
    let markup = json!({"inline_keyboard": rows});
    match message_id {
        Some(id) => edit_message(client, token, chat_id, id, &text, Some(markup)),
        None => send_with_markup(client, token, chat_id, &text, markup),
    }
}
fn main_menu_text() -> &'static str {
    "⚡️ MENU ĐIỀU KHIỂN\nChọn nhóm lệnh:"
}
fn main_menu_keyboard(favorites: &[String]) -> Value {
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for pair in favorites.chunks(2) {
        rows.push(
            pair.iter()
                .filter(|command| command_description(command).is_some())
                .map(|command| {
                    json!({
                        "text": format!("⭐ {}", command_button_label(command)),
                        "callback_data": format!("quick_fav:{}", command.trim_start_matches('/'))
                    })
                })
                .collect(),
        );
    }
    rows.retain(|row| !row.is_empty());
    rows.extend([
        vec![json!({"text":"🌐 Web Remote","callback_data":"quick_remote"})],
        vec![json!({"text":"Hiện tất cả lệnh","callback_data":"menu_all_commands"})],
        vec![json!({"text":"⚡️ HỆ THỐNG LỆNH","callback_data":"menu_group_power"})],
        vec![json!({"text":"⚡️ LỆNH HÌNH ẢNH","callback_data":"menu_group_image"})],
        vec![json!({"text":"⚡️ I/O FILE","callback_data":"menu_group_fileio"})],
        vec![json!({"text":"⚡️ LỆNH THÔNG TIN MÁY","callback_data":"menu_group_info"})],
        vec![json!({"text":"⚡️ LỆNH HỆ THỐNG","callback_data":"menu_group_system"})],
        vec![json!({"text":"⚡️ LỆNH TIỆN ÍCH","callback_data":"menu_group_utility"})],
    ]);
    json!({"inline_keyboard": rows})
}

fn command_button_label(command: &str) -> String {
    match command {
        "/screenshot" => "📸 Screenshot".to_owned(),
        "/volume" | "/customvolume" => "🎚 Volume".to_owned(),
        "/ls" => "📁 Files".to_owned(),
        "/status" => "🖥 Status".to_owned(),
        "/spotify" => "🎵 Spotify".to_owned(),
        "/lockdown" => "🔒 Lock".to_owned(),
        _ => command.trim_start_matches('/').to_owned(),
    }
}

fn command_detail_keyboard(command: &str, favorite: bool, group: &str) -> Value {
    let key = command.trim_start_matches('/');
    let favorite_button = if favorite {
        json!({"text":"★ Xóa khỏi yêu thích","callback_data":format!("fav_remove:{group}:{key}")})
    } else {
        json!({"text":"⭐ Thêm vào yêu thích","callback_data":format!("fav_add:{group}:{key}")})
    };
    let back_data = if group.is_empty() {
        "menu_back".to_owned()
    } else {
        format!("menu_back_group:{group}")
    };
    json!({"inline_keyboard":[
        [{"text":"▶ Chạy","callback_data":format!("cmd_run:{key}")}],
        [favorite_button],
        [{"text":"⬅️ Quay lại","callback_data":back_data}]
    ]})
}

fn menu_group_commands(group: &str) -> Vec<&'static str> {
    match group {
        "power" => vec!["/shutdown", "/shutdownafter", "/restart", "/hibernate", "/hibernateafter", "/sleep", "/lockdown", "/timers", "/canceltimer", "/cancel"],
        "image" => vec!["/screenshot", "/brightness", "/monitoroff", "/playvideo"],
        "fileio" => vec!["/uploadfile", "/downloadfile", "/deletefile", "/ls"],
        "info" => vec!["/status", "/battery", "/uptime", "/publicip", "/wifi", "/systeminfo", "/ipconfig", "/netuser", "/whoami", "/hostname"],
        "system" => vec!["/tasklist", "/windows", "/closewindow", "/kill", "/release", "/renew"],
        "utility" => vec!["/remote", "/clipboard", "/setclipboard", "/type", "/mute", "/unmute", "/volume", "/customvolume", "/controlmouse", "/keyboardemulator", "/openapp", "/openurl", "/spotify", "/openspotify", "/help", "/introduce", "/menu"],
        _ => vec![],
    }
}

fn menu_group_keyboard(group: &str, favorites: &[String]) -> Value {
    let mut rows = menu_group_commands(group)
        .into_iter()
        .map(|command| {
            let star = if favorites.iter().any(|item| item == command) { "⭐ " } else { "" };
            vec![json!({
                "text": format!("{star}{}", command_button_label(command)),
                "callback_data": format!("cmd_detail:{group}:{}", command.trim_start_matches('/'))
            })]
        })
        .collect::<Vec<_>>();
    rows.push(vec![json!({"text":"⬅️ Quay lại","callback_data":"menu_back"})]);
    json!({"inline_keyboard": rows})
}
fn menu_back_keyboard() -> Value {
    json!({"inline_keyboard":[
        [{"text":"⬅️ Quay lại","callback_data":"menu_back"}]
    ]})
}
fn all_commands_text() -> String {
    [
        "⚡️ TẤT CẢ LỆNH:".to_owned(),
        String::new(),
        menu_group_text("power"),
        String::new(),
        menu_group_text("image"),
        String::new(),
        menu_group_text("fileio"),
        String::new(),
        menu_group_text("info"),
        String::new(),
        menu_group_text("system"),
        String::new(),
        menu_group_text("utility"),
    ]
    .join("\n")
}
fn volume_status_text(prefix: &str) -> String {
    match remote::current_volume_percent() {
        Ok(percent) => format!("{prefix}\nÂm lượng hiện tại: {percent}%"),
        Err(_) => format!("{prefix}\nÂm lượng hiện tại: không đọc được"),
    }
}
fn confirm_keyboard() -> Value {
    json!({"inline_keyboard":[[{"text":"✅ Xác nhận","callback_data":"confirm"},{"text":"❌ Hủy","callback_data":"cancel"}]]})
}
fn volume_keyboard() -> Value {
    json!({"inline_keyboard":[
        [{"text":"🔇 Mute","callback_data":"volume_mute"},{"text":"🔊 Unmute","callback_data":"volume_unmute"}],
        [{"text":"-10","callback_data":"volume_delta_-10"},{"text":"-5","callback_data":"volume_delta_-5"},{"text":"+5","callback_data":"volume_delta_5"},{"text":"+10","callback_data":"volume_delta_10"}],
        [{"text":"0%","callback_data":"volume_set_0"},{"text":"25%","callback_data":"volume_set_25"},{"text":"50%","callback_data":"volume_set_50"},{"text":"75%","callback_data":"volume_set_75"},{"text":"100%","callback_data":"volume_set_100"}],
        [{"text":"⬅️ Menu","callback_data":"menu_back"}]
    ]})
}
fn screenshot_keyboard() -> Value {
    json!({"inline_keyboard":[
        [{"text":"🔄 Chụp lại","callback_data":"shot_again"},{"text":"💾 Lưu vào PC","callback_data":"shot_save"}],
        [{"text":"📁 Thư mục lưu","callback_data":"shot_folder"}]
    ]})
}
fn mouse_keyboard() -> Value {
    json!({"inline_keyboard":[[{"text":"⬆️ Lên","callback_data":"up"}],[{"text":"⬅️ Trái","callback_data":"left"},{"text":"➡️ Phải","callback_data":"right"}],[{"text":"⬇️ Xuống","callback_data":"down"}],[{"text":"🖱️ Click trái","callback_data":"left_click"},{"text":"🖱️ Click phải","callback_data":"right_click"}]]})
}
fn keyboard_emulator_keyboard() -> Value {
    json!({"inline_keyboard":[
        [{"text":"Esc","callback_data":"kbd:esc"},{"text":"Tab","callback_data":"kbd:tab"},{"text":"⌫","callback_data":"kbd:backspace"},{"text":"Enter","callback_data":"kbd:enter"}],
        [{"text":"Home","callback_data":"kbd:home"},{"text":"↑","callback_data":"kbd:up"},{"text":"End","callback_data":"kbd:end"},{"text":"PgUp","callback_data":"kbd:pageup"}],
        [{"text":"←","callback_data":"kbd:left"},{"text":"↓","callback_data":"kbd:down"},{"text":"→","callback_data":"kbd:right"},{"text":"PgDn","callback_data":"kbd:pagedown"}],
        [{"text":"Ctrl+C","callback_data":"kbd:ctrl+c"},{"text":"Ctrl+V","callback_data":"kbd:ctrl+v"},{"text":"Ctrl+X","callback_data":"kbd:ctrl+x"},{"text":"Ctrl+A","callback_data":"kbd:ctrl+a"}],
        [{"text":"Alt+Tab","callback_data":"kbd:alt+tab"},{"text":"Win+D","callback_data":"kbd:win+d"},{"text":"Win+L","callback_data":"kbd:win+l"},{"text":"TaskMgr","callback_data":"kbd:ctrl+shift+esc"}],
        [{"text":"F1","callback_data":"kbd:f1"},{"text":"F2","callback_data":"kbd:f2"},{"text":"F3","callback_data":"kbd:f3"},{"text":"F4","callback_data":"kbd:f4"}],
        [{"text":"F5","callback_data":"kbd:f5"},{"text":"F6","callback_data":"kbd:f6"},{"text":"F7","callback_data":"kbd:f7"},{"text":"F8","callback_data":"kbd:f8"}],
        [{"text":"F9","callback_data":"kbd:f9"},{"text":"F10","callback_data":"kbd:f10"},{"text":"F11","callback_data":"kbd:f11"},{"text":"F12","callback_data":"kbd:f12"}],
        [{"text":"⏹ Tắt keyboard","callback_data":"kbd_close"}]
    ]})
}
fn spotify_keyboard() -> Value {
    json!({"inline_keyboard":[[{"text":"⏮ Bài trước","callback_data":"spotify_previous"},{"text":"⏯ Play/Pause","callback_data":"spotify_playpause"},{"text":"⏭ Bài tiếp","callback_data":"spotify_next"}],[{"text":"🔉 -10%","callback_data":"spotify_volume_down"},{"text":"🔊 +10%","callback_data":"spotify_volume_up"}]]})
}
fn video_keyboard() -> Value {
    json!({"inline_keyboard":[[{"text":"⏯ Play/Pause","callback_data":"play_pause"},{"text":"⏪ -10s","callback_data":"rewind"}],[{"text":"⏩ +10s","callback_data":"forward"},{"text":"🔄 Đổi video","callback_data":"change_video"}],[{"text":"❌ Đóng Thorium","callback_data":"close_all"}]]})
}
fn kill_menu_keyboard() -> Value {
    json!({"inline_keyboard":[[{"text":"🔍 Tìm kiếm","callback_data":"search_process"}],[{"text":"🔴 chrome.exe","callback_data":"kill_name_chrome.exe"}],[{"text":"🔴 spotify.exe","callback_data":"kill_name_spotify.exe"}],[{"text":"🔴 thorium.exe","callback_data":"kill_name_thorium.exe"}],[{"text":"📋 Xem tất cả","callback_data":"show_all_processes"}]]})
}

fn menu_group_text(group: &str) -> String {
    let (title, names) = match group {
        "power" => (
            "⚡️ HỆ THỐNG LỆNH:",
            vec![
                "/shutdown",
                "/shutdownafter",
                "/restart",
                "/hibernate",
                "/hibernateafter",
                "/sleep",
                "/lockdown",
                "/timers",
                "/canceltimer",
                "/cancel",
            ],
        ),
        "image" => (
            "⚡️ LỆNH HÌNH ẢNH:",
            vec!["/screenshot", "/brightness", "/monitoroff", "/playvideo"],
        ),
        "fileio" => (
            "⚡️ I/O FILE:",
            vec!["/uploadfile", "/downloadfile", "/deletefile", "/ls"],
        ),
        "info" => (
            "⚡️ LỆNH THÔNG TIN MÁY:",
            vec![
                "/status",
                "/battery",
                "/uptime",
                "/publicip",
                "/wifi",
                "/systeminfo",
                "/ipconfig",
                "/netuser",
                "/whoami",
                "/hostname",
            ],
        ),
        "system" => (
            "⚡️ LỆNH HỆ THỐNG:",
            vec![
                "/tasklist",
                "/windows",
                "/closewindow",
                "/kill",
                "/release",
                "/renew",
            ],
        ),
        "utility" => (
            "⚡️ LỆNH TIỆN ÍCH:",
            vec![
                "/clipboard",
                "/setclipboard",
                "/type",
                "/mute",
                "/unmute",
                "/volume",
                "/customvolume",
                "/controlmouse",
                "/keyboardemulator",
                "/openapp",
                "/openurl",
                "/spotify",
                "/openspotify",
                "/help",
                "/introduce",
                "/menu",
            ],
        ),
        _ => ("⚡️ NHÓM LỆNH:", vec![]),
    };
    let mut lines = vec![title.to_owned()];
    for n in names {
        lines.push(format!("{n} - {}", command_description(n).unwrap_or("")))
    }
    lines.join("\n")
}
fn command_description(name: &str) -> Option<&'static str> {
    COMMANDS.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

const INTRODUCE: &str = "🦀 TELEGRAM REMOTE CONTROL — RUST\nDeveloper & Maintainer: harrytien107\nDiscord: harrytien107\nTelegram: @harrytienthereal\nGitHub: https://github.com/harrytien107\n\nNative Windows remote-control app written in Rust with a lightweight Win32 GUI, system tray support, portable configuration, Telegram controls, and Web Remote features.";
const COMMANDS: &[(&str, &str)] = &[
    ("/introduce", "Giới thiệu về tôi."),
    ("/help", "Xem hướng dẫn chi tiết."),
    ("/menu", "Hiển thị menu inline theo nhóm."),
    ("/remote", "Mở Web Remote: touchpad, volume slider và keyboard."),
    ("/favorite", "Thêm lệnh vào mục yêu thích."),
    ("/unfavorite", "Xóa lệnh khỏi mục yêu thích."),
    ("/status", "Thông tin CPU, RAM, disk, uptime, battery."),
    ("/battery", "Thông tin pin."),
    ("/uptime", "Thời gian máy đã chạy."),
    ("/publicip", "Xem public IP."),
    ("/wifi", "Thông tin Wi-Fi đang kết nối."),
    ("/shutdown", "Lệnh tắt máy."),
    ("/shutdownafter", "Hẹn giờ tắt máy."),
    ("/restart", "Khởi động lại máy."),
    ("/hibernate", "Ngủ đông."),
    ("/hibernateafter", "Hẹn giờ ngủ đông."),
    ("/timers", "Xem timer."),
    ("/canceltimer", "Hủy timer."),
    ("/sleep", "Chế độ ngủ."),
    ("/cancel", "Huỷ lệnh chờ/timer."),
    ("/lockdown", "Khóa máy tính."),
    ("/screenshot", "Chụp và gửi ảnh màn hình."),
    ("/uploadfile", "Gửi file lên máy."),
    ("/downloadfile", "Tải file từ máy."),
    ("/deletefile", "Xoá file."),
    ("/ls", "Liệt kê thư mục."),
    ("/clipboard", "Lấy clipboard."),
    ("/setclipboard", "Đặt clipboard."),
    ("/type", "Gõ nhanh một chuỗi text vào máy."),
    ("/tasklist", "Danh sách tiến trình."),
    ("/systeminfo", "Thông tin hệ thống."),
    ("/ipconfig", "Cấu hình mạng."),
    ("/release", "Giải phóng IP."),
    ("/renew", "Gia hạn IP."),
    ("/netuser", "Danh sách user."),
    ("/whoami", "Tài khoản hiện tại."),
    ("/hostname", "Tên máy."),
    ("/playvideo", "Phát YouTube."),
    ("/customvolume", "Điều chỉnh âm lượng."),
    ("/mute", "Tắt tiếng."),
    ("/unmute", "Bật tiếng."),
    ("/volume", "Đặt âm lượng 0-100."),
    ("/brightness", "Đặt độ sáng 0-100."),
    ("/monitoroff", "Tắt màn hình."),
    ("/controlmouse", "Điều khiển chuột."),
    ("/keyboardemulator", "Bàn phím ảo đầy đủ: phím đặc biệt, F1-F12 và shortcut."),
    ("/windows", "Danh sách cửa sổ."),
    ("/closewindow", "Đóng cửa sổ theo title."),
    ("/openapp", "Mở app theo alias."),
    ("/openurl", "Mở URL."),
    ("/openweb", "Alias tương thích của /openurl."),
    ("/spotify", "Điều khiển Spotify."),
    ("/openspotify", "Mở Spotify."),
    ("/kill", "Kết thúc tiến trình."),
];

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}
#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
    callback_query: Option<CallbackQuery>,
}
#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    document: Option<TelegramFile>,
    photo: Option<Vec<TelegramPhoto>>,
    video: Option<TelegramFile>,
}
#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}
#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
}
#[derive(Debug, Deserialize)]
struct TelegramFile {
    file_id: String,
    file_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct TelegramPhoto {
    file_id: String,
}
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    id: String,
    from: TelegramUser,
    message: Option<TelegramMessage>,
    data: Option<String>,
}
#[derive(Debug, Deserialize)]
struct FileResult {
    file_path: String,
}
