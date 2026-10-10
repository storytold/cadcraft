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
pub mod closing;
pub mod cmdline;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod dyninput;
pub mod gpu;
pub mod i18n;
pub mod icons;
pub mod layers;
pub mod managers;
pub mod menus;
pub mod palettes;
pub mod parametric;
pub mod quick;
pub mod theme;
pub mod viewcube;

use std::sync::mpsc::Receiver;

use cadcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};

/// The host storage key for [`CadApp::prefs_json`].
pub const PREFS_KEY: &str = "cadcraft.prefs";

/// Persisted UI state (serde, so automation can read and set it).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub interface_language: i18n::Preference,
    pub show_toolsets: bool,
    pub show_palettes: bool,
    pub show_toolbar: bool,
    pub show_file_tabs: bool,
    pub show_status_bar: bool,
    pub show_command_line: bool,
    pub show_viewcube: bool,
    pub show_ucs_icon: bool,
    pub show_layer_list: bool,
    /// Keep the system cursor visible over the drawing area (a small crosshair at the centre of the
    /// drawn one) instead of hiding it. Screen magnifiers (Windows Magnifier "follow the mouse
    /// pointer"), screen readers, recorders and remote desktops track the system cursor and lose it
    /// when it is hidden. Off by default; `CADCRAFT_SYSTEM_CURSOR=1` turns it on at startup.
    pub system_cursor: bool,
    /// "Drafting" or "Modeling".
    pub toolset_tab: String,
    pub collapsed_groups: Vec<String>,
    pub properties_all: bool,
    /// Show an in-window menu bar (platforms without a native menu bar, or on request).
    pub in_window_menu: bool,
    pub start_tab: bool,
    pub dialog: Option<String>,
    pub history_lines: usize,
    /// Interface theme: "system" (follow the OS light/dark appearance), "light" or "dark".
    pub theme: theme::ThemePref,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            interface_language: i18n::Preference::default(),
            show_toolsets: true,
            show_palettes: true,
            show_toolbar: true,
            show_file_tabs: true,
            show_status_bar: true,
            show_command_line: true,
            show_viewcube: true,
            show_ucs_icon: true,
            show_layer_list: false,
            system_cursor: std::env::var("CADCRAFT_SYSTEM_CURSOR").is_ok_and(|v| !matches!(v.trim(), "" | "0" | "false" | "off")),
            toolset_tab: "Drafting".into(),
            collapsed_groups: Vec::new(),
            properties_all: true,
            in_window_menu: !cfg!(target_os = "macos"),
            start_tab: false,
            dialog: None,
            history_lines: 3,
            theme: theme::ThemePref::default(),
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
    pub system_languages: Vec<String>,
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
    /// The theme last installed into egui (`None` until the first frame).
    shown_theme: Option<egui::Theme>,
    pub frame_ms: f64,
    pub quit_requested: bool,
    /// The app may close although drawings have unsaved changes (they were dealt with, or the
    /// quit came from the control channel, which never asks).
    pub quit_confirmed: bool,
    /// A close request waiting for "Save changes?" answers ([`closing`]).
    pub closing: Option<closing::Closing>,
}

impl CadApp {
    pub fn new(session: Session, services: Services) -> Self {
        CadApp {
            system_languages: Vec::new(),
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
            shown_theme: None,
            frame_ms: 0.0,
            quit_requested: false,
            quit_confirmed: false,
            closing: None,
        }
    }

