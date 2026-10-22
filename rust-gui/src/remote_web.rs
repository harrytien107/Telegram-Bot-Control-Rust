use crate::remote;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct RemoteServer {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
}

static SERVER: OnceLock<Mutex<Option<RemoteServer>>> = OnceLock::new();

pub fn start_or_get() -> Result<String, String> {
    let slot = SERVER.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|_| "Remote server lock failed".to_owned())?;
    if let Some(server) = guard.as_ref() {
        return Ok(panel_url(server.port, &server.token));
    }

    let listener = TcpListener::bind("0.0.0.0:0").map_err(|e| format!("Không mở được remote server: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = make_token(port);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_token = token.clone();
    thread::Builder::new()
        .name("web-remote".to_owned())
        .spawn(move || serve(listener, worker_token, worker_stop))
        .map_err(|e| e.to_string())?;

    let url = panel_url(port, &token);
    *guard = Some(RemoteServer { port, token, stop });
    Ok(url)
}

pub fn stop() -> bool {
    let Some(slot) = SERVER.get() else { return false; };
    let Ok(mut guard) = slot.lock() else { return false; };
    if let Some(server) = guard.take() {
        server.stop.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

fn serve(listener: TcpListener, token: String, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let _ = handle_connection(&mut stream, &token);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn handle_connection(stream: &mut TcpStream, token: &str) -> Result<(), String> {
    let request = read_request(stream)?;
    let header_end = request.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| "Bad HTTP request".to_owned())?;
    let header = String::from_utf8_lossy(&request[..header_end]);
    let first = header.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let body = &request[header_end + 4..];
    let prefix = format!("/{token}");
    if !target.starts_with(&prefix) {
        return respond(stream, 404, "text/plain; charset=utf-8", b"Not found");
    }

    let rest = &target[prefix.len()..];
    if method == "GET" && (rest == "/" || rest.is_empty()) {
        let html = panel_html();
        return respond(stream, 200, "text/html; charset=utf-8", html.as_bytes());
    }
    if method == "GET" && rest == "/volume" {
        let value = remote::current_volume_percent()?;
        let body = format!("{{\"volume\":{value}}}");
        return respond(stream, 200, "application/json", body.as_bytes());
    }
    if method != "POST" {
        return respond(stream, 405, "text/plain; charset=utf-8", b"Method not allowed");
    }

    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let result = match path {
        "/move" => {
            let dx = query_value(query, "dx").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0).clamp(-120, 120);
            let dy = query_value(query, "dy").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0).clamp(-120, 120);
            remote::mouse_move(dx, dy)
        }
        "/click" => remote::mouse_action(if query_value(query, "button").as_deref() == Some("right") { "right_click" } else { "left_click" }),
        "/doubleclick" => remote::mouse_double_click(),
        "/scroll" => {
            let delta = query_value(query, "delta").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            remote::mouse_scroll(delta)
        }
        "/volume" => {
            let value = query_value(query, "value").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).min(100);
            remote::set_volume(value)
        }
        "/text" => {
            let text = String::from_utf8_lossy(body);
            if text.len() > 4000 { Err("Text too long".to_owned()) } else { remote::type_text(&text) }
        }
        "/key" => {
            let key = query_value(query, "key").unwrap_or_default();
            if key.len() > 64 { Err("Key too long".to_owned()) } else { remote::key_press(&key) }
        }
        _ => Err("Unknown endpoint".to_owned()),
    };

    match result {
        Ok(()) => respond(stream, 200, "application/json", b"{\"ok\":true}"),
        Err(error) => respond(stream, 500, "text/plain; charset=utf-8", error.as_bytes()),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut data = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    let mut expected = None;
    loop {
        let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        data.extend_from_slice(&buf[..n]);
        if expected.is_none() {
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..pos]);
                let content_len = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
                }).unwrap_or(0);
                expected = Some(pos + 4 + content_len);
            }
        }
        if expected.is_some_and(|len| data.len() >= len) || data.len() > 16_384 { break; }
    }
    Ok(data)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(v); i += 3; continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(stream: &mut TcpStream, code: u16, content_type: &str, body: &[u8]) -> Result<(), String> {
    let status = match code { 200 => "OK", 404 => "Not Found", 405 => "Method Not Allowed", _ => "Error" };
    let header = format!("HTTP/1.1 {code} {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n", body.len());
    stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(body)).map_err(|e| e.to_string())
}

fn panel_url(port: u16, token: &str) -> String {
    format!("http://{}:{port}/{token}/", local_ip())
}

fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| { s.connect("8.8.8.8:80")?; s.local_addr() })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_owned())
}

