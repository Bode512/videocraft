//! Headless Chromium browser discovery and frame-by-frame scene rendering
//! over the Chrome DevTools Protocol (CDP).
//!
//! The scene contract: the HTML page must define `window.vcSeek(t)`, a
//! deterministic function that repaints the page for time `t` in seconds.
//! VideoCraft navigates to the page, injects a style that disables CSS
//! animations/transitions, then for each frame calls `vcSeek(t)` and captures
//! a PNG screenshot. Frames are encoded to video by FFmpeg elsewhere.

use base64::Engine as _;
use serde_json::{json, Value};
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

type WsStream = WebSocket<TcpStream>;

const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// A discovered Chromium-based browser executable.
#[derive(Debug, Clone)]
pub struct Browser {
    pub path: PathBuf,
    pub name: String,
}

fn as_browser(path: PathBuf) -> Option<Browser> {
    let name = path.file_name()?.to_str()?.to_string();
    if path.is_file() {
        Some(Browser { path, name })
    } else {
        None
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&exe))
            .find(|p| p.is_file())
    })
}

/// Candidate list per platform. `VIDEOCRAFT_BROWSER` (path to the executable)
/// always wins, so agents can point at any Chromium build.
pub fn find_browser() -> Option<Browser> {
    if let Ok(env_path) = std::env::var("VIDEOCRAFT_BROWSER") {
        return as_browser(PathBuf::from(env_path));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            let root = std::env::var(var).unwrap_or_default();
            if root.is_empty() {
                continue;
            }
            candidates.push(PathBuf::from(&root).join(r"Google\Chrome\Application\chrome.exe"));
            candidates.push(PathBuf::from(&root).join(r"Chromium\Application\chrome.exe"));
            candidates.push(PathBuf::from(&root).join(r"Microsoft\Edge\Application\msedge.exe"));
        }
    } else if cfg!(target_os = "macos") {
        for name in ["Google Chrome", "Microsoft Edge", "Chromium", "Brave Browser"] {
            candidates.push(PathBuf::from(format!(
                "/Applications/{name}.app/Contents/MacOS/{name}"
            )));
        }
    } else {
        for name in [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
        ] {
            if let Some(p) = which(name) {
                if let Some(b) = as_browser(p) {
                    return Some(b);
                }
            }
        }
        return None;
    }
    candidates.into_iter().find_map(as_browser)
}

/// Convert an absolute path into a file:// URL, percent-encoding as needed.
/// Handles Windows canonical `\\?\C:\...` and `\\?\UNC\...` paths.
pub fn to_file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let (prefix, rest) = if let Some(r) = text.strip_prefix("//?/UNC/") {
        ("file://", r)
    } else if let Some(r) = text.strip_prefix("//?/") {
        ("file:///", r)
    } else if let Some(r) = text.strip_prefix("//") {
        ("file://", r)
    } else if text.starts_with('/') {
        ("file://", text.as_str())
    } else {
        ("file:///", text.as_str())
    };
    let mut out = String::with_capacity(rest.len() + prefix.len());
    out.push_str(prefix);
    for b in rest.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~'
            | b':' | b'=' | b'@' | b',' | b'+' | b'(' | b')' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Injected once after load: removes nondeterministic CSS motion so the only
/// driver of the picture is `vcSeek(t)`. Also reports whether the scene
/// implements the required contract.
const FREEZE_AND_CHECK: &str = "(function(){var s=document.createElement('style');\
s.textContent='*{animation:none!important;transition:none!important;caret-color:transparent!important}';\
(document.head||document.documentElement).appendChild(s);\
return String(typeof window.vcSeek==='function');})()";

pub struct SceneRenderer {
    child: Child,
    ws: WsStream,
    session: String,
    next_id: u64,
}

