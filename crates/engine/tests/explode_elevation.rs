//! EXPLODE of an elevated polyline keeps the pieces at the polyline's elevation.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use serde_json::json;

fn explode_elevated(vertices: serde_json::Value, closed: bool) -> Vec<EntityKind> {
    let mut s = Session::new();
    let r = s.execute("pline", &json!({ "vertices": vertices, "closed": closed })).unwrap();
    let hex = r.get("handle").unwrap_or(&r["handles"][0]).as_str().unwrap().to_owned();
    let h = Handle::parse_hex(&hex).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::LwPolyline(p) = &mut e.kind {
                p.elevation = 7.0;
            }
        })
        .unwrap();
    s.execute("explode", &json!({ "handles": [hex] })).unwrap();
    s.doc().unwrap().model.iter().map(|e| e.kind.clone()).collect()
}

fn assert_all_at_seven(kinds: &[EntityKind], lines: usize, arcs: usize) {
    let (mut nl, mut na) = (0, 0);
    for k in kinds {
        match k {
            EntityKind::Line(l) => {
                nl += 1;
                assert_eq!((l.a.z, l.b.z), (7.0, 7.0), "line {l:?}");
            }
            EntityKind::Arc(a) => {
                na += 1;
                assert_eq!(a.center.z, 7.0, "arc {a:?}");
            }
            k => panic!("unexpected entity {k:?}"),
        }
    }
    assert_eq!((nl, na), (lines, arcs));
}

#[test]
fn explode_keeps_elevation_of_open_polyline() {
    let kinds = explode_elevated(json!([[0, 0], {"p": [10, 0], "bulge": 1}, [10, 10]]), false);
    assert_all_at_seven(&kinds, 1, 1);
}

#[test]
fn explode_keeps_elevation_of_closed_polyline() {
    let kinds = explode_elevated(json!([[0, 0], [10, 0], {"p": [10, 10], "bulge": 0.5}]), true);
    assert_all_at_seven(&kinds, 2, 1);
}
