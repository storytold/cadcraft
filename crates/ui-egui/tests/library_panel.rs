//! The Library palette: a folder of block files, one group per sub-folder, inserted with a click.
//! LIBRARY and Window ▸ Library open it under the Tool Sets; clicking a file starts INSERT at its
//! insertion point prompt with the block defined from that file.

use std::path::Path;

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Circle, Drawing, Entity, EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::palettes::library;
use cadcraft_ui_egui::{CadApp, Services, docking::Panel};
use serde_json::{Value, json};

/// A drawing file in a test format: its bytes are the radius of one circle at the origin.
fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let radius: f64 = String::from_utf8_lossy(bytes).trim().parse().map_err(|_| "bad test file")?;
    let mut d = Drawing::new_metric();
    d.model.push(Entity::new(Handle(0x50), EntityKind::Circle(Circle { center: Vec3::ZERO, radius })));
    Ok(d)
}

/// A library folder with two UB sections (their files hold their "radius") and one PFC.
fn folder() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-library-panel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (f, r) in [("UB/310UB40.dxf", "155"), ("UB/150UB14.dxf", "75"), ("PFC/100PFC.dxf", "50")] {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r).unwrap();
    }
    dir
}

fn app(dir: &Path) -> CadApp {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.library_dir = Some(dir.to_string_lossy().into_owned());
    app
}

#[test]
fn library_opens_under_the_tool_sets_and_toggles() {
    let dir = folder();
    let mut app = app(&dir);
    assert!(!app.ui.docking.layout.contains(&Panel::Library));
    app.cmdline("LIBRARY");
    assert!(app.ui.docking.layout.contains(&Panel::Library), "typed LIBRARY opens the palette");
    assert!(app.ui.docking.layout.floating.iter().all(|w| !w.contains(&Panel::Library)), "docked, not floating");
    let layout = app.ui.docking.layout.clone();
    app.run("ui.panel.library", Value::Null).unwrap();
    assert_eq!(app.ui.docking.layout, layout, "an explicit open is idempotent");
    app.run("ui.toggle.library", Value::Null).unwrap();
    assert!(!app.ui.docking.layout.contains(&Panel::Library));
    app.run("ui.toggle.library", Value::Null).unwrap();
    assert!(app.ui.docking.layout.contains(&Panel::Library));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clicking_a_section_defines_its_block_and_asks_for_the_insertion_point() {
    let dir = folder();
    let mut app = app(&dir);
    let groups = library::scan(&dir);
    assert_eq!(groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["PFC", "UB"]);
    let ub = groups.iter().find(|g| g.name == "UB").unwrap();
    assert_eq!(ub.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["150UB14", "310UB40"]);
    let item = ub.items.iter().find(|i| i.label == "310UB40").unwrap();

    // A command already running is replaced, as when a block is picked in the Blocks window.
    app.start("line");
    library::insert(&mut app, item);
    assert!(app.session.running.as_ref().is_some_and(|r| r.id == "insert"));
    assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
    let d = app.session.doc().unwrap();
    let block = d.block("310UB40").expect("the block is defined from the file before the point is asked");
    let radii: Vec<f64> = block.entities.iter().filter_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.radius) } else { None }).collect();
    assert_eq!(radii, vec![155.0], "the block holds the file's model space");

    app.session.input(Input::Point(Vec2::new(5.0, 6.0))).unwrap();
    for _ in 0..3 {
        app.session.input(Input::Enter).unwrap();
    }
    assert!(app.session.running.is_none());
    let d = app.session.doc().unwrap();
    let ins: Vec<_> = d.model.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).collect();
    assert_eq!(ins.len(), 1);
    assert_eq!(ins[0].block, "310UB40");
    assert!((ins[0].insert.x - 5.0).abs() < 1e-9 && (ins[0].insert.y - 6.0).abs() < 1e-9);

    // A second click re-reads the file (same definition) and asks for a point again.
    library::insert(&mut app, item);
    assert_eq!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_leaves_insert_asking_for_a_name() {
    let dir = folder();
    let mut app = app(&dir);
    let item = library::Item { label: "GONE".into(), path: dir.join("UB/gone.dxf").to_string_lossy().into_owned() };
    library::insert(&mut app, &item);
    assert!(app.session.running.as_ref().is_some_and(|r| r.id == "insert"));
    assert_ne!(app.session.current_prompt().map(|p| p.message), Some("Specify insertion point".to_string()));
    assert!(app.session.doc().unwrap().block("GONE").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_library_folder_is_a_preference() {
    let mut a = CadApp::new(Session::new(), Services::default());
    a.ui.library_dir = Some("C:/blocks".into());
    let prefs = a.prefs_json();
    assert!(prefs.contains(r#""libraryDir":"C:/blocks""#), "{prefs}");
    let mut b = CadApp::new(Session::new(), Services::default());
    b.load_prefs(&prefs);
    assert_eq!(b.ui.library_dir.as_deref(), Some("C:/blocks"));
    b.load_prefs(&json!({"libraryDir": ""}).to_string());
    assert_eq!(b.ui.library_dir.as_deref(), Some("C:/blocks"), "an empty value is ignored");
}
