//! CADCraft desktop app.
//!
//! Usage: `cadcraft [--control <port>] [--sample] [files…]`
//!
//! `--control <port>` (or `CADCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}` → `{"id":1,"ok":true,…}`.
//! See `cadcraft_ui_egui::control` for the methods.
//!
//! `CADCRAFT_VSYNC=1` presents with vsync on Linux/BSD, where the default is the low-latency
//! present mode (`CADCRAFT_VSYNC=0` selects that elsewhere); see `cadcraft_ui_egui::gpu::surface_config`.
//!
//! `log` records go to standard error and `<settings dir>/logs/cadcraft.log` (`RUST_LOG` sets the
//! levels); see [`logging`] and [`config_dir`].
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_server;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};

struct App(CadApp, #[cfg(target_os = "macos")] Option<native_menu::NativeMenu>);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.1.is_none() && std::env::var_os("CADCRAFT_NO_NATIVE_MENU").is_none() {
                self.1 = Some(native_menu::NativeMenu::install(&mut self.0));
            }
            if let Some(m) = &mut self.1 {
                m.poll(&mut self.0, ctx);
            }
        }
        self.0.logic(ctx);
        if self.0.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    /// Preferences (the interface theme) survive restarts; window and egui state are not kept.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(cadcraft_ui_egui::PREFS_KEY, self.0.prefs_json());
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
        plot: Some(|d, space, opts| cadcraft_io::plot(d, space, opts).map_err(|e| e.to_string())),
    });
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Drawings (DWG, DXF)", &["dwg", "dxf"])
                .add_filter("All files", &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| {
            // The suggested name's type goes first: Windows adds the first filter's extension.
            let mut filters = [("Drawing (DXF)", "dxf"), ("SVG", "svg"), ("PNG", "png"), ("PDF", "pdf")];
            let lower = name.to_ascii_lowercase();
            filters.sort_by_key(|(_, ext)| !lower.ends_with(&format!(".{ext}")));
            filters
                .iter()
                .fold(rfd::FileDialog::new().set_file_name(name), |d, (label, ext)| d.add_filter(*label, &[*ext]))
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
    }
}

/// The per-user settings directory (today it holds `logs/`): `CADCRAFT_CONFIG_DIR` if set, else
/// `$XDG_CONFIG_HOME/cadcraft` or `~/.config/cadcraft` on Linux and the BSDs,
/// `~/Library/Application Support/CADCraft` on macOS, `%APPDATA%\CADCraft` on Windows.
fn config_dir() -> Option<std::path::PathBuf> {
    config_dir_from(|k| std::env::var_os(k))
}

/// [`config_dir`] with the environment passed in (`var` looks a variable up). An empty variable
/// counts as unset, so a blank value never means the working directory.
fn config_dir_from(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if let Some(dir) = set("CADCRAFT_CONFIG_DIR") {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        set("HOME").map(|h| h.join("Library/Application Support/CADCraft"))
    } else if cfg!(windows) {
        set("APPDATA").map(|a| a.join("CADCraft"))
    } else {
        // The XDG spec ignores a relative XDG_CONFIG_HOME.
        set("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| set("HOME").map(|h| h.join(".config"))).map(|c| c.join("cadcraft"))
    }
}

const APP_ID: &str = "ai.storyteller.cadcraft";

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-macos-512.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-256.png");

fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).map_err(|e| log::warn!("app icon: {e}")).ok()
}

fn version_string() -> String {
    let sha = option_env!("CADCRAFT_BUILD_SHA").unwrap_or("dev");
    format!("cadcraft {} ({sha})", env!("CARGO_PKG_VERSION"))
}

/// The control port from `--control <port>` or `CADCRAFT_CONTROL_PORT` (`source`); a value that is
/// not a port is logged and ignored instead of silently starting no control channel.
fn control_port_from(source: &str, value: Option<String>) -> Option<u16> {
    let value = value?;
    let port = value.trim().parse().ok();
    if port.is_none() {
        log::warn!("{source}: {value:?} is not a port number; the control channel is off");
    }
    port
}

