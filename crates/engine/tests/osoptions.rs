//! OSOPTIONS applies to running snaps, overrides and cycling, and survives profile restores.

use cadcraft_doc::{Block, Common, Entity, EntityKind, Handle, Line, Space};
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_engine::snap::{self, mode};
use cadcraft_engine::{Session, cmd::options};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

fn set(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({"name": name, "value": value})).unwrap();
}

fn get(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({"name": name})).unwrap()["value"].clone()
}

fn fixture() -> (Session, Handle, Vec<Vec2>) {
    let mut s = Session::new();
    for (name, value) in [("DIMEXO", 0.4), ("DIMEXE", 0.6), ("DIMASZ", 0.2), ("DIMTXT", 0.2)] {
        set(&mut s, name, json!(value));
    }
    let r = s.execute("dimlinear", &json!({"p1": [0,0], "p2": [10,0], "at": [5,3]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Dimension(dm) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("dimension fixture") };
    let g = cadcraft_render::dimension_in(s.doc().unwrap(), dm);
    let points = g.extension_lines.iter().flat_map(|i| [*g.lines[*i].first().unwrap(), *g.lines[*i].last().unwrap()]).collect();
    (s, h, points)
}

fn pick(s: &mut Session, p: Vec2, modes: u32) -> Option<snap::SnapHit> {
    s.settings.osmode = modes;
    let q = CursorQuery { base: None, deferred: false, corner: false, aperture: 0.05, tol: 0.01, now: 0.0, shift: false };
    s.snap_cursor(p + Vec2::new(0.01, 0.01), &q).snap
}

#[test]
fn bitcode_and_legacy_hatch_alias_are_synchronized() {
    let mut s = Session::new();
    assert_eq!(get(&mut s, "OSOPTIONS"), json!(7));
    for bits in 0..=7 {
        set(&mut s, "OSOPTIONS", json!(bits));
        assert_eq!(get(&mut s, "OSOPTIONS"), json!(bits));
        assert_eq!(get(&mut s, "OSNAPHATCH"), json!(i32::from(bits & 1 == 0)));
        set(&mut s, "OSNAPHATCH", json!(0));
        assert_eq!(get(&mut s, "OSOPTIONS"), json!(bits | 1));
        set(&mut s, "OSNAPHATCH", json!(1));
        assert_eq!(get(&mut s, "OSOPTIONS"), json!(bits & !1));
    }
    s.cmdline("osoptions 7").unwrap();
    assert!(s.running.is_none());
    assert_eq!(get(&mut s, "OSOPTIONS"), json!(7));
    assert_eq!(s.execute("sysvars", &json!({})).unwrap()["OSOPTIONS"], json!(7));
}

#[test]
fn invalid_bitcodes_do_not_change_preferences() {
    let mut s = Session::new();
    set(&mut s, "OSOPTIONS", json!(6));
    let before = options::profile(&s);
    for value in [json!(-1), json!(8), json!(u64::MAX), json!(1.5), json!("NaN"), json!("9999999999999999999999999"), json!(null), json!([])] {
        assert!(s.execute("setvar", &json!({"name": "OSOPTIONS", "value": value})).is_err());
        assert_eq!(options::profile(&s), before);
        assert!(s.execute("options", &json!({"OSNAPHATCH": 0, "OSOPTIONS": value})).is_err());
        assert_eq!(options::profile(&s), before);
    }
}

#[test]
fn profiles_migrate_restore_reset_and_resolve_alias_conflicts() {
    let mut s = Session::new();
    options::restore(&mut s, &json!({"OSNAPHATCH": 1}));
    assert_eq!(get(&mut s, "OSOPTIONS"), json!(6), "old profiles keep their hatch choice and new default dimension filter");
    options::apply(&mut s, &json!({"OSOPTIONS": 3, "osnaphatch": 1})).unwrap();
    assert_eq!(get(&mut s, "OSOPTIONS"), json!(3), "canonical setting wins regardless of case/order");
    let saved = options::profile(&s);
    let mut restored = Session::new();
    options::restore(&mut restored, &saved);
    assert_eq!(options::profile(&restored), saved);
    options::restore(&mut restored, &json!({"OSOPTIONS": 5, "osnaphatch": 1}));
    assert_eq!(get(&mut restored, "OSOPTIONS"), json!(5));
    options::restore(&mut restored, &json!({"OSOPTIONS": 9, "OSNAPHATCH": 1}));
    assert_eq!(get(&mut restored, "OSOPTIONS"), json!(4), "invalid new setting falls back to the valid alias");
    restored.execute("options", &json!({"reset": true})).unwrap();
    assert_eq!(get(&mut restored, "OSOPTIONS"), json!(7));
}

#[test]
fn only_extension_endpoints_are_filtered_and_cycling_obeys_the_filter() {
    let (mut s, _, points) = fixture();
    for p in points {
        assert!(pick(&mut s, p, mode::END).is_none(), "default filters {p:?}");
        let modes = s.settings.object_snap_modes(mode::END | mode::MID);
        assert!(cadcraft_engine::pointmod::snap_candidates(s.doc().unwrap(), &Space::Model, p, 0.05, modes, None, false).is_empty());
        set(&mut s, "OSOPTIONS", json!(3));
        assert_eq!(pick(&mut s, p, mode::END).unwrap_or_else(|| panic!("enabled endpoint missing at {p:?}")).point, p);
        set(&mut s, "OSOPTIONS", json!(7));
    }
    assert_eq!(pick(&mut s, Vec2::new(10.0, 3.0), mode::END).unwrap().point, Vec2::new(10.0, 3.0));
    assert_eq!(pick(&mut s, Vec2::ZERO, mode::NOD).unwrap().point, Vec2::ZERO);
    assert_eq!(pick(&mut s, Vec2::new(0.0, 2.0), mode::MID).unwrap().point, Vec2::new(0.0, 2.0));
    assert!(pick(&mut s, Vec2::new(0.0, 1.0), mode::NEA).is_some());
    // Coincident real geometry remains a valid endpoint; the filter applies to the dimension only.
    let p = Vec2::new(0.0, 3.6);
    s.execute("line", &json!({"points": [[p.x,p.y], [-2,4]]})).unwrap();
    assert!(pick(&mut s, p, mode::END).unwrap().point.near(p, 1e-9));
}

#[test]
fn typed_endpoint_override_uses_the_same_filter() {
    let (mut s, _, points) = fixture();
    let p = points[0];
    s.cmdline("line").unwrap();
    s.cmdline("end").unwrap();
    s.cmdline(&format!("{},{}", p.x, p.y)).unwrap();
    assert!(s.prompt_text().contains("first point"));
    assert!(s.log.last().unwrap().contains("No Endpoint"));
    set(&mut s, "OSOPTIONS", json!(3));
    s.cmdline(&format!("{},{}", p.x, p.y)).unwrap();
    assert_eq!(s.current_prompt().unwrap().base, Some(p));
    s.cancel();
}

#[test]
fn imported_dimension_blocks_filter_extension_lines_but_keep_dimension_lines() {
    let (mut s, h, points) = fixture();
    let d = s.doc_mut().unwrap();
    let EntityKind::Dimension(dm) = &d.entity(h).unwrap().kind else { panic!("dimension fixture") };
    let g = cadcraft_render::dimension_in(d, dm);
    let mut block = Block::new("DIMGEOM");
    for line in &g.lines {
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            block.entities.push(Entity {
                handle: d.new_handle(),
                common: Common::default(),
                kind: EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) }),
            });
        }
    }
    d.blocks.insert(block.name.clone(), std::sync::Arc::new(block));
    d.model.modify(h, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = Some("DIMGEOM".into());
        }
    });
    for p in points {
        assert!(pick(&mut s, p, mode::END).is_none());
        set(&mut s, "OSOPTIONS", json!(3));
        assert_eq!(pick(&mut s, p, mode::END).unwrap().point, p);
        set(&mut s, "OSOPTIONS", json!(7));
    }
    assert_eq!(pick(&mut s, Vec2::new(10.0, 3.0), mode::END).unwrap().point, Vec2::new(10.0, 3.0));
}

