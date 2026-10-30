//! The CADCraft egui front end.
//!
//! A thin shell over [`cadcraft_engine::Session`]: panels read engine state and act through
//! commands (`app.run(id, params)`) or the command line (`app.cmdline(text)`). Nothing here owns
//! drawing data. The layout follows the familiar CAD desktop: title and tool bar, file tabs,
//! Tool Sets on the left, the drawing area with its command line, Layers and Properties on the
//! right, layout tabs and drafting toggles in the status bar.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// CADCraft as craft-update knows it, with its 128 px icon and accent colour: the update
/// prompt shows both, and an AppImage's menu entry gets the icon.
#[cfg(not(target_arch = "wasm32"))]
pub const UPDATE_APP: craft_update::app::App = craft_update::app::App {
    icon_png: Some(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.cadcraft.png")),
    accent: Some([0x3e, 0xa6, 0xe8]),
    ..craft_update::suite::CADCRAFT
};

pub mod aliasdlg;
pub mod animation;
pub mod autosave;
pub mod bedit;
pub mod canvas;
pub mod capture;
pub mod chrome;
pub mod clipboard;
pub mod closing;
pub mod cmdline;
pub mod context_menu;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod docking;
pub mod hatchdlg;
mod mtext_editor;
mod panel_state;
pub mod spacemouse;
pub mod switch;
// The External References palette.
pub mod xrefs;

pub mod dyngrips;
pub mod dyninput;
pub mod gpu;
pub mod grip_dyn;
pub mod i18n;
pub mod icons;
pub mod layers;
pub mod managers;
pub mod menus;
pub mod modal;
pub mod opening;
pub mod options;
pub mod palettes;
pub mod parametric;
pub mod plotstyles;
pub mod quick;
pub mod shortcut_menus;
pub mod theme;
#[cfg(not(target_arch = "wasm32"))]
pub mod updates_ui;
pub mod viewcube;
pub mod vpscale;

pub mod exchange;

use std::path::Path;
use std::sync::mpsc::Receiver;

use cadcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse, PendingReply};

/// The host storage key for [`CadApp::prefs_json`].
pub const PREFS_KEY: &str = "cadcraft.prefs";

/// How many recently opened drawings are kept (AutoCAD's default for its file history).
pub const RECENT_FILES: usize = 9;

/// Persisted UI state (serde, so automation can read and set it).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub docking: docking::Workspace,

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
    /// when it is hidden. Off unless the host defaults to it ([`CadApp::set_system_cursor_default`]).
    pub system_cursor: bool,
    /// "Drafting" or "Modeling".
    pub toolset_tab: String,
    pub collapsed_groups: Vec<String>,
    /// Tools the user took out of the Tool Sets palette (command ids); each category's gear
    /// menu puts them back.
    pub hidden_tools: Vec<String>,
    pub properties_all: bool,
    /// Show an in-window menu bar (platforms without a native menu bar, or on request).
    pub in_window_menu: bool,
    pub start_tab: bool,
    pub dialog: Option<String>,
    pub history_lines: usize,
    /// Interface theme: "system" (follow the OS light/dark appearance), "light" or "dark".
    pub theme: theme::ThemePref,
    /// Animation effects (dialog and menu fades, smoothed scrolling): "system" (follow the OS
    /// setting), "on" or "off".
    pub animations: animation::AnimationPref,
    /// The format Save As suggests: "dxf" or "dwg", for new drawings and opened files alike.
    pub save_format: SaveFormat,
    /// The version Save As writes for a drawing that has no file yet ("2000" … "2018"); a
    /// drawing opened from a file keeps that file's version.
    pub save_version: String,
    /// Drawings opened from a path, newest first (File ▸ Open Recent and the Start page).
    pub recent_files: Vec<String>,
    /// The folder the last APPLOAD file came from: the next pick starts there.
    pub last_lisp_dir: Option<String>,
}

/// A DXF/DWG file version (2000 … 2018).
pub use cadcraft_engine::cmd::file::Version as SaveVersion;

/// The default save format, like AutoCAD's Options > Open and Save > "Save as": the format Save As
/// suggests for every drawing, an opened DWG or DXF too. Save (QSAVE) of an opened file keeps its file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveFormat {
    /// CADCraft's native format, and the default.
    #[default]
    Dxf,
    Dwg,
}

impl SaveFormat {
    pub const ALL: [SaveFormat; 2] = [SaveFormat::Dxf, SaveFormat::Dwg];

