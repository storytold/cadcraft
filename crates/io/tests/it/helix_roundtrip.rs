//! Helices survive DXF save and reopen (#510): HELIX is written as the DXF Reference describes,
//! a cubic spline (AcDbSpline) followed by the helix data (AcDbHelix), and read back with its
//! radii, height, turns, twist and start point. Foreign helices read the same way; hostile ones
//! don't panic.

use cadcraft_doc::{Common, Drawing, EntityKind, Helix, HelixConstrain, Space};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::Vec3;

fn helices(d: &Drawing) -> Vec<Helix> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Helix(h) => Some(h.clone()),
            _ => None,
        })
        .collect()
}

fn same(a: &Helix, b: &Helix) -> bool {
    let n = |x: f64, y: f64| (x - y).abs() < 1e-9;
    n(a.center.x, b.center.x)
        && n(a.center.y, b.center.y)
        && n(a.center.z, b.center.z)
        && n(a.base_radius, b.base_radius)
        && n(a.top_radius, b.top_radius)
        && n(a.height, b.height)
        && n(a.turns, b.turns)
        && n(a.start_point().x, b.start_point().x)
        && n(a.start_point().y, b.start_point().y)
        && a.ccw == b.ccw
        && a.constrain == b.constrain
}

#[test]
fn helix_survives_save_and_reopen() {
    let mut d = Drawing::new_imperial();
    let a = Helix {
        center: Vec3::new(5.0, -2.0, 1.0),
        base_radius: 3.0,
        top_radius: 1.25,
        height: 12.0,
        turns: 5.5,
        start_angle: 0.7,
        ccw: false,
        constrain: HelixConstrain::Turns,
    };
    let b = Helix {
        center: Vec3::ZERO,
        base_radius: 2.0,
        top_radius: 0.0,
        height: -4.0,
        turns: 1.0,
        start_angle: 0.0,
        ccw: true,
        constrain: HelixConstrain::Height,
    };
    d.add(&Space::Model, Common { layer: "Springs".into(), ..Common::default() }, EntityKind::Helix(a.clone())).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Helix(b.clone())).unwrap();
    let saved = cadcraft_io::write_dxf(&d);
    let back = cadcraft_io::read_dxf(saved.as_bytes()).unwrap();
    let [ra, rb] = helices(&back).try_into().unwrap();
    assert!(same(&ra, &a), "{ra:?}\n{a:?}");
    assert!(same(&rb, &b), "{rb:?}\n{b:?}");
    assert_eq!(back.model.iter().next().unwrap().common.layer, "Springs");

    // The file: a spline whose last control point is the helix's end, the helix data, a CLASS.
    let tags = cadcraft_dxf::parse(saved.as_bytes()).unwrap();
    let secs = sections(&tags);
    let recs = |name: &str| records(&secs.iter().find(|s| s.name == name).unwrap().tags);
    let ents = recs("ENTITIES");
    let (_, ht) = ents.iter().find(|(k, _)| k == "HELIX").unwrap();
    let marker = |m: &str| ht.iter().position(|t| t.code == 100 && t.str() == m).unwrap();
    let (sp, hx) = (marker("AcDbSpline"), marker("AcDbHelix"));
    assert!(sp < hx);
    let last = ht[sp..hx].iter().rposition(|t| t.code == 10).unwrap() + sp;
    let end = Vec3::new(ht[last].f64(), ht[last + 1].f64(), ht[last + 2].f64());
    let e = a.end_point();
    assert!((end.x - e.x).abs() < 1e-9 && (end.y - e.y).abs() < 1e-9 && (end.z - e.z).abs() < 1e-9, "{end:?} vs {e:?}");
    let val = |c: i32| ht[hx..].iter().find(|t| t.code == c).map(Tag::f64).unwrap();
    assert_eq!((val(40), val(41), val(290), val(280)), (3.0, 5.5, 0.0, 1.0));
    assert!((val(42) - 12.0 / 5.5).abs() < 1e-12);
    assert!(recs("CLASSES").iter().any(|(_, t)| t.iter().any(|g| g.code == 1 && g.str() == "HELIX")));

    // Saving again is stable.
    let again = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&back).as_bytes()).unwrap();
    assert!(same(&helices(&again)[0], &a));
}

#[test]
fn foreign_helix_reads() {
    // Spline data first (its own groups 40/41/42), then the helix data.
    let g: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "HELIX"),
        (5, "2A"),
        (100, "AcDbEntity"),
        (8, "0"),
        (100, "AcDbSpline"),
        (70, "0"),
        (71, "3"),
        (72, "8"),
        (73, "4"),
        (74, "0"),
        (42, "1e-10"),
        (43, "1e-10"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "0"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (40, "1"),
        (10, "2"),
        (20, "0"),
        (30, "0"),
        (10, "2"),
        (20, "2"),
        (30, "1"),
        (10, "-1"),
        (20, "1"),
        (30, "2"),
        (10, "0.5"),
        (20, "0"),
        (30, "3"),
        (100, "AcDbHelix"),
        (90, "29"),
        (91, "63"),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (11, "2"),
        (21, "0"),
        (31, "0"),
        (12, "0"),
        (22, "0"),
        (32, "1"),
        (40, "2"),
        (41, "1"),
        (42, "3"),
        (290, "1"),
        (280, "1"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    let text: String = g.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect();
    let d = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let [x] = helices(&d).try_into().unwrap();
    assert_eq!((x.base_radius, x.top_radius, x.turns, x.height, x.ccw, x.constrain), (2.0, 0.5, 1.0, 3.0, true, HelixConstrain::Turns));
}

#[test]
fn hostile_helix_does_not_panic() {
    let mut t = String::from("0\nSECTION\n2\nENTITIES\n");
    for (r, turns, th) in [("nan", "1e308", "inf"), ("-5", "-1", "0"), ("1e308", "0", "nan"), ("1", "1e9", "1e308")] {
        t.push_str(&format!("0\nHELIX\n100\nAcDbSpline\n72\n999999999\n73\n-4\n10\nnan\n20\n1e308\n100\nAcDbHelix\n10\ninf\n12\n0\n22\n0\n32\n0\n40\n{r}\n41\n{turns}\n42\n{th}\n290\n7\n280\n-9\n"));
    }
    t.push_str("0\nHELIX\n0\nENDSEC\n0\nEOF\n");
    let d = cadcraft_io::read_dxf(t.as_bytes()).unwrap();
    assert_eq!(helices(&d).len(), 5);
    for x in helices(&d) {
        assert!(x.turns > 0.0 && x.turns <= 500.0 && x.base_radius.is_finite() && x.height.is_finite());
    }
    let _ = cadcraft_io::write_dxf(&d);
    let _ = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn helix_survives_dwg_save_and_reopen() {
    let mut d = Drawing::new_imperial();
    let a = Helix {
        center: Vec3::ZERO,
        base_radius: 2.0,
        top_radius: 1.0,
        height: 6.0,
        turns: 3.0,
        start_angle: 0.0,
        ccw: true,
        constrain: HelixConstrain::Height,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Helix(a.clone())).unwrap();
    let back = cadcraft_io::read(&cadcraft_io::write(&d, "x.dwg").unwrap(), "x.dwg").unwrap();
    let [x] = helices(&back).try_into().unwrap();
    assert!(same(&x, &a), "{x:?}");
}
