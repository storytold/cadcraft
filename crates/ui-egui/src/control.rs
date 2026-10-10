//! Programmatic control of the running app (agents, tests, MCP).
//!
//! Methods (JSON lines over the host's transport, `{"id", "method", "params"}`):
//! - `engine.execute {command, params}`: run any command with JSON parameters (no dialogs)
//! - `cmdline.input {text}`: type a line at the command line (exactly like a user, incl. prompts)
//! - `cmdline.script {text}`: run a multi-line script (spaces separate inputs, as in .scr files)
//! - `cmdline.key {key}`: Enter / Escape at the command line
//! - `cmdline.state`: current prompt, keywords, history tail
//! - `engine.commands`: every command with menu path, aliases, params and enablement
//! - `drawing.inspect {entities?, limit?}`: drawing summary and entities (verifies agent work)
//! - `ui.inspect`: UI state, canvas rect, view, performance
//! - `ui.menu.list`: the menu tree; `ui.menu.invoke {command}`: like choosing a menu item
//! - `ui.pointer {x, y, space?: "world"|"screen", button?: left|right, action?: click|move}`
//! - `ui.click {x, y}`, `ui.move {x, y}`: real egui pointer input in screen points
//! - `ui.key {key, cmd?, shift?, alt?}`, `ui.text {text}`: synthetic keyboard input
//! - `ui.set {...UiState fields}`, `ui.resize {width, height}`
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path, width?, height?}`: headless
//!   render of the drawing (no window needed)
//! - `app.open {path}`, `app.save {path?}`, `app.quit`

use std::sync::mpsc::Sender;

use cadcraft_engine::Input;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::CadApp;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx }, rx)
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

pub fn all_commands(app: &CadApp) -> Value {
    let mut v: Vec<Value> = cadcraft_engine::command_specs().iter().map(|c| serde_json::to_value(c.info(&app.session)).unwrap_or_default()).collect();
    for (id, label, menu, sc) in crate::menus::UI_COMMANDS {
        v.push(json!({"id": id, "label": label, "menu": menu, "shortcut": sc, "enabled": true, "ui": true}));
    }
    Value::Array(v)
}

fn menu_json(e: &crate::menus::Entry) -> Value {
    match e {
        crate::menus::Entry::Item { label, id, shortcut, enabled } => {
            json!({"label": label, "command": id, "shortcut": shortcut, "enabled": enabled})
        }
        crate::menus::Entry::Sub { label, children } => json!({"label": label, "children": children.iter().map(menu_json).collect::<Vec<_>>()}),
    }
}

pub fn cmdline_state(app: &CadApp) -> Value {
    let p = app.session.current_prompt();
    json!({
        "prompt": app.session.prompt_text(),
        "running": app.session.running.as_ref().map(|r| r.id.clone()),
        "keywords": p.as_ref().map(|p| p.keywords.clone()).unwrap_or_default(),
        "accept": p.as_ref().map(|p| p.accept),
        "buffer": app.cmd.buffer,
        "history": app.session.log.iter().rev().take(20).rev().cloned().collect::<Vec<_>>(),
    })
}

pub fn inspect(app: &CadApp, ctx: &egui::Context) -> Value {
    let r = ctx.content_rect();
    let view = app.session.state().map(|s| s.view()).ok();
    json!({
        "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
        "canvasRect": app.canvas.rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "window": [r.width(), r.height()],
        "view": view.map(|v| json!({"center": [v.center.x, v.center.y], "height": v.height})),
        "cursor": app.canvas.cursor.map(|c| [c.x, c.y]),
        "snap": app.canvas.snap.map(|s| json!({"point": [s.point.x, s.point.y], "mode": s.name})),
        "session": app.session.summary(),
        "closePrompt": app.close_prompt(),
        "perf": {"frameMs": app.frame_ms, "buildMs": app.canvas.build_ms, "drawMs": app.canvas.draw_ms, "meshMs": app.canvas.mesh_ms, "renderer": if app.canvas.gpu.is_some() { "gpu" } else { "cpu" }, "prims": app.canvas.list.as_ref().map(|l| l.prims.len())},
    })
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        _ => None,
    })
}

pub fn handle(app: &mut CadApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let p = &req.params;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let f = |k: &str| p.get(k).and_then(Value::as_f64);
    match req.method.as_str() {
        "engine.execute" | "command" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().unwrap_or(json!({}));
            let params = if params.is_null() { json!({}) } else { params };
            wrap(app.run(id, params))
        }
        "ui.menu.invoke" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            crate::menus::activate(app, id);
            ok(cmdline_state(app))
        }
        "cmdline.input" => {
            let text = s("text").unwrap_or("");
            let before = app.session.log.len();
            app.cmdline(text);
            let out: Vec<String> = app.session.log.iter().skip(before).cloned().collect();
            let mut st = cmdline_state(app);
            if let Some(o) = st.as_object_mut() {
                o.insert("output".into(), json!(out));
            }
            ok(st)
        }
        "cmdline.script" => {
            let before = app.session.log.len();
            let r = app.session.script(s("text").unwrap_or("")).map_err(|e| e.to_string());
            let out: Vec<String> = app.session.log.iter().skip(before).cloned().collect();
            match r {
                Ok(()) => ok(json!({"output": out, "state": cmdline_state(app)})),
                Err(e) => err(e),
            }
        }
        "cmdline.key" => {
            match s("key").unwrap_or("enter").to_ascii_lowercase().as_str() {
                "escape" | "esc" => app.session.cancel(),
                _ => {
                    let _ = app.session.input(Input::Enter);
                }
            }
            ok(cmdline_state(app))
        }
        "cmdline.state" => ok(cmdline_state(app)),
        "engine.commands" => ok(all_commands(app)),
        "drawing.inspect" | "document.inspect" => wrap(app.session.execute("drawing.inspect", p).map_err(|e| e.to_string())),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.menu.list" => ok(Value::Array(
            crate::menus::tree(app).iter().map(|(m, es)| json!({"label": m, "children": es.iter().map(menu_json).collect::<Vec<_>>()})).collect(),
        )),
        "ui.pointer" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            let world = s("space").unwrap_or("world") == "world";
            let action = s("action").unwrap_or("click");
            let wp =
                if world { Vec2::new(x, y) } else { app.canvas.xf.map(|xf| xf.to_world(egui::pos2(x as f32, y as f32))).unwrap_or(Vec2::new(x, y)) };
            app.session.cursor = wp;
            app.canvas.cursor = Some(wp);
            if action == "move" {
                if let Some(sp) = app.canvas.xf.map(|xf| xf.to_screen(wp)) {
                    app.synthetic.push(egui::Event::PointerMoved(sp));
                }
                return ok(json!({"world": [wp.x, wp.y]}));
            }
            let r = match s("button").unwrap_or("left") {
                "right" => app.session.input(Input::Enter),
                _ if app.session.running.is_some() => app.session.input(Input::Point(wp)),
                _ => app.session.idle_click(wp, p.get("shift").and_then(Value::as_bool).unwrap_or(false)),
            };
            match r {
                Ok(()) => ok(json!({"world": [wp.x, wp.y], "state": cmdline_state(app)})),
                Err(e) => err(e),
            }
        }
        "ui.move" | "ui.click" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            let pos = egui::pos2(x as f32, y as f32);
            app.synthetic.push(egui::Event::PointerMoved(pos));
            if req.method == "ui.click" {
                let button = match s("button") {
                    Some("right") => egui::PointerButton::Secondary,
                    Some("middle") => egui::PointerButton::Middle,
                    _ => egui::PointerButton::Primary,
                };
                let modifiers = egui::Modifiers { shift: p.get("shift").and_then(Value::as_bool).unwrap_or(false), ..Default::default() };
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
            }
            ok(Value::Null)
        }
        "ui.key" => {
            let Some(key) = s("key").and_then(key_from) else { return err("unknown key") };
            let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
            let modifiers = egui::Modifiers {
                alt: b("alt"),
                shift: b("shift"),
                command: b("cmd"),
                mac_cmd: b("cmd") && cfg!(target_os = "macos"),
                ctrl: b("ctrl") || (b("cmd") && !cfg!(target_os = "macos")),
            };
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
            ok(Value::Null)
        }
        "ui.text" => {
            app.synthetic.push(egui::Event::Text(s("text").unwrap_or("").to_string()));
            ok(Value::Null)
        }
        "ui.set" => {
            let mut cur = serde_json::to_value(&app.ui).unwrap_or(json!({}));
            if let (Some(o), Some(src)) = (cur.as_object_mut(), p.as_object()) {
                for (k, v) in src {
                    o.insert(k.clone(), v.clone());
                }
            }
            match serde_json::from_value::<crate::UiState>(cur) {
                Ok(u) => {
                    app.ui = u;
                    ok(serde_json::to_value(&app.ui).unwrap_or_default())
                }
                Err(e) => err(e),
            }
        }
        "ui.resize" => {
            let (Some(w), Some(h)) = (f("width"), f("height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w as f32, h as f32)));
            ok(Value::Null)
        }
        "ui.screenshot" => Outcome::Screenshot { path: s("path").map(str::to_string) },
        "ui.render" => {
            let w = f("width").unwrap_or(1600.0).clamp(16.0, 8192.0) as u32;
            let h = f("height").unwrap_or(1000.0).clamp(16.0, 8192.0) as u32;
            let Ok(st) = app.session.state() else { return err("no drawing") };
            let list = cadcraft_render::build(&st.doc, &st.space, &cadcraft_render::Options::default());
            let v = st.view();
            let view = cadcraft_render::raster::View { center: v.center, scale: f64::from(h) / v.height.max(1e-12), width: w, height: h };
            let Some(png) = cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default()) else {
                return err("render failed");
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = s("path") {
                return match std::fs::write(path, &png) {
                    Ok(()) => ok(json!({"path": path, "bytes": png.len()})),
                    Err(e) => err(e),
                };
            }
            ok(json!({"bytes": png.len()}))
        }
        "app.open" => {
            let Some(path) = s("path") else { return err("missing path") };
            wrap(app.run("open", json!({"path": path})))
        }
        "app.save" => wrap(app.run(if s("path").is_some() { "saveas" } else { "qsave" }, p.clone())),
        "app.quit" => {
            app.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Value::Null)
        }
        other => err(format!("unknown method `{other}`")),
    }
}

/// Save a screenshot PNG.
pub fn save_screenshot(image: &egui::ColorImage, path: Option<&str>) -> Value {
    let [w, h] = image.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for c in &image.pixels {
        buf.extend_from_slice(&c.to_array());
    }
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, buf) else { return json!({"ok": false, "error": "bad image"}) };
    let path = path.map(str::to_string).unwrap_or_else(|| std::env::temp_dir().join("cadcraft-shot.png").to_string_lossy().to_string());
    match img.save(&path) {
        Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    }
}
