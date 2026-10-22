#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod bot;
mod commands;
mod config;
mod remote;
mod remote_web;

use std::mem::size_of;

use bot::{BotEvent, BotRuntime};
use config::Settings;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    PAINTSTRUCT, SelectObject,
    SetBkColor, SetBkMode, SetTextColor, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, FW_BOLD, HBRUSH, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
    CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_SELECTED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const APP_CLASS: PCWSTR = w!("TelegramRemoteControlNative");
const APP_TITLE: PCWSTR = w!("Telegram Remote Control");
const WM_TRAYICON: u32 = WM_APP + 1;
const TIMER_POLL: usize = 1;

const ID_TOKEN: i32 = 1001;
const ID_USER: i32 = 1002;
const ID_DIR: i32 = 1003;
const ID_SCREENSHOT_DIR: i32 = 1004;
const ID_START: i32 = 1101;
const ID_STOP: i32 = 1102;
const ID_SAVE: i32 = 1103;
const ID_HIDE: i32 = 1104;
const ID_MINIMIZE: i32 = 1105;
const ID_CLOSE: i32 = 1106;
const ID_TRAY_OPEN: usize = 1201;
const ID_TRAY_EXIT: usize = 1202;
const EM_SETSEL_MSG: u32 = 0x00B1;
const EM_SCROLLCARET_MSG: u32 = 0x00B7;
const EM_REPLACESEL_MSG: u32 = 0x00C2;
const WM_SETFONT_MSG: u32 = 0x0030;
const APP_ICON_ID: usize = 1;

const BG: COLORREF = COLORREF(0x00000000);
const PANEL: COLORREF = COLORREF(0x000F0F0F);
const FIELD: COLORREF = COLORREF(0x00141414);
const BORDER: COLORREF = COLORREF(0x002A2A2A);
const TEXT: COLORREF = COLORREF(0x00EDEDED);
const MUTED: COLORREF = COLORREF(0x00888888);
const ACCENT: COLORREF = COLORREF(0x002119D7);

struct NativeApp {
    settings: Settings,
    config_path: std::path::PathBuf,
    bot: Option<BotRuntime>,
    hwnd: HWND,
    token_edit: HWND,
    user_edit: HWND,
    dir_edit: HWND,
    screenshot_dir_edit: HWND,
    status: HWND,
    log: HWND,
    tray_added: bool,
    font_ui: HFONT,
    font_bold: HFONT,
    font_mono: HFONT,
    brush_bg: HBRUSH,
    brush_field: HBRUSH,
}

impl NativeApp {
    fn new() -> Self {
        let (settings, config_path) =
            Settings::load().unwrap_or_else(|_| (Settings::default(), "config.toml".into()));
        Self {
            settings,
            config_path,
            bot: None,
            hwnd: HWND::default(),
            token_edit: HWND::default(),
            user_edit: HWND::default(),
            dir_edit: HWND::default(),
            screenshot_dir_edit: HWND::default(),
            status: HWND::default(),
            log: HWND::default(),
            tray_added: false,
            font_ui: HFONT::default(),
            font_bold: HFONT::default(),
            font_mono: HFONT::default(),
            brush_bg: HBRUSH::default(),
            brush_field: HBRUSH::default(),
        }
    }

