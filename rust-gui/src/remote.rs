use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[repr(C)]
struct NativePoint {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut NativePoint) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn mouse_event(flags: u32, dx: u32, dy: u32, data: u32, extra_info: usize);
    fn keybd_event(vk: u8, scan: u8, flags: u32, extra_info: usize);
    fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> isize;
    fn SetClipboardData(format: u32, memory: isize) -> isize;
    fn EnumWindows(callback: Option<unsafe extern "system" fn(isize, isize) -> i32>, lparam: isize) -> i32;
    fn IsWindowVisible(hwnd: isize) -> i32;
    fn GetWindowTextLengthW(hwnd: isize) -> i32;
    fn GetWindowTextW(hwnd: isize, text: *mut u16, max_count: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
    fn GetDC(hwnd: isize) -> isize;
    fn GetWindowDC(hwnd: isize) -> isize;
    fn GetDesktopWindow() -> isize;
    fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalFree(memory: isize) -> isize;
    fn GlobalLock(memory: isize) -> *mut c_void;
    fn GlobalUnlock(memory: isize) -> i32;
    fn GlobalSize(memory: isize) -> usize;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> isize;
    fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
    fn TerminateProcess(process: isize, exit_code: u32) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetLastError() -> u32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: isize) -> isize;
    fn CreateCompatibleBitmap(dc: isize, width: i32, height: i32) -> isize;
    fn SelectObject(dc: isize, object: isize) -> isize;
    fn BitBlt(dest: isize, x: i32, y: i32, width: i32, height: i32, src: isize, src_x: i32, src_y: i32, rop: u32) -> i32;
    fn DeleteObject(object: isize) -> i32;
    fn DeleteDC(dc: isize) -> i32;
}

#[link(name = "gdiplus")]
unsafe extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const GdiplusStartupInput, output: *mut c_void) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateBitmapFromHBITMAP(bitmap: isize, palette: isize, image: *mut *mut c_void) -> i32;
    fn GdipSaveImageToFile(image: *mut c_void, filename: *const u16, encoder: *const NativeGuid, params: *const c_void) -> i32;
    fn GdipDisposeImage(image: *mut c_void) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const NativeGuid,
        outer: *mut c_void,
        context: u32,
        iid: *const NativeGuid,
        object: *mut *mut c_void,
    ) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct ProcessEntry32W {
    size: u32,
    usage: u32,
    process_id: u32,
    default_heap_id: usize,
    module_id: u32,
    threads: u32,
    parent_process_id: u32,
    base_priority: i32,
    flags: u32,
    exe_file: [u16; 260],
}

#[repr(C)]
struct GdiplusStartupInput {
    version: u32,
    debug_callback: *mut c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

const CLSID_MMDEVICE_ENUMERATOR: NativeGuid = NativeGuid { data1: 0xbcde0395, data2: 0xe52f, data3: 0x467c, data4: [0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e] };
const IID_IMMDEVICE_ENUMERATOR: NativeGuid = NativeGuid { data1: 0xa95664d2, data2: 0x9614, data3: 0x4f35, data4: [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6] };
const IID_IAUDIO_ENDPOINT_VOLUME: NativeGuid = NativeGuid { data1: 0x5cdf2c82, data2: 0x841e, data3: 0x4546, data4: [0x97, 0x22, 0x0c, 0xf7, 0x40, 0x78, 0x22, 0x9a] };
const CLSID_PNG_ENCODER: NativeGuid = NativeGuid { data1: 0x557cf406, data2: 0x1a04, data3: 0x11d3, data4: [0x9a, 0x73, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e] };

const COINIT_APARTMENTTHREADED: u32 = 0x2;
const CLSCTX_ALL: u32 = 23;
const E_RENDER: i32 = 0;
const E_MULTIMEDIA: i32 = 1;
const WM_SYSCOMMAND: u32 = 0x0112;
const SC_MONITORPOWER: usize = 0xF170;
const HWND_BROADCAST: isize = 0xffff;

const MOUSE_LEFT_DOWN: u32 = 0x0002;
const MOUSE_LEFT_UP: u32 = 0x0004;
const MOUSE_RIGHT_DOWN: u32 = 0x0008;
const MOUSE_RIGHT_UP: u32 = 0x0010;
const MOUSE_WHEEL: u32 = 0x0800;
const KEYEVENTF_KEYUP: u32 = 0x0002;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;
const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const PROCESS_TERMINATE: u32 = 0x0001;
const INVALID_HANDLE_VALUE: isize = -1;
const SRCCOPY: u32 = 0x00CC_0020;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            text.push('\n');
            text.push_str(err.trim());
        }
    }
    Ok(text.trim().to_owned())
}

