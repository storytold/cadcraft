//! Cadcraft on Android — GameActivity + eframe/wgpu, reusing the desktop engine and egui UI.
//!
//! The engine, the ribbon, the grid and the theme are untouched: the same
//! [`cadcraft_engine::Session`] and [`cadcraft_ui_egui::CadApp`] the desktop shell builds are
//! wired up here. What the desktop shell gets from the OS, this crate takes from the activity:
//!
//! | Desktop (`apps/cadcraft`) | Android (this crate) |
//! |---|---|
//! | `std::env::args()` (files, `--sample`) | none: the app starts on a new drawing |
//! | `config_dir()` from `HOME`/`XDG_CONFIG_HOME` | [`AndroidApp::internal_data_path`] |
//! | `rfd` file dialogs, `arboard` clipboard | not wired yet: `Services` is left at its default |
//! | `sys-locale` interface language | English until a language picker is wired |
//! | `craft-print` printers | none |
//! | desktop-sized UI (1440×900, hover menus) | [`TouchShell`] enlarges touch targets |
//!
//! Known gaps are listed in the crate README; none of them stop the UI from coming up.
#![cfg(target_os = "android")]

mod saf;

use std::path::PathBuf;

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use winit::platform::android::activity::AndroidApp;

/// How much to scale the desktop layout up on a phone.
///
/// Measured on a 1080×2400 @ ~420 dpi phone: egui's logical size is about 411×914 pt at
/// `pixels_per_point ≈ 2.625`, while the ribbon + grid layout is designed for ≥900 pt wide.
/// Scaling up only shrinks the logical viewport further and makes the ribbon overlap itself, so
/// the shell does not scale; the layout needs a touch rework (collapsed ribbon, bottom app),
/// not a zoom.
const TOUCH_SCALE: f32 = 1.0;

/// System font files that carry CJK, in preference order. egui's default fonts are Latin-only, so
/// without one of these every Chinese/Japanese/Korean glyph renders as a blank box.
const CJK_CANDIDATES: &[&str] = &[
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/NotoSansCJKsc-Regular.otf",
    "/system/fonts/NotoSansSC-Regular.otf",
    "/system/fonts/DroidSansFallbackFull.ttf",
    "/system/fonts/DroidSansFallback.ttf",
];

/// The Android entry point. winit's `android-activity` glue looks this symbol up in the cdylib,
/// so the name and the signature are fixed — hence `unsafe(no_mangle)` and the crate-level
/// `unsafe_code = "deny"` rather than the workspace's `forbid`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("Cadcraft"));
    log::info!("cadcraft-android: starting (Cadcraft {})", env!("CARGO_PKG_VERSION"));

    let data_dir = app.internal_data_path().or_else(|| app.external_data_path());
    if let Some(dir) = &data_dir {
        log::info!("cadcraft-android: data dir {}", dir.display());
        std::fs::create_dir_all(dir).ok();
    }

    // The SAF pickers answer asynchronously (Java starts the intent, this side polls a flag), but
    // `Services::pick_open` is synchronous and runs on the UI thread. So the hook starts a
    // background task and answers `None`, and the chosen paths come back through the app's own
    // control channel -- which runs the *normal* `file.open` command, as the desktop does.
    let (ctrl_tx, ctrl_rx) = std::sync::mpsc::channel::<cadcraft_ui_egui::ControlRequest>();
    let activity = app.activity_as_ptr() as usize;

    let mut session = Session::new();
    let mut vc = CadApp::new(session, services(ctrl_tx, activity, data_dir.clone())).with_control(ctrl_rx);
    // UI state is not persisted on Android yet: the desktop's ui.json/prefs.json pair is
    // not loaded or saved here.

    let options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: egui::ViewportBuilder::default().with_title("Cadcraft").with_app_id("ai.storyteller.cadcraft"),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Cadcraft",
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(TouchShell::new(vc, data_dir)))
        }),
    );
    if let Err(e) = result {
        log::error!("cadcraft-android: eframe exited: {e}");
    }
}

