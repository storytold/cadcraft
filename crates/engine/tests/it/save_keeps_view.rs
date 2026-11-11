//! A drawing saved where the user zoomed opens there again: saving writes the Model tab's view on
//! screen as the view the drawing is saved at (`$VIEWCTR`/`$VIEWSIZE` and the first `*Active`
//! VPORT record), which OPEN reads (#1602).

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{Session, View};
use cadcraft_geom::Vec2;
use serde_json::json;

#[path = "../support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

fn hooks() {
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// Two lines far apart, so the extents are nowhere near a zoomed-in view.
fn plan() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [1000, 600]] })).unwrap();
    s.execute("line", &json!({ "points": [[5000, 5000], [5100, 5060]] })).unwrap();
    s
}

const ZOOMED: View = View { center: Vec2::new(40.0, 30.0), height: 12.0, twist: 0.0 };

/// The view the file at `path` opens on.
fn opened_at(path: &str) -> View {
    let mut t = Session::empty();
    t.open_drawing(cadcraft_io::read(&std::fs::read(path).unwrap(), path).unwrap(), "reopened", Some(path.to_string()));
    t.state().unwrap().view()
}

/// Center and height only: the saved view (`$VIEWCTR`/`$VIEWSIZE`) carries no twist, and OPEN
/// reads it untwisted (#1602), so twist is not compared.
fn same(a: View, b: View) -> bool {
    a.center.near(b.center, 1e-6) && (a.height - b.height).abs() < 1e-6
}

#[test]
fn the_view_on_screen_is_where_the_drawing_opens_again() {
    hooks();
    let dir = TempDir::new("save-view-roundtrip");
    for name in ["plan.dxf", "plan.dwg"] {
        let path = dir.path_str(name);
        let mut s = plan();
        let undo = s.state().unwrap().undo.len();
        assert!(s.state_mut().unwrap().set_view(ZOOMED));
        s.execute("saveas", &json!({ "path": path })).unwrap();
        assert!(!s.state().unwrap().is_dirty(), "{name}: keeping the view is no change");
        assert_eq!(s.state().unwrap().undo.len(), undo, "{name}: and no undo step");
        let v = opened_at(&path);
        assert!(same(v, ZOOMED), "{name}: opened at {v:?}");
        let d = cadcraft_io::read(&std::fs::read(&path).unwrap(), name).unwrap();
        assert_eq!(d.header.saved_view(), Some((ZOOMED.center, ZOOMED.height)), "{name}: $VIEWCTR/$VIEWSIZE");
    }
}

/// A drawing without unsaved changes, zoomed and saved again, stays without them: the view isn't
/// an edit, and the next save writes the newer view.
#[test]
fn saving_a_clean_drawing_at_a_new_view_leaves_it_clean() {
    hooks();
    let dir = TempDir::new("save-view-clean");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let mut t = Session::empty();
    t.open_drawing(cadcraft_io::read(&std::fs::read(&path).unwrap(), &path).unwrap(), "plan.dxf", Some(path.clone()));
    assert!(t.state_mut().unwrap().set_view(ZOOMED));
    assert!(!t.state().unwrap().is_dirty());
    t.execute("qsave", &json!({})).unwrap();
    assert!(!t.state().unwrap().is_dirty(), "the drawing is still clean");
    assert!(t.state().unwrap().undo.is_empty(), "saving made no undo step");
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at {v:?}");
}

/// Saved from a layout, the drawing keeps the Model tab's view as it was left, not the sheet's.
#[test]
fn saving_from_a_layout_keeps_the_model_view() {
    hooks();
    let dir = TempDir::new("save-view-layout");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    assert!(s.state_mut().unwrap().set_view(View { center: Vec2::new(150.0, 100.0), height: 250.0, twist: 0.0 }));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at {v:?}");
}

/// In the Block Editor the view on screen is the block's: saving keeps the drawing's own view.
#[test]
fn saving_in_the_block_editor_keeps_the_drawings_view() {
    hooks();
    let dir = TempDir::new("save-view-bedit");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    let l = s.execute("line", &json!({ "points": [[10, 10], [12, 10]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [l] })).unwrap();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    assert!(s.state_mut().unwrap().set_view(View { center: Vec2::new(1.0, 0.0), height: 3.0, twist: 0.0 }));
    s.execute("qsave", &json!({})).unwrap();
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "the drawing's view, not the block's: {v:?}");
}

/// With tiled viewports the saved view is the first tile's, as it is now: the first `*Active`
/// record is the one the drawing opens on, so another tile's view must not land there.
#[test]
fn tiled_viewports_save_the_first_tiles_view() {
    hooks();
    let dir = TempDir::new("save-view-tiles");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    // The first tile zoomed in, then the second one current and zoomed elsewhere.
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.activate_tile(1).unwrap();
    let other = View { center: Vec2::new(5050.0, 5030.0), height: 80.0, twist: 0.0 };
    assert!(s.state_mut().unwrap().set_view(other));
    let second = tiles(s.state().unwrap().drawing())[1];
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at the first tile's view, not the current one's: {v:?}");
    // The other tile keeps its record as the drawing holds it: saving writes only the first one.
    let saved = tiles(&cadcraft_io::read(&std::fs::read(&path).unwrap(), &path).unwrap())[1];
    assert!(saved.center.near(second.center, 1e-6) && (saved.height - second.height).abs() < 1e-6, "second tile {saved:?}, was {second:?}");
}