pub fn run_powershell(script: &str) -> Result<String, String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if err.is_empty() {
            format!("PowerShell exited with {}", output.status)
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn status_text() -> Result<String, String> {
    run_powershell(
        r#"$cpu=(Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average; $os=Get-CimInstance Win32_OperatingSystem; $ramTotal=[math]::Round($os.TotalVisibleMemorySize/1MB,1); $ramUsed=[math]::Round(($os.TotalVisibleMemorySize-$os.FreePhysicalMemory)/1MB,1); $ramPct=if($os.TotalVisibleMemorySize){[math]::Round((($os.TotalVisibleMemorySize-$os.FreePhysicalMemory)/$os.TotalVisibleMemorySize)*100,0)}else{0}; $d=Get-CimInstance Win32_LogicalDisk -Filter "DeviceID='C:'"; $diskTotal=[math]::Round($d.Size/1GB,1); $diskUsed=[math]::Round(($d.Size-$d.FreeSpace)/1GB,1); $diskPct=if($d.Size){[math]::Round((($d.Size-$d.FreeSpace)/$d.Size)*100,0)}else{0}; $uptime=(Get-Date)-$os.LastBootUpTime; $b=Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1; $battery=if($b){"$($b.EstimatedChargeRemaining)%"}else{"Không có pin"}; "CPU: $cpu%`nRAM: $ramPct% ($ramUsed/$ramTotal GiB)`nDisk C: $diskPct% ($diskUsed/$diskTotal GiB)`nUptime: $([int]$uptime.TotalDays)d $($uptime.Hours)h $($uptime.Minutes)m`nBattery: $battery`nUser: $env:USERNAME`nHostname: $env:COMPUTERNAME""#,
    )
}

pub fn battery_text() -> Result<String, String> {
    run_powershell(
        "$b=Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1; if($b){\"Pin: $($b.EstimatedChargeRemaining)% - status $($b.BatteryStatus)\"}else{\"Máy không có pin hoặc không đọc được thông tin pin.\"}",
    )
}

pub fn uptime_text() -> Result<String, String> {
    run_powershell(
        "$os=Get-CimInstance Win32_OperatingSystem; $u=(Get-Date)-$os.LastBootUpTime; \"Uptime: $([int]$u.TotalDays)d $($u.Hours)h $($u.Minutes)m $($u.Seconds)s\"",
    )
}

pub fn clipboard_get() -> Result<String, String> {
    with_clipboard(|| unsafe {
        let memory = GetClipboardData(CF_UNICODETEXT);
        if memory == 0 {
            return Err("Clipboard does not contain Unicode text".to_owned());
        }
        let ptr = GlobalLock(memory) as *const u16;
        if ptr.is_null() {
            return Err("GlobalLock(GetClipboardData) failed".to_owned());
        }
        let max_units = GlobalSize(memory) / std::mem::size_of::<u16>();
        let slice = std::slice::from_raw_parts(ptr, max_units);
        let len = slice.iter().position(|&value| value == 0).unwrap_or(max_units);
        let text = String::from_utf16_lossy(&slice[..len]);
        GlobalUnlock(memory);
        Ok(text)
    })
}
pub fn clipboard_set(text: &str) -> Result<(), String> {
    let mut wide = text.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    with_clipboard(|| unsafe {
        if EmptyClipboard() == 0 {
            return Err("EmptyClipboard failed".to_owned());
        }
        let bytes = wide.len() * std::mem::size_of::<u16>();
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if memory == 0 {
            return Err("GlobalAlloc failed".to_owned());
        }
        let ptr = GlobalLock(memory) as *mut u16;
        if ptr.is_null() {
            GlobalFree(memory);
            return Err("GlobalLock failed".to_owned());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        GlobalUnlock(memory);
        if SetClipboardData(CF_UNICODETEXT, memory) == 0 {
            GlobalFree(memory);
            return Err("SetClipboardData failed".to_owned());
        }
        Ok(())
    })
}

pub fn type_text(text: &str) -> Result<(), String> {
    clipboard_set(text)?;
    key_press("ctrl+v")
}

pub fn key_press(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Key is empty".to_owned());
    }
    if !key.contains('+') && key.chars().count() > 1 && virtual_key(key).is_none() {
        return type_text(key);
    }

    let parts = key.split('+').map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>();
    let Some(main) = parts.last() else {
        return Err("Key is empty".to_owned());
    };
    let mut modifiers = Vec::new();
    for modifier in &parts[..parts.len().saturating_sub(1)] {
        let vk = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 0x11,
            "shift" => 0x10,
            "alt" => 0x12,
            "win" | "windows" | "meta" => 0x5B,
            _ => return Err(format!("Unsupported modifier: {modifier}")),
        };
        modifiers.push(vk);
    }
    let vk = virtual_key(main).ok_or_else(|| format!("Unsupported key: {main}"))?;
    unsafe {
        for modifier in &modifiers {
            keybd_event(*modifier, 0, 0, 0);
        }
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
        for modifier in modifiers.iter().rev() {
            keybd_event(*modifier, 0, KEYEVENTF_KEYUP, 0);
        }
    }
    Ok(())
}

