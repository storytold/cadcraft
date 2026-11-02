//! Tiled model space viewports (#463): -VPORTS SIngle, 2/3/4, Join, Save, Restore, Delete and ?,
//! View > Viewports > n in the Model tab, the current tile and its own view. (The DXF VPORT table
//! round trip is tested in `cadcraft-io`'s `view_tables_roundtrip`.)

use cadcraft_engine::doc::{EntityKind, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{ModelTile, Session, View};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s.execute("view.set", &json!({ "center": [10, 10], "height": 20 })).unwrap();
    s
}

fn typed(s: &mut Session, lines: &[&str]) {
    for l in lines {
        s.cmdline(l).unwrap();
    }
}

fn tiles(s: &Session) -> Vec<ModelTile> {
    s.model_tiles()
}

fn current(s: &Session) -> usize {
    tiles(s).iter().position(|t| t.active).unwrap()
}

fn rects(s: &Session) -> Vec<[f64; 4]> {
    let mut r: Vec<[f64; 4]> = tiles(s).iter().map(|t| [t.min.x, t.min.y, t.max.x, t.max.y]).collect();
    r.sort_by(|a, b| a.partial_cmp(b).unwrap());
    r
}

fn view(s: &Session) -> View {
    s.state().unwrap().view()
}

#[test]
fn split_activate_and_each_tile_keeps_its_view() {
    let mut s = session();
    assert!(tiles(&s).is_empty(), "one view: no tiles");
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    assert_eq!(rects(&s), vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]]);
    // Every new tile starts with the view that was split.
    assert!(tiles(&s).iter().all(|t| t.view == View { center: Vec2::new(10.0, 10.0), height: 20.0 }));
    let first = current(&s);
    let other = 1 - first;
    s.activate_tile(other).unwrap();
    assert_eq!(current(&s), other);
    s.execute("view.set", &json!({ "center": [50, 25], "height": 5 })).unwrap();
    s.execute("vports.activate", &json!({ "index": first + 1 })).unwrap();
    assert_eq!(view(&s), View { center: Vec2::new(10.0, 10.0), height: 20.0 });
    assert_eq!(tiles(&s)[other].view, View { center: Vec2::new(50.0, 25.0), height: 5.0 });
    // Switching tiles changes no drawing data; arranging them is an undo step.
    assert_eq!(s.state().unwrap().undo.len(), 2, "line + VPORTS");
    s.undo().unwrap();
    assert!(tiles(&s).is_empty());
    assert!(s.execute("vports.activate", &json!({ "index": 2 })).is_err());
}

#[test]
fn typed_three_right_then_join_and_single() {
    let mut s = session();
    // Enter takes 3, Enter takes Right: a big tile on the right (current) and two on the left.
    typed(&mut s, &["-vports", "", ""]);
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(rects(&s), vec![[0.0, 0.0, 0.5, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]]);
    let cur = tiles(&s)[current(&s)];
    assert_eq!((cur.min, cur.max), (Vec2::new(0.5, 0.0), Vec2::new(1.0, 1.0)));
    // Join the two left tiles: the dominant one's view wins.
    let t = tiles(&s);
    let top = t.iter().position(|t| t.min == Vec2::new(0.0, 0.5)).unwrap();
    let bottom = t.iter().position(|t| t.min == Vec2::ZERO).unwrap();
    s.activate_tile(top).unwrap();
    s.execute("view.set", &json!({ "center": [1, 2], "height": 3 })).unwrap();
    typed(&mut s, &["-vports", "j", ""]);
    // A click in the other tile answers "Select viewport to join".
    s.click_tile(bottom).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(rects(&s), vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]]);
    assert_eq!(view(&s), View { center: Vec2::new(1.0, 2.0), height: 3.0 });
    // Tiles that don't make a rectangle can't be joined.
    s.execute("-vports", &json!({ "option": "3", "arrangement": "above" })).unwrap();
    let t = tiles(&s);
    // The left tile split: bottom left (0,0)-(0.25,0.5) and the right tile don't touch.
    let (a, b) = (t.iter().position(|t| t.max.x <= 0.25).unwrap(), t.iter().position(|t| t.min.x >= 0.5).unwrap());
    assert!(s.execute("-vports", &json!({ "option": "join", "dominant": a + 1, "join": b + 1 })).is_err());
    // SIngle keeps the current tile's view.
    s.execute("view.set", &json!({ "center": [7, 7], "height": 7 })).unwrap();
    typed(&mut s, &["-vports", "si"]);
    assert!(tiles(&s).is_empty());
    assert_eq!(view(&s), View { center: Vec2::new(7.0, 7.0), height: 7.0 });
}

#[test]
fn named_configurations_save_restore_list_delete() {
    let mut s = session();
    s.execute("-vports", &json!({ "option": "4" })).unwrap();
    assert_eq!(tiles(&s).len(), 4);
    typed(&mut s, &["-vports", "s", "Quad"]);
    s.execute("-vports", &json!({ "option": "single" })).unwrap();
    assert!(tiles(&s).is_empty());
    typed(&mut s, &["-vports", "r", "quad"]);
    assert_eq!(tiles(&s).len(), 4);
    let list = s.execute("-vports", &json!({ "option": "list" })).unwrap();
    assert_eq!(list["configurations"][0]["name"], json!("Quad"));
    assert_eq!(list["tiles"].as_array().unwrap().len(), 4);
    typed(&mut s, &["-vports", "d", "Q*"]);
    assert!(s.doc().unwrap().vports.iter().all(|c| c.name != "Quad"));
    for bad in [
        json!({ "option": "save", "name": "*Active" }),
        json!({ "option": "restore", "name": "nope" }),
        json!({ "option": "join", "join": 99 }),
        json!({ "option": "3", "arrangement": "diagonal" }),
        json!({ "option": "7" }),
    ] {
        assert!(s.execute("-vports", &bad).is_err(), "{bad}");
    }
}

#[test]
fn menu_viewports_in_the_model_tab_and_limits() {
    let mut s = session();
    // View > Viewports > 2 Viewports asks for the arrangement in the Model tab.
    s.start("vports.2").unwrap();
    typed(&mut s, &["h"]);
    assert_eq!(rects(&s), vec![[0.0, 0.0, 1.0, 0.5], [0.0, 0.5, 1.0, 1.0]]);
    s.execute("vports.1", &json!({})).unwrap();
    assert!(tiles(&s).is_empty());
    s.execute("vports.3", &json!({ "arrangement": "vertical" })).unwrap();
    // MVIEW / VPORTS JSON stay layout commands.
    assert!(s.execute("vports", &json!({ "count": 2 })).is_err());
    assert_eq!(tiles(&s).len(), 3);
    // Splitting the current tile again and again stops at a bound, without a panic.
    let mut errors = 0;
    for _ in 0..40 {
        if s.execute("-vports", &json!({ "option": "4" })).is_err() {
            errors += 1;
        }
    }
    assert!(errors > 0 && tiles(&s).len() <= 64, "{} tiles", tiles(&s).len());
    // A layout has no tiles; -VPORTS there makes layout viewports (MVIEW).
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    assert!(tiles(&s).is_empty());
    s.execute("-vports", &json!({ "option": "2" })).unwrap();
    let vps = s.doc().unwrap().layout("Layout1").unwrap().entities.iter().filter(|e| matches!(e.kind, EntityKind::Viewport(_))).count();
    assert!(vps >= 2, "{vps}");
    s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
    assert_eq!(s.layout_space(), Space::Model);
    assert!(!tiles(&s).is_empty(), "the tiles come back with the Model tab");
}
