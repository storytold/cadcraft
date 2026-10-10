//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Something that answers control-channel methods (`engine.execute`, `cmdline.input`,
/// `drawing.inspect`, `ui.render`…). See `cadcraft_ui_egui::control` for the full list.
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot` works).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
}

/// A running CADCraft app reached through its loopback control port.
pub struct Remote {
    addr: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
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
            self.reconnect().map_err(|e| (e, false))?;
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

/// Methods that only read state, so resending them after a lost reply cannot change the drawing twice.
fn is_read_only(method: &str) -> bool {
    matches!(method, "drawing.inspect" | "document.inspect" | "engine.commands" | "cmdline.state" | "ui.inspect" | "ui.render" | "ui.screenshot")
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
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
                ));
            }
        };
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
    fn has_ui(&self) -> bool {
        true
    }
    fn describe(&self) -> String {
        format!("connected to CADCraft at {}", self.addr)
    }
}

/// An in-process headless session (no window).
pub struct Headless {
    pub session: Session,
}

impl Default for Headless {
    fn default() -> Self {
        Headless { session: Session::new() }
    }
}

fn state(s: &Session) -> Value {
    let p = s.current_prompt();
    json!({
        "prompt": s.prompt_text(),
        "running": s.running.as_ref().map(|r| r.id.clone()),
        "keywords": p.as_ref().map(|p| p.keywords.clone()).unwrap_or_default(),
        "history": s.log.iter().rev().take(20).rev().cloned().collect::<Vec<_>>(),
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
                if str_p("key").is_some_and(|k| k.eq_ignore_ascii_case("escape") || k.eq_ignore_ascii_case("esc")) {
                    s.cancel();
                } else {
                    s.input(cadcraft_engine::Input::Enter).map_err(|e| e.to_string())?;
                }
                Ok(state(s))
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
                let list = cadcraft_render::build(&st.doc, &st.space, &cadcraft_render::Options::default());
                let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(true);
                let view = if fit {
                    cadcraft_render::raster::View::fit(&list.bounds, w, h, 0.05)
                } else {
                    let v = st.view();
                    cadcraft_render::raster::View { center: v.center, scale: f64::from(h) / v.height.max(1e-12), width: w, height: h }
                };
                let png =
                    cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default()).ok_or("render failed")?;
                if let Some(path) = str_p("path") {
                    std::fs::write(&path, &png).map_err(|e| format!("{path}: {e}"))?;
                }
                Ok(json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(&png), "width": w, "height": h}))
            }
            "app.open" => s.execute("open", &json!({"path": str_p("path")})).map_err(|e| e.to_string()),
            "app.save" => {
                let id = if p.get("path").is_some() { "saveas" } else { "qsave" };
                s.execute(id, &p).map_err(|e| e.to_string())
            }
            "ui.screenshot" => Err("no UI in headless mode; use render".into()),
            other => Err(format!("unknown method `{other}`")),
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
}
