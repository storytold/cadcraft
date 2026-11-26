//! CADCraft desktop app.
//!
//! Usage: `cadcraft [--control <port>] [--sample] [--new-window] [files…]` (`cadcraft --help` lists the rest; see [`launch_args`])
//!
//! `--control <port>` (or `CADCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}` → `{"id":1,"ok":true,…}`.
//! See `cadcraft_ui_egui::control` for the methods.
//!
//! `CADCRAFT_VSYNC=1` presents with vsync on Linux/BSD, where the default is the low-latency
//! present mode (`CADCRAFT_VSYNC=0` selects that elsewhere); see `cadcraft_ui_egui::gpu::surface_config`.
//!
//! `log` records go to standard error and `<settings dir>/logs/cadcraft.log` (`RUST_LOG` sets the
//! levels); see [`logging`]. The `CADCRAFT_*` variables are read once, at startup; see [`env`].
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(any(target_os = "macos", test))]
mod apple_events;
#[cfg(test)]
mod build_channel;
mod build_flavor;
mod control_server;
mod desktop_theme;
mod env;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod launch_args;
mod launch_profile;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod nightly_session;
mod portable;
mod process_identity;
mod profile_root;
#[cfg(target_os = "macos")]
mod sheet;
mod single_instance;
mod spacemouse;
mod system_animations;
mod workspace;

use cadcraft_engine::Session;
use cadcraft_ui_egui::os_window::{self, LaunchIdentity};
use cadcraft_ui_egui::switch::Switch;
use cadcraft_ui_egui::{CadApp, Pick, Services, i18n};

struct App {
    cad: CadApp,
    spacemouse: spacemouse::SpaceMouse,
    /// The desktop's light/dark choice where winit reports none (Linux, the BSDs).
    desktop_theme: desktop_theme::DesktopTheme,
    /// Whether the OS shows animations, for the System choice of View ▸ Accessibility ▸ Animation Effects.
    system_animations: system_animations::SystemAnimations,
    workspace: workspace::Workspace,
    workspace_error: Option<String>,
    /// The first CADCraft of this sign-in: files that later launches hand over (Windows).
    single_instance: Option<single_instance::Server>,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    /// `CADCRAFT_NO_NATIVE_MENU`: the menus stay inside the window.
    #[cfg(target_os = "macos")]
    menus_in_window: bool,
    /// Drawings and quit requests from Finder, the Dock and `open -a` (see `apple_events`).
    #[cfg(target_os = "macos")]
    apple_events: fmv_macos_events::Inbox,
}

impl App {
    /// `ctx` is `None` only for the final attempt at exit.
    fn persist_workspace(&mut self, ctx: Option<&egui::Context>) {
        let result = match ctx {
            None => self.workspace.save_on_exit(&self.cad.ui),
            Some(ctx) => self.workspace.save_if_changed(&self.cad.ui, std::time::Instant::now()).map(|wait| {
                if let Some(wait) = wait {
                    ctx.request_repaint_after(wait);
                }
            }),
        };
        if let Err(error) = result {
            if self.workspace_error.as_ref() != Some(&error) {
                eprintln!("CADCraft workspace: {error}");
                self.cad.set_status(format!("Workspace could not be saved: {error}"));
                self.workspace_error = Some(error);
            }
        } else {
            self.workspace_error = None;
        }
    }
}

/// View ▸ Theme ▸ System resolves against `raw.system_theme`, which winit leaves empty on Linux
/// and the BSDs: the desktop's answer, where it gives one, stands in for it.
fn follow_desktop(raw: &mut egui::RawInput, desktop: Option<egui::Theme>) {
    if desktop.is_some() {
        raw.system_theme = desktop;
    }
}

