//! Recently opened files (#858): File ▸ Open Recent and the Start page list the drawings opened
//! from a path, newest first, and keep them across restarts.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::menus::{self, Entry};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Rect, Shape, vec2};
use egui_kittest::Harness;

/// Two drawing files on disk (the reader below accepts anything, so their contents don't matter).
fn drawings(test: &str) -> (String, String) {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(Vec::new()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-recent-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| {
        let p = dir.join(name);
        std::fs::write(&p, b"0\nEOF\n").unwrap();
        p.to_string_lossy().to_string()
    };
    (path("plan.dxf"), path("section.dwg"))
}

/// Wait for the drawings being read in the background to be added (or reported).
fn settle(app: &mut CadApp) {
    let t = std::time::Instant::now();
    while app.opening.busy() && t.elapsed() < std::time::Duration::from_secs(10) {
        app.finish_opening();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.finish_opening();
}

/// File ▸ Open Recent: (label, command id) of each item.
fn open_recent(app: &CadApp) -> Vec<(String, String)> {
    let file = menus::tree(app).into_iter().find(|(m, _)| m == "File").map(|(_, e)| e).unwrap_or_default();
    let Some(Entry::Sub { children, .. }) = file.into_iter().find(|e| matches!(e, Entry::Sub { label, .. } if label == "Open Recent")) else {
        return Vec::new();
    };
    children
        .into_iter()
        .filter_map(|e| match e {
            Entry::Item { label, id, .. } => Some((label, id)),
            Entry::Sub { .. } => None,
        })
        .collect()
}

#[test]
fn opened_drawings_are_listed_newest_first_and_survive_a_restart() {
    let (plan, section) = drawings("menu");
    let mut app = CadApp::new(Session::empty(), Services::default());
    assert!(app.ui.recent_files.is_empty());

    app.open_path(std::path::Path::new(&plan));
    settle(&mut app);
    app.open_path(std::path::Path::new(&section));
    settle(&mut app);
    app.open_path(std::path::Path::new(&plan));
    settle(&mut app);
    assert_eq!(app.ui.recent_files, [plan.clone(), section.clone()], "newest first, each file once");
    // A file that could not be opened is not listed.
    app.open_path(std::path::Path::new("/nonexistent/missing.dxf"));
    settle(&mut app);
    assert_eq!(app.ui.recent_files, [plan.clone(), section.clone()]);

    let items = open_recent(&app);
    let labels: Vec<_> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["plan.dxf", "section.dwg", "Clear Recent Files"]);

    // Choosing an item opens that file and moves it to the top.
    let before = app.session.docs.len();
    menus::activate(&mut app, &items[1].1);
    settle(&mut app);
    assert_eq!(app.session.docs.len(), before + 1);
    assert_eq!(app.session.state().ok().and_then(|s| s.path.clone()), Some(section.clone()));
    assert_eq!(app.ui.recent_files, [section.clone(), plan.clone()]);

    // Kept with the preferences the host stores across restarts.
    let mut restarted = CadApp::new(Session::empty(), Services::default());
    restarted.load_prefs(&app.prefs_json());
    assert_eq!(restarted.ui.recent_files, [section.clone(), plan.clone()]);

    let clear = items.iter().find(|(l, _)| l == "Clear Recent Files").map(|(_, id)| id.clone()).unwrap();
    menus::activate(&mut app, &clear);
    assert!(app.ui.recent_files.is_empty());
    assert_eq!(open_recent(&app).iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["Clear Recent Files"]);
}

/// Painted texts and where they are.
fn texts(shape: &Shape, out: &mut Vec<(String, Rect)>) {
    match shape {
        Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect())),
        Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

#[test]
fn start_page_lists_recent_files_and_opens_one_on_click() {
    let (plan, section) = drawings("start");
    let mut app = CadApp::new(Session::empty(), Services::default());
    app.ui.recent_files = vec![section.clone(), plan.clone()];
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let mut found = Vec::new();
    for s in &h.output().shapes {
        texts(&s.shape, &mut found);
    }
    assert!(!found.iter().any(|(t, _)| t == "There is nothing here yet."), "{found:?}");
    let names: Vec<_> = found.iter().filter(|(t, _)| t == "section.dwg" || t == "plan.dxf").collect();
    assert_eq!(names.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["section.dwg", "plan.dxf"], "newest first");

    let at = names[1].1.center();
    h.hover_at(at);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    let t = std::time::Instant::now();
    while h.state().opening.busy() && t.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.run_steps(1);
    }
    h.run_steps(1);
    let app = h.state();
    assert_eq!(app.session.state().ok().and_then(|s| s.path.clone()), Some(plan.clone()));
    assert_eq!(app.ui.recent_files, [plan, section]);
}
