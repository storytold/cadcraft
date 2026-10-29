//! CADCraft desktop app.
//!
//! Usage: `cadcraft [--control <port>] [--sample] [files…]` (`cadcraft --help` lists the rest; see [`launch_args`])
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

mod control_server;
mod decorations;
mod desktop_theme;
mod env;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod launch_args;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod spacemouse;
mod system_animations;
mod workspace;

use cadcraft_engine::Session;
use cadcraft_ui_egui::switch::Switch;
use cadcraft_ui_egui::{CadApp, Services, i18n};

struct App {
    cad: CadApp,
    spacemouse: spacemouse::SpaceMouse,
    /// The desktop's light/dark choice where winit reports none (Linux, the BSDs).
    desktop_theme: desktop_theme::DesktopTheme,
    /// Whether the OS shows animations, for the System choice of View ▸ Accessibility ▸ Animation Effects.
    system_animations: system_animations::SystemAnimations,
    workspace: workspace::Workspace,
    workspace_error: Option<String>,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    /// `CADCRAFT_NO_NATIVE_MENU`: the menus stay inside the window.
    #[cfg(target_os = "macos")]
    menus_in_window: bool,
    /// The `movable` flag last set on the main window.
    #[cfg(target_os = "macos")]
    movable: Option<bool>,
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
        // Consume control/open/screenshot queues and time-based autosave, including hidden ticks.
        self.cad.logic(ctx);
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
    }
    /// Apply the desktop-theme fallback, forward raw input to `CadApp`, and capture Tab at the command line.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        follow_desktop(raw, self.desktop_theme.current());
        cadcraft_ui_egui::window_controls::release_after_desktop_grab(ctx, raw);
        self.cad.raw_input_hook(ctx, raw);
        cadcraft_ui_egui::cmdline::capture_tab(ctx, raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.cad.ui(ui);
        #[cfg(target_os = "macos")]
        craft_window::title_bar::sync_movable(ui.ctx(), &mut self.movable);
    }
    /// A normal exit: the workspace is saved, and this run's autosave files are not needed (a
    /// crash leaves them for recovery).
    fn on_exit(&mut self) {
        self.persist_workspace(None);
        self.cad.cancel_screenshots();
        self.cad.shutdown_autosave();
    }
    /// Preferences (the interface theme) survive restarts; window and egui state are not kept.
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
    cadcraft_engine::cmd::file::set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    // AutoLISP's entget, entmake and entmod see entities as DXF groups.
    cadcraft_engine::cmd::file::set_entity_codec(cadcraft_engine::cmd::file::EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
    });
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter(i18n::t("Drawings (DWG, DXF)"), &["dwg", "dxf", "dwt"])
                .add_filter(i18n::t("SVG drawings (SVG, SVGZ)"), &["svg", "svgz"])
                .add_filter(i18n::t("All files"), &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| {
            filters_for_save(name)
                .iter()
                .fold(rfd::FileDialog::new().set_file_name(name), |d, (label, ext)| d.add_filter(i18n::t(label), &[*ext]))
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        // APPLOAD: AutoLISP routines only, starting where the last one was loaded from.
        pick_lisp: Some(Box::new(|dir: &str| {
            let dialog = rfd::FileDialog::new().add_filter(i18n::t("AutoLISP (LSP)"), &["lsp"]).add_filter(i18n::t("All files"), &["*"]);
            let dialog = if dir.is_empty() { dialog } else { dialog.set_directory(dir) };
            dialog.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        no_file_system: false,
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

const APP_ID: &str = "ai.storyteller.cadcraft";

/// Stencil bits of the window's render pass (see `NativeOptions::stencil_buffer`).
const STENCIL_BITS: u8 = 8;

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-macos-512.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-256.png");

fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).map_err(|e| log::warn!("app icon: {e}")).ok()
}

fn version_string() -> String {
    format!("cadcraft {}", cadcraft_engine::build_version())
}

/// Whether the libraries winit's X11 backend opens at startup are installed (libX11, libXcursor,
/// libX11-xcb and libXi; libX11-xcb brings in libxcb; and libxkbcommon-x11, for the keyboard).
/// The deb and rpm only recommend them, and a forced X11 backend without them fails to start
/// (winit panics when libxkbcommon-x11 is missing).
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_libraries_load() -> bool {
    use x11_dl::{xcursor::Xcursor, xinput2::XInput2, xlib::Xlib, xlib_xcb::Xlib_xcb};
    let missing = [Xlib::open().err(), Xcursor::open().err(), Xlib_xcb::open().err(), XInput2::open().err()];
    let mut missing: Vec<String> = missing.into_iter().flatten().map(|e| e.to_string()).collect();
    if xkbcommon_dl::x11::xkbcommon_x11_option().is_none() {
        missing.push("libxkbcommon-x11.so.0 could not be loaded".into());
    }
    if !missing.is_empty() {
        log::warn!("CADCRAFT_X11: X11 libraries missing, using the native Wayland backend: {}", missing.join("; "));
    }
    missing.is_empty()
}

/// Whether to force winit's X11 backend (XWayland on a Wayland desktop): an X display is there,
/// `CADCRAFT_X11` is on, and the X11 libraries load. The libraries are probed last, only when X11
/// would otherwise be chosen.
#[cfg(all(unix, not(target_os = "macos")))]
fn use_x11(display: bool, setting: Switch, x11_libraries_load: impl FnOnce() -> bool) -> bool {
    display && setting == Switch::On && x11_libraries_load()
}

/// Whether winit runs through X11: forced (`use_x11`), or as winit chooses for itself, X11 unless
/// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty (an X11-only session).
#[cfg(all(unix, not(target_os = "macos")))]
fn runs_on_x11(forced: bool, wayland_display: Option<&std::ffi::OsStr>, wayland_socket: Option<&std::ffi::OsStr>) -> bool {
    let set = |v: Option<&std::ffi::OsStr>| v.is_some_and(|v| !v.is_empty());
    forced || !(set(wayland_display) || set(wayland_socket))
}

/// Parse CLI options (`--control`, `--sample`, files, `--version`), install logging and I/O hooks, build the services and app, and run eframe.
fn main() -> eframe::Result {
    // First, so every start-up record is captured; see `logging`.
    let logger = logging::install();
    // The update helper relaunches this binary after a self-update; it runs and exits here.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(done) = craft_update::self_update::finish(&craft_update::suite::CADCRAFT, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        if let Err(e) = done {
            eprintln!("CADCraft update helper failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `--help` and `--version` print to standard output and exit 0. A usage error (an unknown or
    // repeated flag, a missing or bad value, a bad CADCRAFT_CONTROL_PORT) names the argument on
    // standard error and exits 2 before the window opens.
    let launch = launch_args::parse(launch_args::command(), std::env::args_os().skip(1)).unwrap_or_else(|e| e.exit());
    let vars = env::Env::read();
    let (control_port, sample, files) = (launch.control, launch.sample, launch.files);
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match vars.config_dir.as_ref().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("{} ({} {}), log file {}", version_string(), std::env::consts::OS, std::env::consts::ARCH, path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (an empty CADCRAFT_CONFIG_DIR turns it off)");
            }
        }
    }
    install_io();
    cadcraft_engine::settings_dir::set(vars.config_dir.clone());
    if let Some(name) = &vars.font_alt {
        cadcraft_engine::fonts::ttf::set_font_alt(name);
    }
    if let Some(list) = &vars.font_fallback {
        cadcraft_engine::fonts::ttf::set_fallback_fonts(list);
    }
    let surface = cadcraft_ui_egui::gpu::surface_config(vars.vsync, cfg!(all(unix, not(target_os = "macos"))));
    let mut options = eframe::NativeOptions {
        // The crosshair is drawn by the app: present without a queue on Linux/BSD, where X11
        // swapchains hold two frames back under vsync (`CADCRAFT_VSYNC=1` restores vsync).
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(surface),
        viewport: egui::ViewportBuilder::default()
            .with_title("CADCraft")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_app_id(APP_ID),
        persist_window: false,
        // CADCraft installs its own capped font chain (`theme::install_fonts`); never run
        // system-font discovery as a hidden fallback.
        system_font_fallback: false,
        // A stencil buffer clips the contents of clipped layout viewports on the GPU.
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
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
    // winit runs natively on a Wayland desktop: sharp at fractional scales, with working input
    // methods, and file drops from the file manager through storytold/winit's W15 patch.
    // CADCRAFT_X11=1 runs through XWayland instead, as a fallback where the native backend
    // misbehaves; 0, off or unset leaves it off. The X11 libraries are only recommended by the
    // packages, so without them the setting is ignored rather than failing.
    #[cfg(all(unix, not(target_os = "macos")))]
    let force_x11 = use_x11(std::env::var_os("DISPLAY").is_some(), vars.x11, x11_libraries_load);
    #[cfg(all(unix, not(target_os = "macos")))]
    if force_x11 {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Read before the window opens: it decides the window's frame (Use system title bar).
    let (workspace, restored, workspace_error) = workspace::Workspace::load(vars.config_dir.as_deref());
    // GNOME on Wayland leaves the window frame to the app (decorations.rs): a title row carries the
    // window buttons instead, unless Options ▸ Display ▸ Use system title bar is on. On X11, forced
    // or the only display, the window manager draws it.
    #[cfg(all(unix, not(target_os = "macos")))]
    let x11 = runs_on_x11(force_x11, std::env::var_os("WAYLAND_DISPLAY").as_deref(), std::env::var_os("WAYLAND_SOCKET").as_deref());
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let x11 = false;
    let window_controls = decorations::app_drawn(x11, vars.titlebar.as_deref());
    let app_frame = window_controls.is_some() && !restored.as_ref().is_some_and(|ui| ui.system_title_bar);
    if app_frame {
        options.viewport = options.viewport.with_decorations(false);
    }
    eframe::run_native(
        "CADCraft",
        options,
        Box::new(move |cc| {
            let mut app = CadApp::new(Session::empty(), services());
            if let Some(ui) = restored {
                app.ui = ui;
            }
            app.set_system_cursor_default(vars.system_cursor == Switch::On);
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
                app.enable_update_check();
                // Startup LISP (acad.lsp, acaddoc.lsp) is read from `<settings>/support`; the
                // folder is made so it is there to put them in.
                let support = dir.join("support");
                if let Err(e) = std::fs::create_dir_all(&support) {
                    log::warn!("no LISP support folder {}: {e}", support.display());
                }
                app.session.set_lisp_support_dir(Some(support));
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            app.app_frame_shown = window_controls.as_ref().map(|_| app_frame);
            app.window_controls = window_controls;
            if surface.present_mode == eframe::wgpu::PresentMode::AutoNoVsync {
                app.frame_cap = Some(cadcraft_ui_egui::gpu::FrameCap::new(240));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs, eframe::egui_wgpu::depth_format_from_bits(0, STENCIL_BITS));
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
            if app.session.docs.is_empty() {
                // In the units the profile asks for (MEASUREINIT, restored above).
                let metric = app.session.settings.measureinit;
                app.session.new_drawing(metric);
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
                #[cfg(target_os = "macos")]
                native_menu: None,
                #[cfg(target_os = "macos")]
                menus_in_window: vars.no_native_menu == Switch::On,
                #[cfg(target_os = "macos")]
                movable: None,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{filters_for_save, follow_desktop};

    /// A Wayland desktop without the X11 libraries (libxkbcommon-x11 removed, or a package
    /// installed without its recommendations) crashed at startup: X11 was forced through XWayland
    /// and winit panicked loading them. Native Wayland is now the default, `CADCRAFT_X11=1` asks
    /// for X11 only when the libraries load, and probing them never panics, whichever of them
    /// this machine has.
    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn x11_only_when_asked_for_and_the_libraries_load() {
        use super::{use_x11, x11_libraries_load};
        use cadcraft_ui_egui::switch::Switch;
        let unprobed = || -> bool { panic!("the libraries are probed only when X11 would be chosen") };
        assert!(!use_x11(true, Switch::Default, unprobed), "native Wayland by default");
        assert!(!use_x11(true, Switch::Off, unprobed), "CADCRAFT_X11=0 leaves it off");
        assert!(!use_x11(false, Switch::On, unprobed), "no X display");
        assert!(use_x11(true, Switch::On, || true));
        assert!(!use_x11(true, Switch::On, || false), "missing X11 libraries: native Wayland, no crash");
        let loaded = x11_libraries_load();
        assert_eq!(use_x11(true, Switch::On, || loaded), loaded);
    }

    /// On an X11-only session winit picks X11 without `CADCRAFT_X11`, so `CADCRAFT_TITLEBAR=app`
    /// drew the app's window buttons there too; X11 keeps the window manager's frame.
    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn x11_as_winit_chooses_it() {
        use super::runs_on_x11;
        use std::ffi::OsStr;
        let some = |v| Some(OsStr::new(v));
        assert!(runs_on_x11(false, None, None), "an X11-only session");
        assert!(runs_on_x11(false, some(""), some("")), "empty variables count as unset");
        assert!(!runs_on_x11(false, some("wayland-0"), None));
        assert!(!runs_on_x11(false, some(""), some("3")), "WAYLAND_SOCKET alone");
        assert!(runs_on_x11(true, some("wayland-0"), None), "CADCRAFT_X11=1 through XWayland");
    }

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