impl eframe::App for App {
    /// eframe logic callback: syncs the OS animation setting and runs `CadApp::logic`; when the pass index is 0 (the first pass or a hidden tick) it also installs/polls the macOS native menu, polls the spacemouse and persists the workspace; a quit request closes the viewport.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.cad.system_animations = self.system_animations.current();
        // Files another launch handed over open as tabs here, and the window comes forward.
        if let Some(server) = &self.single_instance {
            let paths = server.take_paths();
            if !paths.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                for p in &paths {
                    self.cad.open_path(p);
                }
            }
        }
        // Drawings and quit requests from Finder, the Dock and `open -a` (macOS).
        #[cfg(target_os = "macos")]
        apple_events::poll(&self.apple_events, &mut self.cad, ctx);
        // Consume control/open/screenshot queues and time-based autosave, including hidden ticks.
        self.cad.guarded_logic(ctx);
        // Host frame work runs here, once per frame, and so also on a hidden logic-only tick (which
        // never reaches `App::ui`): native menus stay current, the spacemouse still navigates and a
        // workspace changed while hidden is persisted. `current_pass_index()` is zero on the first
        // pass and on every hidden tick, and non-zero on later passes of a multi-pass frame.
        if ctx.current_pass_index() == 0 {
            #[cfg(target_os = "macos")]
            {
                if self.native_menu.is_none() && !self.menus_in_window {
                    self.native_menu = Some(native_menu::NativeMenu::install(&mut self.cad, ctx));
                }
                if let Some(m) = &mut self.native_menu {
                    m.poll(&mut self.cad, ctx);
                }
            }
            self.spacemouse.poll(&mut self.cad, ctx);
            // After `cad.logic`, so a control command's workspace change is saved in the same frame.
            self.persist_workspace(Some(ctx));
        }
        if std::mem::take(&mut self.cad.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.cad.close_path(ctx);
    }
    /// Apply the desktop-theme fallback, forward raw input to `CadApp`, and hand the frame's root values to the UI
    /// runtime.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        follow_desktop(raw, self.desktop_theme.current());
        self.cad.raw_input_hook(ctx, raw);
        self.cad.before_input(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        cadcraft_ui_egui::framework::before_ui(ui.ctx(), &*frame);
        self.cad.guarded_ui(ui);
    }
    /// A normal exit: the workspace is saved, and this run's autosave files are not needed (a
    /// crash leaves them for recovery).
    fn on_exit(&mut self) {
        self.persist_workspace(None);
        self.cad.cancel_screenshots();
        self.cad.shutdown_autosave();
        // The host's last step, once: on macOS Cmd+Q ends the process here, after this, so the
        // Nightly session record is finished by this hook and never by the code after `run_native`.
        if let Some(at_exit) = self.cad.services.at_exit.take() {
            at_exit();
        }
    }
    /// Preferences (the interface theme) survive restarts; eframe saves the window state itself
    /// (`persist_window`); egui state is not kept.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(cadcraft_ui_egui::options::PROFILE_KEY, self.cad.profile_json());
        storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.cad.prefs_json());
        storage.set_string(cadcraft_ui_egui::autosave::PREFS_KEY, cadcraft_ui_egui::autosave::prefs_json());
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// File format hooks for the engine (it stays I/O-agnostic).
pub fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    cadcraft_engine::cmd::file::set_image_decoder(|b| cadcraft_io::raster::decode(b).map_err(|e| e.to_string()));
    cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    cadcraft_engine::cmd::exchange::set_pdf_import(|bytes, options| cadcraft_io::pdf_read::read_json(bytes, options).map_err(|e| e.to_string()));
    // AutoLISP's entget, entmake and entmod see entities as DXF groups.
    cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
    });
    // ... and non-graphic objects (tables, dictionaries, layouts) as DXF records.
    cadcraft_engine::cmd::file::set_object_records(cadcraft_io::object_records);
    // What a DXF before 2007 leaves out for its proxies' string streams (#3229).
    cadcraft_engine::cmd::file::set_proxies_lost(cadcraft_io::proxies_lost);
    cadcraft_engine::cmd::file::set_dwg_compat_lost(cadcraft_io::dwg_compat_lost);
    cadcraft_engine::cmd::file::set_dwg_notes(cadcraft_io::dwg_notes);
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| file_dialog(&Pick::Open).pick_file().map(|p| p.to_string_lossy().to_string()))),
        pick_save: Some(Box::new(|name: &str| file_dialog(&Pick::Save(name.into())).save_file().map(|p| p.to_string_lossy().to_string()))),
        // macOS shows the dialogs as sheets (`sheet::start_pick`), set once the window exists.
        start_pick: None,
        // APPLOAD: AutoLISP routines only, starting where the last one was loaded from.
        pick_lisp: Some(Box::new(|dir: &str| file_dialog(&Pick::Lisp(dir.into())).pick_file().map(|p| p.to_string_lossy().to_string()))),
        no_file_system: false,
        pick_pgp: Some(Box::new(|dir: Option<&str>| {
            file_dialog(&Pick::Pgp(dir.unwrap_or_default().into())).pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        // Set in `main` once the Nightly session record exists.
        at_exit: None,
    }
}

/// The file types `pick` offers: (label, extensions). Shared by the blocking dialogs and the
/// macOS sheets.
fn filters(pick: &Pick) -> Vec<(&'static str, Vec<&'static str>)> {
    match pick {
        Pick::Open => vec![
            (i18n::t("Drawings (DWG, DXF)"), vec!["dwg", "dxf", "dwt"]),
            (i18n::t("SVG drawings (SVG, SVGZ)"), vec!["svg", "svgz"]),
            (i18n::t("PDF files (PDF)"), vec!["pdf"]),
            (i18n::t("All files"), vec!["*"]),
        ],
        Pick::Save(name) => filters_for_save(name).iter().map(|(label, ext)| (i18n::t(label), vec![*ext])).collect(),
        Pick::Lisp(_) => vec![(i18n::t("AutoLISP (LSP)"), vec!["lsp"]), (i18n::t("All files"), vec!["*"])],
        Pick::Pgp(_) => vec![(i18n::t("Alias files (PGP)"), vec!["pgp"])],
    }
}

/// The blocking dialog for `pick` (Windows, Linux, FreeBSD).
fn file_dialog(pick: &Pick) -> rfd::FileDialog {
    let dialog = filters(pick).iter().fold(rfd::FileDialog::new(), |d, (label, exts)| d.add_filter(*label, exts));
    match pick {
        Pick::Save(name) => dialog.set_file_name(name),
        Pick::Lisp(dir) | Pick::Pgp(dir) if !dir.is_empty() => dialog.set_directory(dir),
        _ => dialog,
    }
}

/// File types for the Save dialog, the suggested name's type first: Windows adds the first
/// filter's extension, and macOS accepts only listed extensions, replacing any other with the
/// first one. DWG must be listed for a `.dwg` name to survive, and a drawing opened from DWG
/// then defaults to DWG; other names keep DXF first.
fn filters_for_save(name: &str) -> [(&'static str, &'static str); 6] {
    let mut filters =
        [("Drawing (DXF)", "dxf"), ("Drawing (DWG)", "dwg"), ("Drawing Template (DWT)", "dwt"), ("SVG", "svg"), ("PNG", "png"), ("PDF", "pdf")];
    let lower = name.to_ascii_lowercase();
    filters.sort_by_key(|(_, ext)| !lower.ends_with(&format!(".{ext}")));
    filters
}

/// Whether eframe's storage file (`app.ron`, a map of keys to strings) holds a saved main window
/// state. An unreadable or malformed file counts as none, as it does for eframe.
fn saved_window(app_ron: &str) -> bool {
    ron::from_str::<std::collections::HashMap<String, String>>(app_ron).is_ok_and(|kv| kv.contains_key(WINDOW_KEY))
}

/// The window state from the last session is saved and will be restored (#2595). A saved size
/// below the minimum (seen on Windows: 64×64 points, which eframe restores un-maximized and
/// unusable) is dropped first, so the window opens at its default size. `portable_file`: the
/// portable copy's settings file, read instead of the user profile's.
fn restores_window(portable_file: Option<&std::path::Path>) -> bool {
    let Some(path) = portable_file.map(std::path::PathBuf::from).or_else(|| eframe::storage_dir(os_window::app_id()).map(|dir| dir.join("app.ron")))
    else {
        return false;
    };
    let Ok(mut ron) = std::fs::read_to_string(&path) else { return false };
    // Through a temporary file: a failed write (a full disk) keeps the other saved settings.
    if let Some(fixed) = without_tiny_window(&ron)
        && cadcraft_engine::atomic_write::write_atomic(&path, fixed.as_bytes()).is_ok()
    {
        ron = fixed;
    }
    saved_window(&ron)
}

/// `app_ron` without its window entry when that entry's inner size is below `MIN_INNER_SIZE`;
/// `None` when there is nothing to drop (no entry, a usable size, or an unreadable file).
fn without_tiny_window(app_ron: &str) -> Option<String> {
    let mut kv = ron::from_str::<std::collections::BTreeMap<String, String>>(app_ron).ok()?;
    let w = kv.get(WINDOW_KEY)?;
    let size = &w[w.find("inner_size_points:")?..];
    let num = |k: &str| size[size.find(k)? + k.len()..].split([',', ')']).next()?.trim().parse::<f32>().ok();
    let (x, y) = (num("x:")?, num("y:")?);
    if x >= MIN_INNER_SIZE[0] && y >= MIN_INNER_SIZE[1] {
        return None;
    }
    kv.remove(WINDOW_KEY);
    ron::ser::to_string_pretty(&kv, ron::ser::PrettyConfig::default()).ok()
}

/// The smallest main window, in points.
const MIN_INNER_SIZE: [f32; 2] = [900.0, 560.0];

/// The main window: its size, title bar, placement and rendering needs. `restoring`: a window state
/// saved by the last session is restored instead of opening centred. `stencil`: the stencil bits of
/// its render pass ([`stencil_bits`]).
fn window_options(restoring: bool, stencil: u8, frame: Option<&cadcraft_ui_egui::framework::Decision>) -> eframe::NativeOptions {
    let viewport = egui::ViewportBuilder::default()
        .with_title(os_window::name_for(os_window::identity()))
        .with_inner_size([1600.0, 1000.0])
        .with_min_inner_size(MIN_INNER_SIZE)
        .with_drag_and_drop(true)
        .with_app_id(os_window::app_id());
    eframe::NativeOptions {
        viewport: cadcraft_ui_egui::framework::apply_frame(viewport, frame),
        // The size, position and maximized state are kept between sessions (#2595); eframe keeps a
        // restored window on the monitors that are connected now.
        persist_window: true,
        // The first launch opens centred on its screen, not cascaded from the top-left corner
        // (#1129); later launches go back where the window was (eframe would centre over that).
        // Wayland compositors place windows themselves.
        centered: !restoring,
        // CADCraft installs its own capped font chain (`theme::install_fonts`); never run
        // system-font discovery as a hidden fallback.
        system_font_fallback: false,
        // A stencil buffer clips the contents of clipped layout viewports on the GPU.
        stencil_buffer: stencil,
        ..Default::default()
    }
}

/// The key eframe saves the main window state under in its storage.
const WINDOW_KEY: &str = "window";

/// Stencil bits of the window's render pass (see `NativeOptions::stencil_buffer`).
const STENCIL_BITS: u8 = 8;

/// The stencil bits of the window's render pass: none when `CADCRAFT_STENCIL` is off. Some old
/// drivers (Mesa on Intel HD 4000, old Intel drivers on Windows) accept a stencil pass without an
/// error and then present a black window, so only an explicit opt-out drops it. Without a stencil
/// buffer, clipped layout viewports draw unclipped.
fn stencil_bits(stencil: Switch) -> u8 {
    if stencil == Switch::Off { 0 } else { STENCIL_BITS }
}

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-macos-512.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-256.png");

fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).map_err(|e| log::warn!("app icon: {e}")).ok()
}