#[test]
fn indexed_dimensions_include_extension_tips_outside_text_padding() {
    let (mut s, _, points) = fixture();
    for i in 0..300 {
        let x = 100.0 + f64::from(i);
        s.doc_mut()
            .unwrap()
            .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 10.0, 0.0) }))
            .unwrap();
    }
    for p in points {
        assert!(pick(&mut s, p, mode::END).is_none());
        set(&mut s, "OSOPTIONS", json!(3));
        assert_eq!(pick(&mut s, p, mode::END).unwrap().point, p);
        set(&mut s, "OSOPTIONS", json!(7));
    }
}

#[test]
fn broken_extension_lines_keep_the_filter_and_radial_centre_marks_are_unaffected() {
    let (mut s, h, _) = fixture();
    s.doc_mut().unwrap().model.modify(h, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.marks.breaks.push(cadcraft_doc::DimBreak { at: Vec3::new(0.0, 2.0, 0.0), size: 0.4, auto: false });
        }
    });
    for p in [Vec2::new(0.0, 1.8), Vec2::new(0.0, 2.2)] {
        assert!(pick(&mut s, p, mode::END).is_none());
        set(&mut s, "OSOPTIONS", json!(3));
        assert!(pick(&mut s, p, mode::END).unwrap().point.near(p, 1e-9));
        set(&mut s, "OSOPTIONS", json!(7));
    }
    let mut s = Session::new();
    s.execute("dimradius", &json!({"center": [0,0], "point": [3,0], "at": [6,0]})).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Dimension(dm) = &d.model.iter().last().unwrap().kind else { panic!("radius fixture") };
    let g = cadcraft_render::dimension_in(d, dm);
    let ends: Vec<Vec2> = g.lines_of(cadcraft_render::LineRole::Ext).flat_map(|l| [*l.first().unwrap(), *l.last().unwrap()]).collect();
    assert!(!ends.is_empty());
    for p in ends {
        assert!(pick(&mut s, p, mode::END).unwrap().point.near(p, 1e-9));
    }
}

#[test]
fn hatch_bit_controls_running_snaps_and_legacy_alias() {
    let mut s = Session::new();
    let r = s.execute("rectang", &json!({"p1": [0,0], "p2": [10,5]})).unwrap();
    s.execute("hatch", &json!({"handles": [r["handle"]]})).unwrap();
    s.execute("erase", &json!({"handles": [r["handle"]]})).unwrap();
    let p = Vec2::new(10.0, 5.0);
    assert!(pick(&mut s, p, mode::END).is_none());
    set(&mut s, "OSOPTIONS", json!(6));
    assert_eq!(pick(&mut s, p, mode::END).unwrap().point, p);
    set(&mut s, "OSNAPHATCH", json!(0));
    assert!(pick(&mut s, p, mode::END).is_none());
}

#[test]
fn negative_z_bit_is_preserved_without_filtering_ordinary_2d_snaps() {
    let mut s = Session::new();
    s.doc_mut()
        .unwrap()
        .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, -2.0), b: Vec3::new(10.0, 0.0, -2.0) }))
        .unwrap();
    for bits in [0, 2] {
        set(&mut s, "OSOPTIONS", json!(bits));
        assert_eq!(pick(&mut s, Vec2::ZERO, mode::END).unwrap().point, Vec2::ZERO);
    }
}