    /// The file extension, which is also the saved name ("dxf" or "dwg").
    pub fn as_str(self) -> &'static str {
        match self {
            SaveFormat::Dxf => "dxf",
            SaveFormat::Dwg => "dwg",
        }
    }

    pub fn parse(s: &str) -> Option<SaveFormat> {
        SaveFormat::ALL.into_iter().find(|f| f.as_str().eq_ignore_ascii_case(s.trim().trim_start_matches('.')))
    }

    /// The name Save As suggests: this format's extension, replacing a drawing's own (DWG, DXF or
    /// DWT, from a drawing opened from a file); any other extension is kept.
    pub fn suggested_name(self, title: Option<&str>) -> String {
        let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) else { return format!("Drawing.{}", self.as_str()) };
        let path = std::path::Path::new(t);
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if ["dwg", "dxf", "dwt"].iter().any(|d| d.eq_ignore_ascii_case(e)) => {
                // the extension is ASCII, so this is a char boundary
                let stem = t.get(..t.len().saturating_sub(e.len() + 1)).unwrap_or(t);
                format!("{stem}.{}", self.as_str())
            }
            Some(_) => t.to_owned(),
            None => format!("{t}.{}", self.as_str()),
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            docking: docking::default_workspace(),

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
            system_cursor: false,
            toolset_tab: "Drafting".into(),
            collapsed_groups: Vec::new(),
            hidden_tools: Vec::new(),
            properties_all: true,
            in_window_menu: !cfg!(target_os = "macos"),
            start_tab: false,
            dialog: None,
            history_lines: 3,
            theme: theme::ThemePref::default(),
            animations: animation::AnimationPref::default(),
            save_format: SaveFormat::default(),
            save_version: SaveVersion::default().year().to_string(),
            recent_files: Vec::new(),
            last_lisp_dir: None,
        }
    }
}

/// Host services (file pickers, clipboard) injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// Pick a `.lsp` file for APPLOAD, starting in the given folder (empty for the default one).
    /// `None` where there is no native file dialog: the desktop without one asks for the name,
    /// and the web (no file system) says to drop the file (`no_file_system`).
    pub pick_lisp: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// The host has no file system to read a path from (the browser): a dropped file's contents
    /// are what it loads, and APPLOAD says so instead of asking for a name it cannot read.
    pub no_file_system: bool,
}

pub struct CadApp {
    pub system_languages: Vec<String>,
    /// Whether the operating system shows animations, as the host reads it; `None` when the OS
    /// states nothing or the host cannot ask ([`animation`]).
    pub system_animations: Option<bool>,
    /// What `ui.system_cursor` goes back to when the interface is reset ([`Self::set_system_cursor_default`]).
    pub system_cursor_default: bool,
    pub session: Session,
    pub ui: UiState,
    /// The interface's own preferences from the Options dialog ([`options`]).
    pub options: options::UiOptions,
    pub services: Services,
    pub canvas: canvas::CanvasState,
    /// Desktop HID status and navigation preferences; the web host supplies no HID backend.
    pub spacemouse: spacemouse::State,
    pub cmd: cmdline::CmdLine,
    pub status: Option<(String, f64)>,
    /// Drawings being read on worker threads ([`opening`]).
    pub opening: opening::Opening,
    pub integrated_titlebar: bool,
    control_rx: Option<Receiver<ControlRequest>>,
    /// Fresh surface and identity snapshot (see [`capture`]).
    pub(crate) capture_geometry: Option<capture::CaptureGeometry>,
    /// Monotonic surface epoch, bumped whenever the window/viewport geometry changes.
    pub(crate) capture_epoch: u64,
    /// Monotonic identity epoch, bumped whenever the drawing or focused widget changes (even
    /// when a later change returns to the same value).
    pub(crate) capture_identity_epoch: u64,
    /// Admitted `ui.screenshot` requests, including completed-but-undrained ones.
    pub(crate) shots: capture::Shots,
    pub synthetic: Vec<egui::Event>,
    /// Keys, text and clipboard events a floating panel's own OS window left over, with the
    /// modifiers held there, for the main window's next input ([`Self::forward_native_input`]).
    forwarded: Vec<egui::Event>,
    forwarded_modifiers: Option<egui::Modifiers>,
    /// The main window's own modifiers, put back the frame after forwarded keys went in.
    restore_modifiers: Option<egui::Modifiers>,
    /// A panel opened from a menu or command, whose floating window comes to the front on the
    /// next frame (craft-ui's `DockArea::raise_panel`).
    pub raise_panel: Option<docking::Panel>,
    /// One synthetic batch has been put into `RawInput` and is waiting for a real `App::ui` call.
    synthetic_injected: bool,
    /// Counts settled UI passes (one per painted frame; never hidden logic-only ticks or
    /// discarded passes).
    pub ui_frames: u64,
    /// Caps the frame rate when presenting without vsync (see [`gpu::FrameCap`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub frame_cap: Option<gpu::FrameCap>,
    /// Language whose interface fonts are installed (`None` until the first frame).
    fonts_language: Option<&'static str>,
    /// Language last given to egui (and so to the AccessKit root; `None` until the first frame).
    shown_language: Option<&'static str>,
    /// The theme last installed into egui (`None` until the first frame).
    shown_theme: Option<egui::Theme>,
    /// Whether the last applied theme was handed to egui as the System preference.
    shown_follows_system: bool,
    /// Whether egui was last told to animate (`None` until the first frame).
    shown_animations: Option<bool>,
    /// The interface scale last handed to egui (`None` until the first frame).
    shown_interface_scale: Option<f32>,
    pub frame_ms: f64,
    pub quit_requested: bool,
    /// The app may close although drawings have unsaved changes (they were dealt with, or the
    /// quit came from the control channel, which never asks).
    pub quit_confirmed: bool,
    /// A close request waiting for "Save changes?" answers ([`closing`]).
    pub closing: Option<closing::Closing>,
    /// Autosave (desktop; `None` until [`Self::enable_autosave`]).
    pub autosave: Option<cadcraft_engine::autosave::Autosaver>,
    /// Drawing Recovery: autosave files a crashed app left behind.
    pub recovery: autosave::Recovery,
    /// GPU errors and device loss reported by wgpu ([`gpu::GpuFaults`]).
    pub gpu_faults: std::sync::Arc<gpu::GpuFaults>,
    /// The notice shown after one (see [`autosave::gpu_notice`]).
    pub gpu_notice: Option<autosave::GpuNotice>,
    /// The Command Aliases dialog while open ([`aliasdlg`]).
    pub alias_dlg: Option<aliasdlg::AliasDlgState>,
    /// The Hatch and Gradient dialog while open or picking ([`hatchdlg`]).
    pub hatch_dlg: Option<hatchdlg::HatchDlgState>,
    /// The Edit Block Definition dialog ([`bedit`]).
    pub bedit: bedit::State,
    /// The startup update check and its prompt (desktop; [`updates_ui`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub updates_rt: updates_ui::Runtime,
}

