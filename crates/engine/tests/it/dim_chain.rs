//! DIMCONTINUE / DIMBASELINE with many points: one snap index per chain (no rescan of the
//! drawing per point), the same associations as DIMREASSOCIATE, and a cap on the input size.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{DimAssoc, EntityKind, Handle};
use serde_json::{Value, json};

fn assoc_of(s: &Session, handles: &[Value]) -> Vec<Vec<DimAssoc>> {
    let d = s.doc().unwrap();
    handles
        .iter()
        .map(|h| match &d.entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap().kind {
            EntityKind::Dimension(dm) => dm.assoc.clone(),
            k => panic!("not a dimension: {k:?}"),
        })
        .collect()
}

#[test]
fn chains_attach_like_dimreassociate_and_long_chains_are_capped() {
    // Lines end to end, a point on every joint (ties with the line start) and a circle on every
    // midpoint (a centre outranks a line midpoint).
    let mut s = Session::new();
    for i in 0..40 {
        let x = f64::from(i);
        s.execute("line", &json!({"points": [[x, 0], [x + 1.0, 0]]})).unwrap();
        s.execute("point", &json!({"at": [x, 0]})).unwrap();
        s.execute("circle", &json!({"center": [x + 0.5, 0], "radius": 0.1})).unwrap();
    }
    for (cmd, at) in [("dimcontinue", 2), ("dimbaseline", 5)] {
        s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [1, 0], "at": [0, at]})).unwrap();
        let pts: Vec<[f64; 2]> = (3..80).map(|i| [f64::from(i) * 0.5, 0.0]).collect();
        let r = s.execute(cmd, &json!({"points": pts})).unwrap();
        let hs = r["handles"].as_array().unwrap().clone();
        assert_eq!(hs.len(), pts.len());
        let chained = assoc_of(&s, &hs);
        assert!(chained.iter().all(|a| a.len() == 2), "{cmd}: every point is on an object");
        // DIMREASSOCIATE scans the drawing per point; the indexed chain must agree with it.
        s.execute("dimreassociate", &json!({"handles": hs})).unwrap();
        assert_eq!(assoc_of(&s, &hs), chained, "{cmd}");
    }

    // A long chain finishes (this was quadratic in the number of points); more than the cap is
    // refused with nothing created.
    let mut s = Session::new();
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [1, 0], "at": [0, 2]})).unwrap();
    let pts: Vec<[f64; 2]> = (2..20_002).map(|i| [f64::from(i), 0.0]).collect();
    assert_eq!(s.execute("dimcontinue", &json!({"points": pts})).unwrap()["handles"].as_array().unwrap().len(), 20_000);
    let n = s.doc().unwrap().model.iter().count();
    let too_many: Vec<[f64; 2]> = (0..100_001).map(|i| [f64::from(i), 1.0]).collect();
    let e = s.execute("dimbaseline", &json!({"points": too_many})).unwrap_err();
    assert!(e.to_string().contains("100000"), "{e}");
    assert_eq!(s.doc().unwrap().model.iter().count(), n);
}