    pub fn language(&self) -> &'static str {
        self.ui.interface_language.resolve(&self.system_languages)
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
            // `cmd {json}` is a programmatic call: the engine runs it with those parameters.
            let json_form = text.trim_start().get(first.len()..).is_some_and(|rest| rest.trim_start().starts_with('{'));
            if !first.is_empty() && !json_form && menus::run_ui_command(self, &first.to_ascii_lowercase(), &Value::Null).is_some() {
                return;
            }
        }
        if let Err(e) = self.session.cmdline(text) {
            self.session.echo(e.to_string());
        }
    }

    /// The theme the interface shows now (the choice resolved against the OS appearance).
    pub fn shown_theme(&self) -> egui::Theme {
        self.shown_theme.unwrap_or_else(|| self.ui.theme.resolve(None))
    }

    /// Preferences kept across restarts, as JSON for the host's storage ([`PREFS_KEY`]).
    pub fn prefs_json(&self) -> String {
        json!({ "theme": self.ui.theme.as_str(), "interfaceLanguage": self.ui.interface_language.code() }).to_string()
    }

    /// Restore preferences saved by [`Self::prefs_json`]; unknown or malformed values are ignored.
    pub fn load_prefs(&mut self, json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(json) else { return };
        if let Some(t) = v.get("theme").and_then(Value::as_str).and_then(theme::ThemePref::parse) {
            self.ui.theme = t;
        }
        if let Some(l) = v.get("interfaceLanguage").and_then(Value::as_str).and_then(i18n::Preference::parse) {
            self.ui.interface_language = l;
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), now_ms()));
    }

    pub fn open_path(&mut self, path: &str) {
        let r = self.run("open", json!({ "path": path }));
        self.opened(r);
    }

    /// The largest file [`Self::open_bytes`] opens. A drawing opened from its contents (the
    /// browser, where files have no path) is held in memory several times over while it is decoded,
    /// so it is capped below the DWG reader's own limit instead of running out of memory.
    pub const MAX_OPEN_BYTES: u64 = 256 << 20;

    /// Why a `len`-byte file called `name` is too large for [`Self::open_bytes`], if it is.
    pub fn open_size_error(name: &str, len: u64) -> Option<String> {
        (len > Self::MAX_OPEN_BYTES).then(|| {
            format!(
                "Open: {name} is {:.1} MB, larger than the {} MB CADCraft opens in the browser",
                len as f64 / 1048576.0,
                Self::MAX_OPEN_BYTES >> 20
            )
        })
    }

    /// Open a drawing from its contents where there is no path to open (on the web: the file
    /// picker, a dropped file). Runs OPEN with `{data, name}` as [`Self::open_path`] runs it with
    /// `{path}`; failures show on the command line and in the status bar.
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) {
        if let Some(e) = Self::open_size_error(name, bytes.len() as u64) {
            return self.open_failed(e);
        }
        let data = cadcraft_engine::cmd::file::base64_encode(bytes);
        let r = self.run("open", json!({ "data": data, "name": name }));
        self.opened(r);
    }

    /// Report a file that could not be opened before OPEN ran (unreadable, too large) as a failed
    /// OPEN is reported: on the command line and in the status bar.
    pub fn open_failed(&mut self, e: impl Into<String>) {
        let e = e.into();
        self.session.echo(e.clone());
        self.set_status(e);
    }

    fn opened(&mut self, r: Result<Value, String>) {
        if let Err(e) = r {
            self.set_status(e);
        } else {
            self.ui.start_tab = false;
            self.canvas.zoom_pending = true;
        }
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        // Closing the window (title bar, File ▸ Exit, Cmd+Q) with unsaved changes asks first.
        if ctx.input(|i| i.viewport().close_requested()) && self.request_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if !self.styled {
            theme::install_fonts(ctx);
            self.styled = true;
        }
        // Resolved every frame, so a System choice follows the OS appearance live (winit and the
        // browser report changes as input); the saved choice itself never changes here.
        let shown = self.ui.theme.resolve(ctx.system_theme());
        theme::set_active(shown);
        if self.shown_theme != Some(shown) {
            theme::apply(ctx, shown);
            self.shown_theme = Some(shown);
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
        i18n::with_language(self.language(), || self.draw(ui));
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
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
        closing::prompt(self, ui.ctx());
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

#[cfg(test)]
mod tests {
    use super::*;
    use theme::{ThemePref, Tokens};

    fn app() -> CadApp {
        CadApp::new(Session::empty(), Services::default())
    }

    /// One frame of `logic` with the OS reporting `system` as its appearance.
    fn frame(app: &mut CadApp, ctx: &egui::Context, system: Option<egui::Theme>) {
        let raw = egui::RawInput { system_theme: system, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
    }

    #[test]
    fn theme_choice_persists_and_drives_tokens() {
        let mut a = app();
        assert_eq!(a.ui.theme, ThemePref::Dark, "default keeps today's look");
        assert!(a.run("ui.theme", json!({ "theme": "Purple" })).is_err());
        assert_eq!(a.run("ui.theme", json!({ "theme": "light" })).ok(), Some(json!({ "theme": "light", "shown": "light" })));
        // Saved and restored by the host.
        let mut b = app();
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.theme, ThemePref::Light);
        b.load_prefs("not json");
        b.load_prefs(r#"{"theme": "sepia"}"#);
        assert_eq!(b.ui.theme, ThemePref::Light, "bad prefs are ignored");
        // A fixed choice ignores the OS; the tokens and egui follow the choice.
        let ctx = egui::Context::default();
        frame(&mut b, &ctx, Some(egui::Theme::Dark));
        assert_eq!(Tokens::get(), Tokens::LIGHT);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        // The menu items reach the same setting; so does `ui.set` (the UiState JSON).
        b.start("ui.theme.dark");
        frame(&mut b, &ctx, Some(egui::Theme::Light));
        assert_eq!(Tokens::get(), Tokens::DARK);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let ui: UiState = serde_json::from_value(json!({ "theme": "system" })).unwrap_or_default();
        assert_eq!(ui.theme, ThemePref::System);
    }

    #[test]
    fn system_theme_follows_the_os_live_and_stays_saved_as_system() {
        let mut a = app();
        a.start("ui.theme.system");
        let ctx = egui::Context::default();
        frame(&mut a, &ctx, Some(egui::Theme::Light));
        assert_eq!(Tokens::get(), Tokens::LIGHT);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        frame(&mut a, &ctx, Some(egui::Theme::Dark));
        assert_eq!(Tokens::get(), Tokens::DARK);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        frame(&mut a, &ctx, None);
        assert_eq!(Tokens::get(), Tokens::of(theme::SYSTEM_FALLBACK), "no OS appearance: the documented fallback");
        assert_eq!(a.ui.theme, ThemePref::System);
        assert_eq!(a.prefs_json(), r#"{"interfaceLanguage":"auto","theme":"system"}"#);
    }
}