impl CadApp {
    /// Construct `CadApp` from a session and services with default UI state, options, canvas and command line; no control channel, capture geometry or pending shots are installed.
    pub fn new(session: Session, services: Services) -> Self {
        CadApp {
            system_languages: Vec::new(),
            system_animations: None,
            system_cursor_default: false,
            session,
            ui: UiState::default(),
            options: options::UiOptions::default(),
            services,
            canvas: canvas::CanvasState::default(),
            spacemouse: spacemouse::State::default(),
            cmd: cmdline::CmdLine::default(),
            status: None,
            opening: opening::Opening::default(),
            integrated_titlebar: false,
            control_rx: None,
            capture_geometry: None,
            capture_epoch: 0,
            capture_identity_epoch: 0,
            shots: capture::Shots::default(),
            synthetic: Vec::new(),
            forwarded: Vec::new(),
            forwarded_modifiers: None,
            restore_modifiers: None,
            raise_panel: None,
            synthetic_injected: false,
            ui_frames: 0,
            #[cfg(not(target_arch = "wasm32"))]
            frame_cap: None,
            fonts_language: None,
            shown_language: None,
            shown_theme: None,
            shown_follows_system: false,
            shown_animations: None,
            shown_interface_scale: None,
            frame_ms: 0.0,
            quit_requested: false,
            quit_confirmed: false,
            closing: None,
            autosave: None,
            recovery: autosave::Recovery::default(),
            gpu_faults: std::sync::Arc::default(),
            gpu_notice: None,
            alias_dlg: None,
            hatch_dlg: None,
            bedit: bedit::State::default(),
            #[cfg(not(target_arch = "wasm32"))]
            updates_rt: updates_ui::Runtime::default(),
        }
    }

