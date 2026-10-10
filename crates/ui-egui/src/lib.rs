//! The CADCraft egui front end.
//!
//! A thin shell over [`cadcraft_engine::Session`]: panels read engine state and act through
//! commands (`app.run(id, params)`) or the command line (`app.cmdline(text)`). Nothing here owns
//! drawing data. The layout follows the familiar CAD desktop: title and tool bar, file tabs,
//! Tool Sets on the left, the drawing area with its command line, Layers and Properties on the
//! right, layout tabs and drafting toggles in the status bar.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod canvas;
pub mod chrome;
pub mod cmdline;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod gpu;
pub mod icons;
pub mod layers;
pub mod menus;
pub mod palettes;
pub mod parametric;
pub mod quick;
pub mod theme;

use std::sync::mpsc::Receiver;

use cadcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};

/// Persisted UI state (serde, so automation can read and set it).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub show_toolsets: bool,
    pub show_palettes: bool,
    pub show_toolbar: bool,
    pub show_file_tabs: bool,
    pub show_status_bar: bool,
    pub show_command_line: bool,
    pub show_viewcube: bool,
    pub show_ucs_icon: bool,
    pub show_layer_list: bool,
    /// "Drafting" or "Modeling".
    pub toolset_tab: String,
    pub collapsed_groups: Vec<String>,
    pub properties_all: bool,
    /// Show an in-window menu bar (platforms without a native menu bar, or on request).
    pub in_window_menu: bool,
    pub start_tab: bool,
    pub dialog: Option<String>,
    pub history_lines: usize,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            show_toolsets: true,
            show_palettes: true,
            show_toolbar: true,
            show_file_tabs: true,
            show_status_bar: true,
            show_command_line: true,
            show_viewcube: true,
            show_ucs_icon: true,
            show_layer_list: false,
            toolset_tab: "Drafting".into(),
            collapsed_groups: Vec::new(),
            properties_all: true,
            in_window_menu: !cfg!(target_os = "macos"),
            start_tab: false,
            dialog: None,
            history_lines: 3,
        }
    }
}

/// Host services (file pickers, clipboard) injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<String>>>,
}

pub struct CadApp {
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub canvas: canvas::CanvasState,
    pub cmd: cmdline::CmdLine,
    pub status: Option<(String, f64)>,
    pub integrated_titlebar: bool,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<(u64, Option<String>, std::sync::mpsc::Sender<Value>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    pub synthetic: Vec<egui::Event>,
    /// Caps the frame rate when presenting without vsync (see [`gpu::FrameCap`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub frame_cap: Option<gpu::FrameCap>,
    styled: bool,
    pub frame_ms: f64,
    pub quit_requested: bool,
}

impl CadApp {
    pub fn new(session: Session, services: Services) -> Self {
        CadApp {
            session,
            ui: UiState::default(),
            services,
            canvas: canvas::CanvasState::default(),
            cmd: cmdline::CmdLine::default(),
            status: None,
            integrated_titlebar: false,
            control_rx: None,
            pending_shots: Vec::new(),
            queued_shots: Vec::new(),
            shot_token: 0,
            synthetic: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            frame_cap: None,
            styled: false,
            frame_ms: 0.0,
            quit_requested: false,
        }
    }

    /// Draw the canvas on the GPU with the app's wgpu render state (eframe's
    /// `CreationContext::wgpu_render_state`). Without it the canvas draws on the CPU.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState) {
        self.canvas.gpu = Some(gpu::install(rs));
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Run a command programmatically (or a UI command).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if let Some(r) = menus::run_ui_command(self, id, &params) {
            return r;
        }
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        if let Ok(v) = &r
            && let Some(m) = v.get("message").and_then(Value::as_str)
        {
            for l in m.lines() {
                self.session.echo(l.to_string());
            }
        }
        if let Err(e) = &r {
            self.session.echo(e.clone());
        }
        r
    }

    /// Start a command as if typed (interactive when it has prompts).
    pub fn start(&mut self, name: &str) {
        if menus::run_ui_command(self, name, &Value::Null).is_some() {
            return;
        }
        if let Err(e) = self.session.start(name) {
            self.session.echo(e.to_string());
        }
    }

    /// Submit a line of command-line text.
    pub fn cmdline(&mut self, text: &str) {
        if self.session.running.is_none() {
            let first = text.split_whitespace().next().unwrap_or("");
            if !first.is_empty() && menus::run_ui_command(self, &first.to_ascii_lowercase(), &Value::Null).is_some() {
                return;
            }
        }
        if let Err(e) = self.session.cmdline(text) {
            self.session.echo(e.to_string());
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), now_ms()));
    }

    pub fn open_path(&mut self, path: &str) {
        if let Err(e) = self.run("open", json!({ "path": path })) {
            self.set_status(e);
        } else {
            self.ui.start_tab = false;
            self.canvas.zoom_pending = true;
        }
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !self.styled {
            theme::install_fonts(ctx);
            theme::apply(ctx);
            self.styled = true;
        }
        self.drain_control(ctx);
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        menus::shortcuts(self, ctx);
        #[cfg(not(target_arch = "wasm32"))]
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                self.open_path(&p);
            }
        }
    }

    /// Wait for the frame cap (if any), then inject synthetic events (one pointer event per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cap) = &mut self.frame_cap {
            cap.wait();
        }
        if self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t0 = now_ms();
        let t = theme::Tokens::get();
        chrome::title_and_toolbar(self, ui);
        if self.ui.in_window_menu {
            menus::menu_bar(self, ui);
        }
        if self.ui.show_status_bar {
            chrome::status_bar(self, ui);
        }
        if self.ui.show_file_tabs {
            chrome::file_tabs(self, ui);
        }
        let has_doc = !self.session.docs.is_empty() && !self.ui.start_tab;
        if has_doc && self.ui.show_toolsets {
            palettes::toolsets(self, ui);
        }
        if has_doc && self.ui.show_palettes {
            palettes::right_palettes(self, ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
            if has_doc {
                canvas::show(self, ui);
            } else {
                chrome::start_page(self, ui);
            }
        });
        dialogs::show(self, ui.ctx());
        self.frame_ms = now_ms() - t0;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.shot_token += 1;
                    let token = self.shot_token;
                    self.queued_shots.push((token, now_ms() + 120.0, 0));
                    self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
                }
            }
            ctx.request_repaint();
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                let _ = reply.send(control::save_screenshot(&image, path.as_deref()));
            }
        }
        let now = now_ms();
        self.pending_shots.retain(|(_, _, reply, deadline)| {
            if now < *deadline {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
            false
        });
    }
}

pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}
