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
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path?, width?, height?, fit?, window?,
//!   background?, plot_style?}`: headless render of the drawing (no window needed; fits the drawing
//!   unless `fit` is false or a `window` is given; on the dark canvas unless `background` is
//!   `white`, `paper` or `#rrggbb`; `plot_style` names a plot style table, which plots on white
//!   unless `background` is given, or is `none`; replies `pngBase64`)
//! - `app.open {path}`, `app.save {path?}`, `app.quit`
//! - `control.methods`: this list, with params and a description; `ping`: replies `"pong"`

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
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

    /// The reply if the app has answered, without waiting: a transport that cannot block (the web
    /// shell) polls this once a frame. A request the app dropped without answering reports that.
    pub fn try_reply(&self) -> Option<ControlResponse> {
        match self.rx.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(json!({"ok": false, "error": "the app dropped the request"})),
        }
    }
}

impl Drop for PendingReply {
    /// A reply nobody waits for any more cancels a request the app has not started, as a timed-out
    /// [`PendingReply::wait`] does: the client was told it failed, so the app must not run it late.
    /// A request the app already started, or answered, is unaffected.
    fn drop(&mut self) {
        let _ = self.state.compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
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

/// egui temp-data key holding the widgets observed during the current frame.
fn widgets_id() -> egui::Id {
    egui::Id::unique("cadcraft-observed-widgets")
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
/// table in `docs/control-protocol.md` (`tests/it/control_methods.rs` checks the table).
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
    ("ui.render", "{path?, width?, height?, fit?, window?, background?, plot_style?}", "Headless render of the drawing; replies pngBase64."),
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

/// Build the `ui.inspect` snapshot: UI state and theme, animations, focus, canvas/window rects, view, cursor, snap, session summary, close prompt and performance counters.
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
        "perf": {"frameMs": app.frame_ms, "buildMs": app.canvas.build_ms, "drawMs": app.canvas.draw_ms, "meshMs": app.canvas.mesh_ms, "building": app.canvas.building(), "renderer": if app.canvas.gpu.is_some() { "gpu" } else { "cpu" }, "prims": app.canvas.list.as_ref().map(|l| l.prims.len()), "truncated": app.canvas.list.as_ref().map(|l| l.truncated), "uiFrames": app.ui_frames, "passIndex": ctx.current_pass_index()},
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
            let r = match s("button").unwrap_or("left") {
                "right" => app.session.input(Input::Enter),
                _ if app.session.prompting() => app.session.input(Input::Point(wp)),
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
                Ok(mut u) => {
                    // Shortcuts are checked as the preferences check them.
                    u.shortcuts = crate::keys::from_prefs(&json!(u.shortcuts));
                    if let Err(error) = crate::docking::validate(&u.docking) {
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
            // `background` and `plot_style` (#803), read as the headless MCP backend reads them.
            let look = match cadcraft_engine::render_look::RenderLook::from_params(&st.doc, p) {
                Ok(look) => look,
                Err(e) => return err(e),
            };
            let opts = cadcraft_render::Options {
                view: window,
                transparency: app.session.settings.transparency_display,
                plot_style_table: look.plot_style_table.clone(),
                ..Default::default()
            };
            let list = cadcraft_render::build(&st.doc, &st.space, &opts);
            let v = st.view();
            let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(true);
            let view = match &window {
                Some(win) => cadcraft_render::raster::View::of_window(win, f("width").map(|x| x as u32), f("height").map(|x| x as u32)),
                None => render_view(&list.bounds, v.center, v.height, fit, w, h),
            };
            let (w, h) = (view.width, view.height);
            let Some(png) = cadcraft_render::raster::render_png(&list, &view, &look.raster()) else {
                return err("render failed");
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = s("path") {
                let file = std::path::Path::new(&path);
                let folder = cadcraft_engine::atomic_write::missing_folder(file);
                if let Err(e) = cadcraft_engine::atomic_write::write_atomic_making_folders(file, &png) {
                    return err(e);
                }
                let mut reply = render_reply(&png, w, h, Some(path), list.truncated);
                if let Some(folder) = folder {
                    reply["createdFolder"] = json!(folder.display().to_string());
                }
                return ok(reply);
            }
            ok(render_reply(&png, w, h, None, list.truncated))
        }
        "app.open" => {
            let Some(path) = s("path") else { return err("missing path") };
            wrap(app.run("open", json!({"path": path})))
        }
        // Never `null` params: those mean a user's Save, which may pick a file or ask.
        "app.save" => wrap(app.run(if s("path").is_some() { "saveas" } else { "qsave" }, if p.is_null() { json!({}) } else { p.clone() })),
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

/// The `ui.render` reply, matching the headless backend (`pngBase64`, `width`, `height`), plus
/// `bytes`, `path` and `truncated` (the build left block contents out, see #1070).
fn render_reply(png: &[u8], w: u32, h: u32, path: Option<&str>, truncated: bool) -> Value {
    let mut v =
        json!({"pngBase64": cadcraft_engine::cmd::file::base64_encode(png), "width": w, "height": h, "bytes": png.len(), "truncated": truncated});
    if let (Some(path), Some(o)) = (path, v.as_object_mut()) {
        o.insert("path".to_string(), json!(path));
    }
    v
}

/// A private, per-process screenshot directory for pathless `ui.screenshot` requests.
///
/// The directory is created with owner-only permissions (`0700` on Unix) under a name that
/// includes the process id, a monotonic nonce and a sequence number. `create_dir` fails rather
/// than following a pre-created symlink, so another local user cannot redirect the write.
fn private_shot_dir() -> std::io::Result<std::path::PathBuf> {
    use std::sync::OnceLock;
    static DIR: OnceLock<Result<std::path::PathBuf, String>> = OnceLock::new();
    DIR.get_or_init(|| create_private_shot_dir().map_err(|e| e.to_string())).clone().map_err(std::io::Error::other)
}

/// Process-wide monotonic counter used to keep pathless screenshot names unique.
fn next_pathless_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Create at most 32 exclusive `cadcraft-shots-<pid>-<stamp>-<nonce>` directories in the temp dir; only `AlreadyExists` is retried.
fn create_private_shot_dir() -> std::io::Result<std::path::PathBuf> {
    let base = std::env::temp_dir();
    let stamp = crate::now_ms().to_bits();
    for _ in 0..32 {
        let nonce = next_pathless_nonce();
        let dir = base.join(format!("cadcraft-shots-{}-{stamp:x}-{nonce}", std::process::id()));
        match create_private_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "could not create a private screenshot directory"))
}

/// Create `path` with mode 0700 on Unix (plain create elsewhere), failing rather than following or reusing a pre-created entry.
fn create_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)
    }
}

/// Create a new file with owner-only permissions. `create_new` uses `O_EXCL` on Unix and refuses
/// an existing path, including a symlink, so a pre-created link cannot redirect the screenshot.
fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// True when the metadata has no group/other permission bits (always true on non-Unix).
fn is_private_shot_dir(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.permissions().mode() & 0o077 == 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

/// Pathless screenshots share one directory per process. Tests that write them hold this guard,
/// so no other test writes while it is cleaned up. Dropping it, even while a failed test unwinds,
/// removes every screenshot file in the directory and then the directory itself.
#[cfg(test)]
pub(crate) struct PathlessShots {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn lock_pathless_shots() -> PathlessShots {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PathlessShots { _lock: LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }
}

#[cfg(test)]
impl Drop for PathlessShots {
    fn drop(&mut self) {
        let Ok(dir) = private_shot_dir() else { return };
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("cadcraft-shot-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Save a pathless screenshot into the private per-process directory: re-validate or recreate it as an owner-only real directory (0700 on Unix; plain create elsewhere), then create up to 64 exclusive files (0600 on Unix) and write a PNG.
fn save_pathless_screenshot(img: &image::RgbaImage, token: u64) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write as _;
    let dir = private_shot_dir()?;
    // Tests and users may remove the private directory between requests; recreate it exclusively
    // rather than following anything that replaced it.
    if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink() && is_private_shot_dir(&m)) {
        match create_private_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let meta = std::fs::symlink_metadata(&dir)?;
                if !meta.file_type().is_dir() || meta.file_type().is_symlink() || !is_private_shot_dir(&meta) {
                    return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the screenshot directory was replaced"));
                }
            }
            Err(e) => return Err(e),
        }
    }
    for _ in 0..64 {
        let path = dir.join(format!("cadcraft-shot-{token}-{}.png", next_pathless_nonce()));
        match create_private_file(&path) {
            Ok(mut file) => {
                let written = img.write_to(&mut file, image::ImageFormat::Png).map_err(std::io::Error::other).and_then(|()| file.flush());
                if let Err(e) = written {
                    // The reply carries no path on failure, so nobody else could remove the partial file.
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "could not allocate a unique screenshot file"))
}

/// Save a screenshot PNG. Only called from the logic drain, never from the screenshot callback.
///
/// An explicit `path` is written as before. Without one, the file gets a unique name inside a
/// private per-process directory, created exclusively with owner-only permissions; concurrent
/// requests (even from two app instances with the same per-process token) cannot collide or
/// follow a pre-created symlink.
pub fn save_screenshot(image: &egui::ColorImage, path: Option<&str>, token: u64) -> Value {
    let [w, h] = image.size;
    let Some(len) = w.checked_mul(h).and_then(|pixels| pixels.checked_mul(4)) else {
        return json!({"ok": false, "error": "screenshot dimensions overflow", "request": token});
    };
    let mut buf = Vec::with_capacity(len);
    for c in &image.pixels {
        buf.extend_from_slice(&c.to_array());
    }
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, buf) else {
        return json!({"ok": false, "error": "bad image", "request": token});
    };
    // No file system to write to (the web build): the PNG comes back inline, as `ui.render`'s does.
    if cfg!(target_arch = "wasm32") {
        return inline_screenshot(&img, token);
    }
    let saved = match path {
        Some(path) => img.save(path).map(|()| path.to_string()).map_err(std::io::Error::other),
        None => save_pathless_screenshot(&img, token).map(|path| path.to_string_lossy().into_owned()),
    };
    match saved {
        Ok(path) => json!({"ok": true, "request": token, "result": {"path": path, "width": w, "height": h}}),
        Err(e) => json!({"ok": false, "error": e.to_string(), "request": token}),
    }
}

/// A screenshot as an inline PNG reply (`pngBase64`, `width`, `height`, `bytes`, like `ui.render`), for a
/// build that has no file system to write it to: the web build, where `std::env::temp_dir` panics.
fn inline_screenshot(img: &image::RgbaImage, token: u64) -> Value {
    let (w, h) = img.dimensions();
    let mut png = Vec::new();
    if let Err(e) = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png) {
        return json!({"ok": false, "error": e.to_string(), "request": token});
    }
    json!({"ok": true, "request": token, "result": {"pngBase64": cadcraft_engine::cmd::file::base64_encode(&png), "width": w, "height": h, "bytes": png.len()}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inline_screenshot_carries_the_png_and_its_size() {
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let reply = inline_screenshot(&img, 7);
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["request"], 7);
        assert_eq!((reply["result"]["width"].as_u64(), reply["result"]["height"].as_u64()), (Some(3), Some(2)));
        let b64 = reply["result"]["pngBase64"].as_str().unwrap_or_default();
        assert!(b64.starts_with("iVBORw0KGgo"), "a PNG signature in base64: {b64:.20}");
        assert!(reply["result"]["bytes"].as_u64().is_some_and(|n| n > 30), "the PNG byte count is reported");
    }

    #[test]
    fn dropping_a_pending_reply_cancels_a_request_the_app_has_not_started() {
        let (request, pending) = ControlRequest::new("ui.inspect", json!({}));
        drop(pending);
        assert!(!request.begin(), "the app must skip a request nobody waits for");
    }

    #[test]
    fn dropping_a_pending_reply_leaves_a_started_request_alone() {
        let (request, pending) = ControlRequest::new("ui.inspect", json!({}));
        assert!(request.begin());
        drop(pending);
        assert_eq!(request.state.load(Ordering::Acquire), STARTED, "a started request is not cancelled");
    }

    #[test]
    fn try_reply_polls_without_blocking() {
        let (request, pending) = ControlRequest::new("ui.inspect", json!({}));
        assert_eq!(pending.try_reply(), None, "no answer yet");
        assert!(request.begin());
        request.reply.send(json!({"ok": true, "result": 1})).unwrap_or_default();
        assert_eq!(pending.try_reply(), Some(json!({"ok": true, "result": 1})));
        let (request, pending) = ControlRequest::new("ui.inspect", json!({}));
        drop(request);
        assert_eq!(pending.try_reply().and_then(|r| r["ok"].as_bool()), Some(false), "the app went away");
    }

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

    /// Two pathless saves with the same token produce distinct, decodable PNGs; on Unix the files are 0600 and their directory 0700.
    #[test]
    fn pathless_screenshots_are_unique_private_png_files() {
        let _shots = lock_pathless_shots();
        let img = egui::ColorImage::filled([4, 3], egui::Color32::RED);
        let first = save_screenshot(&img, None, 1);
        let second = save_screenshot(&img, None, 1);
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(second["ok"], true, "{second}");
        let a = std::path::PathBuf::from(first["result"]["path"].as_str().unwrap_or_default());
        let b = std::path::PathBuf::from(second["result"]["path"].as_str().unwrap_or_default());
        assert_ne!(a, b, "the same per-process token must not reuse a pathless file");
        for path in [&a, &b] {
            let bytes = std::fs::read(path).expect("the screenshot file exists");
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{path:?} is a PNG");
            image::load_from_memory(&bytes).expect("decodable PNG");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for path in [&a, &b] {
                let mode = std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "screenshot {path:?} is owner-only");
            }
            let dir = a.parent().expect("the file has a private directory");
            let dir_mode = std::fs::metadata(dir).expect("directory metadata").permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "the screenshot directory is owner-only");
        }
    }

    /// `create_new` refuses a pre-existing symlink with `AlreadyExists` and leaves the link target unchanged.
    #[cfg(unix)]
    #[test]
    fn private_screenshot_file_refuses_a_preexisting_symlink() {
        let dir = std::env::temp_dir().join(format!("cadcraft-shot-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_private_dir(&dir).expect("private test directory");
        let target = dir.join("target.txt");
        std::fs::write(&target, b"do not overwrite").expect("target file");
        let link = dir.join("shot.png");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = create_private_file(&link).expect_err("an existing symlink is refused");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).expect("target unchanged"), b"do not overwrite");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_reply_has_base64_width_height_and_keeps_old_keys() {
        let r = render_reply(b"abc", 7, 5, Some("x.png"), false);
        assert_eq!(r["pngBase64"], "YWJj");
        assert_eq!(r["width"], 7);
        assert_eq!(r["height"], 5);
        assert_eq!(r["bytes"], 3);
        assert_eq!(r["path"], "x.png");
        assert_eq!(r["truncated"], false);
        assert!(render_reply(b"abc", 7, 5, None, true).get("path").is_none());
        assert_eq!(render_reply(b"abc", 7, 5, None, true)["truncated"], true);
    }

    /// The reply to a `ui.render` request in an app showing one line along y = 0: colour 7, or
    /// red on a red layer.
    fn render_line(red: bool, params: Value) -> Value {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        app.session.new_drawing(false);
        if red {
            app.session.execute("layer.new", &json!({"name": "Red", "color": "red", "current": true})).unwrap();
        }
        app.session.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
        let (req, _rx) = ControlRequest::new("ui.render", params);
        match handle(&mut app, &egui::Context::default(), &req) {
            Outcome::Done(v) => v,
            Outcome::Screenshot { .. } => panic!("ui.render must not take a screenshot"),
        }
    }

    /// The RGB of a reply's corner pixel and of its middle column's darkest pixel.
    fn corner_and_darkest(reply: &Value) -> ([u8; 3], [u8; 3]) {
        let png = cadcraft_engine::cmd::file::base64_decode(reply["result"]["pngBase64"].as_str().unwrap_or_default()).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        let corner = img.get_pixel(0, 0).0;
        let darkest = (0..img.height()).map(|y| img.get_pixel(img.width() / 2, y).0).min_by_key(|p| p.iter().map(|&c| u32::from(c)).sum::<u32>());
        (corner, darkest.unwrap_or_default())
    }

    /// Guards #803 in the app: `ui.render` takes `background` and `plot_style` as the headless MCP
    /// backend does (colour 7 drawn dark on white, a red line black with `monochrome.ctb`, white
    /// by default with a plot style), keeps the dark canvas by default and refuses unknown values.
    #[test]
    fn render_takes_a_background_and_a_plot_style() {
        // The line runs along the middle of pixel row 20 of the 120 x 40 image.
        let window = json!([[-1.0, -1.95], [11.0, 2.05]]);
        let render = |red: bool, mut p: Value| {
            p["window"] = window.clone();
            p["width"] = json!(120);
            p["height"] = json!(40);
            let r = render_line(red, p.clone());
            assert_eq!(r["ok"], true, "{p}: {}", r["error"]);
            corner_and_darkest(&r)
        };
        assert_eq!(render(false, json!({})).0, [33, 40, 48], "the default stays the dark canvas");
        let (corner, ink) = render(false, json!({"background": "white"}));
        assert_eq!(corner, [255, 255, 255]);
        assert!(ink.iter().all(|&c| c < 128), "colour 7 is drawn dark on white: {ink:?}");
        // A red line stays red without a plot style and plots black with monochrome.ctb, on white
        // unless `background` says otherwise.
        let (_, [r, g, b]) = render(true, json!({"background": "paper"}));
        assert!(r > 200 && g < 100 && b < 100, "red without a plot style: {:?}", [r, g, b]);
        for (p, bg) in [
            (json!({"plot_style": "monochrome.ctb"}), [255, 255, 255]),
            (json!({"plot_style": "monochrome.ctb", "background": "#336699"}), [0x33, 0x66, 0x99]),
        ] {
            let (corner, [r, g, b]) = render(true, p.clone());
            assert_eq!(corner, bg, "{p}");
            assert!(r < 100 && r == g && g == b, "{p}: red plots black: {:?}", [r, g, b]);
        }
        for p in [json!({"background": "purple"}), json!({"background": 7}), json!({"plot_style": "nope.ctb"})] {
            let r = render_line(false, p.clone());
            assert_eq!(r["ok"], false, "{p} must be refused");
            assert!(r.get("result").is_none(), "{p}");
        }
    }
}