/// The version this build reports: the prepared nightly identity, or the workspace version.
fn built_version() -> &'static str {
    env!("CADCRAFT_BUILT_VERSION")
}

/// The stable line, exactly what clap prints for `--version`: `cadcraft <workspace version>`.
fn version_string() -> String {
    format!("cadcraft {}", cadcraft_engine::build_version())
}

/// The Nightly identity line every nightly script checks: `<App> <version> (nightly-v1)`, with the
/// prepared nightly version embedded at build time. The app name never gains a "Nightly".
fn nightly_version_line() -> String {
    format!("CADCraft {}", build_flavor::version(build_flavor::BuildFlavor::NightlyV1, built_version()))
}

/// The identity this build reports in its log: the Nightly line on that channel, stable's otherwise.
fn reported_version() -> String {
    match build_flavor::COMPILED {
        build_flavor::BuildFlavor::Stable => version_string(),
        build_flavor::BuildFlavor::NightlyV1 => nightly_version_line(),
    }
}

/// The reported line with the commit, for the log: a bug report's log names the build it came from.
fn log_version() -> String {
    format!("{} ({})", reported_version(), option_env!("CADCRAFT_BUILD_SHA").unwrap_or("dev"))
}

/// Whether this start runs the isolated Nightly flavor.
fn is_nightly() -> bool {
    launch_profile::current().is_nightly()
}

