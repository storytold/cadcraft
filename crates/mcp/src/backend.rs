//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `cmdline.input`,
/// `drawing.inspect`, `ui.render`…). See `cadcraft_ui_egui::control` for the full list.
pub trait Backend {
    /// Call `method`; `Err` is the error text.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.call_detailed(method, params).map_err(|e| e.message)
    }
    /// Like [`Backend::call`], with the error as data when the engine reported it.
    fn call_detailed(&mut self, method: &str, params: Value) -> Result<Value, CallError>;
    /// True when a real UI is attached (`ui.screenshot` works).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
}

/// A failed call: its text and, when the engine refused or failed the call, the error as data:
/// `{code, command, param?, message}` (`cadcraft_engine::ErrorInfo`, codes in `docs/mcp.md`).
#[derive(Clone, Debug, PartialEq)]
pub struct CallError {
    pub message: String,
    pub data: Option<Value>,
}

impl From<String> for CallError {
    fn from(message: String) -> Self {
        CallError { message, data: None }
    }
}

impl From<&str> for CallError {
    fn from(message: &str) -> Self {
        CallError { message: message.to_string(), data: None }
    }
}

/// An engine error from running `command`, with its data.
fn engine_error(e: &cadcraft_engine::EngineError, command: &str) -> CallError {
    CallError { message: e.to_string(), data: serde_json::to_value(e.info(command)).ok() }
}

/// A running CADCraft app reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    /// Open the app when nothing answers (see [`Remote::lazy`]).
    launch: bool,
}

/// How long a launched app has to open its control port.
const LAUNCH_WAIT: Duration = Duration::from_secs(30);

impl Remote {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1, launch: false };
        r.reconnect()?;
        Ok(r)
    }
    /// The app at `addr`, reached at the first call instead of now, so an MCP client can start
    /// the server before the app is open. With `launch`, a call that finds nothing at `addr`
    /// opens the app with that control port first (macOS) and waits for it.
    pub fn lazy(addr: &str, launch: bool) -> Self {
        Self { addr: addr.to_string(), conn: None, next_id: 1, launch }
    }
    /// Connect, opening the app first when asked to and nothing answers.
    fn reach(&mut self) -> std::io::Result<()> {
        let first = match self.reconnect() {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        if !self.launch || !matches!(first.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut) {
            return Err(not_open(&self.addr, first));
        }
        launch_app(&self.addr)?;
        let start = std::time::Instant::now();
        while start.elapsed() < LAUNCH_WAIT {
            std::thread::sleep(Duration::from_millis(500));
            if self.reconnect().is_ok() {
                return Ok(());
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "CADCraft was opened but its control port {} did not answer; if it was already open without the port, quit it and try again",
                self.addr
            ),
        ))
    }
    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(Duration::from_secs(90))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }
    /// On failure the flag says whether the request may already have reached the server.
    fn roundtrip(&mut self, line: &str) -> Result<String, (std::io::Error, bool)> {
        if self.conn.is_none() {
            self.reach().map_err(|e| (e, false))?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err((std::io::Error::other("not connected"), false)) };
        let sent = writer.write_all(line.as_bytes()).and_then(|()| writer.write_all(b"\n")).and_then(|()| writer.flush());
        if let Err(e) = sent {
            self.conn = None;
            return Err((e, false));
        }
        let mut reply = String::new();
        match reader.read_line(&mut reply) {
            Ok(0) => {
                self.conn = None;
                Err((std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"), true))
            }
            Ok(_) => Ok(reply),
            Err(e) => {
                self.conn = None;
                Err((e, true))
            }
        }
    }
}

/// The `window` of a `ui.render` request: two finite corners [[x0, y0], [x1, y1]] around an area
/// (`None` when absent or unusable).
fn render_window(p: &Value) -> Option<cadcraft_engine::geom::Bounds2> {
    let a = p.get("window")?.as_array().filter(|a| a.len() == 2)?;
    let pt = |v: &Value| -> Option<cadcraft_engine::geom::Vec2> {
        let c = v.as_array().filter(|c| c.len() == 2)?;
        let (x, y) = (c[0].as_f64()?, c[1].as_f64()?);
        (x.is_finite() && y.is_finite()).then(|| cadcraft_engine::geom::Vec2::new(x, y))
    };
    let b = cadcraft_engine::geom::Bounds2::new(pt(&a[0])?, pt(&a[1])?);
    (b.width() > 0.0 && b.height() > 0.0).then_some(b)
}

/// The error for an app that doesn't answer: what to do about it.
fn not_open(addr: &str, e: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        e.kind(),
        format!(
            "CADCraft is not answering at {addr} ({e}). Open CADCraft with its control port (`--control PORT` or CADCRAFT_CONTROL_PORT), then try again"
        ),
    )
}

