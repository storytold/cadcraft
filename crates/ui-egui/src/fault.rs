//! What happens when the interface panics. Commands already run under their own guard
//! (`cadcraft_engine::guard`); this one covers the rest of the frame, so a panic in a panel keeps
//! the user's work: every drawing with unsaved changes is autosaved at once and the app says so.
//! A panic that keeps coming back leaves a plain window that only closes, with the recovery copies
//! kept for the next start. A panic hook writes a report for each, which stays on this machine.
//! None of this helps on the web, where a panic aborts.

use std::panic::{AssertUnwindSafe, catch_unwind};

use egui::{RichText, vec2};

use crate::theme::Tokens;
use crate::{CadApp, now_ms};

/// Where crash reports go, under the settings folder.
pub const REPORT_DIR: &str = "crash-reports";
/// This many panics, each within [`QUIET_MS`] of the last, and the interface is given up on.
const MAX_STRIKES: u32 = 3;
/// A stretch this long without a panic means the app has recovered.
const QUIET_MS: f64 = 5_000.0;

/// Panics caught around the frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiFault {
    /// The last panic's message, until the user dismisses the notice.
    pub message: Option<String>,
    /// Drawings autosaved because of panics so far.
    pub autosaved: usize,
    /// Panics in the current run of them.
    pub strikes: u32,
    /// When the last one happened.
    pub last_ms: f64,
    /// The interface keeps panicking: it no longer runs, and recovery copies are kept on exit.
    pub broken: bool,
}

impl CadApp {
    /// [`CadApp::logic`] under the panic guard.
    pub fn guarded_logic(&mut self, ctx: &egui::Context) {
        self.guarded(ctx, |app| app.logic(ctx));
    }

    /// [`CadApp::ui`] under the panic guard, then the notice about any panic. Once the interface
    /// has been given up on, only the window that closes the app is drawn.
    pub fn guarded_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.guarded(&ctx, |app| app.ui(ui));
        self.show_fault(&ctx);
    }

    /// The notice about a caught panic, or the window that closes the app.
    pub fn show_fault(&mut self, ctx: &egui::Context) {
        if self.fault.broken {
            broken_window(self, ctx);
        } else {
            notice(self, ctx);
        }
    }

    /// Run `frame`; a panic in it autosaves unsaved drawings instead of ending the app.
    pub fn guarded(&mut self, ctx: &egui::Context, frame: impl FnOnce(&mut CadApp)) {
        if self.fault.broken {
            return;
        }
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| frame(self))) {
            let message = p.downcast_ref::<&str>().map(ToString::to_string).or_else(|| p.downcast_ref::<String>().cloned());
            self.on_panic(ctx, message.unwrap_or_else(|| "panic".into()));
        }
    }

    fn on_panic(&mut self, ctx: &egui::Context, message: String) {
        let now = now_ms();
        let fault = &mut self.fault;
        fault.strikes = if now - fault.last_ms < QUIET_MS { fault.strikes + 1 } else { 1 };
        fault.last_ms = now;
        fault.broken = fault.strikes >= MAX_STRIKES;
        log::error!("the interface panicked: {message}");
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(saver) = &mut self.autosave {
            let session = &self.session;
            // The save is the point of being here: a panic inside it must not end the app either.
            self.fault.autosaved += catch_unwind(AssertUnwindSafe(|| saver.emergency_save(session))).unwrap_or(0);
        }
        self.fault.message = Some(message);
        ctx.request_repaint();
    }
}

/// The notice after a panic. It doesn't block the drawing: the user needs to be able to save.
fn notice(app: &mut CadApp, ctx: &egui::Context) {
    let Some(message) = app.fault.message.clone() else { return };
    let t = Tokens::get();
    let mut dismiss = false;
    egui::Window::new("Internal error")
        .id(egui::Id::unique("cc_fault_notice"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, vec2(0.0, 48.0))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label("Part of CADCraft failed, and the app kept running.");
            ui.label(saved_text(app.fault.autosaved));
            ui.label(RichText::new("Save your work and restart CADCraft.").strong());
            ui.add_space(4.0);
            details(ui, t, &message);
            // A row just as tall as the button (a plain right-to-left layout takes the rest).
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 0.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Dismiss").clicked() {
                    dismiss = true;
                }
            });
        });
    if dismiss {
        app.fault.message = None;
    }
}