fn initialize_nightly_process_identity() -> Result<(), process_identity::IdentityError> {
    process_identity::initialize(build_flavor::COMPILED == build_flavor::BuildFlavor::NightlyV1, os_window::id_for(LaunchIdentity::NightlyV1))
}

/// Whether a refusal may also open a native message box. Standard error always carries the reason;
/// this only adds a box where one renders without the app's event loop. Never on macOS: rfd's alert
/// there needs the AppKit run loop this start has not reached, so a refused nightly launch hung in
/// an invisible modal instead of exiting (GridCraft Mac smoke, 2026-11-26).
fn refusal_dialog(flavor: build_flavor::BuildFlavor, desktop_session: bool, macos: bool) -> bool {
    build_flavor::needs_native_error(flavor, desktop_session) && !macos
}

/// A refused launch profile ends with a native dialog where a modal renders without the app's run
/// loop (Windows, X11/Wayland), and with the reason on standard error everywhere else: a headless
/// runner must never wait for a modal, and on macOS one opened before the event loop never appears.
fn report_launch_profile_error(error: &str) {
    eprintln!("CADCraft launch profile: {error}");
    let desktop_session = cfg!(windows)
        || cfg!(target_os = "macos")
        || (cfg!(target_os = "linux") && ["DISPLAY", "WAYLAND_DISPLAY"].into_iter().any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty())));
    if refusal_dialog(build_flavor::COMPILED, desktop_session, cfg!(target_os = "macos")) {
        if let Err(error) = initialize_nightly_process_identity() {
            eprintln!("CADCraft launch profile: {error}");
            return;
        }
        let _ = rfd::MessageDialog::new()
            .set_title(os_window::name_for(LaunchIdentity::NightlyV1))
            .set_description(error)
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}