impl SceneRenderer {
    /// Launch a headless browser and attach to a blank page target.
    pub fn launch(
        browser: &Browser,
        width: u32,
        height: u32,
        profile_dir: &Path,
    ) -> Result<SceneRenderer, String> {
        let mut child = Command::new(&browser.path)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--mute-audio",
                "--hide-scrollbars",
                "--noerrdialogs",
            ])
            .arg(format!("--window-size={width},{height}"))
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch browser {}: {e}", browser.path.display()))?;

        let stderr = child.stderr.take();
        let buffer = Arc::new(Mutex::new(String::new()));
        let buf_clone = Arc::clone(&buffer);
        std::thread::spawn(move || {
            if let Some(mut s) = stderr {
                let mut chunk = [0u8; 1024];
                loop {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut b = buf_clone.lock().unwrap();
                            b.push_str(&String::from_utf8_lossy(&chunk[..n]));
                        }
                    }
                }
            }
        });

        let start = Instant::now();
        let ws_url = loop {
            let text = buffer.lock().unwrap().clone();
            if let Some(url) = extract_ws_url(&text) {
                break url;
            }
            if start.elapsed() > START_TIMEOUT {
                return Err(
                    "Browser did not report a DevTools endpoint; update Edge/Chrome to a \
                     recent version or point VIDEOCRAFT_BROWSER at another Chromium binary"
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        };

        let addr = ws_url.trim_start_matches("ws://");
        let host = addr.split('/').next().unwrap_or("127.0.0.1:0");
        let stream = TcpStream::connect(host)
            .map_err(|e| format!("Failed to connect to browser DevTools socket {host}: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| format!("Cannot set socket timeout: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(15)))
            .map_err(|e| format!("Cannot set socket timeout: {e}"))?;
        let (ws, _response) = tungstenite::client::client(ws_url.as_str(), stream)
            .map_err(|e| format!("Browser DevTools handshake failed: {e}"))?;

        let mut me = SceneRenderer {
            child,
            ws,
            session: String::new(),
            next_id: 0,
        };
        let target = me.call("Target.createTarget", json!({"url": "about:blank"}), None)?;
        let target_id = target["targetId"]
            .as_str()
            .ok_or("Browser did not return a target id")?
            .to_string();
        let attached = me.call(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
            None,
        )?;
        me.session = attached["sessionId"]
            .as_str()
            .ok_or("Browser did not return a session id")?
            .to_string();
        me.call("Page.enable", json!({}), Some(me.session.clone()))?;
        me.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
            Some(me.session.clone()),
        )?;
        Ok(me)
    }

    /// Send a CDP command and wait for its matching response. Events are skipped.
    fn call(&mut self, method: &str, params: Value, session: Option<String>) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws
            .send(Message::Text(msg.to_string()))
            .map_err(|e| format!("Browser socket send failed: {e}"))?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                return Err(format!("Browser timed out answering {method}"));
            }
            let message = match self.ws.read() {
                Ok(m) => m,
                Err(tungstenite::Error::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(e) => return Err(format!("Browser socket read failed: {e}")),
            };
            let Message::Text(text) = message else { continue };
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if v["id"].as_u64() == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("Browser error in {method}: {err}"));
                }
                return Ok(v["result"].clone());
            }
            // CDP events and responses for other sessions are ignored.
        }
    }

    fn evaluate(&mut self, expression: &str, await_promise: bool) -> Result<Value, String> {
        let result = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": await_promise}),
            Some(self.session.clone()),
        )?;
        if let Some(detail) = result.get("exceptionDetails") {
            return Err(format!("Scene script error: {detail}"));
        }
        Ok(result["result"]["value"].clone())
    }

    /// Navigate to the scene, wait for load, freeze CSS motion and enforce
    /// the `vcSeek` contract.
    pub fn load_scene(&mut self, url: &str) -> Result<(), String> {
        self.call(
            "Page.navigate",
            json!({"url": url}),
            Some(self.session.clone()),
        )?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                return Err("Scene page did not finish loading in time".into());
            }
            match self.evaluate("document.readyState", false) {
                Ok(Value::String(state)) if state == "complete" => break,
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let defines_seek = self.evaluate(FREEZE_AND_CHECK, false)?;
        if defines_seek != json!("true") {
            return Err(
                "Scene must define window.vcSeek(t): a deterministic function that repaints \
                 the page for time t in seconds. Run `videocraft skills web` for the contract \
                 and a template"
                    .into(),
            );
        }
        // Best effort: wait for fonts so the first frames are not fallback-rendered.
        let _ = self.evaluate("document.fonts.ready.then(function(){return 1})", true);
        Ok(())
    }

    /// Repaint the scene at time `t` and capture it as a PNG file.
    pub fn capture_frame(
        &mut self,
        t: f64,
        png_path: &Path,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.evaluate(&format!("window.vcSeek({t})"), false)?;
        let shot = self.call(
            "Page.captureScreenshot",
            json!({
                "format": "png",
                "clip": {"x": 0, "y": 0, "width": width, "height": height, "scale": 1}
            }),
            Some(self.session.clone()),
        )?;
        let data = shot["data"]
            .as_str()
            .ok_or("Browser returned no screenshot data")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("Screenshot is not valid base64: {e}"))?;
        std::fs::write(png_path, bytes).map_err(|e| format!("Cannot write frame: {e}"))
    }
}