/// What the platform shell can do for the UI: the Storage Access Framework stands in for
/// `rfd`. The picker cannot answer inside the frame that asks, so the hook starts a
/// background task, answers `None`, and the chosen file comes back over the app's own
/// control channel. The clipboard stays unset (`arboard` has no Android backend), so
/// Cadcraft's own clipboard is used instead.
fn services(ctrl: std::sync::mpsc::Sender<cadcraft_ui_egui::ControlRequest>, activity: usize, data_dir: Option<PathBuf>) -> Services {
    let mut services = Services::default();

    let open_ctrl = ctrl.clone();
    let open_dir = data_dir.clone();
    services.pick_open = Some(Box::new(move || {
        let Some(dir) = open_dir.clone() else {
            log::warn!("SAF: no data dir; cannot open the picker");
            return None;
        };
        let ctrl = open_ctrl.clone();
        std::thread::spawn(move || {
            let paths = saf::pick_media(activity, &dir, std::time::Duration::from_secs(300));
            let Some(path) = paths.into_iter().next() else {
                log::info!("SAF: nothing picked");
                return;
            };
            log::info!("SAF: opening {path}");
            let (req, _rx) = cadcraft_ui_egui::ControlRequest::new("app.open", serde_json::json!({ "path": path }));
            if ctrl.send(req).is_err() {
                log::warn!("SAF: control channel closed; {path} was not opened");
            }
        });
        None
    }));

    // Two-stage, the way FilmCraft does it: hand back a path in the app's own export directory so it
    // can write immediately, and run the SAF destination picker on a thread that waits for the file
    // to appear and stop growing, then copies it out. Answering `None` here would make the app treat
    // it as "no destination chosen" and write nothing at all.
    let save_dir = data_dir.clone();
    services.pick_save = Some(Box::new(move |name: &str| {
        let dir = save_dir.clone()?;
        let requested: String = if name.is_empty() { "export".to_owned() } else { name.to_owned() };
        let leaf: String = requested.rsplit(['/', '\\']).next().filter(|s| !s.is_empty()).unwrap_or("export").to_owned();
        let export_dir = dir.join("export");
        if std::fs::create_dir_all(&export_dir).is_err() {
            log::warn!("SAF: could not create {}", export_dir.display());
            return None;
        }
        let scratch_string = export_dir.join(&leaf).to_string_lossy().into_owned();
        log::info!("SAF: save dialog -> {scratch_string}");

        let publish_path = scratch_string.clone();
        std::thread::spawn(move || {
            if !saf::pick_save(activity, &leaf, std::time::Duration::from_secs(300)) {
                log::info!("SAF: no destination chosen; the file stays at {publish_path}");
                return;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
            let (mut size, mut stable_since) = (0u64, std::time::Instant::now());
            loop {
                let now = std::fs::metadata(&publish_path).map(|m| m.len()).unwrap_or(0);
                if now != size {
                    size = now;
                    stable_since = std::time::Instant::now();
                } else if size > 0 && stable_since.elapsed() > std::time::Duration::from_millis(1500) {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    log::warn!("SAF: giving up on {publish_path} (nothing was written)");
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            if saf::publish(activity, &publish_path) {
                log::info!("SAF: published {publish_path} ({size} bytes)");
            } else {
                log::warn!("SAF: publishing {publish_path} failed; the file stays in the export directory");
            }
        });

        Some(scratch_string)
    }));

    services
}

/// Bind a system CJK face as the last fallback of every family. egui applies new definitions on
/// the next pass, so this is called once, before the first frame.
fn setup_fonts(ctx: &egui::Context) {
    let Some((path, bytes)) = CJK_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().map(|b| (*p, b))) else {
        log::warn!("cadcraft-android: no CJK system font found; CJK text will render as boxes");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("system-cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("system-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    log::info!("cadcraft-android: CJK fallback font {path}");
}

/// The eframe app: forward everything to [`CadApp`] and apply the phone adjustments.
struct TouchShell {
    app: CadApp,
    data_dir: Option<PathBuf>,
    /// The one-shot touch tuning has run (after Cadcraft's own theme is in place).
    tuned: bool,
    /// Cadcraft's theme owns the font families; the system CJK face is appended after the first
    /// `ui` pass, because replacing the font set before the theme installs would drop it.
    font_done: bool,
    /// Cadcraft's font setup has run at least once.
    theme_fonts_live: bool,
}

impl TouchShell {
    fn new(app: CadApp, data_dir: Option<PathBuf>) -> Self {
        Self { app, data_dir, tuned: false, font_done: false, theme_fonts_live: false }
    }

    fn tune(ctx: &egui::Context) {
        let before = ctx.pixels_per_point();
        if (TOUCH_SCALE - 1.0).abs() > f32::EPSILON {
            ctx.set_pixels_per_point((before * TOUCH_SCALE).min(4.0));
        }
        ctx.all_styles_mut(|style| {
            // Touch targets: comfortable minimum, roomier padding, a little more separation so
            // neighbouring ribbon buttons are harder to hit by accident.
            style.spacing.interact_size = egui::vec2(40.0, 40.0);
            style.spacing.button_padding = egui::vec2(10.0, 8.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.slider_width = style.spacing.slider_width.max(160.0);
            // A resting finger must not raise a tooltip.
            style.interaction.show_tooltips_only_when_still = true;
        });
        let rect = ctx.viewport_rect();
        log::info!("cadcraft-android: touch mode applied (ppp {before}, logical {}×{} pt)", rect.width().round(), rect.height().round(),);
    }
}

impl eframe::App for TouchShell {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // The desktop shell keeps the window alive while a print job runs; on Android there is no
        // print backend, so only the close-request guard is worth mirroring.
        let _ = frame;
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        /* The window is full-screen on Android and the status bar is hidden, so the surface
         * really does start at y = 0 and the UI needs no safe-area inset. The old 28 pt root-Ui
         * shift only added a black band and did not fix the clipped menu bar: that clipping
         * came from the bar's fixed 22 pt height (see menus::menu_bar). */
        self.app.ui(ui);
        if !self.tuned {
            self.tuned = true;
            Self::tune(ui.ctx());
        }
        // The theme's own font definitions only become visible a frame after the theme install;
        // appending the CJK face any earlier would drop Cadcraft's named families.
        if !self.font_done && self.theme_fonts_live {
            self.font_done = true;
            append_cjk_fallback(ui.ctx());
        }
        if !self.theme_fonts_live && ui.ctx().fonts(|f| f.definitions().families.keys().any(|k| matches!(k, egui::FontFamily::Name(_)))) {
            self.theme_fonts_live = true;
        }
    }

    fn on_exit(&mut self) {
        if let Some(dir) = &self.data_dir {}
    }
}

/// Append the system CJK face to whatever families the theme has installed, keeping the theme's
/// own order (its faces win; the system face only catches glyphs they lack).
fn append_cjk_fallback(ctx: &egui::Context) {
    let Some((path, bytes)) = CJK_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok().map(|b| (*p, b))) else {
        return;
    };
    ctx.all_styles_mut(|_| {});
    let mut fonts = ctx.fonts(|f| f.definitions().clone());
    fonts.font_data.insert("system-cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        if !list.iter().any(|n| n == "system-cjk") {
            list.push("system-cjk".to_owned());
        }
    }
    for family in fonts.families.keys().cloned().collect::<Vec<_>>() {
        if let egui::FontFamily::Name(_) = family {
            let list = fonts.families.entry(family).or_default();
            if !list.iter().any(|n| n == "system-cjk") {
                list.push("system-cjk".to_owned());
            }
        }
    }
    ctx.set_fonts(fonts);
    log::info!("cadcraft-android: CJK fallback appended from {path}");
}