fn virtual_key(key: &str) -> Option<u8> {
    let lower = key.trim().to_ascii_lowercase();
    if lower.len() == 1 {
        let byte = lower.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some(byte.to_ascii_uppercase());
        }
        if byte.is_ascii_digit() {
            return Some(byte);
        }
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=12).contains(&number)
    {
        return Some(0x70 + number - 1);
    }
    Some(match lower.as_str() {
        "backspace" | "bksp" => 0x08,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "shift" => 0x10,
        "ctrl" | "control" => 0x11,
        "alt" => 0x12,
        "pause" => 0x13,
        "capslock" | "caps" => 0x14,
        "esc" | "escape" => 0x1B,
        "space" => 0x20,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "printscreen" | "prtsc" => 0x2C,
        "insert" | "ins" => 0x2D,
        "delete" | "del" => 0x2E,
        "win" | "windows" | "meta" => 0x5B,
        "menu" | "apps" => 0x5D,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        _ => return None,
    })
}

pub fn media_key(vk: u8) -> Result<(), String> {
    unsafe { keybd_event(vk, 0, 0, 0); }
    std::thread::sleep(std::time::Duration::from_millis(40));
    unsafe { keybd_event(vk, 0, KEYEVENTF_KEYUP, 0); }
    Ok(())
}

pub fn video_key(key: &str) -> Result<(), String> {
    key_press(key)
}

pub fn mouse_action(action: &str) -> Result<(), String> {
    match action {
        "up" => mouse_move(0, -30),
        "down" => mouse_move(0, 30),
        "left" => mouse_move(-30, 0),
        "right" => mouse_move(30, 0),
        "left_click" => mouse_click(false),
        "right_click" => mouse_click(true),
        "double_click" => mouse_double_click(),
        _ => Err("Unknown mouse action".to_owned()),
    }
}

pub fn mouse_move(dx: i32, dy: i32) -> Result<(), String> {
    let mut point = NativePoint { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point) == 0 {
            return Err("GetCursorPos failed".to_owned());
        }
        if SetCursorPos(point.x.saturating_add(dx), point.y.saturating_add(dy)) == 0 {
            return Err("SetCursorPos failed".to_owned());
        }
    }
    Ok(())
}

