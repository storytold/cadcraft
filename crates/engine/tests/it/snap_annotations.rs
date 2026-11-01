//! Snaps on dimensions, justified text and hatches; the snap grid's base and angle (#334).

use cadcraft_doc::{Common, EntityKind, HAlign, Space, Text, VAlign};
use cadcraft_engine::Session;
use cadcraft_engine::snap::{self, mode};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::json;

/// The snap of `modes` near `p` (a hair off it, inside a 0.1 aperture).
fn snap_at(s: &Session, p: Vec2, modes: u32) -> Option<(Vec2, u32)> {
    snap::osnap(s.doc().unwrap(), &Space::Model, p + Vec2::new(0.02, -0.01), 0.1, modes, None, false).map(|h| (h.point, h.mode))
}

fn handle(s: &mut Session, cmd: &str, p: serde_json::Value) -> String {
    let r = s.execute(cmd, &p).unwrap();
    r.get("handle").unwrap_or(&r["handles"][0]).as_str().unwrap().to_string()
}

#[test]
fn dimensions_snap_to_their_lines_and_definition_points() {
    let mut s = Session::new();
    s.cmdline("dimlinear 0,0 10,0 5,3").unwrap();
    assert!(s.running.is_none());
    // Endpoint: the dimension line's end; Node: the extension line origins.
    assert_eq!(snap_at(&s, Vec2::new(10.0, 3.0), mode::END), Some((Vec2::new(10.0, 3.0), mode::END)));
    assert_eq!(snap_at(&s, Vec2::new(0.0, 0.0), mode::NOD), Some((Vec2::new(0.0, 0.0), mode::NOD)));
    assert_eq!(snap_at(&s, Vec2::new(10.0, 0.0), mode::NOD), Some((Vec2::new(10.0, 0.0), mode::NOD)));

    // A dimension with a block (as read from a file) snaps to the block's objects.
    let l = handle(&mut s, "line", json!({ "points": [[0, 7], [10, 7]] }));
    s.execute("block", &json!({ "name": "DIMGEOM", "base": [0, 0], "handles": [l], "keep": "delete" })).unwrap();
    let d = s.doc_mut().unwrap();
    let dim = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    d.model.modify(dim, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = Some("DIMGEOM".into());
        }
    });
    assert_eq!(snap_at(&s, Vec2::new(10.0, 7.0), mode::END), Some((Vec2::new(10.0, 7.0), mode::END)));
}

#[test]
fn justified_text_snaps_and_grips_at_its_alignment_point() {
    let mut s = Session::new();
    // As read from DXF: a right-justified text keeps group 10 (start) and group 11 (alignment).
    let t = Text {
        insert: Vec3::new(0.0, 0.0, 0.0),
        align_pt: Some(Vec3::new(10.0, 0.0, 0.0)),
        height: 1.0,
        value: "ABCDEFGHIJ".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Right,
        valign: VAlign::Baseline,
    };
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Text(t.clone())).unwrap();
    assert_eq!(snap_at(&s, Vec2::new(10.0, 0.0), mode::INS), Some((Vec2::new(10.0, 0.0), mode::INS)));
    assert_eq!(snap_at(&s, Vec2::new(0.0, 0.0), mode::INS), None);
    assert_eq!(EntityKind::Text(t.clone()).grips(), vec![Vec2::new(10.0, 0.0)]);
    // Left-baseline and fit text: the start point.
    for (h, v) in [(HAlign::Left, VAlign::Baseline), (HAlign::Fit, VAlign::Baseline)] {
        assert_eq!(Text { halign: h, valign: v, ..t.clone() }.justify_point(), Vec2::new(0.0, 0.0));
    }
    assert_eq!(Text { halign: HAlign::Left, valign: VAlign::Top, ..t.clone() }.justify_point(), Vec2::new(10.0, 0.0));
}

#[test]
fn hatches_snap_only_with_osnaphatch() {
    let mut s = Session::new();
    let r = handle(&mut s, "rectang", json!({ "p1": [0, 0], "p2": [10, 5] }));
    s.execute("hatch", &json!({ "handles": [r] })).unwrap();
    s.execute("erase", &json!({ "handles": [r] })).unwrap();
    assert!(matches!(s.doc().unwrap().model.iter().last().map(|e| &e.kind), Some(EntityKind::Hatch(_))));
    assert_eq!(snap_at(&s, Vec2::new(10.0, 5.0), mode::END), None);
    assert_eq!(s.execute("getvar", &json!({ "name": "OSNAPHATCH" })).unwrap()["value"], json!(0));
    s.cmdline("setvar osnaphatch 1").unwrap();
    assert!(s.settings.osnaphatch);
    let modes = mode::END | mode::HATCH;
    assert_eq!(snap_at(&s, Vec2::new(10.0, 5.0), modes).map(|h| h.0), Some(Vec2::new(10.0, 5.0)));
}

#[test]
fn grid_snap_follows_snapbase_and_snapang() {
    let mut s = Session::new();
    assert_eq!(snap::grid_frame(s.doc().unwrap()), (Vec2::ZERO, 0.0));
    s.cmdline("setvar snapbase 0.5,0.25").unwrap();
    s.cmdline("setvar snapang 30").unwrap();
    let (origin, angle) = snap::grid_frame(s.doc().unwrap());
    assert_eq!(origin, Vec2::new(0.5, 0.25));
    assert!((angle - 30f64.to_radians()).abs() < 1e-12);
    // Two units along the turned x axis and one along its y axis, nudged off the grid.
    let (u, v) = (Vec2::from_angle(angle), Vec2::from_angle(angle).perp());
    let on = origin + u * 2.0 + v * 1.0;
    let p = snap::grid_snap(on + Vec2::new(0.1, -0.15), Vec2::new(1.0, 1.0), origin, angle);
    assert!(p.near(on, 1e-12), "{p:?} vs {on:?}");
    // Unturned, from SNAPBASE.
    assert_eq!(snap::grid_snap(Vec2::new(1.4, 1.1), Vec2::new(1.0, 1.0), origin, 0.0), Vec2::new(1.5, 1.25));
}
