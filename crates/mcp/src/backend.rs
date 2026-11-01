//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use cadcraft_engine::Session;
use cadcraft_engine::consent::Consent;
use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `cmdline.input`,
/// `drawing.inspect`, `ui.render`…). See `cadcraft_ui_egui::control` for the full list.
pub trait Backend {
    /// Invoke a control method and return its result or a descriptive error.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot` works).
    fn has_ui(&self) -> bool;
    /// Describe the backend connection for client-facing status.
    fn describe(&self) -> String;
    /// [`Backend::call`] allowing at most `consent` (see `cadcraft_engine::consent`): the engine
    /// refuses the file writes and closes it leaves out. By default the call runs on this thread,
    /// as [`Headless`] does; a backend that runs it elsewhere carries the consent there.
    fn call_with(&mut self, method: &str, params: Value, consent: Consent) -> Result<Value, String> {
        cadcraft_engine::consent::with(consent, || self.call(method, params))
    }
}

/// A running CADCraft app reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
    /// Open the app when nothing answers (see [`Remote::lazy`]).
    launch: bool,
    /// The per-session control token for this port, read from the owner-only token file on first
    /// use so requests can authenticate (CWE-306).
    token: Option<String>,
    /// Longest reply line accepted (`MAX_REPLY_LINE`; smaller in tests).
    max_reply: usize,
}

/// The port component of a `HOST:PORT` address, for locating the token file.
fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit(':').next().and_then(|p| p.parse::<u16>().ok())
}

/// How long a launched app has to open its control port.
const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// Longest reply line in bytes the client will buffer. It matches the control server's request
/// cap (256 MiB, room for whole drawings and images as base64), so a peer squatting on the port
/// can't make the client allocate without bound (#1925).
const MAX_REPLY_LINE: usize = 256 * 1024 * 1024;

/// Read one `\n`-terminated reply of at most `max` bytes (newline excluded). `Ok(0)` is end of
/// stream; a longer line is an `InvalidData` error and the rest of it stays unread.
fn read_reply_line(reader: &mut impl BufRead, reply: &mut String, max: usize) -> std::io::Result<usize> {
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.take(limit).read_line(reply)?;
    if n > max && !reply.ends_with('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("reply from the control port is longer than {max} bytes; is that CADCraft's control server?"),
        ));
    }
    Ok(n)
}

impl Remote {
    /// Connect to a running control server; its session token is read lazily when the first request is sent.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1, launch: false, token: None, max_reply: MAX_REPLY_LINE };
        r.reconnect()?;
        Ok(r)
    }
    /// The app at `addr`, reached at the first call instead of now, so an MCP client can start
    /// the server before the app is open. With `launch`, a call that finds nothing at `addr`
    /// opens the app with that control port first (macOS) and waits for it.
    pub fn lazy(addr: &str, launch: bool) -> Self {
        Self { addr: addr.to_string(), conn: None, next_id: 1, launch, token: None, max_reply: MAX_REPLY_LINE }
    }

    /// The per-session token for this port, read (and cached) from the owner-only token file. A
    /// missing file is not fatal here: the request is sent without a token and the server replies
    /// with an authentication error the user can act on.
    fn token(&mut self) -> Option<&str> {
        if self.token.is_none()
            && let Some(port) = port_of(&self.addr)
            && let Ok(t) = crate::control_token::read(port)
        {
            self.token = Some(t);
        }
        self.token.as_deref()
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
            // write_all can fail after partial delivery. The server may have received the full
            // JSON before the newline write failed, so a mutation must not be retried blindly.
            return Err((e, true));
        }
        let mut reply = String::new();
        match read_reply_line(reader, &mut reply, self.max_reply) {
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

impl Remote {
    /// Put the current token (if any) on `req`, replacing the one it carries.
    fn set_token(&mut self, req: &mut Value) {
        let token = self.token().map(str::to_string);
        if let Some(obj) = req.as_object_mut() {
            obj.remove("token");
            if let Some(token) = token {
                obj.insert("token".into(), Value::String(token));
            }
        }
    }
    /// Send `req` and parse the reply, refreshing the token before a safe retry after a transport
    /// error. A mutation that may have reached the server is never resent automatically.
    fn exchange(&mut self, method: &str, req: &mut Value) -> Result<Value, String> {
        let reply = match self.roundtrip(&req.to_string()) {
            Ok(r) => r,
            Err((e, sent)) => {
                self.token = None;
                self.conn = None;
                // A mutation whose reply was lost must never be sent twice.
                if sent && !is_read_only(method) {
                    return Err(format!(
                        "CADCraft at {}: {e}; the reply was lost, so `{method}` may or may not have been applied. Inspect the drawing first",
                        self.addr
                    ));
                }
                // A restarted server may have rotated its token before dropping the old socket.
                self.set_token(req);
                self.roundtrip(&req.to_string()).map_err(|(e, _)| {
                    self.token = None;
                    self.conn = None;
                    format!("CADCraft at {}: {e}", self.addr)
                })?
            }
        };
        serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))
    }
}

