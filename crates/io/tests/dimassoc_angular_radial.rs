//! #1651: angular, diameter and radius dimensions are saved with a DIMASSOC object, as linear
//! and ordinate dimensions already are, so AutoCAD follows their geometry. The standard objects
//! alone (CADCraft xdata stripped) must associate the same points, in DXF and in DWG.

use cadcraft_doc::{AssocSnap, Circle, Common, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Handle, Line, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{Version, read, read_dxf, write_dxf};

/// A dimension entity of the given kind with all its definition points and associations, for
/// adding straight to a drawing; the style and text are left at defaults since only the
/// geometry and associations are under test.
fn dim(kind: DimKind, defpt: Vec3, p13: Vec3, p14: Vec3, p15: Vec3, p16: Vec3, assoc: Vec<DimAssoc>) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt,
        text_mid: Vec3::ZERO,
        p13,
        p14,
        p15,
        p16,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc,
        marks: Default::default(),
    })
}

/// The drawing of the issue's repro: a circle with a diameter and a radius dimension, two lines
/// with an angular dimension between them, and a line with a linear dimension. The associations
/// are the ones the commands make (`assoc::radial_assoc` and `assoc::reassociate`).
struct Case {
    d: Drawing,
    circle: Handle,
    l1: Handle,
    l2: Handle,
    hd: Handle,
    hr: Handle,
    ha: Handle,
    hl: Handle,
}

fn case() -> Case {
    let mut d = Drawing::new_metric();
    let c = Common::default;
    let circle = d.add(&Space::Model, c(), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 30.0 })).unwrap();
    let l1 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(100.0, 0.0, 0.0), b: Vec3::new(200.0, 0.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(100.0, 0.0, 0.0), b: Vec3::new(180.0, 60.0, 0.0) })).unwrap();
    let l3 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(0.0, -80.0, 0.0), b: Vec3::new(100.0, -80.0, 0.0) })).unwrap();

    let on = |point: &str, angle: f64| DimAssoc { point: point.into(), handle: circle, snap: AssocSnap::OnCircle { angle } };
    let diameter = dim(
        DimKind::Diameter,
        Vec3::new(30.0, 0.0, 0.0),
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(-30.0, 0.0, 0.0),
        Vec3::ZERO,
        vec![on("defpt", 0.0), on("p15", std::f64::consts::PI)],
    );
    let side = 30.0 / std::f64::consts::SQRT_2;
    let radius = dim(
        DimKind::Radius,
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::ZERO,
        Vec3::new(-side, -side, 0.0),
        Vec3::ZERO,
        vec![DimAssoc { point: "defpt".into(), handle: circle, snap: AssocSnap::Center }, on("p15", 225f64.to_radians())],
    );
    let angular = dim(
        DimKind::Angular3P,
        Vec3::new(160.0, 20.0, 0.0),
        Vec3::new(200.0, 0.0, 0.0),
        Vec3::new(180.0, 60.0, 0.0),
        Vec3::new(100.0, 0.0, 0.0),
        Vec3::ZERO,
        vec![
            DimAssoc { point: "p13".into(), handle: l1, snap: AssocSnap::End },
            DimAssoc { point: "p14".into(), handle: l2, snap: AssocSnap::End },
            DimAssoc { point: "p15".into(), handle: l1, snap: AssocSnap::Intersection { other: l2 } },
        ],
    );
    let linear = dim(
        DimKind::Linear { rotation: 0.0 },
        Vec3::new(50.0, -100.0, 0.0),
        Vec3::new(0.0, -80.0, 0.0),
        Vec3::new(100.0, -80.0, 0.0),
        Vec3::ZERO,
        Vec3::ZERO,
        vec![
            DimAssoc { point: "p13".into(), handle: l3, snap: AssocSnap::Start },
            DimAssoc { point: "p14".into(), handle: l3, snap: AssocSnap::End },
        ],
    );
    let hd = d.add(&Space::Model, Common::default(), diameter).unwrap();
    let hr = d.add(&Space::Model, Common::default(), radius).unwrap();
    let ha = d.add(&Space::Model, Common::default(), angular).unwrap();
    let hl = d.add(&Space::Model, Common::default(), linear).unwrap();
    Case { d, circle, l1, l2, hd, hr, ha, hl }
}

