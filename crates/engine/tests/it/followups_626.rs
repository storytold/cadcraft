//! Drafting follow-ups (#626): isometric snap gives way to PolarSnap, and MVIEW Restore puts a
//! model space viewport configuration on a layout.

use cadcraft_doc::{EntityKind, Space, Viewport, VportTile};
use cadcraft_engine::Session;
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::snap::tracking::CursorQuery;
use serde_json::{Value, json};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

/// Move the cursor to `raw` as the canvas does.
fn hover(s: &mut Session, raw: Vec2, base: Option<Vec2>) -> Vec2 {
    let q = CursorQuery { base, deferred: false, corner: false, aperture: 0.5, tol: 0.1, now: 100.0, shift: false };
    let r = s.snap_cursor(raw, &q);
    s.cursor = r.point;
    r.point
}

#[test]
fn isometric_snap_gives_way_to_polarsnap() {
    let mut s = Session::new();
    setvar(&mut s, "OSMODE", json!(0));
    s.execute("isodraft", &json!({ "plane": "left" })).unwrap();
    setvar(&mut s, "SNAPMODE", json!(1));
    setvar(&mut s, "SNAPUNIT", json!([1, 1]));
    s.cmdline("line 0,0").unwrap();
    let base = Some(v(0.0, 0.0));
    // Grid snap (SNAPTYPE 0): the isometric lattice.
    let off = v(0.3, 2.2);
    let lattice = hover(&mut s, off, base);
    assert!(!lattice.near(off, 1e-6), "snapped to the lattice: {lattice:?}");
    // PolarSnap: steps of POLARDIST along the polar paths, and the cursor is free off them.
    setvar(&mut s, "SNAPTYPE", json!(1));
    setvar(&mut s, "POLARDIST", json!(2.5));
    let p = hover(&mut s, v(0.02, 6.1), base);
    assert!(p.near(v(0.0, 5.0), 1e-9), "PolarSnap along 90°: {p:?}");
    let q = hover(&mut s, off, base);
    assert!(q.near(off, 1e-9), "off the paths the isometric lattice no longer snaps: {q:?}");
    // Back to grid snap: the lattice again.
    setvar(&mut s, "SNAPTYPE", json!(0));
    assert!(hover(&mut s, off, base).near(lattice, 1e-9));
}

/// The layout viewports with the given handles, in order.
fn viewports(s: &Session, layout: &str, handles: &Value) -> Vec<Viewport> {
    let d = s.doc().unwrap();
    let st = d.space(&Space::Paper(layout.into())).unwrap();
    handles
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            let h = cadcraft_doc::Handle::parse_hex(h.as_str().unwrap()).unwrap();
            match &st.get(h).unwrap().kind {
                EntityKind::Viewport(v) => v.clone(),
                other => panic!("not a viewport: {other:?}"),
            }
        })
        .collect()
}

/// A model space with two side-by-side tiles saved as "Two" (the left tile zoomed in).
fn two_tiles() -> (Session, Vec<VportTile>) {
    let mut s = Session::new();
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    s.execute("-vports", &json!({ "option": "2", "arrangement": "vertical" })).unwrap();
    s.execute("zoom", &json!({ "mode": "center", "center": [10, 10], "height": 8 })).unwrap();
    s.execute("-vports", &json!({ "option": "save", "name": "Two" })).unwrap();
    let tiles = s.doc().unwrap().vports.iter().find(|c| c.name == "Two").unwrap().tiles.clone();
    assert_eq!(tiles.len(), 2);
    (s, tiles)
}

