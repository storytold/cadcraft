//! The craft-ui runtime, hosted from CADCraft's own eframe `App` (craft-libs docs/craft-framework.md, section 2.2).
//! The desktop app calls [`install`] once, [`CadApp::before_input`] in `raw_input_hook` after CADCraft's own hook,
//! [`CadApp::close_path`] last in `logic`, and [`before_ui`] first in `ui`; the browser build and tests install with
//! [`install_headless`]. The framework dialogs run on [`CadApp::framework_dialogs`] after CADCraft's own UI.
//!
//! The runtime takes Escape, Tab and the clipboard events for the whole window unless one of CADCraft's own egui
//! widgets has the keyboard. The window's Escape is read with [`escape_pressed`]; the drawing is declared to the
//! runtime with [`host_canvas`], which keeps Enter, the arrows, the edit chords and the clipboard events in egui's
//! input for the command line and CADCraft's shortcuts while nothing has the focus.

use std::sync::{Arc, OnceLock};

use craft_host::target::ShellInfo;
use craft_host::{AppInfo, Host};
use craft_keymap::Overrides;
use craft_ui::fonts::{I18nScripts, SystemFonts, UiFonts};
use craft_ui::runtime::commands::{CommandInput, CommandSet as _, EditCommands, FirePhase, FireSource, Fired, NavCommand};
use craft_ui::runtime::env::{RootEnv, Scale};
use craft_ui::runtime::focus::{EditSet, KeepKeys, KeyUse};
use craft_ui::runtime::frame::{FrameInput, FrameOutput};
use craft_ui::runtime::keys::{Intent, with_window_intents};
use craft_ui::runtime::overlay::{self, OpenFilter};
use craft_ui::shell::embed::{self, CloseDecision};
use craft_window::system::WindowSystem;
use egui::ViewportId;

use crate::CadApp;
use crate::options::INTERFACE_SCALES;

/// Each pass of the root window, first in `App::ui`: hands the runtime the native window it answers the hit test of
/// (Windows) and fits the window to its monitor once.
#[cfg(not(target_arch = "wasm32"))]
pub use craft_ui::shell::embed::before_ui;

/// The override variable's prefix: `CADCRAFT_TITLEBAR`.
#[cfg(not(target_arch = "wasm32"))]
const PREFIX: craft_window::Prefix = craft_window::Prefix::new("CADCRAFT");

/// The scripts of the interface languages, for the font stack.
static SCRIPTS: I18nScripts = I18nScripts;

/// The runtime's first font stack: egui's bundled faces and no installed system fonts. CADCraft's own definitions
/// ([`crate::theme::install_fonts`]) replace it in the first pass, before anything is drawn.
fn ui_fonts() -> UiFonts<'static> {
    UiFonts::new(&SCRIPTS).system_fonts(SystemFonts::Off)
}

fn install_with(ctx: &egui::Context, shell: ShellInfo, windows: Arc<dyn WindowSystem>, frame: Option<Decision>) -> Result<(), String> {
    let mut spec = craft_ui::InstallSpec::new(shell, windows).app_id(crate::os_window::app_id());
    if let Some(frame) = frame {
        spec = spec.frame(frame);
    }
    embed::install(ctx, spec, &ui_fonts()).map(drop).map_err(|error| format!("the UI runtime could not be installed: {error}"))
}

/// Who draws the main window's frame, decided once before the window opens: on macOS the traffic lights sit over
/// CADCraft's title row (a full-size content view with a hidden title), elsewhere the system draws the frame.
/// `CADCRAFT_TITLEBAR=system` keeps the macOS title bar. `None` where the system decides alone.
#[cfg(not(target_arch = "wasm32"))]
pub fn frame_decision(forced_x11: bool) -> Option<Decision> {
    use craft_host::target::Backend;
    let backend = if forced_x11 { Backend::X11 } else { Backend::from_env() };
    cfg!(target_os = "macos").then(|| craft_window::policy::decide_for(PREFIX, backend, craft_window::policy::TitleBarPref::Integrated))
}

/// Installs the runtime on the desktop window's context, once, before the first pass. `forced_x11` when the app
/// opens its window through XWayland; `frame` is [`frame_decision`]'s.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(ctx: &egui::Context, forced_x11: bool, frame: Option<Decision>) -> Result<(), String> {
    use craft_host::target::Backend;
    let backend = if forced_x11 { Backend::X11 } else { Backend::from_env() };
    install_with(ctx, ShellInfo::default().backend(backend), Arc::new(craft_window::system::SystemWindows), frame)
}

/// Installs the runtime on a context with no native window of its own: the browser build's canvas and test windows.
pub fn install_headless(ctx: &egui::Context) {
    if let Err(error) = install_with(ctx, ShellInfo::default(), Arc::new(craft_window::system::NoWindows), None) {
        log::warn!("{error}");
    }
}

