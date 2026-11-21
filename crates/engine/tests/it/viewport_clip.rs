//! Clipped layout viewports and MVIEW's options (#453): MVIEW Polygonal / Object / ON / OFF /
//! Lock / Restore / 2 / 3 / 4, VPCLIP, and model space drawn through a clip outline.

use cadcraft_doc::{EntityKind, Handle, Space, Viewport};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

fn layout_viewports(s: &Session) -> Vec<(Handle, Viewport)> {
    let d = s.doc().unwrap();
    d.layout("Layout1")
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 => Some((e.handle, v.clone())),
            _ => None,
        })
        .collect()
}

fn viewport(s: &Session, h: &str) -> Viewport {
    layout_viewports(s).into_iter().find(|(x, _)| x.hex() == h).map(|(_, v)| v).expect("viewport")
}

/// A model of horizontal lines y = 0, 10, … 100 from x = 0 to 100, with Layout1 current and
/// its default viewport erased.
fn session() -> Session {
    let mut s = Session::new();
    s.new_drawing(true);
    s.viewport_px = (1600.0, 900.0);
    for i in 0..=10 {
        let y = f64::from(i) * 10.0;
        s.execute("line", &json!({"points": [[0, y], [100, y]]})).unwrap();
    }
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let old: Vec<String> = layout_viewports(&s).iter().map(|(h, _)| h.hex()).collect();
    if !old.is_empty() {
        s.execute("erase", &json!({"handles": old})).unwrap();
    }
    s
}

