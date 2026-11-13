//! Saving and plotting from the window without freezing it: the long part (writing the DWG or
//! PDF bytes) runs on another thread ([`cadcraft_engine::slow`]) while a progress window covers
//! the drawing, which takes no input until the file is written. Scripts, the control channel and
//! MCP keep running these commands in place ([`CadApp::run`]).

use std::time::Instant;

use cadcraft_engine::slow::{self, Done, Step};
use serde_json::Value;

use crate::CadApp;
use crate::theme::Tokens;

/// What to do with a command's result once it finished.
pub type Then = Box<dyn FnOnce(&mut CadApp, &Result<Value, String>)>;

/// A command waiting for its long work.
pub struct Busy {
    id: String,
    params: Value,
    kind: &'static str,
    started: Instant,
    rx: std::sync::mpsc::Receiver<Done>,
    then: Option<Then>,
}

impl Busy {
    /// What the progress window says: "Saving..." or "Plotting...".
    pub fn label(&self) -> &'static str {
        if self.kind == "save" { "Saving..." } else { "Plotting..." }
    }
}

/// Run command `id` with `params` as [`CadApp::run`] does, its long work on another thread; `then`
/// gets the result when it finished (at once, when there was no long work).
pub fn run(app: &mut CadApp, id: &str, params: Value, then: impl FnOnce(&mut CadApp, &Result<Value, String>) + 'static) {
    if app.busy.is_some() {
        app.session.echo(crate::tl!("Wait until the file is written.").to_string());
        return;
    }
    advance(app, id.to_string(), params, Instant::now(), Box::new(then));
}

/// Run the command up to its long work and start that, or finish.
fn advance(app: &mut CadApp, id: String, params: Value, started: Instant, then: Then) {
    match slow::step(&mut app.session, &id, &params) {
        Step::Done(r) => finish(app, r.map_err(|e| e.to_string()), then),
        Step::Slow(work) => {
            let kind = work.kind();
            let (tx, rx) = std::sync::mpsc::channel();
            spawn(work, tx);
            app.busy = Some(Busy { id, params, kind, started, rx, then: Some(then) });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(work: slow::Deferred, tx: std::sync::mpsc::Sender<Done>) {
    let run = move || {
        let _ = tx.send(work.run());
    };
    // Without a thread the sender is dropped with the work: [`poll`] reports the file unwritten.
    let _ = std::thread::Builder::new().name("cadcraft-write".into()).spawn(run);
}

/// The web has no threads: the work runs at once.
#[cfg(target_arch = "wasm32")]
fn spawn(work: slow::Deferred, tx: std::sync::mpsc::Sender<Done>) {
    let _ = tx.send(work.run());
}

/// The command's result, as [`CadApp::run`] reports it, then `then`.
fn finish(app: &mut CadApp, r: Result<Value, String>, then: Then) {
    if let Ok(v) = &r
        && let Some(m) = v.get("message").and_then(Value::as_str)
    {
        for l in m.lines() {
            app.session.echo(l.to_string());
        }
    }
    if let Err(e) = &r {
        app.session.echo(e.clone());
    }
    then(app, &r);
}

/// Each frame: take the finished work and go on with the command.
pub fn poll(app: &mut CadApp, ctx: &egui::Context) {
    let Some(busy) = app.busy.as_mut() else { return };
    match busy.rx.try_recv() {
        Ok(done) => {
            let Some(mut busy) = app.busy.take() else { return };
            slow::provide(done);
            let then = busy.then.take().unwrap_or_else(|| Box::new(|_, _| {}));
            advance(app, busy.id, busy.params, busy.started, then);
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            // The thread ended without an answer (it panicked): the command did not finish.
            let Some(mut busy) = app.busy.take() else { return };
            slow::forget();
            let then = busy.then.take().unwrap_or_else(|| Box::new(|_, _| {}));
            finish(app, Err(crate::tl!("The file could not be written.").to_string()), then);
        }
    }
}

/// Wait for the file being written (scripts and tests that go on with the saved drawing).
pub fn wait(app: &mut CadApp) {
    let ctx = egui::Context::default();
    while app.busy.is_some() {
        poll(app, &ctx);
        if let Some(busy) = &app.busy
            && let Ok(done) = busy.rx.recv()
        {
            // Hand the answer back as `poll` would have taken it.
            let Some(mut busy) = app.busy.take() else { return };
            slow::provide(done);
            let then = busy.then.take().unwrap_or_else(|| Box::new(|_, _| {}));
            advance(app, busy.id, busy.params, busy.started, then);
        }
    }
}

/// While a file is written the window takes no input: keys and clicks are dropped (the pointer
/// still moves, so the window redraws).
pub fn swallow_input(app: &CadApp, ctx: &egui::Context) {
    if app.busy.is_none() {
        return;
    }
    ctx.input_mut(|i| {
        i.events.retain(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::WindowFocused(_) | egui::Event::PointerGone));
    });
}

/// The progress window over the drawing, as opening a file shows its own.
pub fn show(app: &CadApp, ctx: &egui::Context) {
    let Some(busy) = &app.busy else { return };
    let t = Tokens::get();
    let screen = ctx.content_rect();
    // The drawing dims and takes no clicks.
    egui::Area::new(egui::Id::unique("cc_busy_cover")).order(egui::Order::Tooltip).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
        ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(70));
    });
    let secs = busy.started.elapsed().as_secs();
    egui::Area::new(egui::Id::unique("cc_busy"))
        .order(egui::Order::Debug)
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
                        ui.label(egui::RichText::new(crate::i18n::t(busy.label())).color(t.text));
                        ui.label(egui::RichText::new(format!("{secs} s")).color(t.text_dim));
                    });
                });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use cadcraft_engine::Session;
    use serde_json::json;

    /// A save from the window waits for its bytes with the window drawing; then the drawing is
    /// saved and `then` runs.
    #[test]
    fn a_save_runs_its_writing_away_from_the_window() {
        cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
            read: |_, _| Err("no reader in tests".into()),
            write: |d, name| Ok(format!("{name}:{}", d.model.len()).into_bytes()),
            plot: None,
        });
        let works = cadcraft_engine::cmd::file::io().is_some_and(|h| (h.write)(&cadcraft_doc::Drawing::new_imperial(), "probe.dxf").is_ok());
        if !works {
            return;
        }
        let mut app = CadApp::new(Session::new(), Services::default());
        let dir = std::env::temp_dir().join(format!("cadcraft-busy-{}", std::process::id()));
        let path = dir.join("a.dxf").to_string_lossy().to_string();
        let done = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = done.clone();
        run(&mut app, "saveas", json!({ "path": path }), move |_, r| {
            assert!(r.is_ok(), "{r:?}");
            seen.set(true);
        });
        assert!(app.busy.is_some(), "the writing runs away from the window");
        assert_eq!(app.busy.as_ref().map(Busy::label), Some("Saving..."));
        let ctx = egui::Context::default();
        let start = Instant::now();
        while app.busy.is_some() && start.elapsed().as_secs() < 10 {
            poll(&mut app, &ctx);
            std::thread::yield_now();
        }
        assert!(done.get(), "the save finished");
        assert_eq!(app.session.state().unwrap().path.as_deref(), Some(path.as_str()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