/// The DXF tag pairs, `(code, value)`.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for chunk in lines.chunks(2) {
        let [code, value] = chunk else { break };
        let Ok(c) = code.trim().parse::<i32>() else { continue };
        out.push((c, value.trim().to_string()));
    }
    out
}

/// The records of a DXF (0-group name and the groups that follow it).
fn records(text: &str) -> Vec<Vec<(i32, String)>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<(i32, String)>> = None;
    for (c, v) in pairs(text) {
        if c == 0 {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            cur = Some(vec![(c, v)]);
        } else if let Some(r) = cur.as_mut() {
            r.push((c, v));
        }
    }
    if let Some(r) = cur {
        out.push(r);
    }
    out
}

/// The DIMASSOC object of a dimension, if the file has one.
fn dimassoc(text: &str, dim: Handle) -> Option<Vec<(i32, String)>> {
    let hex = dim.hex();
    records(text)
        .into_iter()
        .filter(|r| r.first().is_some_and(|(c, v)| *c == 0 && v == "DIMASSOC"))
        .find(|r| r.iter().rfind(|(c, _)| *c == 330).is_some_and(|(_, v)| v.eq_ignore_ascii_case(&hex)))
}

/// The AcDbOsnapPointRef blocks of a DIMASSOC record.
fn point_refs(rec: &[(i32, String)]) -> Vec<Vec<(i32, String)>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<(i32, String)>> = None;
    for (c, v) in rec {
        if *c == 1 && v == "AcDbOsnapPointRef" {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            cur = Some(Vec::new());
        } else if let Some(r) = cur.as_mut() {
            r.push((*c, v.clone()));
        }
    }
    if let Some(r) = cur {
        out.push(r);
    }
    out
}

/// The first integer value of a group code in a record (72 the osnap type, 90 the flags).
fn int(rec: &[(i32, String)], code: i32) -> Option<i64> {
    rec.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse().ok())
}

/// The first string value of a group code in a record (331 the referenced entity, 332 the other
/// entity of an intersection).
fn string(rec: &[(i32, String)], code: i32) -> Option<&str> {
    rec.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

/// A DXF without CADCraft's xdata, as a file from another writer (whose DIMASSOC objects must
/// stand on their own).
fn without_cadcraft_xdata(text: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for (c, v) in pairs(text) {
        if c == 1001 {
            skipping = v == "CADCRAFT";
        } else if c < 1000 {
            skipping = false;
        }
        if !skipping {
            out.push_str(&format!("{c}\n{v}\n"));
        }
    }
    out
}

/// The associations a read-back dimension carries, or none if the handle is not a dimension.
fn assoc_of(d: &Drawing, h: Handle) -> Vec<DimAssoc> {
    match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Dimension(dm)) => dm.assoc.clone(),
        _ => Vec::new(),
    }
}

/// Whether two angles name the same direction, allowing for a whole turn and rounding.
fn same_angle(a: f64, b: f64) -> bool {
    let turn = std::f64::consts::TAU;
    let d = (a - b).rem_euclid(turn);
    d.min(turn - d) < 1e-6
}

/// The associations the standard objects give, without CADCraft's xdata.
fn assert_standard_assoc(c: &Case, d: &Drawing, what: &str) {
    let dia = assoc_of(d, c.hd);
    assert_eq!(dia.len(), 2, "{what}: diameter defpt and p15");
    assert!(dia.iter().all(|a| a.handle == c.circle), "{what}: diameter follows the circle: {dia:?}");
    assert!(matches!(dia[0].snap, AssocSnap::OnCircle { angle } if same_angle(angle, 0.0)), "{what}: {:?}", dia[0]);
    assert!(matches!(dia[1].snap, AssocSnap::OnCircle { angle } if same_angle(angle, std::f64::consts::PI)), "{what}: {:?}", dia[1]);

    let rad = assoc_of(d, c.hr);
    assert_eq!(rad.len(), 2, "{what}: radius defpt and p15");
    assert_eq!(rad[0].handle, c.circle, "{what}: radius centre: {rad:?}");
    assert!(matches!(rad[0].snap, AssocSnap::Center), "{what}: {:?}", rad[0]);
    assert!(matches!(rad[1].snap, AssocSnap::OnCircle { angle } if same_angle(angle, 225f64.to_radians())), "{what}: {:?}", rad[1]);

    let ang = assoc_of(d, c.ha);
    assert_eq!(ang.len(), 3, "{what}: angular leg points and vertex");
    assert_eq!((ang[0].handle, &ang[0].snap), (c.l1, &AssocSnap::End), "{what}: {ang:?}");
    assert_eq!((ang[1].handle, &ang[1].snap), (c.l2, &AssocSnap::End), "{what}: {ang:?}");
    assert_eq!(ang[2].handle, c.l1, "{what}: the vertex is the lines' intersection: {ang:?}");
    assert!(matches!(ang[2].snap, AssocSnap::Intersection { other } if other == c.l2), "{what}: {ang:?}");

    let lin = assoc_of(d, c.hl);
    assert_eq!(lin.len(), 2, "{what}: linear is unchanged");
    assert_eq!((lin[0].handle, &lin[0].snap), (lin[1].handle, &AssocSnap::Start), "{what}: {lin:?}");
    assert!(matches!(lin[1].snap, AssocSnap::End), "{what}: {lin:?}");
}