fn mouse_click(right: bool) -> Result<(), String> {
    let (down, up) = if right {
        (MOUSE_RIGHT_DOWN, MOUSE_RIGHT_UP)
    } else {
        (MOUSE_LEFT_DOWN, MOUSE_LEFT_UP)
    };
    unsafe {
        mouse_event(down, 0, 0, 0, 0);
        mouse_event(up, 0, 0, 0, 0);
    }
    Ok(())
}

pub fn mouse_double_click() -> Result<(), String> {
    mouse_click(false)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    mouse_click(false)
}

pub fn mouse_scroll(delta: i32) -> Result<(), String> {
    let wheel_delta = delta.clamp(-1200, 1200);
    unsafe {
        mouse_event(MOUSE_WHEEL, 0, 0, wheel_delta as u32, 0);
    }
    Ok(())
}

pub fn set_volume(percent: u32) -> Result<(), String> {
    with_endpoint_volume(|endpoint| unsafe {
        let vtable = *(endpoint as *mut *mut *mut c_void);
        let set_scalar: unsafe extern "system" fn(*mut c_void, f32, *const NativeGuid) -> i32 =
            std::mem::transmute(*vtable.add(7));
        hresult(set_scalar(endpoint, percent.min(100) as f32 / 100.0, std::ptr::null()), "SetMasterVolumeLevelScalar")
    })
}
pub fn change_volume(delta: i32) -> Result<(), String> {
    let current = current_volume_percent()? as i32;
    set_volume((current + delta).clamp(0, 100) as u32)
}
pub fn current_volume_percent() -> Result<u32, String> {
    with_endpoint_volume(|endpoint| unsafe {
        let vtable = *(endpoint as *mut *mut *mut c_void);
        let get_scalar: unsafe extern "system" fn(*mut c_void, *mut f32) -> i32 =
            std::mem::transmute(*vtable.add(9));
        let mut value = 0.0f32;
        hresult(get_scalar(endpoint, &mut value), "GetMasterVolumeLevelScalar")?;
        Ok((value.clamp(0.0, 1.0) * 100.0).round() as u32)
    })
}
pub fn mute(muted: bool) -> Result<(), String> {
    with_endpoint_volume(|endpoint| unsafe {
        let vtable = *(endpoint as *mut *mut *mut c_void);
        let set_mute: unsafe extern "system" fn(*mut c_void, i32, *const NativeGuid) -> i32 =
            std::mem::transmute(*vtable.add(14));
        hresult(set_mute(endpoint, muted as i32, std::ptr::null()), "SetMute")
    })
}
pub fn monitor_off() -> Result<(), String> {
    unsafe { SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2); }
    Ok(())
}
pub fn brightness(percent: u32) -> Result<(), String> {
    run_powershell(&format!("(Get-WmiObject -Namespace root/WMI -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1,{})", percent.min(100))).map(|_| ())
}

pub fn lock_pc() -> Result<(), String> {
    spawn("rundll32.exe", &["user32.dll,LockWorkStation"])
}
pub fn shutdown_now() -> Result<(), String> {
    spawn("shutdown.exe", &["/s", "/t", "3"])
}
pub fn restart_now() -> Result<(), String> {
    spawn("shutdown.exe", &["/r", "/t", "3"])
}
pub fn hibernate_now() -> Result<(), String> {
    spawn("shutdown.exe", &["/h"])
}
pub fn sleep_now() -> Result<(), String> {
    spawn("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"])
}
pub fn cancel_shutdown() -> Result<(), String> {
    spawn("shutdown.exe", &["/a"])
}
pub fn shutdown_after(seconds: u64) -> Result<(), String> {
    spawn("shutdown.exe", &["/s", "/t", &seconds.to_string()])
}

pub fn screenshot(download_dir: &str) -> Result<PathBuf, String> {
    let requested = PathBuf::from(download_dir);
    let dir = if requested.is_absolute() {
        requested
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(requested)
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("screenshot_{}.png", unix_seconds()));
    capture_screen_png(&path)?;
    Ok(path)
}