/// An `unauthorized` reply (the app also closes the connection).
fn unauthorized(v: &Value) -> bool {
    v.get("error").and_then(Value::as_str).is_some_and(|e| e.starts_with("unauthorized"))
}

impl Backend for Remote {
    /// The app on the other end runs the call, where a consent can't follow it: anything short of
    /// everything is refused.
    fn call_with(&mut self, method: &str, params: Value, consent: Consent) -> Result<Value, String> {
        if consent != (Consent { write_files: true, close_drawings: true }) {
            return Err("a call with limited consent can't run in another CADCraft".into());
        }
        self.call(method, params)
    }
    /// Send a control request with the token when available. A refused token is read again and the
    /// request resent once, only when the app says it did not run it.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = json!({"id": id, "method": method, "params": params});
        self.set_token(&mut req);
        let mut v = self.exchange(method, &mut req)?;
        // The token is read before `--launch` opens the app, which then writes a new one (#2087); the
        // file can also be left over from an earlier session. Once, so a wrong token can't loop.
        if unauthorized(&v) && v.get("state").and_then(Value::as_str) == Some("not-run") {
            self.token = None;
            self.conn = None;
            self.set_token(&mut req);
            v = self.exchange(method, &mut req)?;
        }
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            let e = v.get("error").and_then(Value::as_str).unwrap_or("error");
            if unauthorized(&v) {
                self.token = None;
                self.conn = None;
            }
            // A timeout says whether the request can still have run (#345).
            Err(match v.get("state").and_then(Value::as_str) {
                Some("not-run") => format!("{e}: CADCraft did not start `{method}` and won't; it is safe to send again"),
                Some("may-have-run") => format!("{e}: `{method}` may or may not have been applied. Inspect the drawing first"),
                _ => e.to_string(),
            })
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
    fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(&id, &params).map_err(|e| e.to_string())
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
                s.script(&str_p("text").unwrap_or_default()).map_err(|e| e.to_string())?;
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
            "drawing.inspect" | "document.inspect" => s.execute("drawing.inspect", &p).map_err(|e| e.to_string()),
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
                    cadcraft_engine::consent::check_write(&path).map_err(|e| e.to_string())?;
                    std::fs::write(&path, &png).map_err(|e| format!("{path}: {e}"))?;
                }
                Ok(json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(&png), "width": w, "height": h}))
            }
            "app.open" => {
                let path = str_p("path").ok_or("missing path")?;
                s.execute("open", &json!({ "path": path })).map_err(|e| e.to_string())
            }
            "app.save" => {
                let id = if p.get("path").is_some() { "saveas" } else { "qsave" };
                s.execute(id, &p).map_err(|e| e.to_string())
            }
            "ui.screenshot" => Err("no UI in headless mode; use render".into()),
            "control.methods" => Ok(HEADLESS_METHODS
                .iter()
                .map(|(method, params, description)| json!({"method": method, "params": params, "description": description}))
                .collect()),
            "ping" => Ok(json!("pong")),
            other => Err(format!("unknown method `{other}`; `control.methods` lists them (docs/control-protocol.md)")),
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
    use std::sync::Arc;

    /// A peer squatting on the control port can't make the client buffer an endless reply line (#1925).
    #[test]
    fn an_overlong_reply_is_refused_without_buffering_it() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
            // No newline: the reply never ends. A bounded client gives up after the limit.
            let _ = conn.write_all(&[b'x'; 4096]);
            let _ = conn.flush();
            std::thread::sleep(Duration::from_millis(500));
        });
        let mut remote = Remote::connect(&addr).unwrap();
        remote.max_reply = 100;
        let err = remote.roundtrip("{}").unwrap_err().0;
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{err}");
        assert!(err.to_string().contains("longer than 100 bytes"), "{err}");
        assert!(remote.conn.is_none());
        server.join().unwrap();
    }

    #[test]
    fn replies_up_to_the_limit_are_read() {
        let mut input: &[u8] = b"0123456789\nrest\n";
        let mut reply = String::new();
        assert_eq!(read_reply_line(&mut input, &mut reply, 10).unwrap(), 11);
        assert_eq!(reply, "0123456789\n");
    }

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
        remote.token = Some("stale-token".into());
        let err = remote.call("engine.execute", json!({"command": "circle", "params": {}})).unwrap_err();
        assert!(err.contains("may or may not have been applied"), "{err}");
        assert_eq!(server.join().unwrap(), 1);
        assert!(remote.token.is_none());
        assert!(remote.conn.is_none());
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
    /// Retrying a read after a lost reply reloads the replacement server session token.
    #[test]
    fn safe_retry_rereads_a_rotated_token_after_a_lost_reply() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let old = crate::control_token::issue(addr.port()).unwrap();
        let server = std::thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            let mut reader = BufReader::new(conn);
            reader.read_line(&mut line).unwrap();
            let first: Value = serde_json::from_str(&line).unwrap();
            assert!(first["token"].as_str() == Some(old.as_str()));
            let fresh = crate::control_token::issue(addr.port()).unwrap();
            drop(reader);
            let (mut conn, _) = listener.accept().unwrap();
            line.clear();
            BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
            let retry: Value = serde_json::from_str(&line).unwrap();
            assert!(retry["token"].as_str() == Some(fresh.as_str()));
            writeln!(conn, "{}", json!({"ok": true, "result": "pong"})).unwrap();
            crate::control_token::revoke(addr.port());
        });
        let mut remote = Remote::connect(&addr.to_string()).unwrap();
        assert_eq!(remote.call("ping", json!({})).unwrap(), json!("pong"));
        server.join().unwrap();
    }
    /// Answer one request per connection like the app's control server: `reply` sees the request
    /// and the connection number and says what to send back; the server closes after each reply.
    /// Returns how many requests arrived, waiting a short while for each next connection.
    fn fake_app(listener: TcpListener, reply: impl Fn(&Value, usize) -> Value + Send + 'static) -> std::thread::JoinHandle<usize> {
        std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let mut seen = 0;
            loop {
                let start = std::time::Instant::now();
                let conn = loop {
                    match listener.accept() {
                        Ok((conn, _)) => break Some(conn),
                        Err(_) if start.elapsed() < Duration::from_millis(500) => std::thread::sleep(Duration::from_millis(10)),
                        Err(_) => break None,
                    }
                };
                let Some(conn) = conn else { return seen };
                conn.set_nonblocking(false).unwrap();
                let mut line = String::new();
                BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
                let mut out = conn;
                writeln!(out, "{}", reply(&serde_json::from_str(&line).unwrap(), seen)).unwrap();
                seen += 1;
            }
        })
    }

    /// `--launch` reads the token before it opens the app, so the first request carries the token
    /// of an earlier session (or none); the app refuses it without running it, and the call is
    /// resent once with the token the app has just written (#2087).
    #[test]
    fn unauthorized_not_run_is_resent_once_with_the_new_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let old = crate::control_token::issue(addr.port()).unwrap();
        let fresh = Arc::new(std::sync::Mutex::new(String::new()));
        let f = fresh.clone();
        let server = fake_app(listener, move |req, n| {
            if n == 0 {
                assert_eq!(req["token"].as_str(), Some(old.as_str()));
                // The app starts and writes its own session's token.
                *f.lock().unwrap() = crate::control_token::issue(addr.port()).unwrap();
                json!({"ok": false, "error": "unauthorized: missing or invalid control token", "state": "not-run"})
            } else if req["token"].as_str() == Some(f.lock().unwrap().as_str()) {
                json!({"ok": true, "result": "drawing"})
            } else {
                json!({"ok": false, "error": "unauthorized: missing or invalid control token", "state": "not-run"})
            }
        });
        let mut remote = Remote::lazy(&addr.to_string(), false);
        let first = remote.call("engine.execute", json!({"command": "circle", "params": {}}));
        let requests = server.join().unwrap();
        crate::control_token::revoke(addr.port());
        assert_eq!(first, Ok(json!("drawing")));
        assert_eq!(requests, 2);
        assert_eq!(remote.token.as_deref(), Some(fresh.lock().unwrap().as_str()));
    }

    /// A token the app keeps refusing is resent once, not in a loop, and a refusal that doesn't
    /// say the request never ran is not resent at all.
    #[test]
    fn unauthorized_is_resent_at_most_once_and_only_when_not_run() {
        for (state, connections) in [(Some("not-run"), 2), (Some("may-have-run"), 1), (None, 1)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let server = fake_app(listener, move |_, _| {
                let mut r = json!({"ok": false, "error": "unauthorized: missing or invalid control token"});
                if let (Some(o), Some(state)) = (r.as_object_mut(), state) {
                    o.insert("state".into(), json!(state));
                }
                r
            });
            let mut remote = Remote::lazy(&addr, false);
            let err = remote.call("engine.execute", json!({"command": "circle", "params": {}})).unwrap_err();
            assert!(err.starts_with("unauthorized"), "{err}");
            assert_eq!(server.join().unwrap(), connections, "{state:?}");
            assert!(remote.token.is_none() && remote.conn.is_none());
        }
    }
    /// A failed mutation write reports uncertain delivery and does not open a retry connection.
    #[test]
    fn failed_mutation_write_is_not_resent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut remote = Remote::connect(&addr).unwrap();
        remote.conn.as_ref().unwrap().1.shutdown(std::net::Shutdown::Write).unwrap();
        let server = std::thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            assert_eq!(BufReader::new(conn).read_line(&mut line).unwrap(), 0);
            listener.set_nonblocking(true).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            listener.accept().is_ok()
        });
        let error = remote.call("engine.execute", json!({"command": "circle", "params": {}})).unwrap_err();
        assert!(error.contains("may or may not have been applied"));
        assert!(!server.join().unwrap(), "a failed mutation write was retried");
        assert!(remote.token.is_none());
    }
}