/// The writer half of #1651: the DXF of the repro's drawing has one DIMASSOC per dimension, with
/// the references AutoCAD writes for each kind -- one centre reference (flag 2) for radius and
/// diameter, three for an angular dimension (both leg ends and the lines' intersection), and the
/// two origins of the linear control. Guards the kind-to-slot table and the radial/diametric
/// branch in `std_assoc_refs`, which used to emit no reference for those kinds at all.
#[test]
fn angular_diameter_and_radius_dimensions_are_saved_with_dimassoc() {
    let c = case();
    let text = write_dxf(&c.d);

    let dia = dimassoc(&text, c.hd).unwrap_or_else(|| panic!("diameter dimension {} has a DIMASSOC", c.hd.hex()));
    let rad = dimassoc(&text, c.hr).unwrap_or_else(|| panic!("radius dimension {} has a DIMASSOC", c.hr.hex()));
    let ang = dimassoc(&text, c.ha).unwrap_or_else(|| panic!("angular dimension {} has a DIMASSOC", c.ha.hex()));
    assert!(dimassoc(&text, c.hl).is_some(), "the linear control still has one");

    // Radial and diametric dimensions associate their circle once, by its centre, as AutoCAD
    // writes them (90 = 2).
    for (dm, what) in [(&dia, "diameter"), (&rad, "radius")] {
        let refs = point_refs(dm);
        assert_eq!(int(dm, 90), Some(2), "{what}: associativity flag");
        assert_eq!(refs.len(), 1, "{what}: one reference");
        assert_eq!(int(&refs[0], 72), Some(3), "{what}: a centre snap");
        assert!(string(&refs[0], 331).is_some_and(|h| h.eq_ignore_ascii_case(&c.circle.hex())), "{what}: to the circle");
    }

    // The angular dimension associates its two leg points and their intersection (90 = 7).
    let refs = point_refs(&ang);
    assert_eq!(int(&ang, 90), Some(7), "angular: three references");
    assert_eq!(refs.len(), 3, "angular: three references");
    for (i, (osnap, line)) in [(1, c.l1), (1, c.l2), (6, c.l1)].into_iter().enumerate() {
        assert_eq!(int(&refs[i], 72), Some(osnap), "angular ref {i}");
        assert!(string(&refs[i], 331).is_some_and(|h| h.eq_ignore_ascii_case(&line.hex())), "angular ref {i}: to its line");
    }
    assert!(string(&refs[2], 332).is_some_and(|h| h.eq_ignore_ascii_case(&c.l2.hex())), "angular: the intersection's other line");
}

/// The reader half of #1651, and the one that matters for interchange: with CADCraft's xdata
/// stripped, the standard DIMASSOC objects alone must rebuild the same associations, both from
/// DXF and through the DWG bridge. Guards the dimension-kind slot table and the radial centre
/// reference in `dxf_read::dimassoc`, plus the curve start/end osnap semantics in `infer_snap`.
#[test]
fn standard_dimassoc_associates_in_dxf_and_dwg() {
    let c = case();
    let plain = without_cadcraft_xdata(&write_dxf(&c.d));

    let back = read_dxf(plain.as_bytes()).unwrap();
    assert_standard_assoc(&c, &back, "DXF");

    let dwg = cadcraft_dwg::dxf_to_dwg_as(plain.as_bytes(), Version::R2018.code()).unwrap();
    let back = read(&dwg, "case.dwg").unwrap();
    assert_standard_assoc(&c, &back, "DWG");
}