/// Who CADCraft is to the framework's dock and dialogs (the update dialogs show its name and icon).
pub(crate) fn host() -> Arc<Host> {
    Arc::new(
        Host::builder(AppInfo { id: "cadcraft", name: "CADCraft", version: env!("CARGO_PKG_VERSION") })
            .icon_png(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.cadcraft.png"))
            .build(),
    )
}

/// Every function key is CADCraft's to assign (Tools ▸ Shortcuts records any of them), so the runtime's F6 and
/// Shift+F6 pane moves are unbound. Overrides are keyed by command id, which the command set gives.
fn command_overrides() -> Arc<Overrides> {
    static OVERRIDES: OnceLock<Arc<Overrides>> = OnceLock::new();
    OVERRIDES
        .get_or_init(|| {
            let mut overrides = Overrides::new();
            for id in [NavCommand::NextPane.id(), NavCommand::PreviousPane.id()] {
                overrides.insert(id, Vec::new());
            }
            Arc::new(overrides)
        })
        .clone()
}

impl CadApp {
    /// The framework's part of `App::raw_input_hook`, once per frame before the first pass: takes what the runtime
    /// gathered since the last frame and gives the coming passes the root values (theme mode, interface scale).
    pub fn before_input(&mut self, ctx: &egui::Context) {
        embed::before_input(ctx, |output| self.frame_input(ctx, output));
    }

    /// Takes the Copy, Cut and Paste chords the runtime ran as its own `edit.*` commands, because nothing in a
    /// floating panel's window had them (the runtime keeps those clipboard events out of the window's input, so
    /// [`CadApp::forward_native_input`] never sees them). The shortcuts read them as the clipboard events they stand
    /// for, with the paste's text. Undo, Redo, Select All and Delete are not read here: their keys are still forwarded.
    /// A chord with Shift (COPYBASE, PASTEBLOCK) is no `edit.*` command and arrives as a key press.
    pub fn queue_fired_edits(&mut self, ctx: &egui::Context, fired: &[Fired]) {
        let before = self.fired_edits.len();
        for fired in fired.iter().filter(|fired| fired.source == FireSource::Key && fired.phase == FirePhase::Press && !fired.mods.shift) {
            match fired.command.id.as_str() {
                id if id == EditCommands::Copy.id() => self.fired_edits.push(egui::Event::Copy),
                id if id == EditCommands::Cut.id() => self.fired_edits.push(egui::Event::Cut),
                // A paste with no text (an empty or image clipboard) sends no event in the main window either.
                id if id == EditCommands::Paste.id() => {
                    if let Some(text) = fired.pasted.as_ref().and_then(|content| content.text.as_deref()) {
                        self.fired_edits.push(egui::Event::Paste(text.to_string()));
                    }
                }
                _ => {}
            }
        }
        if self.fired_edits.len() > before {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    /// The root values of this frame. CADCraft registers no commands with the runtime and lets it toggle no chrome, so
    /// it acts only on the clipboard commands the output carries. The interface scale is the Options value: the runtime
    /// is the one writer of egui's zoom factor. While one of CADCraft's own modals shows (Save Changes, the lossy-save
    /// warning, Drawing Recovery, an egui modal), the runtime blocks the panels floated into windows of their own.
    fn frame_input(&mut self, ctx: &egui::Context, output: FrameOutput) -> FrameInput {
        self.queue_fired_edits(ctx, &output.fired);
        let mode = match self.ui.theme.resolve(ctx.system_theme()) {
            egui::Theme::Light => craft_theme::Mode::Light,
            egui::Theme::Dark => craft_theme::Mode::Dark,
        };
        let root = RootEnv::new().mode(mode).scale(Scale::new(self.options.interface_scale));
        let app_modal = crate::docking::native_input_blocked(self, ctx).then_some(ViewportId::ROOT);
        FrameInput::new(root).commands(CommandInput::new().overrides(command_overrides())).host(Arc::clone(&self.framework_host)).app_modal(app_modal)
    }

    /// The keyboard zoom: Ctrl+=, Ctrl++ and Ctrl+- (Cmd on macOS) step the Options dialog's interface scale by a tenth
    /// within its limits, and Ctrl+0 sets it back to the system's own size, also while a text field has the keyboard.
    /// It takes the keys out of `viewport`'s input, so it runs after the shortcuts, where the user may have given one of
    /// these keys to a command. A shortcut being recorded gets them, and in the browser they zoom the page.
    pub(crate) fn zoom_keys(&mut self, ctx: &egui::Context, viewport: ViewportId) {
        use egui::gui_zoom::kb_shortcuts::{ZOOM_IN, ZOOM_IN_SECONDARY, ZOOM_OUT, ZOOM_RESET};
        if cfg!(target_arch = "wasm32") || crate::keys::recording(ctx) {
            return;
        }
        let (smallest, largest) = (*INTERFACE_SCALES.start(), *INTERFACE_SCALES.end());
        let step = |scale: f32, by: f32| ((scale + by).clamp(smallest, largest) * 10.0).round() / 10.0;
        let shown = self.options.interface_scale;
        let scale = ctx.input_mut_for(viewport, |input| {
            if input.consume_shortcut(&ZOOM_RESET) {
                return 1.0;
            }
            let mut scale = shown;
            if input.consume_shortcut(&ZOOM_IN) || input.consume_shortcut(&ZOOM_IN_SECONDARY) {
                scale = step(scale, 0.1);
            }
            if input.consume_shortcut(&ZOOM_OUT) {
                scale = step(scale, -0.1);
            }
            scale
        });
        if scale != shown {
            self.options.interface_scale = scale;
            // The next frame's root values carry it.
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    /// The framework's part of `App::logic`, after CADCraft's own: the root window's close path. Unsaved drawings hold
    /// the window open while Save Changes asks about each ([`CadApp::request_quit`]); otherwise it closes.
    pub fn close_path(&mut self, ctx: &egui::Context) {
        embed::logic(ctx, || if self.request_quit() { CloseDecision::Hold } else { CloseDecision::Close });
    }

    /// The framework dialogs open on [`CadApp::framework_dialogs`], once per pass after CADCraft's own UI.
    pub(crate) fn run_framework_dialogs(&mut self, ui: &mut egui::Ui) {
        self.framework_dialogs.run(ui);
    }
}

/// The drawing's widget, the window's document canvas.
pub(crate) fn drawing_id() -> egui::Id {
    egui::Id::unique("cadcraft-drawing")
}

/// Declares the drawing, drawn in this pass, as the window's document canvas. With nothing focused it is the window's
/// key target: Enter, the arrows, Delete, the edit chords and the clipboard events stay in egui's input for the command
/// line and CADCraft's shortcuts. Tab and Shift+Tab stop at the drawing; with `keeps_tab` the focused drawing keeps
/// them in egui's input, else they move on from it.
pub(crate) fn host_canvas(ctx: &egui::Context, keeps_tab: bool) {
    const KEYS: KeyUse = KeyUse::CANVAS.with_edits(EditSet::ALL);
    let keys = if keeps_tab { KEYS.with_keep(KeepKeys::Tab) } else { KEYS };
    craft_ui::runtime::focus::host_canvas(ctx, drawing_id(), keys);
}

/// Whether a modal dialog, CADCraft's, egui's or the framework's, or an egui menu or popup is up. A framework menu holds
/// the focus in its own layer.
pub(crate) fn overlay_open(app: &CadApp, ctx: &egui::Context) -> bool {
    crate::docking::native_input_blocked(app, ctx) || egui::Popup::is_any_open(ctx) || overlay::any_open(ctx, OpenFilter::Modal)
}

/// Whether a widget other than the drawing has the keyboard. The command line and CADCraft's shortcuts have it
/// otherwise: with nothing focused, and while the drawing is the focused Tab stop.
pub(crate) fn widget_has_keyboard(ctx: &egui::Context) -> bool {
    ctx.egui_wants_keyboard_input() && !ctx.memory(|memory| memory.has_focus(drawing_id()))
}

/// Whether Escape was pressed in this pass: from egui while one of CADCraft's own widgets or popups has the keyboard,
/// otherwise the window's cancel, which the runtime routes to no component. Leaves it for later readers.
pub(crate) fn escape_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(egui::Key::Escape)) || window_cancel_pending(ctx)
}

/// Whether the window's cancel (an Escape while nothing has framework focus) is there and untaken.
pub(crate) fn window_cancel_pending(ctx: &egui::Context) -> bool {
    let mut pending = false;
    with_window_intents(ctx, false, |intents| {
        pending = intents.untaken(&Intent::Cancel);
        false
    });
    pending
}

/// The window frame the app decides, for the main window's builder and the runtime.
pub use craft_window::policy::Decision;

/// `builder` with the decided `frame` applied: the macOS inset frame's full-size content view and hidden title.
pub fn apply_frame(builder: egui::ViewportBuilder, frame: Option<&Decision>) -> egui::ViewportBuilder {
    match frame {
        Some(decision) => craft_window::policy::apply(builder, &decision.frame),
        None => builder,
    }
}

/// Whether CADCraft draws the title row: the frame is not the system's.
pub fn draws_title_row(frame: Option<&Decision>) -> bool {
    frame.is_some_and(|decision| !matches!(decision.frame, craft_window::policy::Frame::Native))
}
