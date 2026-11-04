//! QDIM over many objects: one snap index per call (no rescan of the drawing per dimension), the
//! same associations as DIMREASSOCIATE, and a cap on the input size.

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

fn handles(r: &Value) -> Vec<Value> {
    r["handles"].as_array().unwrap().clone()
}

#[test]
fn qdim_attaches_like_dimreassociate_and_large_input_is_capped() {
    // Lines end to end with a point on every joint (ties with the line ends) and a circle around
    // every midpoint (its centre is a QDIM point and outranks the line midpoint).
    let mut s = Session::new();
    for i in 0..40 {
        let x = f64::from(i);
        s.execute("line", &json!({"points": [[x, 0], [x + 1.0, 0]]})).unwrap();
        s.execute("point", &json!({"at": [x, 0]})).unwrap();
        s.execute("circle", &json!({"center": [x + 0.5, 0], "radius": 0.1})).unwrap();
    }
    let objs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    let r = s.execute("qdim", &json!({"handles": objs, "at": [0, 3]})).unwrap();
    let hs = handles(&r);
    assert_eq!(hs.len(), 80, "one dimension between each pair of neighbouring points");
    let quick = assoc_of(&s, &hs);
    assert!(quick.iter().all(|a| a.len() == 2), "every point is on an object");
    // DIMREASSOCIATE scans the drawing per point; the indexed QDIM must agree with it.
    s.execute("dimreassociate", &json!({"handles": hs})).unwrap();
    assert_eq!(assoc_of(&s, &hs), quick);

    // Many objects finish (this was quadratic in the number of dimensions).
    let mut s = Session::new();
    let pts: Vec<[f64; 2]> = (0..=20_000).map(|i| [f64::from(i), 0.0]).collect();
    let objs = handles(&s.execute("line", &json!({"points": pts})).unwrap());
    let r = s.execute("qdim", &json!({"handles": objs, "at": [0, 3]})).unwrap();
    let hs = handles(&r);
    assert_eq!(hs.len(), 20_000);
    assert!(assoc_of(&s, &hs[..10]).iter().all(|a| a.len() == 2));

    // More than the cap (objects, or distinct points) is refused with nothing created.
    let n = s.doc().unwrap().model.iter().count();
    let too_many: Vec<Value> = (0..100_001).map(|_| objs[0].clone()).collect();
    let e = s.execute("qdim", &json!({"handles": too_many, "at": [0, 3]})).unwrap_err();
    assert!(e.to_string().contains("100000"), "{e}");
    let vertices: Vec<[f64; 2]> = (0..100_002).map(|i| [f64::from(i), f64::from(i % 2)]).collect();
    s.execute("pline", &json!({"vertices": vertices})).unwrap();
    let pl = s.doc().unwrap().model.last().unwrap().handle.hex();
    let n = n + 1;
    let e = s.execute("qdim", &json!({"handles": [pl], "at": [0, 3]})).unwrap_err();
    assert!(e.to_string().contains("100000"), "{e}");
    assert_eq!(s.doc().unwrap().model.iter().count(), n);
}