/// Model-space vertices drawn on Layout1 (through viewports).
fn content(s: &Session) -> Vec<Vec2> {
    let d = s.doc().unwrap();
    let l = cadcraft_render::build(d, &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
    l.prims.iter().filter(|p| p.handle == cadcraft_render::VIEWPORT_CONTENT).flat_map(|p| l.points(p).to_vec()).collect()
}

/// An L-shaped outline: 10..110 × 10..110 without the top-right quarter.
const L: [[f64; 2]; 6] = [[10.0, 10.0], [110.0, 10.0], [110.0, 60.0], [60.0, 60.0], [60.0, 110.0], [10.0, 110.0]];

#[test]
fn polygonal_viewport_clips_model_space() {
    let mut s = session();
    let r = s.execute("mview", &json!({"option": "polygonal", "points": L})).unwrap();
    let vh = r["viewport"].as_str().unwrap().to_string();
    let v = viewport(&s, &vh);
    // The rectangle is the outline's extents; the boundary is a closed polyline on the sheet.
    assert!(v.center.xy().near(Vec2::new(60.0, 60.0), 1e-9) && (v.width - 100.0).abs() < 1e-9 && (v.height - 100.0).abs() < 1e-9);
    let b = v.clip.expect("clipped");
    assert_eq!(b.hex(), r["boundary"].as_str().unwrap());
    assert!(matches!(s.doc().unwrap().entity(b).map(|e| &e.kind), Some(EntityKind::LwPolyline(p)) if p.closed));
    // Model space shows through the L only: nothing in the cut-out quarter.
    let pts = content(&s);
    assert!(!pts.is_empty());
    assert!(pts.iter().all(|p| !(p.x > 60.0 + 1e-6 && p.y > 60.0 + 1e-6)), "content in the notch");
    assert!(pts.iter().any(|p| p.x > 100.0 && p.y < 60.0), "content in the lower right");
    // No rectangle border is drawn for a clipped viewport (its boundary draws itself).
    let d = s.doc().unwrap();
    let l = cadcraft_render::build(d, &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
    assert!(l.prims.iter().all(|p| p.handle.hex() != vh));
    // MSPACE at a point in the notch finds no viewport; inside the L it does.
    assert!(s.execute("mspace", &json!({"at": [100, 100]})).is_err());
    s.execute("mspace", &json!({"at": [20, 100]})).unwrap();
}

#[test]
fn typed_polygonal_with_arc_and_object() {
    let mut s = session();
    s.cmdline("mview p 10,10 100,10 a 100,60 l 10,60 c").unwrap();
    let vps = layout_viewports(&s);
    assert_eq!(vps.len(), 1, "{:?}", s.log);
    let (_, v) = &vps[0];
    let Some(EntityKind::LwPolyline(p)) = s.doc().unwrap().entity(v.clip.unwrap()).map(|e| e.kind.clone()) else { panic!() };
    assert_eq!(p.vertices.len(), 4);
    // The arc from 100,10 to 100,60 leaves along +x: a half circle bulging right.
    assert!((p.vertices[1].bulge.abs() - 1.0).abs() < 1e-9, "{:?}", p.vertices[1]);
    assert!(v.width > 110.0, "the arc widens the extents: {}", v.width);

    // Object: a circle on the sheet becomes a circular viewport.
    s.execute("circle", &json!({"center": [200, 60], "radius": 30})).unwrap();
    let c = s.state().unwrap().doc.layout("Layout1").unwrap().entities.iter().last().unwrap().handle;
    s.cmdline("mview o 230,60").unwrap();
    let vps = layout_viewports(&s);
    assert_eq!(vps.len(), 2);
    let v = &vps[1].1;
    assert_eq!(v.clip, Some(c));
    assert!(v.center.xy().near(Vec2::new(200.0, 60.0), 1e-6) && (v.width - 60.0).abs() < 1e-6);
    // An open object or one already used is refused.
    s.execute("line", &json!({"points": [[0, 150], [50, 150]]})).unwrap();
    let line = s.state().unwrap().doc.layout("Layout1").unwrap().entities.iter().last().unwrap().handle;
    assert!(s.execute("mview", &json!({"option": "object", "object": line.hex()})).is_err());
    assert!(s.execute("mview", &json!({"option": "object", "object": c.hex()})).is_err());
}

#[test]
fn vpclip_clips_keeps_the_view_and_deletes() {
    let mut s = session();
    let r = s.execute("mview", &json!({"p1": [0, 0], "p2": [200, 100]})).unwrap();
    let vh = r["viewports"][0].as_str().unwrap().to_string();
    s.execute("viewport.set", &json!({"handle": vh, "scale": 1, "center": [100, 50]})).unwrap();
    // Model 0,0 is at paper 0,0 before clipping.
    let before = viewport(&s, &vh);
    let paper_of = |v: &Viewport, m: Vec2| v.center.xy() + (m - v.view_center) * (v.height / v.view_height);
    let r = s.execute("vpclip", &json!({"handle": vh, "points": [[20, 20], [80, 20], [50, 90]]})).unwrap();
    let after = viewport(&s, &vh);
    assert_eq!(after.clip.unwrap().hex(), r["clip"].as_str().unwrap());
    for m in [Vec2::ZERO, Vec2::new(50.0, 50.0)] {
        assert!(paper_of(&before, m).near(paper_of(&after, m), 1e-9), "the model stays put on the sheet");
    }
    assert!((after.width - 60.0).abs() < 1e-9 && (after.height - 70.0).abs() < 1e-9);
    let pts = content(&s);
    assert!(!pts.is_empty() && pts.iter().all(|p| p.y >= 20.0 - 1e-6 && p.y <= 90.0 + 1e-6 && p.x >= 20.0 - 1e-6 && p.x <= 80.0 + 1e-6));

    // Re-clipping with an object replaces (and erases) the old outline.
    let old = after.clip.unwrap();
    s.execute("circle", &json!({"center": [50, 45], "radius": 15})).unwrap();
    let c = s.state().unwrap().doc.layout("Layout1").unwrap().entities.iter().last().unwrap().handle;
    // Typed: pick the viewport (inside its outline), then the circle.
    s.cmdline("vpclip 50,50 35,45").unwrap();
    assert_eq!(viewport(&s, &vh).clip, Some(c), "{:?}", s.log);
    assert!(s.doc().unwrap().entity(old).is_none());
    // Delete: back to a rectangle; the outline goes.
    let b = viewport(&s, &vh).clip.unwrap();
    s.execute("vpclip", &json!({"handle": vh, "delete": true})).unwrap();
    assert_eq!(viewport(&s, &vh).clip, None);
    assert!(s.doc().unwrap().entity(b).is_none());
    assert!(s.execute("vpclip", &json!({"handle": vh, "delete": true})).is_err());
    // Typed: select the viewport, then Polygonal points.
    s.cmdline("vpclip 50,45 p 10,10 90,10 90,90 c").unwrap();
    assert!(viewport(&s, &vh).clip.is_some(), "{:?}", s.log);
}

#[test]
fn on_off_lock_restore_and_arrangements() {
    let mut s = session();
    let r = s.execute("mview", &json!({"p1": [0, 0], "p2": [100, 100]})).unwrap();
    let vh = r["viewports"][0].as_str().unwrap().to_string();
    assert!(!content(&s).is_empty());
    s.execute("mview", &json!({"option": "off", "handles": [vh]})).unwrap();
    assert!(!viewport(&s, &vh).on);
    assert!(content(&s).is_empty(), "an off viewport shows no model space");
    assert!(s.execute("mspace", &json!({"handle": vh})).is_err());
    s.cmdline("mview on 0,50").unwrap();
    s.cmdline("").unwrap();
    assert!(viewport(&s, &vh).on, "{:?}", s.log);
    // Lock, typed and JSON.
    s.cmdline("mview l on 50,50").unwrap();
    s.cmdline("").unwrap();
    assert!(viewport(&s, &vh).locked, "{:?}", s.log);
    s.execute("mview", &json!({"option": "lock", "locked": false, "handles": [vh]})).unwrap();
    assert!(!viewport(&s, &vh).locked);
    // Restore needs a viewport configuration that exists (followups_626.rs covers the rest).
    assert!(s.execute("mview", &json!({"option": "restore", "name": "none saved"})).is_err());
    // 3 viewports arranged Above: the large one on top.
    s.cmdline("mview 3 a 200,0 300,100").unwrap();
    let vps = layout_viewports(&s);
    assert_eq!(vps.len(), 4, "{:?}", s.log);
    let top = &vps[1].1;
    assert!((top.width - 100.0).abs() < 1e-9 && (top.center.y - 75.0).abs() < 1e-9, "{top:?}");
}

#[test]
fn hostile_viewport_params_never_panic() {
    let mut s = session();
    let r = s.execute("mview", &json!({"p1": [0, 0], "p2": [100, 100]})).unwrap();
    let vh = r["viewports"][0].clone();
    let big: Vec<[f64; 2]> = (0..20_000).map(|i| [f64::from(i), 0.0]).collect();
    let cases: Vec<Value> = vec![
        json!({"option": "polygonal"}),
        json!({"option": "polygonal", "points": [[0, 0], [1, 1]]}),
        json!({"option": "polygonal", "points": [[0, 0], [1, 1], [2, 2]]}),
        json!({"option": "polygonal", "points": [[0, 0], [1, "x"], [2, 2]]}),
        json!({"option": "polygonal", "points": [[0, 0], [10, 0], [0, 10]], "bulges": [1e308, "x"]}),
        json!({"option": "polygonal", "points": [[0, 0], [1e300, 0], [0, 1e300]]}),
        json!({"option": "polygonal", "points": big}),
        json!({"option": "object"}),
        json!({"option": "object", "object": vh}),
        json!({"option": "object", "object": "FFFFFFFFFFFFFFFF"}),
        json!({"option": "object", "object": -1}),
        json!({"option": "on"}),
        json!({"option": "lock", "handles": [vh]}),
        json!({"option": "bogus"}),
    ];
    for p in cases {
        assert!(s.execute("mview", &p).is_err(), "{p}");
    }
    for p in [
        json!({}),
        json!({"handle": vh}),
        json!({"handle": vh, "object": vh}),
        json!({"handle": vh, "points": [[0, 0]]}),
        json!({"handle": "0", "points": [[0, 0], [10, 0], [0, 10]]}),
        json!({"handle": vh, "delete": true}),
    ] {
        assert!(s.execute("vpclip", &p).is_err(), "{p}");
    }
    // Hostile typed input.
    for t in ["mview p 0,0 c", "mview p 0,0 a 0,0 u u u c", "vpclip 5000,5000", "mview o 9999,9999"] {
        let _ = s.cmdline(t);
        let _ = s.cmdline("\u{1b}");
    }
    // A clip handle pointing at nothing (or at the viewport itself) renders as a rectangle.
    let h = Handle::parse_hex(vh.as_str().unwrap()).unwrap();
    for bad in [Handle(0xDEAD_BEEF), h] {
        s.doc_mut()
            .unwrap()
            .modify_entity(h, |e| {
                if let EntityKind::Viewport(v) = &mut e.kind {
                    v.clip = Some(bad);
                }
            })
            .unwrap();
        assert!(!content(&s).is_empty());
    }
}

#[test]
fn polygonal_viewport_menu_item_asks_for_points() {
    let mut s = session();
    s.start("mview.polygonal").unwrap();
    assert!(s.prompt_text().contains("Specify start point"), "{}", s.prompt_text());
    for t in ["10,10", "100,10", "60,80", "c"] {
        s.cmdline(t).unwrap();
    }
    assert!(s.running.is_none());
    assert!(layout_viewports(&s)[0].1.clip.is_some());
    // The JSON form takes the points.
    s.execute("mview.polygonal", &json!({"points": [[120, 10], [200, 10], [160, 80]]})).unwrap();
    assert_eq!(layout_viewports(&s).len(), 2);
}