pub fn list_directory(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if !path.is_dir() {
        return Err(format!("Thư mục không tồn tại: {}", path.display()));
    }
    let mut entries = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| {
        (
            !e.path().is_dir(),
            e.file_name().to_string_lossy().to_ascii_lowercase(),
        )
    });
    let mut lines = vec![path.display().to_string()];
    for entry in entries.into_iter().take(100) {
        let p = entry.path();
        if p.is_dir() {
            lines.push(format!("FOLDER {}", entry.file_name().to_string_lossy()))
        } else {
            let s = entry.metadata().map(|m| m.len()).unwrap_or(0);
            lines.push(format!(
                "[FILE] {} - {} bytes",
                entry.file_name().to_string_lossy(),
                s
            ))
        }
    }
    if lines.len() == 1 {
        lines.push("Thư mục trống.".to_owned())
    }
    Ok(lines.join("\n"))
}

pub fn windows_list() -> Result<String, String> {
    let windows = visible_windows()?;
    let names = process_list()?.into_iter().map(|(name, pid)| (pid, name)).collect::<std::collections::HashMap<_, _>>();
    if windows.is_empty() {
        return Ok("Không có cửa sổ hiển thị.".to_owned());
    }
    Ok(windows.into_iter().map(|(pid, title)| {
        let name = names.get(&pid).map(String::as_str).unwrap_or("unknown.exe");
        format!("{name} | PID {pid} | {title}")
    }).collect::<Vec<_>>().join("\n"))
}
pub fn close_window(title: &str) -> Result<(), String> {
    let needle = title.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Err("Window title is empty".to_owned());
    }
    let mut matched = std::collections::HashSet::new();
    for (pid, window_title) in visible_windows()? {
        if window_title.to_ascii_lowercase().contains(&needle) {
            matched.insert(pid);
        }
    }
    if matched.is_empty() {
        return Err(format!("Không tìm thấy cửa sổ chứa: {title}"));
    }
    for pid in matched {
        terminate_process(pid)?;
    }
    Ok(())
}

pub fn open_url(url: &str) -> Result<(), String> {
    let l = url.to_ascii_lowercase();
    if !(l.starts_with("http://") || l.starts_with("https://")) {
        return Err("URL không hợp lệ. Chỉ hỗ trợ http:// hoặc https://".to_owned());
    }
    spawn("rundll32.exe", &["url.dll,FileProtocolHandler", url])
}

pub fn open_app(alias: &str) -> Result<(), String> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let roaming = std::env::var("APPDATA").unwrap_or_default();
    let (path, args): (String, Vec<String>) = match alias.to_ascii_lowercase().as_str() {
        "chrome" => (
            r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
            vec![],
        ),
        "brave" => (
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe".into(),
            vec![],
        ),
        "thorium" => (
            format!(r"{}\Thorium\Application\thorium.exe", local),
            vec![],
        ),
        "spotify" => (format!(r"{}\Spotify\Spotify.exe", roaming), vec![]),
        "discord" => (
            format!(r"{}\Discord\Update.exe", local),
            vec!["--processStart".into(), "Discord.exe".into()],
        ),
        _ => return Err(format!("Alias hỗ trợ: {}", aliases())),
    };
    if !Path::new(&path).exists() {
        return Err(format!("Không tìm thấy app: {alias}"));
    }
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    spawn(&path, &refs)
}

pub fn open_video(url: &str) -> Result<(), String> {
    if !(url.contains("youtube.com/") || url.contains("youtu.be/")) {
        return Err("Hãy gửi link YouTube: /playvideo <link>".to_owned());
    }
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let thorium = format!(r"{}\Thorium\Application\thorium.exe", local);
    if Path::new(&thorium).exists() {
        spawn(&thorium, &[url])
    } else {
        open_url(url)
    }
}
pub fn close_thorium() -> Result<(), String> {
    kill_name("thorium.exe")
}
pub fn kill_name(name: &str) -> Result<(), String> {
    spawn("taskkill.exe", &["/F", "/IM", name])
}
pub fn kill_pid(pid: u32) -> Result<(), String> {
    spawn("taskkill.exe", &["/F", "/PID", &pid.to_string()])
}

