//! Opening drawings without freezing the window (#694). A file is read and decoded on a worker
//! thread (the slow part of OPEN, which needs no session); the UI shows an "Opening ..." card with
//! a spinner and the elapsed time and keeps painting. The finished drawing is added to the session
//! on the UI thread. The web build has no threads and opens synchronously. A file that can't be
//! opened is reported in an alert that stays until dismissed ([`failed_alert`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use cadcraft_engine::cmd::file::{ReadFile, read_file};
use egui::{RichText, vec2};

use crate::CadApp;
use crate::theme::Tokens;

type Outcome = Result<ReadFile, String>;

/// The most failures the alert lists; past it (a folder of bad files dropped) they are counted.
const MAX_FAILED: usize = 8;

struct Job {
    path: PathBuf,
    started: Instant,
    rx: Receiver<Outcome>,
}

/// The files being read in the background, oldest first.
#[derive(Default)]
pub struct Opening {
    jobs: Vec<Job>,
    /// Why files could not be opened, until the alert is dismissed.
    failed: Vec<String>,
    /// Failures past [`MAX_FAILED`].
    more_failed: usize,
}

impl Opening {
    /// Note a file that could not be opened, for [`failed_alert`].
    pub fn note_failure(&mut self, e: &str) {
        if self.failed.len() < MAX_FAILED {
            self.failed.push(e.to_string());
        } else {
            self.more_failed += 1;
        }
    }