fn check_restored(vps: &[Viewport], tiles: &[VportTile], window: Bounds2) {
    assert_eq!(vps.len(), tiles.len());
    for (vp, t) in vps.iter().zip(tiles) {
        let at = |f: Vec2| v(window.min.x + f.x * window.width(), window.min.y + f.y * window.height());
        let (lo, hi) = (at(t.min), at(t.max));
        assert!(vp.center.xy().near((lo + hi) * 0.5, 1e-9), "{vp:?} {t:?}");
        assert!((vp.width - (hi.x - lo.x)).abs() < 1e-9 && (vp.height - (hi.y - lo.y)).abs() < 1e-9);
        assert!(vp.view_center.near(t.center, 1e-9), "each viewport shows its tile's view");
        assert!(vp.view_height >= t.height - 1e-9);
    }
}

#[test]
fn mview_restore_puts_a_saved_configuration_on_a_layout() {
    let (mut s, tiles) = two_tiles();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let window = Bounds2::new(v(10.0, 10.0), v(210.0, 110.0));
    let r = s.execute("mview", &json!({ "option": "restore", "name": "two", "p1": [10, 10], "p2": [210, 110] })).unwrap();
    assert_eq!(r["configuration"], "two");
    check_restored(&viewports(&s, "Layout1", &r["viewports"]), &tiles, window);

    // Typed: name (? lists the saved ones), then two corners.
    s.cmdline("mview").unwrap();
    s.cmdline("r").unwrap();
    assert!(s.prompt_text().contains("Enter viewport configuration name"), "{}", s.prompt_text());
    s.cmdline("?").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Two")), "{:?}", s.log);
    assert!(s.cmdline("nope").is_err() || s.running.is_some(), "an unknown name asks again");
    assert!(s.prompt_text().contains("configuration name"), "{}", s.prompt_text());
    s.cmdline("Two").unwrap();
    s.cmdline("10,120").unwrap();
    s.cmdline("110,170").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l.contains("2 viewport(s) restored")), "{:?}", s.log);

    // Enter takes *Active (the tiles on show) and Fit.
    let before = s.doc().unwrap().space(&Space::Paper("Layout1".into())).unwrap().iter().count();
    s.cmdline("mview").unwrap();
    s.cmdline("r").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let after = s.doc().unwrap().space(&Space::Paper("Layout1".into())).unwrap().iter().count();
    assert_eq!(after, before + 2, "the model space shows two tiles");

    // Unknown names, model space and hostile windows are refused.
    assert!(s.execute("mview", &json!({ "option": "restore", "name": "nope" })).is_err());
    assert!(s.execute("mview", &json!({ "option": "restore", "p1": [0, 0], "p2": [0, 5] })).is_err());
    assert!(s.execute("mview", &json!({ "option": "restore", "p1": [0, 0], "p2": [1e300, 5] })).is_err());
    assert!(s.execute("mview", &json!({ "option": "restore", "layout": "Model" })).is_err());
}

#[test]
fn mview_restore_ignores_unusable_tiles_from_files() {
    let (mut s, _) = two_tiles();
    let bad = [
        VportTile { min: v(f64::NAN, 0.0), max: v(1.0, 1.0), center: v(0.0, 0.0), height: 1.0, aspect: 1.0 },
        VportTile { min: v(0.5, 0.5), max: v(0.5, 0.9), center: v(0.0, 0.0), height: 1.0, aspect: 1.0 },
        VportTile { min: v(-3.0, -3.0), max: v(9.0, 9.0), center: v(f64::INFINITY, 0.0), height: -1.0, aspect: f64::NAN },
    ];
    if let Some(c) = s.doc_mut().unwrap().vports.iter_mut().find(|c| c.name == "Two") {
        c.tiles = bad.to_vec();
    }
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let r = s.execute("mview", &json!({ "option": "restore", "name": "Two", "p1": [0, 0], "p2": [100, 50] })).unwrap();
    let vps = viewports(&s, "Layout1", &r["viewports"]);
    assert_eq!(vps.len(), 1, "only the third tile has an area (clamped to the window)");
    assert!((vps[0].width - 100.0).abs() < 1e-9 && vps[0].view_height.is_finite() && vps[0].view_height > 0.0);
}
