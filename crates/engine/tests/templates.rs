//! Drawing templates (#552): SAVEAS to a .dwt, NEW/QNEW from a template file, and the default
//! template (QNEWTEMPLATE), and a layout saved as a template (LAYOUT SAveas). The io hooks here keep drawings in memory: a "file" holds the number
//! of the drawing written, so the test sees exactly what the engine hands the file formats.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;

static FILES: Mutex<Vec<(String, Drawing)>> = Mutex::new(Vec::new());

fn write(d: &Drawing, name: &str) -> Result<Vec<u8>, String> {
    let mut files = FILES.lock().unwrap();
    files.push((name.to_string(), d.clone()));
    Ok(format!("TEST {}", files.len() - 1).into_bytes())
}

fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let text = String::from_utf8_lossy(bytes);
    let i: usize = text.strip_prefix("TEST ").and_then(|n| n.trim().parse().ok()).ok_or("not a drawing")?;
    FILES.lock().unwrap().get(i).map(|f| f.1.clone()).ok_or_else(|| "no such drawing".into())
}

fn hooks() {
    set_io(IoHooks { read, write, plot: None });
}

fn path(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("cadcraft-templates-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().into_owned()
}

/// A drawing with an office layer and a title block circle, saved as a template.
fn office_template(s: &mut Session, name: &str) -> String {
    s.execute("layer.new", &json!({ "name": "A-WALL", "color": 3 })).unwrap();
    s.execute("circle", &json!({ "center": [5, 5], "radius": 2 })).unwrap();
    let p = path(name);
    s.execute("saveas", &json!({ "path": p })).unwrap();
    p
}

#[test]
fn saveas_dwt_then_new_from_the_template() {
    hooks();
    let mut s = Session::new();
    let dwt = office_template(&mut s, "office.dwt");
    // A template is saved like a drawing: the format sees the .dwt name, the drawing is saved.
    assert!(FILES.lock().unwrap().iter().any(|(n, _)| n.ends_with("office.dwt")));
    let st = s.state().unwrap();
    assert!(!st.is_dirty());
    assert_eq!(st.title, "office.dwt");

    let r = s.execute("new", &json!({ "template": dwt })).unwrap();
    let st = s.state().unwrap();
    assert_eq!(r["title"], json!(st.title));
    assert!(st.title.starts_with("Drawing"), "{}", st.title);
    assert!(st.path.is_none(), "an untitled drawing: saving asks for a name");
    assert!(!st.is_dirty());
    assert!(st.doc.layers.iter().any(|l| l.name == "A-WALL"));
    assert_eq!(st.doc.model.iter().count(), 1);
    assert!(s.execute("qsave", &json!({})).is_err(), "QSAVE needs a name: the template stays as it is");
}

#[test]
fn qnewtemplate_is_the_default_for_new_and_qnew() {
    hooks();
    let mut s = Session::new();
    let dwt = office_template(&mut s, "default.dwt");
    let docs = s.docs.len();

    s.execute("setvar", &json!({ "name": "QNEWTEMPLATE", "value": dwt })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "qnewtemplate" })).unwrap()["value"], json!(dwt));
    s.cmdline("qnew").unwrap();
    assert_eq!(s.docs.len(), docs + 1);
    assert!(s.doc().unwrap().layers.iter().any(|l| l.name == "A-WALL"), "QNEW starts from the default template");
    s.execute("new", &json!({})).unwrap();
    assert!(s.doc().unwrap().layers.iter().any(|l| l.name == "A-WALL"));

    // Units asked for, or "." for no template: a blank drawing.
    s.execute("new", &json!({ "metric": true })).unwrap();
    assert!(s.doc().unwrap().layers.iter().all(|l| l.name != "A-WALL"));
    assert_eq!(s.doc().unwrap().header.i64("MEASUREMENT", 0), 1);
    s.execute("new", &json!({ "template": "." })).unwrap();
    assert!(s.doc().unwrap().layers.iter().all(|l| l.name != "A-WALL"));

    s.execute("setvar", &json!({ "name": "QNEWTEMPLATE", "value": "." })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "QNEWTEMPLATE" })).unwrap()["value"], json!(""));
    s.execute("new", &json!({})).unwrap();
    assert!(s.doc().unwrap().layers.iter().all(|l| l.name != "A-WALL"));
}

#[test]
fn a_template_that_cannot_be_read_makes_no_drawing() {
    hooks();
    let mut s = Session::new();
    let docs = s.docs.len();
    let err = s.execute("new", &json!({ "template": "/no/such/folder/office.dwt" })).unwrap_err().to_string();
    assert!(err.contains("office.dwt"), "{err}");
    let junk = path("junk.dwt");
    std::fs::write(&junk, b"not a drawing at all").unwrap();
    assert!(s.execute("new", &json!({ "template": junk })).is_err());
    assert!(s.execute("new", &json!({ "template": 42 })).is_err());
    assert_eq!(s.docs.len(), docs);
}

/// The drawing the io hooks were last handed for a file whose name ends with `name`.
fn written(name: &str) -> Drawing {
    FILES.lock().unwrap().iter().rev().find(|(n, _)| n.ends_with(name)).map(|f| f.1.clone()).unwrap()
}