/// Select the immutable launch profile before logging, updates, fonts or saved state, and answer
/// the version query before any of that: `None` means the query has been answered and the process
/// should return success without opening a window.
fn prepare_launch() -> Option<Vec<std::ffi::OsString>> {
    let argv: Vec<_> = std::env::args_os().skip(1).collect();
    // Plain `--version`, or the Launcher's `--launch-profile nightly-v1 --profile-root <dir>
    // --version`: answered before root validation, process identity, logging, IPC or any window.
    if build_flavor::COMPILED == build_flavor::BuildFlavor::NightlyV1 && (build_flavor::version_query(&argv) || launch_profile::version_query(&argv))
    {
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), "{}", nightly_version_line());
        return None;
    }
    let early = launch_profile::parse(argv)
        .and_then(|launch| {
            build_flavor::require_profile(build_flavor::COMPILED, &launch.profile)?;
            initialize_nightly_process_identity().map_err(|error| error.to_string())?;
            Ok(launch)
        })
        .and_then(|launch| {
            launch_profile::prepare(launch, |root| {
                launch_profile::validate_root(root, |key| std::env::var_os(key), std::env::current_exe().ok().as_deref())
            })
        })
        .unwrap_or_else(|error| {
            report_launch_profile_error(&error);
            std::process::exit(2)
        });
    let identity = if early.profile.is_nightly() { LaunchIdentity::NightlyV1 } else { LaunchIdentity::Stable };
    if let Err(error) = launch_profile::install(early.profile).and_then(|()| os_window::select_identity(identity)) {
        report_launch_profile_error(&error);
        std::process::exit(2);
    }
    Some(early.args)
}