/// All that's left once the interface keeps panicking.
fn broken_window(app: &CadApp, ctx: &egui::Context) {
    let t = Tokens::get();
    egui::Window::new("CADCraft has to close")
        .id(egui::Id::unique("cc_fault_broken"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label("Part of CADCraft keeps failing, so it can't continue.");
            ui.label(saved_text(app.fault.autosaved));
            ui.label(RichText::new("Drawing Recovery offers them at the next start.").strong());
            ui.add_space(4.0);
            if let Some(message) = &app.fault.message {
                details(ui, t, message);
            }
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 0.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close CADCraft").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
}

fn saved_text(autosaved: usize) -> String {
    match autosaved {
        0 => "No drawing needed a new recovery copy.".to_string(),
        1 => "A recovery copy of the drawing with unsaved changes was autosaved.".to_string(),
        k => format!("Recovery copies of {k} drawings with unsaved changes were autosaved."),
    }
}

fn details(ui: &mut egui::Ui, t: Tokens, message: &str) {
    ui.collapsing("Details", |ui| {
        ui.label(RichText::new(message).color(t.text_dim).monospace());
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = report::last() {
            ui.label(RichText::new(format!("Report: {}", path.display())).color(t.text_dim).monospace());
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
pub use report::install_report_hook;

#[cfg(not(target_arch = "wasm32"))]
mod report {
    use std::fmt::Write as _;
    use std::panic::PanicHookInfo;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Reports one run may write: a panic every frame must not fill the disk.
    const MAX_PER_RUN: u32 = 5;
    /// Reports kept in the folder.
    const KEEP: usize = 20;

    static WRITTEN: AtomicU32 = AtomicU32::new(0);
    static LAST: Mutex<Option<PathBuf>> = Mutex::new(None);

    /// The report of the latest panic, once one was written.
    pub(super) fn last() -> Option<PathBuf> {
        LAST.lock().ok().and_then(|g| g.clone())
    }

    /// Write a report into `dir` for every panic, on any thread, before the hook already in
    /// place runs. Panics that a guard then catches get one too: they are still bugs. A panic that
    /// can't be reported goes on as it would have: a report never makes things worse.
    pub fn install_report_hook(dir: PathBuf) {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if WRITTEN.fetch_add(1, Ordering::Relaxed) < MAX_PER_RUN
                && let Ok(path) = write_report(&dir, info)
                && let Ok(mut last) = LAST.lock()
            {
                *last = Some(path);
            }
            previous(info);
        }));
    }

    fn write_report(dir: &Path, info: &PanicHookInfo<'_>) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let mut text = String::new();
        let _ = writeln!(text, "CADCraft {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
        let _ = writeln!(text, "Time: {} UTC", crate::autosave::utc(now.as_secs()));
        let _ = writeln!(text, "Thread: {}", std::thread::current().name().unwrap_or("unnamed"));
        if let Some(l) = info.location() {
            let _ = writeln!(text, "Location: {}:{}:{}", l.file(), l.line(), l.column());
        }
        let _ = writeln!(text, "Message: {}", info.payload_as_str().unwrap_or("(not text)"));
        let _ = writeln!(text, "\n{}", std::backtrace::Backtrace::force_capture());
        let path = dir.join(format!("crash-{:012x}-{}.txt", now.as_millis(), std::process::id()));
        // The hook runs before the panic unwinds, so still inside the AI Assistant's call when it
        // panicked there: the report is the app's own file, not one the call asked for.
        cadcraft_engine::consent::outside(|| cadcraft_engine::atomic_write::write_atomic(&path, text.as_bytes()))?;
        prune(dir);
        Ok(path)
    }

    /// Keep the newest [`KEEP`] reports. Names start with the time, so they sort by age.
    fn prune(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut reports: Vec<PathBuf> = entries
            .flatten()
            .take(10_000)
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt")))
            .collect();
        reports.sort();
        let extra = reports.len().saturating_sub(KEEP);
        for old in reports.drain(..extra) {
            let _ = std::fs::remove_file(old);
        }
    }
}
