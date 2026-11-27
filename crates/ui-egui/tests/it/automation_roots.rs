//! The app's control channel is held to `--automation-read-root` / `--automation-write-root`
//! (#1585): what a request does, and what the input it injects does, reads and writes only under
//! the roots. The person at the keyboard is not held to them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cadcraft_engine::Session;
use cadcraft_engine::file_access::AutomationRoots;
use cadcraft_ui_egui::control::{ControlRequest, Outcome, handle};
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

use crate::typed_open::temp_dir::TempDir;

/// A reader and a writer that accept any file, as the other tests in this binary install.
fn install_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |_, _| Ok(cadcraft_engine::doc::Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
}

/// Folders `work/in` (the read root, holding `plan.dxf`), `work/out` (the write root) and
/// `elsewhere` (holding `secret.dxf`), which no root covers.
struct Site {
    dir: TempDir,
    roots: Arc<AutomationRoots>,
}

impl Site {
    fn new(tag: &str) -> Self {
        install_io();
        let dir = TempDir::new(tag);
        for folder in ["work/in", "work/out", "elsewhere"] {
            std::fs::create_dir_all(dir.join(folder)).expect("create a folder");
        }
        dir.file("work/in/plan.dxf", b"0\nEOF\n");
        dir.file("elsewhere/secret.dxf", b"0\nEOF\n");
        let roots = AutomationRoots::new(Some(&dir.join("work/in")), Some(&dir.join("work/out"))).expect("the roots open").expect("two roots");
        Self { dir, roots }
    }

    fn app(&self) -> CadApp {
        CadApp::new(Session::new(), Services::default()).with_automation_roots(Some(self.roots.clone()))
    }

    fn path(&self, name: &str) -> String {
        self.dir.path_str(name)
    }
}

fn call(app: &mut CadApp, method: &str, params: Value) -> Value {
    let (req, _reply) = ControlRequest::new(method, params);
    match handle(app, &egui::Context::default(), &req) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { path } => json!({"ok": true, "screenshot": path}),
    }
}

fn refused(reply: &Value) -> bool {
    reply["ok"] == false && reply["error"].as_str().is_some_and(|e| e.contains("automation"))
}

/// One window frame. `person`: events from the keyboard, put in as the operating system puts them.
fn frame(app: &mut CadApp, ctx: &egui::Context, person: Vec<egui::Event>) {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut raw = egui::RawInput { screen_rect: Some(screen), events: person, ..Default::default() };
    app.raw_input_hook(ctx, &mut raw);
    cadcraft_ui_egui::cmdline::capture_tab(ctx, &mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

/// Frames until the injected input has been processed and the background reads have finished.
fn pump(app: &mut CadApp, ctx: &egui::Context) {
    let t = Instant::now();
    frame(app, ctx, vec![]);
    while (!app.synthetic.is_empty() || app.opening.busy()) && t.elapsed() < Duration::from_secs(10) {
        frame(app, ctx, vec![]);
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..3 {
        frame(app, ctx, vec![]);
    }
}

fn enter() -> [egui::Event; 2] {
    [true, false].map(|pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    })
}

#[test]
fn requests_read_and_write_only_under_the_roots() {
    let site = Site::new("roots-requests");
    let mut app = site.app();
    let before = app.session.docs.len();

    let r = call(&mut app, "app.open", json!({"path": site.path("elsewhere/secret.dxf")}));
    assert!(refused(&r), "{r}");
    assert_eq!(app.session.docs.len(), before, "nothing outside the roots was opened");
    let r = call(&mut app, "app.open", json!({"path": site.path("work/in/plan.dxf")}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(app.session.docs.len(), before + 1);

    let r = call(&mut app, "app.save", json!({"path": site.path("elsewhere/saved.dxf")}));
    assert!(refused(&r), "{r}");
    assert!(!site.dir.join("elsewhere/saved.dxf").exists(), "nothing was written outside the write root");
    // A read root grants no writes.
    let r = call(&mut app, "app.save", json!({"path": site.path("work/in/saved.dxf")}));
    assert!(refused(&r), "{r}");
    let r = call(&mut app, "app.save", json!({"path": site.path("work/out/saved.dxf")}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(site.dir.join("work/out/saved.dxf").exists());

    // The files `ui.render` and `ui.screenshot` write are held to the roots as a command's are.
    let r = call(&mut app, "ui.render", json!({"path": site.path("elsewhere/shot.png")}));
    assert!(refused(&r), "{r}");
    assert!(!site.dir.join("elsewhere/shot.png").exists());
    let r = call(&mut app, "ui.render", json!({"path": site.path("work/out/shot.png")}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(site.dir.join("work/out/shot.png").exists());
    assert!(refused(&call(&mut app, "ui.screenshot", json!({"path": site.path("elsewhere/win.png")}))));
    assert!(call(&mut app, "ui.screenshot", json!({"path": site.path("work/out/win.png")})).get("screenshot").is_some());
}

/// Input a request injects (`ui.text`, `ui.key`) runs in a frame of its own, after the request has
/// answered: that frame is held to the roots, and so is the background read it starts.
#[test]
fn input_the_control_channel_injects_is_held_to_the_roots_and_the_persons_is_not() {
    let site = Site::new("roots-input");
    let ctx = egui::Context::default();
    let mut app = site.app();
    pump(&mut app, &ctx);
    let before = app.session.docs.len();
    let secret = site.path("elsewhere/secret.dxf");

    call(&mut app, "ui.text", json!({"text": format!("OPEN {secret}")}));
    call(&mut app, "ui.key", json!({"key": "enter"}));
    pump(&mut app, &ctx);
    assert_eq!(app.session.docs.len(), before, "the agent's typing did not open a file outside the roots");
    assert!(app.session.log.iter().any(|l| l.contains("automation path rejected")), "{:?}", app.session.log);

    // The same line, from the keyboard.
    let mut typed = vec![egui::Event::Text(format!("OPEN {secret}"))];
    typed.extend(enter());
    frame(&mut app, &ctx, typed);
    pump(&mut app, &ctx);
    assert_eq!(app.session.docs.len(), before + 1, "the person's typing is not confined: {:?}", app.session.log);
}

#[test]
fn the_person_opens_any_file_and_a_channel_without_roots_is_not_confined() {
    let site = Site::new("roots-person");
    let mut app = site.app();
    let before = app.session.docs.len();
    app.open_path(std::path::Path::new(&site.path("elsewhere/secret.dxf")));
    let t = Instant::now();
    while app.opening.busy() && t.elapsed() < Duration::from_secs(10) {
        app.finish_opening();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.finish_opening();
    assert_eq!(app.session.docs.len(), before + 1, "a file dialog's answer or a dropped file: {:?}", app.session.log);

    let mut open = CadApp::new(Session::new(), Services::default());
    let r = call(&mut open, "app.open", json!({"path": site.path("elsewhere/secret.dxf")}));
    assert_eq!(r["ok"], true, "no roots, no confinement: {r}");
}