/// Parse CLI options (`--control`, `--sample`, files, `--version`), install logging and I/O hooks, build the services and app, and run eframe.
fn main() -> eframe::Result {
    // A copy of this program started to read or write a DWG file does only that. It is started
    // with `--dwg-worker` alone, so it is answered before the launch profile is required.
    if let Some(code) = cadcraft_io::dwg_worker::serve_if_asked() {
        std::process::exit(code);
    }
    let Some(arguments) = prepare_launch() else { return Ok(()) };
    cadcraft_io::dwg_worker::use_own_worker();
    // First, so every start-up record is captured; see `logging`.
    let logger = logging::install();
    // The update helper relaunches this binary after a self-update; it runs and exits here. A
    // nightly build is updated by the Launcher only, so it never finishes a stable helper.
    #[cfg(not(target_arch = "wasm32"))]
    if !is_nightly()
        && let Some(done) = craft_update::self_update::finish(&cadcraft_ui_egui::UPDATE_APP, &arguments)
    {
        if let Err(e) = done {
            eprintln!("CADCraft update helper failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `--help` and `--version` print to standard output and exit 0. A usage error (an unknown or
    // repeated flag, a missing or bad value, a bad CADCRAFT_CONTROL_PORT) names the argument on
    // standard error and exits 2 before the window opens.
    let launch = launch_args::parse(launch_args::command(), arguments).unwrap_or_else(|e| e.exit());
    let vars = env::Env::read();
    let (control_port, sample, files) = (launch.control, launch.sample, launch.files);
    // A nightly launch accepts no control server: the inherited CADCRAFT_CONTROL_PORT and the
    // Launcher's agent link belong to the stable app, and a second unauthenticated surface over
    // the nightly profile is not part of the protocol.
    let control_port = if is_nightly() {
        if control_port.is_some() {
            eprintln!("CADCraft Nightly: the control server is off in this channel; the option is ignored");
        }
        None
    } else {
        control_port
    };
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match vars.config_dir.as_ref().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("{} ({} {}), log file {}", log_version(), std::env::consts::OS, std::env::consts::ARCH, path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (an empty CADCRAFT_CONFIG_DIR turns it off)");
            }
        }
    }
    // A launch that only names files hands them to the CADCraft already running and exits.
    let handed = single_instance::absolute(&files);
    let may_hand_off = handed.is_some() && !launch.new_window && !files.is_empty() && control_port.is_none() && !sample;
    let instance = match single_instance::claim(handed.as_deref().unwrap_or_default(), may_hand_off, launch_profile::current().root()) {
        single_instance::Claim::HandedOff => return Ok(()),
        single_instance::Claim::Primary(server) => Some(server),
        single_instance::Claim::Alone => None,
    };
    install_io();
    cadcraft_engine::settings_dir::set(vars.config_dir.clone());
    // Nightly session health: the owner's lease and unclean-exit record live in the profile, and the
    // record is finished on the way out (on macOS in `on_exit`, see `nightly_session`).
    let (nightly_watch, previous_unclean) =
        match nightly_session::Session::begin_with_health(launch_profile::current().root(), built_version(), option_env!("CADCRAFT_BUILD_SHA")) {
            Ok((watch, unavailable)) => {
                if unavailable {
                    log::warn!("another CADCraft Nightly holds this profile's session lease; this window keeps no health record");
                }
                let unclean = watch.as_ref().is_some_and(nightly_session::Session::previous_unclean);
                (std::sync::Arc::new(std::sync::Mutex::new(watch)), unclean)
            }
            Err(error) => {
                log::warn!("Nightly session state: {error}");
                (std::sync::Arc::new(std::sync::Mutex::new(None)), false)
            }
        };
    if previous_unclean {
        log::warn!("the previous CADCraft Nightly ended without finishing its session record");
    }
    if let Some(dir) = &vars.config_dir {
        cadcraft_ui_egui::fault::install_report_hook(dir.join(cadcraft_ui_egui::fault::REPORT_DIR));
    }
    if let Some(name) = &vars.font_alt {
        cadcraft_engine::fonts::ttf::set_font_alt(name);
    }
    if let Some(list) = &vars.font_fallback {
        cadcraft_engine::fonts::ttf::set_fallback_fonts(list);
    }
    let surface = cadcraft_ui_egui::gpu::surface_config(vars.vsync, cfg!(all(unix, not(target_os = "macos"))));
    // The window's render pass and the canvas's pipelines must agree on it; a mismatch is a wgpu
    // validation error.
    let stencil = stencil_bits(vars.stencil);
    if stencil == 0 {
        log::warn!("CADCRAFT_STENCIL is off: the window has no stencil buffer; clipped layout viewports draw unclipped");
    }
    // XWayland when it's there (see below), unless CADCRAFT_WAYLAND=1 keeps the native Wayland backend.
    let forced_x11 = cfg!(all(unix, not(target_os = "macos"))) && std::env::var_os("DISPLAY").is_some() && vars.wayland != Switch::On;
    let frame = cadcraft_ui_egui::framework::frame_decision(forced_x11);
    let persistence_path = vars
        .portable_storage_file
        .clone()
        .or_else(|| std::env::var_os("CADCRAFT_CONFIG_DIR").and_then(|_| vars.config_dir.as_ref().map(|d| d.join("app.ron"))));
    let mut options = eframe::NativeOptions {
        // The crosshair is drawn by the app: present without a queue on Linux/BSD, where X11
        // swapchains hold two frames back under vsync (`CADCRAFT_VSYNC=1` restores vsync).
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(surface),
        persistence_path: persistence_path.clone(),
        ..window_options(restores_window(persistence_path.as_deref()), stencil, frame.as_ref())
    };
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 with FXC (see graphics.rs).
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env(), eframe::wgpu::Dx12Compiler::from_env());
    // Command previews (COPY, MOVE...) of large selections are tessellated by egui, whose index
    // buffer outgrew wgpu's default 256 MiB cap; the validation error is fatal. Ask for what the
    // adapter actually supports.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let base = std::sync::Arc::clone(&create.device_descriptor);
        create.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_limits.max_buffer_size = d.required_limits.max_buffer_size.max(adapter.limits().max_buffer_size);
            d
        });
    }
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; CADCRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if forced_x11 {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Registered before the event loop starts, so it catches the Finder event that launched us as
    // well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    // The exit hook's own handle: the app creator is a `move` closure, so the watch is cloned before it.
    let exit_watch = std::sync::Arc::clone(&nightly_watch);
    let result = eframe::run_native(
        os_window::name_for(os_window::identity()),
        options,
        Box::new(move |cc| {
            cadcraft_ui_egui::framework::install(&cc.egui_ctx, forced_x11, frame.clone())?;
            let mut app = CadApp::new(Session::empty(), services());
            // The exit hook runs in `on_exit`: macOS ends the process inside AppKit's terminate after
            // it, so nothing after `run_native` can be relied on there.
            app.services.at_exit = Some(Box::new(move || nightly_session::finish_once(&exit_watch, previous_unclean)));
            if previous_unclean {
                app.set_status("CADCraft Nightly ended unexpectedly last time; your drawings were recovered where autosave had them.");
            }
            // File dialogs are sheets on macOS: a modal loop inside the event handler crashes the
            // app when the dialog is resized (#1527).
            #[cfg(target_os = "macos")]
            if let Some(window) = cc.winit_window().cloned() {
                app.services.start_pick = Some(sheet::start_pick(window, cc.egui_ctx.clone()));
            }
            let (workspace, restored, workspace_error) = workspace::Workspace::load(vars.config_dir.as_deref());
            if let Some(ui) = restored {
                app.ui = ui;
            }
            app.set_system_cursor_default(vars.system_cursor == Switch::On);
            // Native Wayland answers a clipboard request asynchronously; every other backend in the
            // next frame (egui-winit).
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                use eframe::egui_wgpu::wgpu::rwh::{HasDisplayHandle, RawDisplayHandle};
                if let Ok(display) = cc.display_handle() {
                    app.set_clipboard_late_answers(matches!(display.as_raw(), RawDisplayHandle::Wayland(_)));
                }
            }
            #[cfg(any(not(unix), target_os = "macos"))]
            app.set_clipboard_late_answers(false);
            // macOS: CADCRAFT_NO_NATIVE_MENU=1 keeps the menus inside the window.
            #[cfg(target_os = "macos")]
            app.set_native_menus(vars.no_native_menu != Switch::On);
            if let Some(error) = &workspace_error {
                eprintln!("CADCraft workspace: {error}");
                app.set_status(error);
            }
            app.system_languages = vars.locale.map(|s| vec![s]).unwrap_or_else(|| sys_locale::get_locales().collect());
            // The user profile (Options): drafting settings, selection sizes, drawing-area colours.
            if let Some(profile) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::options::PROFILE_KEY)) {
                app.load_profile(&profile);
            }
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::PREFS_KEY)) {
                app.load_prefs(&prefs);
            }
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::autosave::PREFS_KEY)) {
                cadcraft_ui_egui::autosave::load_prefs(&prefs);
            }
            // Autosave files (SAVEFILEPATH) default to `<settings dir>/autosave`.
            if let Some(dir) = &vars.config_dir {
                app.enable_autosave(&dir.join("autosave"));
                // A nightly build is updated by the Launcher, never by the app's own update check.
                if !is_nightly() {
                    app.enable_update_check();
                }
                // Startup LISP (acad.lsp, acaddoc.lsp) is read from `<settings>/support`; the
                // folder is made so it is there to put them in.
                let support = dir.join("support");
                if let Err(e) = std::fs::create_dir_all(&support) {
                    log::warn!("no LISP support folder {}: {e}", support.display());
                }
                app.session.set_lisp_support_dir(Some(support));
            }
            app.integrated_titlebar = cadcraft_ui_egui::framework::draws_title_row(frame.as_ref());
            if surface.present_mode == eframe::wgpu::PresentMode::AutoNoVsync {
                app.frame_cap = Some(cadcraft_ui_egui::gpu::FrameCap::new(240));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs, eframe::egui_wgpu::depth_format_from_bits(0, stencil));
            }
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample {
                let _ = app.run("ui.sample", serde_json::json!({}));
            }
            for f in &files {
                app.open_path(f);
            }
            // Start reading any launch-time documents before the first frame. Later events use
            // this same inbox, including the normal unsaved-changes handling for quit requests.
            #[cfg(target_os = "macos")]
            let apple_events = {
                let inbox = apple_events.connect(&cc.egui_ctx);
                apple_events::poll(&inbox, &mut app, &cc.egui_ctx);
                inbox
            };
            if app.session.docs.is_empty() {
                // In the units the profile asks for (MEASUREINIT, restored above).
                let metric = app.session.settings.measureinit;
                app.session.new_drawing(metric);
            }
            if let Some(server) = &instance {
                let ctx = cc.egui_ctx.clone();
                server.set_wake(move || ctx.request_repaint());
            }
            let ctx = cc.egui_ctx.clone();
            let desktop_theme = desktop_theme::DesktopTheme::start(move || ctx.request_repaint());
            let ctx = cc.egui_ctx.clone();
            let system_animations = system_animations::SystemAnimations::start(move || ctx.request_repaint());
            Ok(Box::new(App {
                desktop_theme,
                system_animations,
                cad: app,
                spacemouse: spacemouse::SpaceMouse::start(cc.egui_ctx.clone()),
                workspace,
                workspace_error,
                single_instance: instance,
                #[cfg(target_os = "macos")]
                native_menu: None,
                #[cfg(target_os = "macos")]
                menus_in_window: vars.no_native_menu == Switch::On,
                #[cfg(target_os = "macos")]
                apple_events,
            }))
        }),
    );
    // The controlled return path: a start that created no window still finishes its record. On macOS
    // a quit never reaches here, and the exit hook above has done it. No window ran, so no notice was
    // shown: pass false and an unclean record survives for the next launch to report.
    nightly_session::finish_once(&nightly_watch, false);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        STENCIL_BITS, filters_for_save, follow_desktop, nightly_version_line, saved_window, stencil_bits, version_string, window_options,
        without_tiny_window,
    };

    #[test]
    fn save_dialog_offers_dwg_with_the_drawing_format_first() {
        let first = |name: &str| filters_for_save(name)[0].1;
        assert_eq!(first("Waco Winnelson Floorplan.dwg"), "dwg");
        assert_eq!(first("FLOORPLAN.DWG"), "dwg");
        // New drawings and DXF files keep DXF first.
        assert_eq!(first("Drawing1.dxf"), "dxf");
        assert_eq!(first("Drawing1"), "dxf");
        assert_eq!(first("plan.pdf"), "pdf");
        assert_eq!(first("office.dwt"), "dwt");
        for name in ["a.dwg", "b.dxf", "c"] {
            let exts: Vec<_> = filters_for_save(name).iter().map(|(_, e)| *e).collect();
            assert!(exts.contains(&"dwg") && exts.contains(&"dxf"), "{name}");
        }
    }

    /// #1129: the first launch opens centred, not cascaded from the top-left corner.
    #[test]
    fn the_first_launch_opens_centred() {
        let o = window_options(false, STENCIL_BITS, None);
        assert!(o.centered, "centred on launch");
        assert!(o.persist_window, "the window state is saved for the next launch");
    }

    /// #2595: a saved window state (size, position, maximized) is restored, not centred over.
    #[test]
    fn a_saved_window_reopens_where_it_was() {
        let o = window_options(true, STENCIL_BITS, None);
        assert!(!o.centered, "centring would move a restored window");
        assert!(o.persist_window);
    }

    /// `CADCRAFT_STENCIL=0` (also `no`, `false`, any case) opens the window without a stencil
    /// buffer; unset, blank, `1` or anything else keeps it.
    #[test]
    fn the_stencil_buffer_is_dropped_only_when_asked() {
        let bits = |value: Option<&str>| {
            let vars = crate::env::Env::from_vars(|k| (k == "CADCRAFT_STENCIL").then_some(value).flatten().map(Into::into));
            stencil_bits(vars.stencil)
        };
        for off in ["0", "no", "NO", "false", "False", " 0 "] {
            assert_eq!(bits(Some(off)), 0, "{off:?}");
        }
        for on in [None, Some(""), Some("1"), Some("yes"), Some("8"), Some("maybe")] {
            assert_eq!(bits(on), STENCIL_BITS, "{on:?}");
        }
    }

    /// The window's render pass carries the chosen stencil bits, and the canvas gets the same.
    #[test]
    fn the_window_carries_the_chosen_stencil_bits() {
        for bits in [0, STENCIL_BITS] {
            assert_eq!(window_options(false, bits, None).stencil_buffer, bits);
            assert_eq!(window_options(true, bits, None).stencil_buffer, bits);
        }
        let format = |bits| eframe::egui_wgpu::depth_format_from_bits(0, bits);
        assert_eq!(format(0), None, "no stencil: the canvas builds no clip pipelines");
        assert!(format(STENCIL_BITS).is_some_and(|f| f.has_stencil_aspect()));
    }

    /// Only a readable `app.ron` with a window entry counts as a saved window state.
    #[test]
    fn a_saved_window_is_read_from_app_ron() {
        assert!(saved_window(r#"{"cadcraft.prefs": "{}", "window": "(maximized: true)"}"#));
        assert!(!saved_window(r#"{"cadcraft.prefs": "{\"window\":1}"}"#), "a value mentioning it is not the key");
        assert!(!saved_window(""));
        assert!(!saved_window("{\"window\": "), "truncated");
        assert!(!saved_window("not ron at all"));
    }

    /// A saved 64×64 window (below the minimum) is dropped; a usable one is kept.
    #[test]
    fn a_tiny_saved_window_is_dropped() {
        let tiny = r#"{"window": "(normal_geometry:(outer_position_pixels:Some((x:0.0,y:0.0)),scale_factor:1.25,inner_size_points:(x:64.0,y:64.0)),fullscreen:false,maximized:true)", "cadcraft.prefs": "{}"}"#;
        let fixed = without_tiny_window(tiny).expect("dropped");
        assert!(!saved_window(&fixed) && fixed.contains("cadcraft.prefs"));
        let ok = tiny.replace("x:64.0,y:64.0)),", "x:1600.0,y:1000.0)),");
        assert_eq!(without_tiny_window(&ok), None);
        assert_eq!(without_tiny_window("not ron"), None);
    }

    /// The two identity lines. Nightly prints the kit's `<App> <version> (nightly-v1)` -- the app
    /// name never gains a "Nightly" -- while stable keeps exactly the line clap prints, which
    /// `launch_args` asserts against its own `-V` output.
    #[test]
    fn the_version_lines_name_the_app_and_its_channel() {
        let nightly = nightly_version_line();
        assert_eq!(nightly, format!("CADCraft {} (nightly-v1)", super::built_version()));
        assert!(!nightly.contains("Nightly "), "the version line must not rename the app: {nightly}");
        assert_eq!(version_string(), format!("cadcraft {}", cadcraft_engine::build_version()));
    }

    /// View ▸ Theme ▸ System on Linux: winit reports no theme, so the desktop's answer stands in;
    /// without one (or where winit answers and the desktop doesn't) the input is left alone.
    #[test]
    fn the_desktop_answers_for_the_system_theme() {
        let mut raw = egui::RawInput::default();
        follow_desktop(&mut raw, Some(egui::Theme::Light));
        assert_eq!(raw.system_theme, Some(egui::Theme::Light));
        let mut raw = egui::RawInput { system_theme: Some(egui::Theme::Dark), ..Default::default() };
        follow_desktop(&mut raw, None);
        assert_eq!(raw.system_theme, Some(egui::Theme::Dark));
        let mut raw = egui::RawInput::default();
        follow_desktop(&mut raw, None);
        assert_eq!(raw.system_theme, None);
    }
}

#[cfg(test)]
mod refusal_dialog_tests {
    use super::refusal_dialog;
    use crate::build_flavor::BuildFlavor;

    #[test]
    fn a_refusal_never_blocks_on_a_modal_before_the_event_loop_on_macos() {
        assert!(refusal_dialog(BuildFlavor::NightlyV1, true, false), "a desktop refusal still shows the box where one renders");
        assert!(!refusal_dialog(BuildFlavor::NightlyV1, true, true), "macOS has no run loop yet: the modal would hang");
        assert!(!refusal_dialog(BuildFlavor::NightlyV1, false, false), "a headless session has no box");
        assert!(!refusal_dialog(BuildFlavor::Stable, true, false), "only a nightly refusal explains itself in a box");
    }
}