    /// The interface language shown: the chosen one, or English when this computer has no font
    /// for it (Chinese or Korean without a suitable font would be boxes).
    pub fn language(&self) -> &'static str {
        let l = self.ui.interface_language.resolve(&self.system_languages);
        if theme::can_show(l) { l } else { "en" }
    }

    /// Draw the canvas on the GPU with the app's wgpu render state (eframe's
    /// `CreationContext::wgpu_render_state`). Without it the canvas draws on the CPU. `stencil`:
    /// the format of the render pass's stencil buffer when the app asked for one (eframe
    /// `NativeOptions::stencil_buffer`); clipped viewports are clipped with it.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState, stencil: Option<egui_wgpu::wgpu::TextureFormat>) {
        self.canvas.gpu = Some(gpu::install(rs, &self.gpu_faults, stencil));
        self.canvas.gpu_device = Some(rs.device.clone());
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

    /// Submit a line of command-line text. A refused line is echoed to the history; the error is
    /// also returned.
    pub fn cmdline(&mut self, text: &str) -> Option<String> {
        if !self.session.prompting() {
            let first = text.split_whitespace().next().unwrap_or("");
            // `cmd {json}` is a programmatic call: the engine runs it with those parameters.
            let json_form = text.trim_start().get(first.len()..).is_some_and(|rest| rest.trim_start().starts_with('{'));
            if !first.is_empty() && !json_form && menus::run_ui_command(self, &first.to_ascii_lowercase(), &Value::Null).is_some() {
                return None;
            }
        }
        let e = self.session.cmdline(text).err()?.to_string();
        self.session.echo(e.clone());
        Some(e)
    }

    /// The theme the interface shows now (the choice resolved against the OS appearance).
    pub fn shown_theme(&self) -> egui::Theme {
        self.shown_theme.unwrap_or_else(|| self.ui.theme.resolve(None))
    }

    /// Whether the interface shows animation effects now (the choice resolved against the OS
    /// setting).
    pub fn shows_animations(&self) -> bool {
        self.ui.animations.resolve(self.system_animations)
    }

    /// Preferences kept across restarts, as JSON for the host's storage ([`PREFS_KEY`]).
    pub fn prefs_json(&self) -> String {
        let mut prefs = json!({
            "theme": self.ui.theme.as_str(),
            "animations": self.ui.animations.as_str(),
            "interfaceLanguage": self.ui.interface_language.code(),
            "saveFormat": self.ui.save_format.as_str(),
            "saveVersion": self.ui.save_version,
        });
        if self.spacemouse.settings != spacemouse::Settings::default() {
            prefs["spacemouse"] = json!(self.spacemouse.settings);
        }
        if !self.ui.recent_files.is_empty() {
            prefs["recentFiles"] = json!(self.ui.recent_files);
        }
        if let Some(dir) = &self.ui.last_lisp_dir {
            prefs["lispDir"] = json!(dir);
        }
        prefs.to_string()
    }

    /// Restore preferences saved by [`Self::prefs_json`]; unknown or malformed values are ignored.
    pub fn load_prefs(&mut self, json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(json) else { return };
        if let Some(prefs) = v.get("spacemouse") {
            let _ = self.spacemouse.load_settings(prefs);
        }
        if let Some(t) = v.get("theme").and_then(Value::as_str).and_then(theme::ThemePref::parse) {
            self.ui.theme = t;
        }
        if let Some(a) = v.get("animations").and_then(Value::as_str).and_then(animation::AnimationPref::parse) {
            self.ui.animations = a;
        }
        if let Some(l) = v.get("interfaceLanguage").and_then(Value::as_str).and_then(i18n::Preference::parse) {
            self.ui.interface_language = l;
        }
        if let Some(f) = v.get("saveFormat").and_then(Value::as_str).and_then(SaveFormat::parse) {
            self.ui.save_format = f;
        }
        if let Some(v) = v.get("saveVersion").and_then(Value::as_str).and_then(SaveVersion::parse) {
            self.ui.save_version = v.year().to_string();
        }
        if let Some(files) = v.get("recentFiles").and_then(Value::as_array) {
            self.ui.recent_files = files.iter().filter_map(Value::as_str).filter(|p| !p.is_empty()).take(RECENT_FILES).map(str::to_string).collect();
        }
        if let Some(dir) = v.get("lispDir").and_then(Value::as_str).filter(|d| !d.is_empty()) {
            self.ui.last_lisp_dir = Some(dir.to_string());
        }
    }

    /// Make the system cursor the interface default (`on`) and turn it on; saved preferences and
    /// resets never turn it off then.
    pub fn set_system_cursor_default(&mut self, on: bool) {
        self.system_cursor_default = on;
        if on {
            self.ui.system_cursor = true;
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), now_ms()));
    }

    /// Cancel queued/in-flight screenshots (host exit). Late callbacks write nothing.
    pub fn cancel_screenshots(&mut self) {
        capture::cancel_all(self);
    }

    /// Open the drawing at `path`. Natively the file is read on a worker thread while an
    /// "Opening ..." card shows ([`opening`]); [`Self::finish_opening`] adds it when ready. On the
    /// web it opens at once.
    pub fn open_path(&mut self, path: &Path) {
        // A LISP file (dropped on the window): loaded as APPLOAD loads it.
        if is_lisp_file(path) {
            match cadcraft_engine::lisp::builtins::read_text_file(path) {
                Some(text) => self.load_lisp(&path.to_string_lossy(), &text),
                None => self.open_failed(format!("APPLOAD: can't read {}", path.display())),
            }
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.opening.start(path);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let r = match path.to_str() {
                Some(text) => self.run("open", json!({ "path": text })),
                None => Err(format!("open: {}: the path is not valid Unicode", path.display())),
            };
            self.opened(r);
        }
    }

    /// Add the files whose background read has finished; report the ones that failed as a failed
    /// OPEN is reported. Called every frame.
    pub fn finish_opening(&mut self) {
        for (_, outcome) in self.opening.poll() {
            let r = match outcome {
                Ok(f) => self.session.finish_open(f).map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if let Err(e) = &r {
                self.session.echo(e.clone());
            }
            self.opened(r);
        }
    }

    /// Load a LISP file: its routine runs and may wait for the user (APPLOAD typed or from the
    /// menu runs this way too).
    fn load_lisp(&mut self, name: &str, text: &str) {
        if let Err(e) = self.session.lisp_load(name, text, false) {
            self.session.echo(e.to_string());
        }
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
        if is_lisp_file(name) {
            return self.load_lisp(name, &String::from_utf8_lossy(bytes));
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
        match r {
            Err(e) => self.set_status(e),
            Ok(v) => {
                self.ui.start_tab = false;
                self.canvas.zoom_pending = true;
                let doc = v.get("index").and_then(Value::as_u64).and_then(|i| self.session.docs.get(usize::try_from(i).ok()?));
                if let Some(path) = doc.and_then(|d| d.path.clone()) {
                    self.remember_recent(path);
                }
                // A partial read (DWG records left out) shows in the status bar too, not only on
                // the command line.
                if let Some(w) = v.get("warnings").and_then(Value::as_array).and_then(|w| w.first()).and_then(Value::as_str) {
                    self.set_status(w.to_string());
                }
                // A dynamic block's own drawing: ask whether to open it in the Block Editor.
                if self.session.state().is_ok_and(|st| cadcraft_engine::blockedit::has_authoring_elements(&st.doc)) {
                    self.ui.dialog = Some("authoring".into());
                }
            }
        }
    }

    /// Put `path` first in File ▸ Open Recent and on the Start page.
    fn remember_recent(&mut self, path: String) {
        let recent = &mut self.ui.recent_files;
        recent.retain(|p| *p != path);
        recent.insert(0, path);
        recent.truncate(RECENT_FILES);
    }

    /// Consume asynchronous work on every logic callback, including hidden `run_logic` ticks.
    ///
    /// Consume-based queues and screenshot settlement run on **every** call, whichever pass
    /// delivers it. Frame work (LISP progress, autosave, close requests, screenshot issuance
    /// pacing, hidden repaints) runs once per host frame: eframe calls `App::logic` inside every
    /// pass of a multi-pass frame, and also on hidden logic-only ticks where no pass runs and the
    /// pass index is zero. `current_pass_index() == 0` is therefore the frame guard: true on the
    /// first pass and every hidden tick, false on later passes.
    pub fn logic(&mut self, ctx: &egui::Context) {
        // Consume-based queues: each item is handled once, whichever callback delivers it. A
        // reply arriving during a later pass is never starved until the next frame.
        self.drain_control(ctx);
        self.finish_opening();
        // Finishing an open switches to the new drawing.
        capture::note_identity(self, ctx);
        capture::drain(self, ctx);
        self.gpu_fault_tick(ctx);
        managers::plot_window_resume(self, ctx);
        hatchdlg::resume(self);
        // What follows is frame work, once per layout frame or hidden tick.
        if ctx.current_pass_index() != 0 {
            return;
        }
        if self.restore_modifiers.is_some() {
            ctx.request_repaint();
        }
        // Closing the window with unsaved changes asks first. `run_logic` refreshes viewport
        // close/focus state, so this must also work while hidden.
        if ctx.input(|i| i.viewport().close_requested()) && self.request_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // A long AutoLISP routine runs one bounded slice per frame, including hidden web/native
        // ticks; it must not depend on a paint or `RequestPaintWhileHidden`.
        self.session.lisp_tick();
        // Time-based and idempotent by its own deadline; keeps autosave alive while hidden.
        self.autosave_tick(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::tick(self, ctx);
        if !self.synthetic.is_empty() || self.synthetic_injected {
            request_paint_while_hidden(ctx);
        }
        if self.session.lisp_busy() || self.opening.busy() {
            request_paint_while_hidden(ctx);
        }
        capture::request_hidden_paint(self, ctx);
    }

    /// Queue keys typed in a floating panel's own OS window (craft-ui's
    /// `DockOutput::forwarded_events` and `forwarded_modifiers`) for the main window's next
    /// input. egui gives each OS window only its own input, and the shortcuts, the command line
    /// and the canvas all read the main window's, so the keys go in there as if typed in it.
    pub fn forward_native_input(&mut self, ctx: &egui::Context, events: Vec<egui::Event>, modifiers: Option<egui::Modifiers>) {
        if events.is_empty() {
            return;
        }
        self.forwarded.extend(events);
        if modifiers.is_some() {
            self.forwarded_modifiers = modifiers;
        }
        if self.restore_modifiers.is_none() {
            self.restore_modifiers = Some(ctx.input(|i| i.modifiers));
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    /// Whether a modal dialog is up: the "Save changes?" prompt or Drawing Recovery. The
    /// floating panels' own OS windows take no input meanwhile (egui's `Modal` blocks only the
    /// main window). The app's other dialogs are ordinary windows that leave the drawing usable.
    pub fn shows_modal(&self) -> bool {
        self.close_prompt().is_some() || !self.recovery.items.is_empty()
    }

    /// Input hook: refresh the raw surface snapshot, put in keys forwarded from floating panels'
    /// OS windows, and release at most one synthetic batch.
    ///
    /// Eframe keeps the unused `RawInput` across hidden logic-only ticks, so a batch already in
    /// the buffer is never injected again; the next batch waits for a real `ui` pass to consume
    /// the previous one.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        capture::note_raw_geometry(self, ctx, raw);
        // The forwarded window's modifiers hold for the frame its keys go in (Shift+Copy is
        // COPYBASE); the next frame puts the main window's back, before its own changes.
        if !self.forwarded.is_empty() {
            // A modifier the main window changed meanwhile is the one to put back.
            if let Some(own) = raw.events.iter().rev().find_map(|event| match event {
                egui::Event::ModifiersChanged(modifiers) => Some(*modifiers),
                _ => None,
            }) {
                self.restore_modifiers = Some(own);
            }
            if let Some(modifiers) = self.forwarded_modifiers.take() {
                raw.events.push(egui::Event::ModifiersChanged(modifiers));
            }
            raw.events.append(&mut self.forwarded);
        } else if let Some(modifiers) = self.restore_modifiers.take() {
            raw.events.insert(0, egui::Event::ModifiersChanged(modifiers));
        }
        if self.synthetic_injected || self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        // A synthetic click holds its Shift/Ctrl (Shift+click a grip); the next move lets go.
        match self.synthetic.first() {
            Some(egui::Event::PointerButton { modifiers, .. }) => raw.events.push(egui::Event::ModifiersChanged(*modifiers)),
            Some(egui::Event::PointerMoved(_)) => raw.events.push(egui::Event::ModifiersChanged(egui::Modifiers::default())),
            _ => {}
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
        self.synthetic_injected = true;
    }

    /// Lay out the whole window. This is the only place with a real UI pass.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let first_pass = ctx.current_pass_index() == 0;
        if first_pass {
            self.ui_frame_begin(ui);
        } else {
            // A second pass must render the state the first pass produced. egui's raw input is
            // empty there, but the smoothed scroll delta can persist; never pan/zoom twice.
            ctx.input_mut(|input| {
                input.smooth_scroll_delta = egui::Vec2::ZERO;
                input.raw.events.retain(|event| !matches!(event, egui::Event::MouseWheel { .. } | egui::Event::Zoom(_)));
            });
        }
        // The settled geometry and ppp of this pass, before drawing and after the frame's saved
        // interface zoom was applied (egui applies a new zoom at the start of a pass).
        capture::note_pass_geometry(self, &ctx);
        i18n::with_language(self.language(), || self.draw(ui));
        if first_pass {
            // The batch reached an actual Ui callback; a discarded pass still consumed it.
            self.synthetic_injected = false;
        }
        // `will_discard()` is false on the settled (final) pass of a frame, whether the frame is
        // single-pass or the second pass after a layout discard; a discarded pass is skipped.
        if !ctx.will_discard() {
            self.ui_frames = self.ui_frames.saturating_add(1);
            capture::issue(self, &ctx);
        }
    }

    /// Frame-only work, run from the first real UI pass (never from a hidden logic tick).
    fn ui_frame_begin(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let language = self.language();
        if self.fonts_language != Some(language) {
            theme::install_fonts(ctx, language);
            self.fonts_language = Some(language);
        }
        // The AccessKit root carries the language the interface is actually shown in; egui copies
        // it onto the root node at the end of the pass.
        if self.shown_language != Some(language) {
            ctx.set_language(Some(language.to_string()));
            self.shown_language = Some(language);
        }
        // Resolved every frame, so a System choice follows the OS appearance live (winit and the
        // browser report changes as input); the saved choice itself never changes here.
        let shown = self.ui.theme.resolve(ctx.system_theme());
        theme::set_active(shown);
        theme::set_canvas(self.options.canvas());
        let follows_system = self.ui.theme == theme::ThemePref::System;
        if self.shown_theme != Some(shown) || self.shown_follows_system != follows_system {
            theme::apply(ctx, shown, follows_system);
            self.shown_theme = Some(shown);
            self.shown_follows_system = follows_system;
        }
        // Resolved every frame too, so a System choice follows the OS setting live.
        let animate = self.shows_animations();
        if self.shown_animations != Some(animate) {
            animation::apply(ctx, animate);
            self.shown_animations = Some(animate);
        }
        if !animate {
            animation::unsmooth_scroll(ctx);
        }
        // Applied only when the saved scale changes, so egui's own keyboard zoom keeps working
        // within a session (and is never saved).
        let scale = self.options.interface_scale;
        if self.shown_interface_scale != Some(scale) {
            ctx.set_zoom_factor(scale);
            self.shown_interface_scale = Some(scale);
        }
        // Wait for the frame cap on a painted pass, where a wait cannot delay a hidden tick.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(cap) = &mut self.frame_cap {
            cap.wait();
        }
        panel_state::migrate(ui);
        self.ui_input(ctx);
    }

    /// Input-derived work, once per layout frame.
    fn ui_input(&mut self, ctx: &egui::Context) {
        menus::shortcuts(self, ctx);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dropped = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
            for file in dropped {
                let path = file.path();
                if !path.as_os_str().is_empty() {
                    self.open_path(path);
                }
            }
        }
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Draw one UI pass: title/toolbar, optional menu/status bar/file tabs, the standalone tool-set strip when a drawing is shown but Tool Sets is not docked, docking or the start page, and the floating overlays; record `frame_ms`. Runs on every pass, including one egui later discards.
    fn draw(&mut self, ui: &mut egui::Ui) {
        let t0 = now_ms();
        control::begin_frame(ui.ctx());
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
        if has_doc && !self.ui.show_toolsets {
            palettes::toolsets_strip(self, ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.canvas)).show(ui, |ui| {
            if has_doc {
                docking::show(self, ui);
            } else {
                chrome::start_page(self, ui);
            }
        });
        self.opening.show(ui.ctx());
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::show(self, ui.ctx());
        dialogs::show(self, ui.ctx());
        palettes::patterns::prompt_picker(self, ui.ctx());
        closing::prompt(self, ui.ctx());
        autosave::show(self, ui.ctx());
        autosave::gpu_notice(self, ui.ctx());
        self.frame_ms = now_ms() - t0;
    }

    /// Drain each queued control request at most once (timed-out requests are skipped), dispatch it to `control::handle` or `capture::admit`, note the resulting capture identity and request a repaint.
    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            // Timed out while queued: the client was told it did not run (#345).
            if !req.begin() {
                continue;
            }
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => capture::admit(self, ctx, path, reply),
            }
            // A command can switch drawings or move the focus; a screenshot queued before it must
            // see that transition even if a later command returns to the same drawing.
            capture::note_identity(self, ctx);
            ctx.request_repaint();
        }
        self.control_rx = Some(rx);
    }
}