impl Drop for SceneRenderer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn extract_ws_url(stderr: &str) -> Option<String> {
    let marker = "DevTools listening on ";
    let at = stderr.find(marker)? + marker.len();
    let rest = &stderr[at..];
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let url = &rest[..end];
    if url.starts_with("ws://") {
        Some(url.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_encoding() {
        let p = if cfg!(windows) {
            PathBuf::from(r"C:\media esc\intro escena.html")
        } else {
            PathBuf::from("/media esc/intro escena.html")
        };
        let url = to_file_url(&p);
        assert!(url.starts_with("file:///"));
        assert!(!url.contains(' '));
        assert!(!url.contains('\\'));
        assert!(url.ends_with("intro%20escena.html"));
    }

    #[test]
    fn file_url_handles_canonical_windows_paths() {
        // std::fs::canonicalize returns \\?\-prefixed paths on Windows.
        let p = PathBuf::from(r"\\?\C:\Users\test\escena.html");
        let url = to_file_url(&p);
        assert_eq!(url, "file:///C:/Users/test/escena.html");
        let unc = PathBuf::from(r"\\?\UNC\server\share\scene.html");
        assert_eq!(to_file_url(&unc), "file://server/share/scene.html");
    }

    #[test]
    fn ws_url_extraction() {
        let line = "[123:456:0728/120000.000:INFO:devtools_server.cc(71)] DevTools listening on ws://127.0.0.1:54321/devtools/browser/guid-here\n";
        assert_eq!(
            extract_ws_url(line).as_deref(),
            Some("ws://127.0.0.1:54321/devtools/browser/guid-here")
        );
        assert!(extract_ws_url("nothing here").is_none());
    }

    #[test]
    #[ignore = "manual diagnostic: launches a real browser"]
    fn diagnostic_scene_capture() {
        let browser = find_browser().expect("no browser found");
        let tmp = std::env::temp_dir().join(format!("vc-diag-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let scene_html = tmp.join("scene.html");
        std::fs::write(
            &scene_html,
            "<!doctype html><html><head><style>html,body{margin:0;height:100%;background:#000}</style></head><body id='b'><script>var D=2.0;window.vcSeek=function(t){var p=Math.min(Math.max(t/D,0),1);var v=Math.round(255*(0.2+0.6*p));document.getElementById('b').style.backgroundColor='rgb('+v+','+v+','+v+')';};</script></body></html>",
        )
        .unwrap();
        let url = to_file_url(&scene_html);
        eprintln!("URL: {url}");
        let mut sr = SceneRenderer::launch(&browser, 540, 960, &tmp.join("profile")).expect("launch");
        sr.load_scene(&url).expect("load");
        for f in 0..3u32 {
            let t = f as f64 / 15.0;
            sr.capture_frame(t, &tmp.join(format!("frame{f}.png")), 540, 960)
                .expect("capture");
        }
        for f in 0..3u32 {
            let p = tmp.join(format!("frame{f}.png"));
            eprintln!("{} exists={} size={}", p.display(), p.is_file(), std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn browser_detection_reports_something() {
        // Must never panic; on a machine with Edge/Chrome it should find one.
        if let Some(b) = find_browser() {
            assert!(!b.name.is_empty());
            assert!(b.path.is_file(), "found browser must exist: {}", b.path.display());
        }
    }
}