fn make_token(port: u16) -> String {
    let mut hasher = DefaultHasher::new();
    port.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn panel_html() -> &'static str {
    r#"<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no"><meta name="color-scheme" content="dark"><title>PC Remote</title><style>
*{box-sizing:border-box}body{margin:0;background:#0b0d10;color:#f5f7fa;font:15px system-ui,-apple-system,Segoe UI,sans-serif;padding:18px;max-width:700px;margin:auto}h1{font-size:22px;margin:0 0 16px}.card{background:#15191f;border:1px solid #2a313b;border-radius:16px;padding:14px;margin:12px 0}.title{font-weight:700;margin-bottom:10px}.pad{height:260px;border-radius:14px;background:#0f1318;border:1px solid #343d49;touch-action:none;display:grid;place-items:center;color:#8f9aa8;user-select:none}.clicks{display:grid;grid-template-columns:1fr 1fr;gap:10px;margin-top:10px}button,input{font:inherit}button{border:1px solid #39424e;background:#20262e;color:#fff;border-radius:10px;padding:10px 8px}button:active{transform:scale(.98);background:#2b333e}.range{width:100%;accent-color:#fff}.row{display:flex;gap:8px}.row input{min-width:0;flex:1;background:#0f1318;color:#fff;border:1px solid #39424e;border-radius:12px;padding:12px}.keys{display:grid;grid-template-columns:repeat(4,1fr);gap:6px;margin-top:8px}.keyboard{display:flex;flex-direction:column;gap:6px;margin-top:10px}.keyrow{display:grid;gap:6px;grid-template-columns:repeat(var(--cols),1fr)}.small{color:#8f9aa8;font-size:12px;margin-top:8px}</style></head><body>
<h1>PC Remote</h1><div class="card"><div class="title">🖱 Touchpad</div><div id="pad" class="pad">Drag to move · swipe with 2 fingers to scroll</div><div class="clicks"><button onclick="post('click?button=left')">Left click</button><button onclick="post('click?button=right')">Right click</button><button onclick="post('doubleclick')">Double click</button><button onclick="post('scroll?delta=120')">Scroll ↑</button><button onclick="post('scroll?delta=-120')">Scroll ↓</button></div></div>
<div class="card"><div class="title">🎚 Volume <span id="volLabel">50%</span></div><input id="vol" class="range" type="range" min="0" max="100" value="50"></div>
<div class="card"><div class="title">⌨️ Keyboard</div><div class="row"><input id="text" maxlength="4000" placeholder="Type text to PC"><button id="send">Send</button></div><div id="keyboard" class="keyboard"></div><div class="small">Full remote keys + shortcuts. Text box is the fast way to type sentences.</div></div>
<script>const base=location.pathname;function post(p,body){return fetch(base+p,{method:'POST',body:body||''}).catch(()=>{})}function key(k){post('key?key='+encodeURIComponent(k))}const pad=document.getElementById('pad');let last=null,queued=false,dx=0,dy=0;pad.onpointerdown=e=>{last=[e.clientX,e.clientY];pad.setPointerCapture(e.pointerId)};pad.onpointermove=e=>{if(!last)return;dx+=Math.round((e.clientX-last[0])*1.7);dy+=Math.round((e.clientY-last[1])*1.7);last=[e.clientX,e.clientY];if(!queued){queued=true;setTimeout(()=>{let x=Math.max(-120,Math.min(120,dx)),y=Math.max(-120,Math.min(120,dy));dx=dy=0;queued=false;if(x||y)post(`move?dx=${x}&dy=${y}`)},35)}};pad.onpointerup=pad.onpointercancel=()=>last=null;pad.addEventListener('wheel',e=>{e.preventDefault();post('scroll?delta='+(e.deltaY<0?120:-120))},{passive:false});let touchY=null;pad.addEventListener('touchstart',e=>{if(e.touches.length===2)touchY=(e.touches[0].clientY+e.touches[1].clientY)/2},{passive:false});pad.addEventListener('touchmove',e=>{if(e.touches.length!==2||touchY===null)return;e.preventDefault();let y=(e.touches[0].clientY+e.touches[1].clientY)/2,d=y-touchY;if(Math.abs(d)>8){post('scroll?delta='+(d<0?120:-120));touchY=y}},{passive:false});pad.addEventListener('touchend',()=>touchY=null);const vol=document.getElementById('vol'),label=document.getElementById('volLabel');fetch(base+'volume').then(r=>r.json()).then(v=>{vol.value=v.volume;label.textContent=v.volume+'%'}).catch(()=>{});let vt;vol.oninput=()=>{label.textContent=vol.value+'%';clearTimeout(vt);vt=setTimeout(()=>post('volume?value='+vol.value),70)};document.getElementById('send').onclick=()=>{let t=document.getElementById('text');if(t.value){post('text',t.value);t.value=''}};document.getElementById('text').onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();document.getElementById('send').click()}};const rows=[['Esc','F1','F2','F3','F4','F5','F6','F7','F8','F9','F10','F11','F12'],['1','2','3','4','5','6','7','8','9','0','Backspace'],['Q','W','E','R','T','Y','U','I','O','P'],['A','S','D','F','G','H','J','K','L','Enter'],['Z','X','C','V','B','N','M','Left','Down','Up','Right'],['Tab','Space','Home','End','PageUp','PageDown','Insert','Delete'],['Ctrl+C','Ctrl+V','Ctrl+X','Ctrl+A','Ctrl+Z','Ctrl+Y','Alt+Tab','Win+D','Win+L','Ctrl+Shift+Esc']];const keyboard=document.getElementById('keyboard');for(const row of rows){let box=document.createElement('div');box.className='keyrow';box.style.setProperty('--cols',row.length);for(const k of row){let b=document.createElement('button');b.textContent=k;b.onclick=()=>key(k);box.appendChild(b)}keyboard.appendChild(box)}</script></body></html>"#
}