/// The tiles of the drawing's active tiled configuration, as (center, height).
fn tiles(d: &cadcraft_engine::doc::Drawing) -> Vec<Tile> {
    let cfg = d.vports.iter().find(|c| c.is_active() && c.tiles.len() > 1).expect("a tiled configuration");
    cfg.tiles.iter().map(|t| Tile { center: t.center, height: t.height }).collect()
}

#[derive(Clone, Copy, Debug)]
struct Tile {
    center: Vec2,
    height: f64,
}

/// SAVE, typed: it resolves to QSAVE, and with a path it keeps the view like SAVEAS.
#[test]
fn plain_save_keeps_the_view() {
    hooks();
    let dir = TempDir::new("save-view-save");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    let save = cadcraft_engine::cmd::resolve_alias("save");
    assert_eq!(save, "qsave");
    s.execute(&save, &json!({ "path": path })).unwrap();
    assert!(!s.state().unwrap().is_dirty());
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at {v:?}");
}

/// Nothing to keep: an empty new drawing, and a save at the view already saved, stay clean and
/// make no undo step.
#[test]
fn saving_without_a_new_view_changes_nothing() {
    hooks();
    let dir = TempDir::new("save-view-nothing");
    let empty = dir.path_str("empty.dxf");
    let mut s = Session::new();
    s.execute("saveas", &json!({ "path": empty })).unwrap();
    assert!(!s.state().unwrap().is_dirty(), "an empty drawing saves clean");
    assert!(s.state().unwrap().undo.is_empty());

    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let undo = s.state().unwrap().undo.len();
    let before = s.state().unwrap().drawing().clone();
    s.execute("qsave", &json!({})).unwrap();
    assert!(!s.state().unwrap().is_dirty());
    assert_eq!(s.state().unwrap().undo.len(), undo, "no undo step");
    assert!(std::sync::Arc::ptr_eq(&before, s.state().unwrap().drawing()), "the drawing is not touched when the view is already saved");
}

/// The `.bak` is the previous file as it was, at its own view; only the new file has the new one.
#[test]
fn the_backup_keeps_the_previous_view() {
    hooks();
    let dir = TempDir::new("save-view-bak");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let first = std::fs::read(&path).unwrap();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.execute("qsave", &json!({})).unwrap();
    let bak = cadcraft_engine::cmd::file::backup_path(std::path::Path::new(&path));
    assert_eq!(std::fs::read(&bak).unwrap(), first, "the .bak is the previous file, byte for byte");
    assert!(same(opened_at(&path), ZOOMED));
}

/// Autosave writes the drawing as it is: the view on screen goes into a file only on a real save.
#[test]
fn autosave_does_not_take_the_view_on_screen() {
    hooks();
    let mut s = plan();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    let mut a = cadcraft_engine::autosave::Autosaver::new("77-view");
    assert!(a.plan(&s, 0.0, 1, "/auto").is_empty(), "the change starts the clock");
    let jobs = a.plan(&s, 61.0, 1, "/auto");
    let doc = jobs
        .iter()
        .find_map(|j| match j {
            cadcraft_engine::autosave::Job::Write { doc, .. } => Some(doc.clone()),
            cadcraft_engine::autosave::Job::Delete { .. } => None,
        })
        .expect("an autosave");
    assert_ne!(doc.header.saved_view(), Some((ZOOMED.center, ZOOMED.height)));
    assert!(s.state().unwrap().is_dirty(), "and the drawing keeps its unsaved changes");
}

/// A save that fails leaves the file on disk as it was. A clean drawing stays clean: the view is
/// not an edit, so there is nothing a later close should ask to save (the next save writes it).
#[test]
fn a_failed_save_leaves_the_file_and_a_clean_drawing_alone() {
    hooks();
    let dir = TempDir::new("save-view-failed");
    let path = dir.path_str("plan.dxf");
    let mut s = plan();
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let first = std::fs::read(&path).unwrap();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    let taken = dir.path_str("taken.dxf");
    std::fs::create_dir(&taken).unwrap();
    assert!(s.execute("saveas", &json!({ "path": taken })).is_err(), "a folder in the way");
    assert_eq!(std::fs::read(&path).unwrap(), first, "the saved file is untouched");
    assert!(!s.state().unwrap().is_dirty(), "the view alone does not make the drawing dirty");
    assert_eq!(s.state().unwrap().path.as_deref(), Some(path.as_str()), "and it keeps its file name");
}