/// Open the app with the control port of `addr` (macOS: `open -a CADCraft`).
fn launch_app(addr: &str) -> std::io::Result<()> {
    let port = addr.rsplit(':').next().filter(|p| p.parse::<u16>().is_ok()).ok_or_else(|| std::io::Error::other(format!("no port in {addr}")))?;
    if cfg!(target_os = "macos") {
        let ok = std::process::Command::new("open").args(["-a", "CADCraft", "--args", "--control", port]).status()?.success();
        if ok { Ok(()) } else { Err(std::io::Error::other("could not open CADCraft (is it in Applications?)")) }
    } else {
        Err(std::io::Error::other(format!("CADCraft is not running; start it with `cadcraft --control {port}`")))
    }
}

/// Methods that only read state, so resending them after a lost reply cannot change the drawing twice.
fn is_read_only(method: &str) -> bool {
    matches!(
        method,
        "drawing.inspect"
            | "document.inspect"
            | "engine.commands"
            | "cmdline.state"
            | "control.methods"
            | "ping"
            | "ui.inspect"
            | "ui.render"
            | "ui.screenshot"
    )
}

impl Backend for Remote {
    fn call_detailed(&mut self, method: &str, params: Value) -> Result<Value, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            // Resend only when the request cannot have been applied. After a sent mutation with no reply the outcome is unknown, and
            // resending could apply it twice (for example two circles from one call), so report that instead.
            Err((_, sent)) if !sent || is_read_only(method) => self.roundtrip(&line).map_err(|(e, _)| format!("CADCraft at {}: {e}", self.addr))?,
            Err((e, _)) => {
                return Err(format!(
                    "CADCraft at {}: {e}; the reply was lost, so `{method}` may or may not have been applied. Inspect the drawing first",
                    self.addr
                )
                .into());
            }
        };
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            let e = v.get("error").and_then(Value::as_str).unwrap_or("error");
            // A timeout says whether the request can still have run (#345).
            let message = match v.get("state").and_then(Value::as_str) {
                Some("not-run") => format!("{e}: CADCraft did not start `{method}` and won't; it is safe to send again"),
                Some("may-have-run") => format!("{e}: `{method}` may or may not have been applied. Inspect the drawing first"),
                _ => e.to_string(),
            };
            // The app's `data` ({code, command, param?}) plus the message: the same object as headless.
            let data = v.get("data").and_then(Value::as_object).map(|d| {
                let mut d = d.clone();
                d.insert("message".into(), json!(e));
                Value::Object(d)
            });
            Err(CallError { message, data })
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        format!("connected to CADCraft at {}", self.addr)
    }
}

/// What the headless `control.methods` replies: the methods [`Headless`] answers, a subset of the
/// app's (`docs/control-protocol.md`).
const HEADLESS_METHODS: &[(&str, &str, &str)] = &[
    ("cmdline.input", "{text}", "Type a line at the command line exactly like a user (commands, prompts, keywords; empty = Enter)."),
    ("cmdline.script", "{text}", "Run a multi-line script, like a .scr file."),
    ("cmdline.key", "{key: \"enter\"|\"escape\"}", "Press Enter or Escape at the command line."),
    ("cmdline.state", "", "Current prompt, keywords, accepted input kinds and history tail."),
    ("engine.execute", "{command, params}", "Run any command with JSON parameters. Never opens a dialog."),
    ("engine.commands", "", "Every command: id, label, menu path, shortcut, aliases, params doc, enabled."),
    ("drawing.inspect", "{entities?, limit?}", "Drawing summary and entities with handles and geometry."),
    ("ui.render", "{path?, width?, height?, fit?}", "Render the drawing; replies pngBase64."),
    ("app.open", "{path}", "Open a drawing."),
    ("app.save", "{path?}", "Save the drawing (Save As with a path)."),
    ("control.methods", "", "This list."),
    ("ping", "", "Replies \"pong\"."),
];

