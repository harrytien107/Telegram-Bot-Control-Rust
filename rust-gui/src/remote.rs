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
}

const MOUSE_LEFT_DOWN: u32 = 0x0002;
const MOUSE_LEFT_UP: u32 = 0x0004;
const MOUSE_RIGHT_DOWN: u32 = 0x0008;
const MOUSE_RIGHT_UP: u32 = 0x0010;
const MOUSE_WHEEL: u32 = 0x0800;
const KEYEVENTF_KEYUP: u32 = 0x0002;

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

pub fn run_powershell_env(script: &str, key: &str, value: &str) -> Result<String, String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env(key, value)
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
    run_powershell("Get-Clipboard -Raw")
}
pub fn clipboard_set(text: &str) -> Result<(), String> {
    run_powershell_env("Set-Clipboard -Value $env:TG_TEXT", "TG_TEXT", text).map(|_| ())
}

pub fn type_text(text: &str) -> Result<(), String> {
    run_powershell_env("Set-Clipboard -Value $env:TG_TEXT; Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('^v')", "TG_TEXT", text).map(|_| ())
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
    let script = format!(
        "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class K {{ [DllImport(\"user32.dll\")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo); }}'; [K]::keybd_event({vk},0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [K]::keybd_event({vk},0,2,[UIntPtr]::Zero)"
    );
    run_powershell(&script).map(|_| ())
}

pub fn video_key(key: &str) -> Result<(), String> {
    run_powershell_env("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait($env:TG_KEY)", "TG_KEY", key).map(|_| ())
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
    let v = ((percent.min(100) as f64) * 655.35) as u32;
    nircmd(&["setsysvolume", &v.to_string()])
}
pub fn change_volume(delta: i32) -> Result<(), String> {
    nircmd(&["changesysvolume", &delta.to_string()])
}
pub fn current_volume_percent() -> Result<u32, String> {
    let value = run_powershell(
        r#"$src=@"
using System;
using System.Runtime.InteropServices;
public enum EDataFlow { eRender, eCapture, eAll }
public enum ERole { eConsole, eMultimedia, eCommunications }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] public class MMDeviceEnumeratorComObject {}
[ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDeviceEnumerator { int NotImpl1(); [PreserveSig] int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IMMDevice ppDevice); }
[ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDevice { [PreserveSig] int Activate(ref Guid iid, int dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface); }
[ComImport, Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioEndpointVolume {
 int RegisterControlChangeNotify(IntPtr pNotify);
 int UnregisterControlChangeNotify(IntPtr pNotify);
 int GetChannelCount(out uint channelCount);
 int SetMasterVolumeLevel(float levelDB, Guid eventContext);
 int SetMasterVolumeLevelScalar(float level, Guid eventContext);
 int GetMasterVolumeLevel(out float levelDB);
 int GetMasterVolumeLevelScalar(out float level);
}
public static class AudioVolume {
 public static int Get() {
  var e=(IMMDeviceEnumerator)(new MMDeviceEnumeratorComObject());
  IMMDevice d; Marshal.ThrowExceptionForHR(e.GetDefaultAudioEndpoint(EDataFlow.eRender, ERole.eMultimedia, out d));
  object o; var iid=typeof(IAudioEndpointVolume).GUID; Marshal.ThrowExceptionForHR(d.Activate(ref iid, 23, IntPtr.Zero, out o));
  float v; Marshal.ThrowExceptionForHR(((IAudioEndpointVolume)o).GetMasterVolumeLevelScalar(out v));
  return (int)Math.Round(v*100);
 }
}
"@; Add-Type -TypeDefinition $src; [AudioVolume]::Get()"#,
    )?;
    value
        .trim()
        .parse::<u32>()
        .map(|v| v.min(100))
        .map_err(|e| format!("Không đọc được âm lượng hiện tại: {e}"))
}
pub fn mute(muted: bool) -> Result<(), String> {
    nircmd(&["mutesysvolume", if muted { "1" } else { "0" }])
}
pub fn monitor_off() -> Result<(), String> {
    nircmd(&["monitor", "off"])
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
    run_powershell_env(
        "$p=$env:TG_PATH; Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $bmp.Save($p,[System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()",
        "TG_PATH",
        &path.to_string_lossy(),
    )?;
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
    run_powershell(
        "Get-Process | Where-Object {$_.MainWindowTitle} | Select-Object ProcessName,Id,MainWindowTitle | Format-Table -AutoSize | Out-String -Width 300",
    )
}
pub fn close_window(title: &str) -> Result<(), String> {
    run_powershell_env("$t=$env:TG_TITLE; Get-Process | Where-Object {$_.MainWindowTitle -like \"*$t*\"} | Stop-Process -Force", "TG_TITLE", title).map(|_| ())
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
    let text = run_powershell(
        "Get-Process | Sort-Object ProcessName | ForEach-Object { \"$($_.ProcessName).exe|$($_.Id)\" }",
    )?;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((n, p)) = line.rsplit_once('|') {
            if let Ok(pid) = p.parse() {
                out.push((n.to_owned(), pid))
            }
        }
    }
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
fn nircmd(args: &[&str]) -> Result<(), String> {
    let exe = find_nircmd().ok_or_else(|| {
        "nircmdc.exe was not found. Put nircmd-x64 next to the portable app folder.".to_owned()
    })?;
    spawn(exe.to_string_lossy().as_ref(), args)
}
fn find_nircmd() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            roots.extend(p.ancestors().take(6).map(Path::to_path_buf));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend(cwd.ancestors().take(4).map(Path::to_path_buf));
    }
    for root in roots {
        for rel in [
            "nircmd-x64/nircmdc.exe",
            "nircmd-x64/nircmd.exe",
            "nircmdc.exe",
            "nircmd.exe",
        ] {
            let p = root.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
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
