//! #1651: the issue's repro through the commands. Every dimension CADCraft reports as
//! associated (the diameter, radius and angular ones, as well as the linear control) is saved
//! with a DIMASSOC object whose references name its geometry.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::{Value, json};

/// The entity handle a command result reports, from its `handle` field.
fn handle(r: &Value) -> Handle {
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

/// The first line of a LINE result (`handles`).
fn first_line(r: &Value) -> Handle {
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
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

/// The DIMASSOC object of a dimension, if the file has one.
fn dimassoc(text: &str, dim: Handle) -> Option<Vec<(i32, String)>> {
    let hex = dim.hex();
    let mut recs = Vec::new();
    let mut cur: Option<Vec<(i32, String)>> = None;
    for (c, v) in pairs(text) {
        if c == 0 {
            if let Some(r) = cur.take() {
                recs.push(r);
            }
            cur = Some(vec![(c, v)]);
        } else if let Some(r) = cur.as_mut() {
            r.push((c, v));
        }
    }
    if let Some(r) = cur {
        recs.push(r);
    }
    recs.into_iter()
        .filter(|r| r.first().is_some_and(|(c, v)| *c == 0 && v == "DIMASSOC"))
        .find(|r| r.iter().rfind(|(c, _)| *c == 330).is_some_and(|(_, v)| v.eq_ignore_ascii_case(&hex)))
}

/// The `AcDbOsnapPointRef` blocks of a DIMASSOC record, in file order: one per reference the
/// dimension makes to its geometry.
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

/// #1651's repro through the commands: after DIMREASSOCIATE, every dimension the session calls
/// associated (diameter, radius, angular and a linear control) is written with a DIMASSOC whose
/// references name real geometry. Guards the saving side, where radial and diametric dimensions
/// used to fall through the kind-to-slot match and be written without any DIMASSOC object.
#[test]
fn the_issue_repro_saves_every_associated_dimension_with_dimassoc() {
    let mut s = Session::new();
    s.execute("dimstyle", &json!({ "name": "D", "current": true, "DIMTXT": 5, "DIMASZ": 5, "DIMDEC": 0 })).unwrap();
    let circle = handle(&s.execute("circle", &json!({ "center": [0, 0], "radius": 30 })).unwrap());
    let diameter = handle(&s.execute("dimdiameter", &json!({ "handle": circle.hex(), "at": [40, 40] })).unwrap());
    let radius = handle(&s.execute("dimradius", &json!({ "handle": circle.hex(), "at": [-40, -40] })).unwrap());
    let l1 = first_line(&s.execute("line", &json!({ "points": [[100, 0], [200, 0]] })).unwrap());
    let l2 = first_line(&s.execute("line", &json!({ "points": [[100, 0], [180, 60]] })).unwrap());
    let angular = handle(&s.execute("dimangular", &json!({ "lines": [l1.hex(), l2.hex()], "at": [160, 20] })).unwrap());
    let l3 = first_line(&s.execute("line", &json!({ "points": [[0, -80], [100, -80]] })).unwrap());
    let linear = handle(&s.execute("dimlinear", &json!({ "object": l3.hex(), "at": [50, -100] })).unwrap());

    let dims = [diameter, radius, angular, linear];
    {
        let d = s.doc().unwrap();
        for h in dims {
            match d.entity(h).map(|e| &e.kind) {
                Some(EntityKind::Dimension(dm)) => assert!(!dm.assoc.is_empty(), "{} is associative in CADCraft", h.hex()),
                _ => panic!("{} is not a dimension", h.hex()),
            }
        }
    }
    // The issue runs DIMREASSOCIATE before saving; it does not lose any of them.
    let hs: Vec<String> = dims.iter().map(|h| h.hex()).collect();
    let r = s.execute("dimreassociate", &json!({ "handles": hs })).unwrap();
    assert_eq!(r["associated"], json!(4), "{r}");

    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    for (h, what) in [(diameter, "diameter"), (radius, "radius"), (angular, "angular"), (linear, "linear")] {
        let rec = dimassoc(&text, h).unwrap_or_else(|| panic!("{what} dimension {} has a DIMASSOC", h.hex()));
        let refs = point_refs(&rec);
        assert!(!refs.is_empty(), "{what}: the DIMASSOC has point references");
        assert!(refs.iter().all(|r| string(r, 331).is_some()), "{what}: every reference names geometry");
    }
    // The radial and diametric dimensions name the circle by its centre.
    for (h, what) in [(diameter, "diameter"), (radius, "radius")] {
        let refs = point_refs(&dimassoc(&text, h).unwrap());
        assert_eq!(refs.len(), 1, "{what}: one reference, as AutoCAD writes radial dimensions");
        assert_eq!(int(&refs[0], 72), Some(3), "{what}: a centre snap");
        assert!(string(&refs[0], 331).is_some_and(|h| h.eq_ignore_ascii_case(&circle.hex())), "{what}: to the circle");
    }
    // The angular dimension names both lines, and the linear one its line.
    let refs = point_refs(&dimassoc(&text, angular).unwrap());
    assert!(refs.len() >= 2, "angular: at least both leg points: {refs:?}");
    let named = |r: &[(i32, String)], h: Handle| string(r, 331).is_some_and(|x| x.eq_ignore_ascii_case(&h.hex()));
    assert!(refs.iter().any(|r| named(r, l1)) && refs.iter().any(|r| named(r, l2)), "angular: both lines");
    let refs = point_refs(&dimassoc(&text, linear).unwrap());
    assert_eq!(refs.len(), 2, "linear: both origins");
    assert!(refs.iter().all(|r| named(r, l3)), "linear: its line");
}