fn layout_names(d: &Drawing) -> Vec<String> {
    d.layouts.iter().map(|l| l.name.clone()).collect()
}

/// LAYOUT SAveas: the template holds the layout alone, with the named objects it uses, and the
/// drawing it came from keeps everything.
#[test]
fn layout_saveas_writes_the_layout_and_the_named_objects_it_uses() {
    hooks();
    let mut s = Session::new();
    for name in ["TITLE", "PARTS-LAYER", "UNUSED", "WALLS", "NOTES"] {
        s.execute("layer.new", &json!({ "name": name })).unwrap();
    }
    s.execute("layer.current", &json!({ "name": "PARTS-LAYER" })).unwrap();
    let c = s.execute("circle", &json!({ "center": [5, 5], "radius": 2 })).unwrap()["handle"].clone();
    s.execute("block", &json!({ "name": "PARTS", "base": [0, 0], "handles": [c], "keep": "retain" })).unwrap();
    // WALLS has model space objects only, and NOTES none; the sheet's viewport freezes one and recolors the other.
    s.execute("layer.current", &json!({ "name": "WALLS" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [0, 10]] })).unwrap();
    let vp = s.execute("layout.new", &json!({ "name": "Sheet" })).unwrap()["viewport"].clone();
    s.execute("viewport.set", &json!({ "handle": vp, "freeze": ["WALLS"], "colors": { "NOTES": 1 } })).unwrap();
    s.execute("layout.set", &json!({ "name": "Sheet" })).unwrap();
    s.execute("layer.current", &json!({ "name": "TITLE" })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    s.execute("layer.current", &json!({ "name": "0" })).unwrap();
    let sheet_objects = s.doc().unwrap().layout("Sheet").unwrap().entities.iter().count();

    let p = path("sheet.dwt");
    let r = s.execute("layout.saveas", &json!({ "path": p })).unwrap();
    assert_eq!(r["name"], json!("Sheet"), "the current layout by default");
    assert_eq!(r["path"], json!(p));
    let t = written("sheet.dwt");
    assert_eq!(layout_names(&t), ["Sheet"]);
    assert_eq!(t.active_layout.as_deref(), Some("Sheet"));
    assert_eq!(t.model.iter().count(), 0, "no model space objects");
    assert_eq!(t.layout("Sheet").unwrap().entities.iter().count(), sheet_objects, "the layout's viewport and line");
    assert!(t.layer("TITLE").is_some() && t.layer("0").is_some());
    assert!(t.layer("UNUSED").is_none() && t.layer("PARTS-LAYER").is_none(), "unused layers are left out");
    assert!(t.block("PARTS").is_none(), "unused blocks are left out");
    // Layers the viewport freezes or recolors stay, and so do those settings.
    assert!(t.layer("WALLS").is_some() && t.layer("NOTES").is_some());
    let sheet_vp = t.layout("Sheet").unwrap().entities.iter().find_map(|e| match &e.kind {
        cadcraft_engine::doc::EntityKind::Viewport(v) if v.id != 1 => Some(v.clone()),
        _ => None,
    });
    let sheet_vp = sheet_vp.unwrap();
    assert_eq!(sheet_vp.frozen_layers, ["WALLS"]);
    assert_eq!(sheet_vp.layer_colors.len(), 1);

    // The drawing is not changed.
    let d = s.doc().unwrap();
    assert_eq!(layout_names(d), ["Layout1", "Layout2", "Sheet"]);
    assert_eq!(d.model.iter().count(), 2);
    assert!(d.layer("UNUSED").is_some() && d.block("PARTS").is_some());

    // A drawing started from the template has the layout.
    s.execute("new", &json!({ "template": p })).unwrap();
    assert_eq!(layout_names(s.doc().unwrap()), ["Sheet"]);
}

#[test]
fn typed_layout_saveas_asks_for_the_layout_and_the_template_file() {
    hooks();
    let mut s = Session::new();
    s.cmdline("LAYOUT").unwrap();
    s.cmdline("sa").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter layout to save to template <Layout1>:");
    s.cmdline("Layout2").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Enter template file name <Layout2.dwt>:");
    // No extension: the template is a .dwt.
    let p = path("typed");
    s.cmdline(&p).unwrap();
    assert!(s.current_prompt().is_none());
    let saved = format!("{p}.dwt");
    assert!(s.log.iter().any(|l| *l == format!("Layout \"Layout2\" saved to template \"{saved}\".")), "{:?}", s.log);
    assert_eq!(layout_names(&written("typed.dwt")), ["Layout2"]);

    // The JSON form of LAYOUT, other file types, a missing path and an unknown layout.
    let dxf = path("sheet1.dxf");
    let r = s.execute("layout", &json!({ "option": "saveas", "name": "layout1", "path": dxf })).unwrap();
    assert_eq!(r["name"], json!("Layout1"));
    assert_eq!(layout_names(&written("sheet1.dxf")), ["Layout1"]);
    assert!(s.execute("layout.saveas", &json!({ "path": path("sheet.png") })).is_err());
    assert!(s.execute("layout.saveas", &json!({})).is_err());
    assert!(s.execute("layout.saveas", &json!({ "name": "Nope", "path": path("nope.dwt") })).is_err());
}