/// Ask for a painted frame while no frame is being painted. Native eframe honours the viewport
/// command; the pinned web runner logs it as unhandled, so web only schedules a repaint (hidden
/// work still advances from `logic`).
fn request_paint_while_hidden(ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
    ctx.request_repaint();
}

/// Monotonic milliseconds since the first call, on native and web (`performance.now` on wasm).
/// Durations (screenshot deadlines, status expiry, frame timings) must not follow the wall clock.
pub fn now_ms() -> f64 {
    static START: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(web_time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// A LISP source file, by its name.
fn is_lisp_file(name: impl AsRef<Path>) -> bool {
    name.as_ref().extension().is_some_and(|e| e.eq_ignore_ascii_case("lsp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use theme::{ThemePref, Tokens};

    fn app() -> CadApp {
        CadApp::new(Session::empty(), Services::default())
    }

    /// One real UI frame, with the OS reporting `system` as its appearance.
    fn frame(app: &mut CadApp, ctx: &egui::Context, system: Option<egui::Theme>) {
        let raw = egui::RawInput { system_theme: system, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    /// One real UI frame whose first pass asks egui for a second layout pass, as a popup or a
    /// changed layout does.
    fn two_pass_frame(app: &mut CadApp, ctx: &egui::Context) {
        let mut discard = true;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx());
            if std::mem::take(&mut discard) {
                ui.ctx().request_discard("test: force a second pass");
            }
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    /// Eframe runs `App::logic` inside every pass of a multi-pass frame. Frame work (here one
    /// AutoLISP slice) belongs to the frame, not to a pass.
    #[test]
    fn a_two_pass_frame_runs_frame_logic_once() {
        use cadcraft_engine::lisp::value::Value;

        let routine = "(setq cc-two 0) (while (< cc-two 400000) (setq cc-two (1+ cc-two)))";
        let start = |app: &mut CadApp| {
            let started = app.session.lisp_eval(routine, false, false, Vec::new()).expect("the routine starts");
            assert!(started.is_none(), "a non-blocking routine yields before finishing");
            assert!(app.session.lisp_busy());
        };
        let mut single = CadApp::new(Session::new(), Services::default());
        let mut twice = CadApp::new(Session::new(), Services::default());
        start(&mut single);
        start(&mut twice);
        let ctx = egui::Context::default();
        frame(&mut single, &ctx, None);
        two_pass_frame(&mut twice, &ctx);
        let counter = |app: &CadApp| match app.session.lisp_var("CC-TWO") {
            Value::Int(n) => n,
            other => panic!("CC-TWO is {other:?}, not an integer"),
        };
        assert!(counter(&single) > 0, "the frame ran the first slice");
        assert_eq!(counter(&twice), counter(&single), "the second pass did not run a second slice");
        assert_eq!(twice.session.lisp_busy(), single.session.lisp_busy(), "the same amount of the routine ran");
    }

    /// A control request is consumed once per frame, not once per pass.
    #[test]
    fn a_two_pass_frame_executes_a_control_request_once() {
        use std::time::Duration;

        use crate::control::ControlRequest;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = CadApp::new(Session::new(), Services::default()).with_control(rx);
        let (request, reply) = ControlRequest::new("engine.execute", json!({ "command": "line", "params": { "points": [[0.0, 0.0], [10.0, 0.0]] } }));
        tx.send(request).expect("the request is queued");
        let ctx = egui::Context::default();
        two_pass_frame(&mut app, &ctx);
        let entities = app.session.doc().map(cadcraft_doc::Drawing::entity_count).unwrap_or_default();
        assert_eq!(entities, 1, "one command ran once, in the first pass");
        let reply = reply.wait(Duration::from_secs(1));
        assert_eq!(reply["ok"], true, "{reply}");
    }

    /// A hidden logic-only tick runs no frame-only work: `ui_frames` stays 0 and fonts are not installed; a real `Ui` pass advances both.
    #[test]
    fn hidden_logic_does_not_run_frame_only_work() {
        let mut a = app();
        let ctx = egui::Context::default();
        // A logic-only call (what eframe runs for a hidden window) reports pass index zero too.
        a.logic(&ctx);
        assert_eq!(a.ui_frames, 0, "no real Ui callback ran");
        assert_eq!(a.fonts_language, None, "fonts are a frame-only concern");
        frame(&mut a, &ctx, None);
        assert_eq!(a.ui_frames, 1);
        assert!(a.fonts_language.is_some());
    }

    /// Repeated hidden logic-only ticks finish a long Lisp routine (`CC-HIDDEN` reaches 400000) while `ui_frames` stays 0.
    #[test]
    fn hidden_logic_advances_a_long_lisp_routine_without_a_ui_pass() {
        use cadcraft_engine::lisp::value::Value;

        let mut a = CadApp::new(Session::new(), Services::default());
        let ctx = egui::Context::default();
        let started = a
            .session
            .lisp_eval("(setq cc-hidden 0) (while (< cc-hidden 400000) (setq cc-hidden (1+ cc-hidden)))", false, false, Vec::new())
            .expect("the routine starts");
        assert!(started.is_none(), "a non-blocking routine yields before finishing");
        assert!(a.session.lisp_busy(), "the first slice reached the yield point");
        for _ in 0..2_000 {
            if !a.session.lisp_busy() {
                break;
            }
            // What eframe runs for a hidden window: logic only, no `Ui` callback.
            a.logic(&ctx);
        }
        assert!(!a.session.lisp_busy(), "hidden logic ticks carried the routine to the end");
        assert!(matches!(a.session.lisp_var("CC-HIDDEN"), Value::Int(400_000)));
        assert_eq!(a.ui_frames, 0, "no frame was painted");
    }

    /// An injected synthetic event batch is delivered by the first raw hook, a second raw hook before a real `Ui` pass adds nothing, and a new batch is accepted after that pass.
    #[test]
    fn one_synthetic_batch_waits_for_a_real_ui_pass() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.synthetic.push(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let mut first = egui::RawInput::default();
        a.raw_input_hook(&ctx, &mut first);
        assert!(first.events.iter().any(|event| matches!(event, egui::Event::Key { key: egui::Key::A, .. })));
        // A hidden tick (or a second raw hook before the UI) must not append another batch.
        let mut second = egui::RawInput::default();
        a.raw_input_hook(&ctx, &mut second);
        assert!(second.events.is_empty(), "the injected batch is still pending");
        let mut out = ctx.run_ui(first, |ui| {
            a.logic(ui.ctx());
            a.ui(ui);
        });
        out.textures_delta.clear();
        // The batch was consumed by a real Ui callback; a new synthetic event can now be injected.
        a.synthetic.push(egui::Event::Key { key: egui::Key::B, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let mut third = egui::RawInput::default();
        a.raw_input_hook(&ctx, &mut third);
        assert!(third.events.iter().any(|event| matches!(event, egui::Event::Key { key: egui::Key::B, .. })));
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
        assert_eq!(a.prefs_json(), r#"{"animations":"system","interfaceLanguage":"auto","saveFormat":"dxf","saveVersion":"2018","theme":"system"}"#);
    }

    #[test]
    fn system_theme_keeps_egui_on_the_system_preference() {
        let mut a = app();
        a.start("ui.theme.system");
        let ctx = egui::Context::default();
        frame(&mut a, &ctx, Some(egui::Theme::Light));
        // A concrete Light would pin the native window appearance and hide later OS changes.
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
        a.start("ui.theme.dark");
        frame(&mut a, &ctx, Some(egui::Theme::Light));
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::Dark);
        a.start("ui.theme.system");
        frame(&mut a, &ctx, Some(egui::Theme::Light));
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
    }

    #[test]
    fn default_save_format_persists_and_names_new_drawings() {
        let mut a = app();
        assert_eq!(a.ui.save_format, SaveFormat::Dxf, "default keeps DXF, CADCraft's native format");
        assert!(a.run("ui.saveformat", json!({ "format": "pdf" })).is_err());
        assert_eq!(a.run("ui.saveformat", json!({ "format": "DWG" })).ok(), Some(json!({ "saveFormat": "dwg" })));
        assert_eq!(a.run("ui.saveformat", json!({})).ok(), Some(json!({ "saveFormat": "dwg" })), "no format: report it");
        // Saved and restored by the host; bad values are ignored.
        let mut b = app();
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        b.load_prefs(r#"{"saveFormat": "svg"}"#);
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        // The menu items reach the same setting, and resetting the palettes keeps it.
        b.start("ui.saveformat.dxf");
        assert_eq!(b.ui.save_format, SaveFormat::Dxf);
        b.start("ui.saveformat.dwg");
        b.start("ui.resetpalettes");
        assert_eq!(b.ui.save_format, SaveFormat::Dwg);
        // Untitled drawings get the format's extension, and so do opened drawings (#2199).
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("Drawing1")), "Drawing1.dwg");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("Drawing1")), "Drawing1.dxf");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("Floorplan.dxf")), "Floorplan.dwg");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("Floorplan.DWG")), "Floorplan.dxf");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("Floorplan.dxf")), "Floorplan.dxf");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("Title block.dwt")), "Title block.dwg");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("v1.2 plan.Dxf")), "v1.2 plan.dwg");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("平面図.dxf")), "平面図.dwg");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some(".dxf")), ".dxf.dwg", "a dot file has no extension");
        assert_eq!(SaveFormat::Dxf.suggested_name(Some("logo.svg")), "logo.svg", "other files keep their type");
        assert_eq!(SaveFormat::Dwg.suggested_name(None), "Drawing.dwg");
        assert_eq!(SaveFormat::Dwg.suggested_name(Some("  ")), "Drawing.dwg");
        let ui: UiState = serde_json::from_value(json!({ "saveFormat": "dwg" })).unwrap_or_default();
        assert_eq!(ui.save_format, SaveFormat::Dwg);
    }
}