fn main() -> eframe::Result {
    // First, so every start-up record is captured; see `logging`.
    let logger = logging::install();
    // lossy, so a value that is not Unicode is reported like any other bad port instead of ignored
    let mut control_port =
        control_port_from("CADCRAFT_CONTROL_PORT", std::env::var_os("CADCRAFT_CONTROL_PORT").map(|p| p.to_string_lossy().into_owned()));
    let mut files = Vec::new();
    let mut sample = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = control_port_from("--control", args.next()),
            "--sample" => sample = true,
            "--version" | "-V" => {
                println!("{}", version_string());
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match config_dir().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("{} ({} {}), log file {}", version_string(), std::env::consts::OS, std::env::consts::ARCH, path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (set CADCRAFT_CONFIG_DIR)");
            }
        }
    }
    install_io();
    let surface = cadcraft_ui_egui::gpu::surface_config(std::env::var("CADCRAFT_VSYNC").ok().as_deref(), cfg!(all(unix, not(target_os = "macos"))));
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
        ..Default::default()
    };
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 only (see graphics.rs).
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env());
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; CADCRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("CADCRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    eframe::run_native(
        "CADCraft",
        options,
        Box::new(move |cc| {
            let mut app = CadApp::new(Session::empty(), services());
            if let Some(prefs) = cc.storage.and_then(|s| s.get_string(cadcraft_ui_egui::PREFS_KEY)) {
                app.load_prefs(&prefs);
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            if surface.present_mode == eframe::wgpu::PresentMode::AutoNoVsync {
                app.frame_cap = Some(cadcraft_ui_egui::gpu::FrameCap::new(240));
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs);
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
                app.session.new_drawing(false);
            }
            Ok(Box::new(App(
                app,
                #[cfg(target_os = "macos")]
                None,
            )))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{config_dir_from, control_port_from};
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn control_port_values_that_are_not_ports_are_ignored() {
        assert_eq!(control_port_from("--control", Some("7979".into())), Some(7979));
        assert_eq!(control_port_from("--control", Some(" 7979 ".into())), Some(7979));
        assert_eq!(control_port_from("--control", None), None);
        for bad in ["", "abc", "-1", "65536", "99999999999999999999", "\u{0}"] {
            assert_eq!(control_port_from("--control", Some(bad.into())), None, "{bad:?}");
        }
    }

    #[test]
    fn the_override_variable_wins_and_empty_values_are_unset() {
        let vars = [("CADCRAFT_CONFIG_DIR", "/tmp/cc"), ("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/x"), ("APPDATA", "C:\\a")];
        assert_eq!(config_dir_from(env(&vars)), Some(PathBuf::from("/tmp/cc")));
        let empty = [("CADCRAFT_CONFIG_DIR", ""), ("HOME", ""), ("XDG_CONFIG_HOME", ""), ("APPDATA", "")];
        assert_eq!(config_dir_from(env(&empty)), None, "no usable variable means no directory, not the working directory");
        assert_eq!(config_dir_from(env(&[])), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_and_bsd_follow_xdg_config_home() {
        assert_eq!(config_dir_from(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/home/u")])), Some(PathBuf::from("/x/cadcraft")));
        assert_eq!(config_dir_from(env(&[("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/cadcraft")));
        // The XDG spec says a relative XDG_CONFIG_HOME is ignored.
        assert_eq!(config_dir_from(env(&[("XDG_CONFIG_HOME", "rel"), ("HOME", "/home/u")])), Some(PathBuf::from("/home/u/.config/cadcraft")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support() {
        assert_eq!(config_dir_from(env(&[("HOME", "/Users/u")])), Some(PathBuf::from("/Users/u/Library/Application Support/CADCraft")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata() {
        assert_eq!(
            config_dir_from(env(&[("APPDATA", "C:\\Users\\u\\AppData\\Roaming")])),
            Some(PathBuf::from("C:\\Users\\u\\AppData\\Roaming").join("CADCraft"))
        );
    }
}