pub fn process_list() -> Result<Vec<(String, u32)>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("CreateToolhelp32Snapshot failed".to_owned());
    }
    let mut entry = ProcessEntry32W {
        size: std::mem::size_of::<ProcessEntry32W>() as u32,
        usage: 0,
        process_id: 0,
        default_heap_id: 0,
        module_id: 0,
        threads: 0,
        parent_process_id: 0,
        base_priority: 0,
        flags: 0,
        exe_file: [0; 260],
    };
    let mut out = Vec::new();
    unsafe {
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry.exe_file.iter().position(|&c| c == 0).unwrap_or(entry.exe_file.len());
                out.push((String::from_utf16_lossy(&entry.exe_file[..len]), entry.process_id));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    out.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()).then(a.1.cmp(&b.1)));
    Ok(out)
}
pub fn system_command(name: &str) -> Result<String, String> {
    match name {
        "tasklist" => run_capture("tasklist.exe", &[]),
        "systeminfo" => run_capture("systeminfo.exe", &[]),
        "ipconfig" => run_capture("ipconfig.exe", &["/all"]),
        "release" => run_capture("ipconfig.exe", &["/release"]),
        "renew" => run_capture("ipconfig.exe", &["/renew"]),
        "netuser" => run_capture("net.exe", &["user"]),
        "whoami" => run_capture("whoami.exe", &[]),
        "hostname" => run_capture("hostname.exe", &[]),
        "wifi" => run_capture("netsh.exe", &["wlan", "show", "interfaces"]),
        _ => Err("Unknown system command".to_owned()),
    }
}
pub fn aliases() -> &'static str {
    "chrome, brave, thorium, spotify, discord"
}

fn with_clipboard<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(0) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err("OpenClipboard failed".to_owned());
    }
    let result = f();
    unsafe { CloseClipboard(); }
    result
}

fn capture_screen_png(path: &Path) -> Result<(), String> {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if width <= 0 || height <= 0 {
        return Err("GetSystemMetrics returned an invalid screen size".to_owned());
    }

    unsafe {
        let desktop = GetDesktopWindow();
        let screen_dc = GetWindowDC(desktop);
        if screen_dc == 0 {
            return Err(format!("GetWindowDC failed: {}", GetLastError()));
        }
        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc == 0 {
            ReleaseDC(desktop, screen_dc);
            return Err(format!("CreateCompatibleDC failed: {}", GetLastError()));
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap == 0 {
            DeleteDC(memory_dc);
            ReleaseDC(desktop, screen_dc);
            return Err(format!("CreateCompatibleBitmap failed: {}", GetLastError()));
        }
        let previous = SelectObject(memory_dc, bitmap);
        let copied = BitBlt(memory_dc, 0, 0, width, height, screen_dc, 0, 0, SRCCOPY);
        if previous != 0 {
            SelectObject(memory_dc, previous);
        }
        DeleteDC(memory_dc);
        ReleaseDC(desktop, screen_dc);
        if copied == 0 {
            let error = GetLastError();
            DeleteObject(bitmap);
            return Err(format!("BitBlt failed: {error}"));
        }

        let input = GdiplusStartupInput {
            version: 1,
            debug_callback: std::ptr::null_mut(),
            suppress_background_thread: 0,
            suppress_external_codecs: 0,
        };
        let mut token = 0usize;
        let status = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
        if status != 0 {
            DeleteObject(bitmap);
            return Err(format!("GdiplusStartup failed: {status}"));
        }
        let result = (|| {
            let mut image = std::ptr::null_mut();
            let status = GdipCreateBitmapFromHBITMAP(bitmap, 0, &mut image);
            if status != 0 || image.is_null() {
                return Err(format!("GdipCreateBitmapFromHBITMAP failed: {status}"));
            }
            let mut filename = path.as_os_str().encode_wide().collect::<Vec<_>>();
            filename.push(0);
            let status = GdipSaveImageToFile(image, filename.as_ptr(), &CLSID_PNG_ENCODER, std::ptr::null());
            GdipDisposeImage(image);
            if status != 0 {
                return Err(format!("GdipSaveImageToFile failed: {status}"));
            }
            Ok(())
        })();
        GdiplusShutdown(token);
        DeleteObject(bitmap);
        result
    }
}

fn visible_windows() -> Result<Vec<(u32, String)>, String> {
    unsafe extern "system" fn collect(hwnd: isize, lparam: isize) -> i32 {
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut text = vec![0u16; len as usize + 1];
            let copied = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
            if copied <= 0 {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let windows = &mut *(lparam as *mut Vec<(u32, String)>);
            windows.push((pid, String::from_utf16_lossy(&text[..copied as usize])));
            1
        }
    }

    let mut windows: Vec<(u32, String)> = Vec::new();
    let ok = unsafe { EnumWindows(Some(collect), (&mut windows as *mut Vec<(u32, String)>) as isize) };
    if ok == 0 {
        return Err("EnumWindows failed".to_owned());
    }
    windows.sort_by(|a, b| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase()));
    Ok(windows)
}