/// An in-process headless session (no window).
pub struct Headless {
    pub session: Session,
}

impl Default for Headless {
    fn default() -> Self {
        Headless { session: Session::new() }
    }
}

/// The command-line state, with the same fields as the app's (`cadcraft_ui_egui::control`). There
/// is no command-line widget here: nothing is typed (`buffer`) and the history isn't expanded.
fn state(s: &Session) -> Value {
    let p = s.current_prompt();
    json!({
        "prompt": s.prompt_text(),
        "running": s.running.as_ref().map(|r| r.id.clone()),
        "keywords": p.as_ref().map(|p| p.keywords.clone()).unwrap_or_default(),
        "accept": p.as_ref().map(|p| p.accept),
        "buffer": "",
        "history": s.log.iter().rev().take(20).rev().cloned().collect::<Vec<_>>(),
        "historyExpanded": false,
    })
}

impl Backend for Headless {
    fn call_detailed(&mut self, method: &str, p: Value) -> Result<Value, CallError> {
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(&id, &params).map_err(|e| engine_error(&e, &id))
            }
            "cmdline.input" => {
                let before = s.log.len();
                let r = s.cmdline(&str_p("text").unwrap_or_default());
                // Like the app: a refused line is echoed to the history (and `output`), and also
                // reported in `error`.
                if let Err(e) = &r {
                    s.echo(e.to_string());
                }
                let out: Vec<String> = s.log.iter().skip(before).cloned().collect();
                let mut st = state(s);
                if let Some(o) = st.as_object_mut() {
                    o.insert("output".into(), json!(out));
                    if let Err(e) = r {
                        o.insert("error".into(), json!(e.to_string()));
                    }
                }
                Ok(st)
            }
            "cmdline.script" => {
                let before = s.log.len();
                s.script(&str_p("text").unwrap_or_default()).map_err(|e| engine_error(&e, "script"))?;
                let out: Vec<String> = s.log.iter().skip(before).cloned().collect();
                Ok(json!({"output": out, "state": state(s)}))
            }
            "cmdline.key" => {
                let before = s.log.len();
                let r = if str_p("key").is_some_and(|k| k.eq_ignore_ascii_case("escape") || k.eq_ignore_ascii_case("esc")) {
                    s.cancel();
                    Ok(())
                } else {
                    s.input(cadcraft_engine::Input::Enter)
                };
                // Like the app: a refused Enter is echoed to the history (and `output`), and also
                // reported in `error`; the call itself succeeds.
                if let Err(e) = &r {
                    s.echo(e.to_string());
                }
                let out: Vec<String> = s.log.iter().skip(before).cloned().collect();
                let mut st = state(s);
                if let Some(o) = st.as_object_mut() {
                    o.insert("output".into(), json!(out));
                    if let Err(e) = r {
                        o.insert("error".into(), json!(e.to_string()));
                    }
                }
                Ok(st)
            }
            "cmdline.state" => Ok(state(s)),
            "engine.commands" => {
                Ok(Value::Array(cadcraft_engine::command_specs().iter().map(|c| serde_json::to_value(c.info(s)).unwrap_or_default()).collect()))
            }
            "drawing.inspect" | "document.inspect" => s.execute("drawing.inspect", &p).map_err(|e| engine_error(&e, "drawing.inspect")),
            "ui.render" => {
                let w = p.get("width").and_then(Value::as_f64).unwrap_or(1600.0).clamp(16.0, 8192.0) as u32;
                let h = p.get("height").and_then(Value::as_f64).unwrap_or(1000.0).clamp(16.0, 8192.0) as u32;
                let st = s.state().map_err(|e| e.to_string())?;
                let window = render_window(&p);
                let opts = cadcraft_render::Options { view: window, transparency: s.settings.transparency_display, ..Default::default() };
                let list = cadcraft_render::build(&st.doc, &st.space, &opts);
                let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(true);
                let side = |k: &str| p.get(k).and_then(Value::as_f64).map(|x| x as u32);
                let view = if let Some(win) = &window {
                    cadcraft_render::raster::View::of_window(win, side("width"), side("height"))
                } else if fit {
                    cadcraft_render::raster::View::fit(&list.bounds, w, h, 0.05)
                } else {
                    let v = st.view();
                    cadcraft_render::raster::View { center: v.center, scale: f64::from(h) / v.height.max(1e-12), width: w, height: h }
                };
                let (w, h) = (view.width, view.height);
                let png =
                    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default()).ok_or("render failed")?;
                if let Some(path) = str_p("path") {
                    std::fs::write(&path, &png).map_err(|e| CallError::from(format!("{path}: {e}")))?;
                }
                Ok(json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(&png), "width": w, "height": h}))
            }
            "app.open" => {
                let path = str_p("path").ok_or("missing path")?;
                s.execute("open", &json!({ "path": path })).map_err(|e| engine_error(&e, "open"))
            }
            "app.save" => {
                let id = if p.get("path").is_some() { "saveas" } else { "qsave" };
                s.execute(id, &p).map_err(|e| engine_error(&e, id))
            }
            "ui.screenshot" => Err("no UI in headless mode; use render".into()),
            "control.methods" => Ok(HEADLESS_METHODS
                .iter()
                .map(|(method, params, description)| json!({"method": method, "params": params, "description": description}))
                .collect()),
            "ping" => Ok(json!("pong")),
            other => Err(format!("unknown method `{other}`; `control.methods` lists them (docs/control-protocol.md)").into()),
        }
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless CADCraft session".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A server that reads one request per connection and closes without replying must not see the request twice.
    #[test]
    fn lost_reply_to_a_mutation_is_not_resent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let mut requests = 0;
            listener.set_nonblocking(false).unwrap();
            let (conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(conn).read_line(&mut line).unwrap();
            requests += 1;
            // Anything arriving on a second connection would be a resend.
            listener.set_nonblocking(true).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            requests += usize::from(listener.accept().is_ok());
            requests
        });
        let mut remote = Remote::connect(&addr).unwrap();
        let err = remote.call("engine.execute", json!({"command": "circle", "params": {}})).unwrap_err();
        assert!(err.contains("may or may not have been applied"), "{err}");
        assert_eq!(server.join().unwrap(), 1);
    }

    /// The app's timeout reply says whether the request can still run; the error passes that on (#345).
    #[test]
    fn timeout_reply_says_whether_a_retry_is_safe() {
        for (state, want) in [("not-run", "safe to send again"), ("may-have-run", "Inspect the drawing first")] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let server = std::thread::spawn(move || {
                let (conn, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
                let mut out = conn;
                writeln!(out, "{}", json!({"id": 1, "ok": false, "error": "timeout", "state": state})).unwrap();
            });
            let mut remote = Remote::connect(&addr).unwrap();
            let err = remote.call("engine.execute", json!({"command": "circle", "params": {}})).unwrap_err();
            assert!(err.starts_with("timeout") && err.contains(want), "{err}");
            server.join().unwrap();
        }
    }

    /// The app's error `data` reaches the MCP side with the message added, as headless gives it.
    #[test]
    fn remote_error_data_is_passed_on() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let mut line = String::new();
                let (conn, _) = listener.accept().unwrap();
                let mut read = BufReader::new(conn.try_clone().unwrap());
                read.read_line(&mut line).unwrap();
                let mut out = conn;
                let reply = if line.contains("frob") {
                    json!({"id": 1, "ok": false, "error": "unknown command `frob`", "data": {"code": "unknown_command", "command": "frob"}})
                } else {
                    json!({"id": 2, "ok": false, "error": "missing x/y"})
                };
                writeln!(out, "{reply}").unwrap();
            }
        });
        let mut remote = Remote::connect(&addr).unwrap();
        let e = remote.call_detailed("engine.execute", json!({"command": "frob"})).unwrap_err();
        assert_eq!(e.message, "unknown command `frob`");
        assert_eq!(e.data, Some(json!({"code": "unknown_command", "command": "frob", "message": "unknown command `frob`"})));
        remote.conn = None;
        let e = remote.call_detailed("ui.pointer", json!({})).unwrap_err();
        assert_eq!(e, CallError::from("missing x/y"));
        server.join().unwrap();
    }
}