    unsafe fn init_controls(&mut self, instance: HINSTANCE) {
        unsafe {
            self.font_ui = CreateFontW(
                -15, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, w!("Bahnschrift"),
            );
            self.font_bold = CreateFontW(
                -20, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, w!("Bahnschrift"),
            );
            self.font_mono = CreateFontW(
                -13, 0, 0, 0, 500, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, w!("Bahnschrift SemiCondensed"),
            );
            self.brush_bg = CreateSolidBrush(BG);
            self.brush_field = CreateSolidBrush(FIELD);

            self.create_button("—", 354, 7, 25, 24, ID_MINIMIZE, instance);
            self.create_button("×", 383, 7, 25, 24, ID_CLOSE, instance);

            self.status = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("[OFFLINE]"),
                WS_CHILD | WS_VISIBLE,
                16,
                42,
                388,
                24,
                Some(self.hwnd),
                None,
                Some(instance),
                None,
            )
            .unwrap_or_default();

            self.create_label("BOT TOKEN", 16, 78, 76, 18, instance);
            self.token_edit =
                self.create_edit(&self.settings.telegram_token, 96, 74, 308, 25, ID_TOKEN, instance, true);

            self.create_label("USER ID", 16, 110, 76, 18, instance);
            self.user_edit =
                self.create_edit(&self.settings.allowed_user_id, 96, 106, 308, 25, ID_USER, instance, false);

            self.create_label("DOWNLOAD", 16, 142, 76, 18, instance);
            self.dir_edit =
                self.create_edit(&self.settings.download_dir, 96, 138, 308, 25, ID_DIR, instance, false);

            self.create_label("SCREENSHOT", 16, 174, 76, 18, instance);
            self.screenshot_dir_edit = self.create_edit(
                &self.settings.screenshot_dir,
                96,
                170,
                308,
                25,
                ID_SCREENSHOT_DIR,
                instance,
                false,
            );

            self.create_button("START", 16, 208, 76, 27, ID_START, instance);
            self.create_button("STOP", 98, 208, 76, 27, ID_STOP, instance);
            self.create_button("SAVE", 180, 208, 76, 27, ID_SAVE, instance);
            self.create_button("HIDE", 262, 208, 76, 27, ID_HIDE, instance);

            self.create_label("ACTIVITY / TELEGRAM", 16, 246, 160, 18, instance);

            self.log = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!("[READY] Native Rust controller.\r\n"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY) as u32),
                16,
                266,
                388,
                54,
                Some(self.hwnd),
                None,
                Some(instance),
                None,
            )
            .unwrap_or_default();

            for child in [self.token_edit, self.user_edit, self.dir_edit, self.screenshot_dir_edit, self.log] {
                let _ = SendMessageW(child, WM_SETFONT_MSG, Some(WPARAM(self.font_ui.0 as usize)), Some(LPARAM(1)));
            }
            let _ = SendMessageW(self.status, WM_SETFONT_MSG, Some(WPARAM(self.font_bold.0 as usize)), Some(LPARAM(1)));

            if !self.settings.telegram_token.trim().is_empty() {
                self.start_bot();
            }
        }
    }

    unsafe fn create_label(
        &self,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        instance: HINSTANCE,
    ) {
        let text = wide(text);
        unsafe {
            let label = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                x,
                y,
                width,
                height,
                Some(self.hwnd),
                None,
                Some(instance),
                None,
            ).unwrap_or_default();
            let _ = SendMessageW(label, WM_SETFONT_MSG, Some(WPARAM(self.font_mono.0 as usize)), Some(LPARAM(1)));
        }
    }

    unsafe fn create_edit(
        &self,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        id: i32,
        instance: HINSTANCE,
        password: bool,
    ) -> HWND {
        let text = wide(text);
        let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        if password {
            style |= WINDOW_STYLE(ES_PASSWORD as u32);
        }
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                PCWSTR(text.as_ptr()),
                style,
                x,
                y,
                width,
                height,
                Some(self.hwnd),
                Some(HMENU(id as usize as *mut core::ffi::c_void)),
                Some(instance),
                None,
            )
            .unwrap_or_default()
        }
    }

    unsafe fn create_button(
        &self,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        id: i32,
        instance: HINSTANCE,
    ) {
        let text = wide(text);
        unsafe {
            let result = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                x,
                y,
                width,
                height,
                Some(self.hwnd),
                Some(HMENU(id as usize as *mut core::ffi::c_void)),
                Some(instance),
                None,
            );
            if let Ok(button) = result {
                let _ = SendMessageW(button, WM_SETFONT_MSG, Some(WPARAM(self.font_mono.0 as usize)), Some(LPARAM(1)));
            }
        }
    }

    unsafe fn sync_settings_from_ui(&mut self) {
        unsafe {
            self.settings.telegram_token = window_text(self.token_edit);
            self.settings.allowed_user_id = window_text(self.user_edit);
            self.settings.download_dir = window_text(self.dir_edit);
            self.settings.screenshot_dir = window_text(self.screenshot_dir_edit);
        }
    }

    unsafe fn save(&mut self) {
        unsafe {
            self.sync_settings_from_ui();
            // Favorites can be changed from Telegram while this GUI is running.
            // Preserve the latest persisted list instead of overwriting it with
            // the copy that was loaded when the process started.
            if let Ok((latest, _)) = Settings::load() {
                self.settings.favorites = latest.favorites;
            }
            match self.settings.save(&self.config_path) {
                Ok(()) => self.append_log("Settings saved."),
                Err(error) => self.append_log(&format!("Save failed: {error}")),
            }
        }
    }

    unsafe fn start_bot(&mut self) {
        if self.bot.is_some() {
            return;
        }
        unsafe {
            self.sync_settings_from_ui();
            match BotRuntime::start(
                self.settings.telegram_token.clone(),
                self.settings.allowed_user_id.clone(),
                self.settings.download_dir.clone(),
                self.settings.screenshot_dir.clone(),
            ) {
                Ok(runtime) => {
                    self.bot = Some(runtime);
                    self.set_status("[CONNECTING]");
                    self.append_log("Telegram bot starting.");
                }
                Err(error) => {
                    self.set_status("[OFFLINE]");
                    self.append_log(&error);
                }
            }
        }
    }

    unsafe fn stop_bot(&mut self) {
        if let Some(bot) = self.bot.take() {
            bot.stop();
        }
        unsafe {
            self.set_status("[OFFLINE]");
            self.append_log("Telegram bot stopped.");
        }
    }

    unsafe fn poll_bot(&mut self) {
        let events = self.bot.as_ref().map(BotRuntime::poll).unwrap_or_default();
        for event in events {
            unsafe {
                match event {
                    BotEvent::Online => {
                        self.set_status("[ONLINE]");
                        self.append_log("Connected to Telegram.");
                    }
                    BotEvent::Message(message) => self.append_log(&message),
                    BotEvent::Error(error) => self.append_log(&format!("Error: {error}")),
                    BotEvent::Stopped => {
                        self.bot = None;
                        self.set_status("[OFFLINE]");
                    }
                }
            }
        }
    }

    unsafe fn set_status(&self, text: &str) {
        let text = wide(text);
        unsafe {
            let _ = SetWindowTextW(self.status, PCWSTR(text.as_ptr()));
        }
    }

    unsafe fn append_log(&self, text: &str) {
        unsafe {
            let current_len = GetWindowTextLengthW(self.log);
            let _ = SendMessageW(
                self.log,
                EM_SETSEL_MSG,
                Some(WPARAM(current_len as usize)),
                Some(LPARAM(current_len as isize)),
            );
            let line = wide(&format!("{text}\r\n"));
            let _ = SendMessageW(
                self.log,
                EM_REPLACESEL_MSG,
                Some(WPARAM(0)),
                Some(LPARAM(line.as_ptr() as isize)),
            );
            let _ = SendMessageW(self.log, EM_SCROLLCARET_MSG, None, None);
        }
    }

    unsafe fn add_tray(&mut self) {
        if self.tray_added {
            return;
        }
        unsafe {
            let module = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
            let icon = LoadIconW(Some(module), PCWSTR(APP_ICON_ID as *const u16))
                .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
            let mut data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: icon,
                ..Default::default()
            };
            copy_wide_fixed(&mut data.szTip, "Telegram Remote Control");
            if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                self.tray_added = true;
            }
        }
    }

    unsafe fn remove_tray(&mut self) {
        if !self.tray_added {
            return;
        }
        unsafe {
            let data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
        self.tray_added = false;
    }

    unsafe fn show_window(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    unsafe fn tray_menu(&self) {
        unsafe {
            let menu = CreatePopupMenu().unwrap_or_default();
            let open = wide("Open");
            let exit = wide("Exit");
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN, PCWSTR(open.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, PCWSTR(exit.as_ptr()));
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let app = create.lpCreateParams as *mut NativeApp;
            (*app).hwnd = hwnd;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        }
        let app_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut NativeApp;

        match msg {
            WM_CREATE if !app_ptr.is_null() => {
                let instance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
                (*app_ptr).init_controls(instance);
                (*app_ptr).add_tray();
                let _ = SetTimer(Some(hwnd), TIMER_POLL, 1000, None);
                return LRESULT(0);
            }
            WM_COMMAND if !app_ptr.is_null() => {
                match (wparam.0 & 0xffff) as usize {
                    x if x == ID_START as usize => (*app_ptr).start_bot(),
                    x if x == ID_STOP as usize => (*app_ptr).stop_bot(),
                    x if x == ID_SAVE as usize => (*app_ptr).save(),
                    x if x == ID_HIDE as usize => {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    x if x == ID_MINIMIZE as usize => {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    x if x == ID_CLOSE as usize => {
                        if (*app_ptr).settings.hide_to_tray {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        } else {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    ID_TRAY_OPEN => (*app_ptr).show_window(),
                    ID_TRAY_EXIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_TIMER if !app_ptr.is_null() && wparam.0 == TIMER_POLL => {
                (*app_ptr).poll_bot();
                return LRESULT(0);
            }
            WM_CTLCOLOREDIT if !app_ptr.is_null() => {
                let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
                let _ = SetTextColor(hdc, TEXT);
                let _ = SetBkColor(hdc, FIELD);
                return LRESULT((*app_ptr).brush_field.0 as isize);
            }
            WM_CTLCOLORSTATIC if !app_ptr.is_null() => {
                let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
                let child = HWND(lparam.0 as *mut core::ffi::c_void);
                if child == (*app_ptr).log {
                    let _ = SetTextColor(hdc, TEXT);
                    let _ = SetBkColor(hdc, FIELD);
                    return LRESULT((*app_ptr).brush_field.0 as isize);
                }
                let _ = SetTextColor(hdc, MUTED);
                let _ = SetBkMode(hdc, TRANSPARENT);
                return LRESULT((*app_ptr).brush_bg.0 as isize);
            }
            WM_DRAWITEM if !app_ptr.is_null() => {
                let draw = &*(lparam.0 as *const DRAWITEMSTRUCT);
                let id = draw.CtlID as i32;
                let pressed = (draw.itemState.0 & ODS_SELECTED.0) != 0;
                let fill_color = if id == ID_START {
                    if pressed { COLORREF(0x001714A8) } else { ACCENT }
                } else if id == ID_MINIMIZE || id == ID_CLOSE {
                    if pressed { BORDER } else { BG }
                } else if pressed {
                    BORDER
                } else {
                    PANEL
                };
                let brush = CreateSolidBrush(fill_color);
                let _ = FillRect(draw.hDC, &draw.rcItem, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = SetBkMode(draw.hDC, TRANSPARENT);
                let _ = SetTextColor(draw.hDC, TEXT);
                let old_font = SelectObject(draw.hDC, HGDIOBJ((*app_ptr).font_bold.0));
                let mut rect = draw.rcItem;
                let mut buffer = [0u16; 64];
                let copied = GetWindowTextW(draw.hwndItem, &mut buffer);
                if copied > 0 {
                    let _ = DrawTextW(
                        draw.hDC,
                        &mut buffer[..copied as usize],
                        &mut rect,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
                let _ = SelectObject(draw.hDC, old_font);
                return LRESULT(1);
            }
            WM_NCHITTEST if !app_ptr.is_null() => {
                let sx = (lparam.0 & 0xffff) as i16 as i32;
                let sy = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let mut wr = RECT::default();
                if GetWindowRect(hwnd, &mut wr).is_ok() {
                    let cx = sx - wr.left;
                    let cy = sy - wr.top;
                    if (0..38).contains(&cy) && cx < 344 {
                        return LRESULT(HTCAPTION as isize);
                    }
                }
            }
            WM_PAINT if !app_ptr.is_null() => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut title_rect = RECT {
                    left: 16,
                    top: 8,
                    right: 338,
                    bottom: 34,
                };
                let title = wide("TELEGRAM // REMOTE CONTROL");
                let old_font = SelectObject(hdc, HGDIOBJ((*app_ptr).font_mono.0));
                let _ = SetBkMode(hdc, TRANSPARENT);
                let _ = SetTextColor(hdc, TEXT);
                let mut title_buf = title[..title.len() - 1].to_vec();
                let _ = DrawTextW(
                    hdc,
                    &mut title_buf,
                    &mut title_rect,
                    DT_SINGLELINE | DT_VCENTER,
                );
                let divider = CreateSolidBrush(BORDER);
                let line = RECT {
                    left: 16,
                    top: 35,
                    right: 408,
                    bottom: 36,
                };
                let _ = FillRect(hdc, &line, divider);
                let _ = DeleteObject(HGDIOBJ(divider.0));
                let accent = CreateSolidBrush(ACCENT);
                let tick = RECT {
                    left: 16,
                    top: 35,
                    right: 48,
                    bottom: 37,
                };
                let _ = FillRect(hdc, &tick, accent);
                let _ = DeleteObject(HGDIOBJ(accent.0));
                let _ = SelectObject(hdc, old_font);
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_CLOSE if !app_ptr.is_null() => {
                if (*app_ptr).settings.hide_to_tray {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_TRAYICON if !app_ptr.is_null() => {
                let event = lparam.0 as u32;
                if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                    (*app_ptr).show_window();
                } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    (*app_ptr).tray_menu();
                }
                return LRESULT(0);
            }
            WM_DESTROY if !app_ptr.is_null() => {
                let _ = KillTimer(Some(hwnd), TIMER_POLL);
                (*app_ptr).stop_bot();
                (*app_ptr).remove_tray();
                if !(*app_ptr).font_ui.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*app_ptr).font_ui.0));
                }
                if !(*app_ptr).font_bold.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*app_ptr).font_bold.0));
                }
                if !(*app_ptr).font_mono.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*app_ptr).font_mono.0));
                }
                if !(*app_ptr).brush_bg.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*app_ptr).brush_bg.0));
                }
                if !(*app_ptr).brush_field.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ((*app_ptr).brush_field.0));
                }
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let instance = HINSTANCE(module.0);
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let app_icon = LoadIconW(Some(instance), PCWSTR(APP_ICON_ID as *const u16))
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default());
        let background = CreateSolidBrush(BG);

        let class = WNDCLASSW {
            hCursor: cursor,
            hIcon: app_icon,
            hInstance: instance,
            lpszClassName: APP_CLASS,
            lpfnWndProc: Some(wnd_proc),
            hbrBackground: background,
            ..Default::default()
        };
        RegisterClassW(&class);

        let mut app = Box::new(NativeApp::new());
        let mut style = WS_POPUP;
        if !app.settings.start_minimized {
            style |= WS_VISIBLE;
        }

        let window_width = 428;
        let window_height = 338;
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);
        let window_x = ((screen_width - window_width) / 2).max(0);
        let window_y = ((screen_height - window_height) / 2).max(0);

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            APP_CLASS,
            APP_TITLE,
            style,
            window_x,
            window_y,
            window_width,
            window_height,
            None,
            None,
            Some(instance),
            Some((&mut *app as *mut NativeApp).cast()),
        )?;
        app.hwnd = hwnd;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_wide_fixed<const N: usize>(target: &mut [u16; N], text: &str) {
    for (slot, value) in target
        .iter_mut()
        .zip(text.encode_utf16().take(N.saturating_sub(1)))
    {
        *slot = value;
    }
}

unsafe fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}
