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
//! - `ui.elements {label?, role?, focused?}`: observed visible controls and actual egui focus
//! - `ui.menu.list`: the menu tree; `ui.menu.invoke {command}`: like choosing a menu item
//! - `ui.pointer {x, y, space?: "world"|"screen", button?: left|right, action?: click|move}`
//! - `ui.click {x, y, shift?, ctrl?}`, `ui.move {x, y}`: real egui pointer input in screen points
//! - `ui.key {key, cmd?, shift?, alt?}`, `ui.text {text}`: synthetic keyboard input
//! - `ui.set {...UiState fields}`, `ui.resize {width, height}`
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path?, width?, height?, fit?}`: headless
//!   render of the drawing (no window needed; fits the drawing unless `fit` is false; replies `pngBase64`)
//! - `app.open {path}`, `app.save {path?}`, `app.quit`
//! - `control.methods`: this list, with params and a description; `ping`: replies `"pong"`

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use cadcraft_engine::Input;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::CadApp;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
    state: Arc<AtomicU8>,
}

/// [`ControlRequest`] states, shared by the app and the transport waiting for the reply.
const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, PendingReply) {
        let (tx, rx) = std::sync::mpsc::channel();
        let state = Arc::new(AtomicU8::new(QUEUED));
        (Self { method: method.into(), params, reply: tx, state: state.clone() }, PendingReply { rx, state })
    }

    /// Mark the request as started. False when the transport already gave up on it
    /// ([`PendingReply::wait`] timed out): the app must then skip it, since the client was told it
    /// did not run and may send it again.
    pub fn begin(&self) -> bool {
        self.state.compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }
}

/// The transport's side of a [`ControlRequest`]: waits for the reply.
pub struct PendingReply {
    rx: Receiver<ControlResponse>,
    state: Arc<AtomicU8>,
}

impl PendingReply {
    /// The reply, or after `timeout` an error that says whether the request can still have
    /// changed anything:
    /// - `{"ok": false, "error": "timeout", "state": "not-run"}`: the app had not started it, and
    ///   now never will. Sending it again is safe.
    /// - `{"ok": false, "error": "timeout", "state": "may-have-run"}`: the app had started it, so
    ///   it may still finish and change the drawing (its late reply is discarded). Inspect before
    ///   sending it again.
    pub fn wait(self, timeout: Duration) -> ControlResponse {
        if let Ok(r) = self.rx.recv_timeout(timeout) {
            return r;
        }
        if self.state.compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            return json!({"ok": false, "error": "timeout", "state": "not-run"});
        }
        // Started: a reply that arrived just now still counts.
        self.rx.try_recv().unwrap_or_else(|_| json!({"ok": false, "error": "timeout", "state": "may-have-run"}))
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

#[derive(Clone)]
struct Widget {
    id: egui::Id,
    label: String,
    role: &'static str,
    rect: [f32; 4],
    enabled: bool,
    visible: bool,
    selected: Option<bool>,
}

fn widgets_id() -> egui::Id {
    egui::Id::new("cadcraft-observed-widgets")
}

pub(crate) fn begin_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Vec<Widget>>(widgets_id()));
}

/// Observe a real response. This never assigns focus or changes input routing.
pub(crate) fn record_widget(ui: &egui::Ui, response: &egui::Response, label: &str, role: &'static str, selected: Option<bool>) {
    if !response.rect.is_finite() || !response.rect.is_positive() {
        return;
    }
    let widget = Widget {
        id: response.id,
        label: label.chars().take(512).collect(),
        role,
        rect: [response.rect.left(), response.rect.top(), response.rect.width(), response.rect.height()],
        enabled: response.enabled(),
        visible: ui.is_rect_visible(response.rect),
        selected,
    };
    ui.ctx().data_mut(|data| {
        let widgets = data.get_temp_mut_or_default::<Vec<Widget>>(widgets_id());
        if let Some(previous) = widgets.iter_mut().find(|previous| previous.id == widget.id) {
            *previous = widget;
        } else if widgets.len() < 2048 {
            widgets.push(widget);
        }
    });
}

fn widgets(ctx: &egui::Context) -> Vec<Widget> {
    ctx.data_mut(|data| data.get_temp::<Vec<Widget>>(widgets_id()).unwrap_or_default())
}

fn widget_json(widget: &Widget, focused: Option<egui::Id>) -> Value {
    json!({
        "id": format!("{:016x}", widget.id.value()), "label": widget.label, "role": widget.role,
        "rect": widget.rect, "enabled": widget.enabled, "visible": widget.visible,
        "selected": widget.selected, "focused": focused == Some(widget.id),
    })
}

fn focus_json(ctx: &egui::Context) -> Value {
    let focused = ctx.memory(|memory| memory.focused());
    let observed = widgets(ctx);
    let widget = observed.iter().find(|widget| focused == Some(widget.id));
    json!({
        "id": focused.map(|id| format!("{:016x}", id.value())),
        "label": widget.map(|widget| &widget.label),
        "role": widget.map(|widget| widget.role),
        "keyboardInput": ctx.egui_wants_keyboard_input(),
    })
}

fn elements(ctx: &egui::Context, params: &Value) -> Value {
    let focused = ctx.memory(|memory| memory.focused());
    Value::Array(
        widgets(ctx)
            .iter()
            .filter(|widget| {
                params.get("label").and_then(Value::as_str).is_none_or(|label| widget.label == label)
                    && params.get("role").and_then(Value::as_str).is_none_or(|role| widget.role == role)
                    && params.get("focused").and_then(Value::as_bool).is_none_or(|value| value == (focused == Some(widget.id)))
            })
            .map(|widget| widget_json(widget, focused))
            .collect(),
    )
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

/// What `control.methods` replies: method, params, description. Keep in step with `handle` and the
/// table in `docs/control-protocol.md` (`tests/control_methods.rs` checks the table).
const METHODS: &[(&str, &str, &str)] = &[
    ("cmdline.input", "{text}", "Type a line at the command line exactly like a user (commands, prompts, keywords; empty = Enter)."),
    ("cmdline.script", "{text}", "Run a multi-line script, like a .scr file."),
    ("cmdline.key", "{key: \"enter\"|\"escape\"}", "Press Enter or Escape at the command line."),
    ("cmdline.state", "", "Current prompt, keywords, accepted input kinds and history tail."),
    ("engine.execute", "{command, params}", "Run any command with JSON parameters. Never opens a dialog."),
    ("engine.commands", "", "Every command: id, label, menu path, shortcut, aliases, params doc, enabled."),
    ("drawing.inspect", "{entities?, limit?}", "Drawing summary and entities with handles and geometry."),
    ("ui.inspect", "", "UI state, canvas rect, view, cursor and snap, performance, keyboard focus."),
    ("ui.elements", "{label?, role?, focused?}", "Observed visible controls with their egui id, rectangle, state and focus."),
    ("ui.menu.list", "", "The menu tree."),
    ("ui.menu.invoke", "{command}", "Invoke a menu item like a click (interactive)."),
    ("ui.pointer", "{x, y, space?, button?, action?}", "Click or move in the drawing area (world coordinates by default). Right button = Enter."),
    ("ui.click", "{x, y, button?, shift?, ctrl?}", "Real egui click in screen points."),
    ("ui.move", "{x, y}", "Real egui pointer move in screen points."),
    ("ui.drag", "{x, y, to: [x, y], button?}", "Real press, move and release."),
    ("ui.key", "{key, cmd?, shift?, alt?}", "Synthetic key press."),
    ("ui.text", "{text}", "Synthetic text input."),
    ("ui.set", "UiState fields", "Change UI state: palettes, toolbars, command line, theme…"),
    ("ui.resize", "{width, height}", "Resize the window."),
    ("ui.focus", "", "Activate the native window."),
    ("ui.screenshot", "{path?}", "PNG of the window (needs a presented frame)."),
    ("ui.render", "{path?, width?, height?, fit?}", "Headless render of the drawing; replies pngBase64."),
    ("app.open", "{path}", "Open a drawing."),
    ("app.save", "{path?}", "Save the drawing (Save As with a path)."),
    ("app.quit", "", "Quit without the unsaved-changes prompt."),
    ("control.methods", "", "This list."),
    ("ping", "", "Replies \"pong\"."),
];

fn methods_json() -> Value {
    METHODS.iter().map(|(method, params, description)| json!({"method": method, "params": params, "description": description})).collect()
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
        "historyExpanded": app.cmd.expanded,
    })
}

pub fn inspect(app: &CadApp, ctx: &egui::Context) -> Value {
    let r = ctx.content_rect();
    let view = app.session.state().map(|s| s.view()).ok();
    json!({
        "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
        "theme": if app.shown_theme() == egui::Theme::Light { "light" } else { "dark" },
        "animations": app.shows_animations(),
        "focus": focus_json(ctx),
        "canvasRect": app.canvas.rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "window": [r.width(), r.height()],
        "view": view.map(|v| json!({"center": [v.center.x, v.center.y], "height": v.height})),
        "cursor": app.canvas.cursor.map(|c| [c.x, c.y]),
        "snap": app.canvas.snap.map(|s| json!({"point": [s.point.x, s.point.y], "mode": s.name})),
        "session": app.session.summary(),
        "closePrompt": app.close_prompt(),
        "perf": {"frameMs": app.frame_ms, "buildMs": app.canvas.build_ms, "drawMs": app.canvas.draw_ms, "meshMs": app.canvas.mesh_ms, "building": app.canvas.building(), "renderer": if app.canvas.gpu.is_some() { "gpu" } else { "cpu" }, "prims": app.canvas.list.as_ref().map(|l| l.prims.len())},
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
            let error = app.cmdline(text);
            let out: Vec<String> = app.session.log.iter().skip(before).cloned().collect();
            let mut st = cmdline_state(app);
            if let Some(o) = st.as_object_mut() {
                o.insert("output".into(), json!(out));
                if let Some(e) = error {
                    o.insert("error".into(), json!(e));
                }
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
            let before = app.session.log.len();
            let error = match s("key").unwrap_or("enter").to_ascii_lowercase().as_str() {
                "escape" | "esc" => {
                    app.session.cancel();
                    None
                }
                // As pressing Enter in the command line: a refused Enter is echoed to the history.
                _ => app.session.input(Input::Enter).err().map(|e| e.to_string()),
            };
            if let Some(e) = &error {
                app.session.echo(e.clone());
            }
            let out: Vec<String> = app.session.log.iter().skip(before).cloned().collect();
            let mut st = cmdline_state(app);
            if let Some(o) = st.as_object_mut() {
                o.insert("output".into(), json!(out));
                if let Some(e) = error {
                    o.insert("error".into(), json!(e));
                }
            }
            ok(st)
        }
        "cmdline.state" => ok(cmdline_state(app)),
        "engine.commands" => ok(all_commands(app)),
        "drawing.inspect" | "document.inspect" => wrap(app.session.execute("drawing.inspect", p).map_err(|e| e.to_string())),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.elements" => ok(elements(ctx, p)),
        "ui.focus" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ok(Value::Null)
        }
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
            app.session.cursor_deferred = None;
            app.canvas.cursor = Some(wp);
            if action == "move" {
                if let Some(sp) = app.canvas.xf.map(|xf| xf.to_screen(wp)) {
                    app.synthetic.push(egui::Event::PointerMoved(sp));
                }
                return ok(json!({"world": [wp.x, wp.y]}));
            }
            let shift = p.get("shift").and_then(Value::as_bool).unwrap_or(false);
            let r = match s("button").unwrap_or("left") {
                "right" => app.session.input(Input::Enter),
                _ if app.session.prompting() => app.session.with_shift(shift, |s| s.input(Input::Point(wp))),
                _ => app.session.idle_click(wp, shift),
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
                let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
                let modifiers = egui::Modifiers { shift: b("shift"), ctrl: b("ctrl"), ..Default::default() };
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
                if modifiers.any() {
                    app.synthetic.push(egui::Event::PointerMoved(pos));
                }
            }
            ok(Value::Null)
        }
        // Press at (x, y), move in steps to `to`, release there: a real drag through egui.
        "ui.drag" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            let to = p.get("to").and_then(Value::as_array);
            let (Some(tx), Some(ty)) = (to.and_then(|a| a.first()).and_then(Value::as_f64), to.and_then(|a| a.get(1)).and_then(Value::as_f64)) else {
                return err("missing to: [x, y]");
            };
            let button = match s("button") {
                Some("right") => egui::PointerButton::Secondary,
                Some("middle") => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            let modifiers = egui::Modifiers::default();
            let from = egui::pos2(x as f32, y as f32);
            let to = egui::pos2(tx as f32, ty as f32);
            app.synthetic.push(egui::Event::PointerMoved(from));
            app.synthetic.push(egui::Event::PointerButton { pos: from, button, pressed: true, modifiers });
            for i in 1..=8 {
                app.synthetic.push(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
            }
            app.synthetic.push(egui::Event::PointerButton { pos: to, button, pressed: false, modifiers });
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
                    if let Err(error) = u.docking.validate() {
                        return err(error);
                    }
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
            // `window`: just that area, at the size given or matching its shape.
            let window = render_window(p);
            let opts = cadcraft_render::Options { view: window, transparency: app.session.settings.transparency_display, ..Default::default() };
            let list = cadcraft_render::build(&st.doc, &st.space, &opts);
            let v = st.view();
            let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(true);
            let view = match &window {
                Some(win) => cadcraft_render::raster::View::of_window(win, f("width").map(|x| x as u32), f("height").map(|x| x as u32)),
                None => render_view(&list.bounds, v.center, v.height, fit, w, h),
            };
            let (w, h) = (view.width, view.height);
            let Some(png) = cadcraft_render::raster::render_png(&list, &view, &cadcraft_render::raster::RasterOptions::default()) else {
                return err("render failed");
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = s("path") {
                if let Err(e) = std::fs::write(path, &png) {
                    return err(e);
                }
                return ok(render_reply(&png, w, h, Some(path)));
            }
            ok(render_reply(&png, w, h, None))
        }
        "app.open" => {
            let Some(path) = s("path") else { return err("missing path") };
            wrap(app.run("open", json!({"path": path})))
        }
        "app.save" => wrap(app.run(if s("path").is_some() { "saveas" } else { "qsave" }, p.clone())),
        "app.quit" => {
            // Programmatic calls never open dialogs: quit without the unsaved-changes prompt.
            app.quit_confirmed = true;
            app.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Value::Null)
        }
        "control.methods" => ok(methods_json()),
        "ping" => ok(json!("pong")),
        other => err(format!("unknown method `{other}`; `control.methods` lists them (docs/control-protocol.md)")),
    }
}

/// The `window` of a `ui.render` request: two finite corners [[x0, y0], [x1, y1]] around an area
/// (`None` when absent or unusable).
pub fn render_window(p: &Value) -> Option<cadcraft_geom::Bounds2> {
    let a = p.get("window")?.as_array().filter(|a| a.len() == 2)?;
    let pt = |v: &Value| -> Option<cadcraft_geom::Vec2> {
        let c = v.as_array().filter(|c| c.len() == 2)?;
        let (x, y) = (c[0].as_f64()?, c[1].as_f64()?);
        (x.is_finite() && y.is_finite()).then(|| cadcraft_geom::Vec2::new(x, y))
    };
    let b = cadcraft_geom::Bounds2::new(pt(&a[0])?, pt(&a[1])?);
    (b.width() > 0.0 && b.height() > 0.0).then_some(b)
}

/// The raster view for `ui.render`: fitted to the drawing's bounds, or the current screen view.
fn render_view(bounds: &cadcraft_geom::Bounds2, center: Vec2, height: f64, fit: bool, w: u32, h: u32) -> cadcraft_render::raster::View {
    if fit {
        cadcraft_render::raster::View::fit(bounds, w, h, 0.05)
    } else {
        cadcraft_render::raster::View { center, scale: f64::from(h) / height.max(1e-12), width: w, height: h }
    }
}

/// The `ui.render` reply, matching the headless backend (`pngBase64`, `width`, `height`), plus `bytes` and `path`.
fn render_reply(png: &[u8], w: u32, h: u32, path: Option<&str>) -> Value {
    let mut v = json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(png), "width": w, "height": h, "bytes": png.len()});
    if let (Some(path), Some(o)) = (path, v.as_object_mut()) {
        o.insert("path".to_string(), json!(path));
    }
    v
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_view_fit_finds_offscreen_drawing() {
        let far = cadcraft_geom::Bounds2::new(Vec2::new(9000.0, 9000.0), Vec2::new(9100.0, 9050.0));
        let fitted = render_view(&far, Vec2::ZERO, 100.0, true, 800, 500);
        assert_eq!(fitted.center, far.center());
        let kept = render_view(&far, Vec2::ZERO, 100.0, false, 800, 500);
        assert_eq!(kept.center, Vec2::ZERO);
        assert!((kept.scale - 5.0).abs() < 1e-9);
    }

    #[test]
    fn timed_out_request_that_never_started_is_skipped() {
        let (req, pending) = ControlRequest::new("engine.execute", json!({}));
        let r = pending.wait(Duration::from_millis(1));
        assert_eq!(r, json!({"ok": false, "error": "timeout", "state": "not-run"}));
        // The app reaches the request after the timeout: it must not run it.
        assert!(!req.begin());
    }

    #[test]
    fn timed_out_request_already_running_may_have_run() {
        let (req, pending) = ControlRequest::new("engine.execute", json!({}));
        assert!(req.begin());
        let r = pending.wait(Duration::from_millis(1));
        assert_eq!(r, json!({"ok": false, "error": "timeout", "state": "may-have-run"}));
        // The late result goes nowhere (the receiver is gone); sending it doesn't fail the app.
        assert!(req.reply.send(json!({"ok": true})).is_err());
    }

    #[test]
    fn reply_in_time_is_returned() {
        let (req, pending) = ControlRequest::new("engine.execute", json!({}));
        let app = std::thread::spawn(move || {
            assert!(req.begin());
            let _ = req.reply.send(json!({"ok": true, "result": 1}));
        });
        assert_eq!(pending.wait(Duration::from_secs(30)), json!({"ok": true, "result": 1}));
        let _ = app.join();
    }

    #[test]
    fn reply_sent_right_at_the_timeout_still_counts() {
        let (req, pending) = ControlRequest::new("engine.execute", json!({}));
        assert!(req.begin());
        let _ = req.reply.send(json!({"ok": true}));
        assert_eq!(pending.wait(Duration::ZERO), json!({"ok": true}));
    }

    #[test]
    fn render_reply_has_base64_width_height_and_keeps_old_keys() {
        let r = render_reply(b"abc", 7, 5, Some("x.png"));
        assert_eq!(r["pngBase64"], "YWJj");
        assert_eq!(r["width"], 7);
        assert_eq!(r["height"], 5);
        assert_eq!(r["bytes"], 3);
        assert_eq!(r["path"], "x.png");
        assert!(render_reply(b"abc", 7, 5, None).get("path").is_none());
    }
}