fn terminate_process(pid: u32) -> Result<(), String> {
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process == 0 {
        return Err(format!("OpenProcess failed for PID {pid}"));
    }
    let ok = unsafe { TerminateProcess(process, 1) };
    unsafe { CloseHandle(process); }
    if ok == 0 {
        Err(format!("TerminateProcess failed for PID {pid}"))
    } else {
        Ok(())
    }
}

fn with_endpoint_volume<T>(f: impl FnOnce(*mut c_void) -> Result<T, String>) -> Result<T, String> {
    unsafe {
        let init = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        let should_uninit = init >= 0;
        if init < 0 && init != -2147417850 {
            return Err(format!("CoInitializeEx failed: 0x{:08X}", init as u32));
        }

        let result = (|| {
            let mut enumerator = std::ptr::null_mut();
            hresult(
                CoCreateInstance(
                    &CLSID_MMDEVICE_ENUMERATOR,
                    std::ptr::null_mut(),
                    CLSCTX_ALL,
                    &IID_IMMDEVICE_ENUMERATOR,
                    &mut enumerator,
                ),
                "CoCreateInstance(MMDeviceEnumerator)",
            )?;

            let enum_vtable = *(enumerator as *mut *mut *mut c_void);
            let get_default: unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> i32 =
                std::mem::transmute(*enum_vtable.add(4));
            let mut device = std::ptr::null_mut();
            let hr = get_default(enumerator, E_RENDER, E_MULTIMEDIA, &mut device);
            release_com(enumerator);
            hresult(hr, "GetDefaultAudioEndpoint")?;

            let device_vtable = *(device as *mut *mut *mut c_void);
            let activate: unsafe extern "system" fn(*mut c_void, *const NativeGuid, u32, *mut c_void, *mut *mut c_void) -> i32 =
                std::mem::transmute(*device_vtable.add(3));
            let mut endpoint = std::ptr::null_mut();
            let hr = activate(device, &IID_IAUDIO_ENDPOINT_VOLUME, CLSCTX_ALL, std::ptr::null_mut(), &mut endpoint);
            release_com(device);
            hresult(hr, "IMMDevice::Activate(IAudioEndpointVolume)")?;

            let out = f(endpoint);
            release_com(endpoint);
            out
        })();

        if should_uninit {
            CoUninitialize();
        }
        result
    }
}

unsafe fn release_com(object: *mut c_void) {
    if object.is_null() { return; }
    let vtable = unsafe { *(object as *mut *mut *mut c_void) };
    let release: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(*vtable.add(2)) };
    unsafe { release(object); }
}

fn hresult(hr: i32, operation: &str) -> Result<(), String> {
    if hr >= 0 { Ok(()) } else { Err(format!("{operation} failed: 0x{:08X}", hr as u32)) }
}
fn spawn(program: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}
fn hide_console(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
