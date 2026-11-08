//! A drawing saved where the user zoomed opens there again: saving writes the Model tab's view on
//! screen as the view the drawing is saved at (`$VIEWCTR`/`$VIEWSIZE` and the first `*Active`
//! VPORT record), which OPEN reads (#1602).

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{Session, View};
use cadcraft_geom::Vec2;
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-save-view-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn same(a: View, b: View) -> bool {
    a.center.near(b.center, 1e-6) && (a.height - b.height).abs() < 1e-6
}

#[test]
fn the_view_on_screen_is_where_the_drawing_opens_again() {
    hooks();
    let dir = dir("roundtrip");
    for name in ["plan.dxf", "plan.dwg"] {
        let path = dir.join(name).to_string_lossy().into_owned();
        let mut s = plan();
        let undo = s.state().unwrap().undo.len();
        assert!(s.state_mut().unwrap().set_view(ZOOMED));
        s.execute("saveas", &json!({ "path": path })).unwrap();
        assert!(!s.state().unwrap().is_dirty(), "{name}: keeping the view is no change");
        assert_eq!(s.state().unwrap().undo.len(), undo, "{name}: and no undo step");
        let v = opened_at(&path);
        assert!(same(v, ZOOMED), "{name}: opened at {v:?}");
        if name.ends_with(".dxf") {
            let d = cadcraft_io::read(&std::fs::read(&path).unwrap(), name).unwrap();
            assert_eq!(d.header.saved_view(), Some((ZOOMED.center, ZOOMED.height)), "{name}: $VIEWCTR/$VIEWSIZE");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drawing without unsaved changes, zoomed and saved again, stays without them: the view isn't
/// an edit, and the next save writes the newer view.
#[test]
fn saving_a_clean_drawing_at_a_new_view_leaves_it_clean() {
    hooks();
    let dir = dir("clean");
    let path = dir.join("plan.dxf").to_string_lossy().into_owned();
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
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saved from a layout, the drawing keeps the Model tab's view as it was left, not the sheet's.
#[test]
fn saving_from_a_layout_keeps_the_model_view() {
    hooks();
    let dir = dir("layout");
    let path = dir.join("plan.dxf").to_string_lossy().into_owned();
    let mut s = plan();
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    assert!(s.state_mut().unwrap().set_view(View { center: Vec2::new(150.0, 100.0), height: 250.0, twist: 0.0 }));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at {v:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// In the Block Editor the view on screen is the block's: saving keeps the drawing's own view.
#[test]
fn saving_in_the_block_editor_keeps_the_drawings_view() {
    hooks();
    let dir = dir("bedit");
    let path = dir.join("plan.dxf").to_string_lossy().into_owned();
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
    let _ = std::fs::remove_dir_all(&dir);
}

/// With tiled viewports the saved view is the first tile's, as it is now: the first `*Active`
/// record is the one the drawing opens on, so another tile's view must not land there.
#[test]
fn tiled_viewports_save_the_first_tiles_view() {
    hooks();
    let dir = dir("tiles");
    let path = dir.join("plan.dxf").to_string_lossy().into_owned();
    let mut s = plan();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    // The first tile zoomed in, then the second one current and zoomed elsewhere.
    assert!(s.state_mut().unwrap().set_view(ZOOMED));
    s.activate_tile(1).unwrap();
    let other = View { center: Vec2::new(5050.0, 5030.0), height: 80.0, twist: 0.0 };
    assert!(s.state_mut().unwrap().set_view(other));
    s.execute("saveas", &json!({ "path": path })).unwrap();
    let v = opened_at(&path);
    assert!(same(v, ZOOMED), "opened at the first tile's view, not the current one's: {v:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