    /// Whether the alert for files that could not be opened is up.
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }

    /// Start reading `path` on a worker thread.
    pub fn start(&mut self, path: &Path) {
        let (tx, rx) = channel();
        let p = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("cadcraft-open".into()).spawn(move || {
            let _ = tx.send(read_file(&p));
        });
        if spawned.is_err() {
            // No thread to read on: report it like any other failed open.
            let (tx, rx2) = channel();
            let _ = tx.send(Err(format!("open: {}: could not start a reader thread", path.display())));
            self.jobs.push(Job { path: path.to_path_buf(), started: Instant::now(), rx: rx2 });
            return;
        }
        self.jobs.push(Job { path: path.to_path_buf(), started: Instant::now(), rx });
    }

    /// Whether any file is still being read.
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// The finished reads, as (path, result). A reader that died without answering is an error.
    pub fn poll(&mut self) -> Vec<(PathBuf, Outcome)> {
        let mut done = Vec::new();
        self.jobs.retain(|j| match j.rx.try_recv() {
            Ok(r) => {
                done.push((j.path.clone(), r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                done.push((j.path.clone(), Err(format!("open: {}: the file could not be read", j.path.display()))));
                false
            }
        });
        done
    }

    /// The text of the card: the file and how long it has taken, or how many files are queued.
    fn label(&self) -> Option<String> {
        let first = self.jobs.first()?;
        let name = first.path.file_name().unwrap_or(first.path.as_os_str()).to_string_lossy();
        let secs = first.started.elapsed().as_secs_f32();
        let more = self.jobs.len() - 1;
        Some(if more == 0 { format!("Opening {name}... {secs:.0} s") } else { format!("Opening {name} and {more} more... {secs:.0} s") })
    }

    /// Paint the card in the middle of the window and keep frames coming while files load.
    pub fn show(&self, ctx: &egui::Context) {
        let Some(label) = self.label() else { return };
        let t = Tokens::get();
        egui::Area::new(egui::Id::unique("cc_opening"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(t.panel)
                    .stroke(egui::Stroke::new(1.0, t.accent))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(18, 12))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(t.accent));
                            ui.label(egui::RichText::new(label).color(t.text));
                        });
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

/// The alert naming the files that could not be opened (#2458): a refused drawing is too
/// important for a status bar message that fades, so it stays until OK (or Escape).
pub fn failed_alert(app: &mut CadApp, ctx: &egui::Context) {
    if !app.opening.has_failures() {
        return;
    }
    // One prompt at a time: the "Save changes?" prompt and the lossy-save warning read Escape
    // whatever is on top, so one press would answer them and the alert. The failure stays queued
    // and the alert shows once they are answered.
    if app.close_prompt().is_some() || app.save_loss.is_some() {
        return;
    }
    let t = Tokens::get();
    let mut ok = false;
    let shown = crate::modal::instant(ctx, egui::Id::unique("cc_open_failed")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(crate::tl!("CADCraft")).strong());
        ui.add_space(4.0);
        for e in &app.opening.failed {
            ui.label(e);
        }
        if app.opening.more_failed > 0 {
            let more = crate::tf!("{n} more files could not be opened; the command line lists them.", n = app.opening.more_failed);
            ui.label(RichText::new(more).color(t.text_dim));
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = egui::Button::new(RichText::new(crate::tl!("OK")).color(t.text)).fill(t.accent).min_size(vec2(84.0, 22.0));
            ok = ui.add(button).clicked();
        });
    });
    // Escape answers only the alert on top: Drawing Recovery can be up too at startup, and one
    // press must not dismiss both.
    ok |= shown.is_top_modal && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if ok {
        app.opening.failed.clear();
        app.opening.more_failed = 0;
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use cadcraft_engine::Session;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::{CadApp, Services};

    /// Opening starts in the background (the UI is not blocked), shows the card, and a file that
    /// cannot be read is reported like a failed OPEN without adding a drawing.
    #[test]
    fn open_path_reads_in_the_background_and_reports_failure() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let before = app.session.docs.len();
        let missing = std::env::temp_dir().join(format!("cadcraft-missing-{}.dxf", std::process::id()));
        let p = missing.to_string_lossy().to_string();
        app.open_path(&missing);
        assert!(app.opening.busy() || app.status.is_some(), "the read starts in the background");
        let t = Instant::now();
        while app.opening.busy() && t.elapsed() < Duration::from_secs(10) {
            app.finish_opening();
            std::thread::sleep(Duration::from_millis(5));
        }
        app.finish_opening();
        assert!(!app.opening.busy());
        assert_eq!(app.session.docs.len(), before);
        let (msg, _) = app.status.clone().unwrap_or_default();
        assert!(msg.contains(&p), "status reports the file: {msg}");
        assert!(app.session.log.iter().any(|l| l.contains(&p)));
        assert!(app.opening.failed.iter().any(|e| e.contains(&p)), "and the alert: {:?}", app.opening.failed);
    }

    /// A file that can't be opened stays reported in an alert until OK, not only in the status bar
    /// where the message fades (#2458). It lists a few failures and counts the rest.
    #[test]
    fn a_failed_open_stays_in_an_alert_until_ok() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let refused = "open: big.dxf: the drawing has more than 50000000 group codes";
        app.open_failed(refused);
        assert!(app.shows_modal());
        for i in 0..10 {
            app.open_failed(format!("open: bad{i}.dxf: not a drawing"));
        }
        let mut h =
            Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(|ui, app: &mut CadApp| super::failed_alert(app, &ui.ctx().clone()), app);
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_some(), "the alert names the refused file");
        assert!(h.query_by_label_contains("3 more files could not be opened").is_some(), "11 failures: 8 listed, 3 counted");
        // It stays until it is dismissed.
        h.run_steps(30);
        assert!(h.query_by_label(refused).is_some());
        h.get_by_label("OK").click();
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_none(), "OK closes it");
        assert!(!h.state().shows_modal());
    }

    /// With another modal over the alert (Drawing Recovery at startup), Escape answers that one
    /// and the alert stays until it is on top itself.
    #[test]
    fn escape_under_another_modal_leaves_the_alert() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let refused = "open: big.dxf: the drawing has more than 50000000 group codes";
        app.open_failed(refused);
        let mut h = Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(
            |ui, (app, other): &mut (CadApp, bool)| {
                let ctx = ui.ctx().clone();
                super::failed_alert(app, &ctx);
                if *other {
                    egui::Modal::new(egui::Id::unique("zz other modal")).show(&ctx, |ui| ui.label("Other"));
                }
            },
            (app, true),
        );
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_some(), "Escape went to the modal on top");
        h.state_mut().1 = false;
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_none(), "on top, Escape closes it");
    }

    /// A deep path with no spaces wraps inside the alert instead of widening it off the screen.
    #[test]
    fn a_long_path_wraps_inside_the_alert() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let deep = format!("open: C:/{}drawing.dxf: not a drawing", "very-long-folder-name/".repeat(20));
        app.open_failed(deep.as_str());
        let mut h =
            Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(|ui, app: &mut CadApp| super::failed_alert(app, &ui.ctx().clone()), app);
        h.run_steps(3);
        let r = h.get_by_label(&deep).rect();
        assert!(r.width() <= 440.0 && r.left() >= 0.0 && r.right() <= 800.0, "the path runs out of the alert: {r:?}");
        assert!(r.height() > 20.0, "it wraps onto several lines: {r:?}");
    }

    /// A "Save changes?" prompt reads Escape whether or not it is on top, so the alert waits
    /// until it is answered: one Escape cancels the close and leaves the alert, which then shows.
    #[test]
    fn the_alert_waits_for_the_close_prompt() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("line", &serde_json::json!({ "points": [[0, 0], [1, 0]] })).unwrap();
        app.request_close(None);
        let title = app.close_prompt().map(str::to_string).unwrap_or_default();
        assert!(!title.is_empty(), "the edited drawing asks before it closes");
        let refused = "open: big.dxf: the drawing has more than 50000000 group codes";
        app.open_failed(refused);
        // Drawn in the order the app draws them.
        let mut h = Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(
            |ui, app: &mut CadApp| {
                let ctx = ui.ctx().clone();
                crate::closing::prompt(app, &ctx);
                super::failed_alert(app, &ctx);
            },
            app,
        );
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_none(), "not over the close prompt");
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.state().close_prompt().is_none(), "Escape cancelled the close");
        assert_eq!(h.state().session.docs.len(), 1, "and kept the drawing");
        assert!(h.query_by_label(refused).is_some(), "the alert shows now");
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label(refused).is_none(), "and its own Escape closes it");
    }

    #[test]
    fn card_text_names_the_file_and_counts_queued_ones() {
        let mut o = super::Opening::default();
        assert!(o.label().is_none());
        o.start(Path::new("/nonexistent/dir/a.dxf"));
        o.start(Path::new("/nonexistent/dir/b.dxf"));
        let l = o.label().unwrap_or_default();
        assert!(l.starts_with("Opening a.dxf and 1 more"), "{l}");
    }
}
