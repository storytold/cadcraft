//! Programmatic SPLINE keeps the degree within 1..=10 like the Degree prompt (issue #268): a high
//! degree on many control points made tessellation and extents take minutes.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

#[test]
fn programmatic_spline_degree_is_limited() {
    let control: Vec<[f64; 2]> = (0..3000).map(|i| [f64::from(i), f64::from(i % 7)]).collect();
    for (degree, want) in [(json!(2_147_483_647), 10), (json!(2999), 10), (json!(11), 10), (json!(0), 1), (json!(4), 4)] {
        let mut s = Session::new();
        s.execute("spline", &json!({ "control": control, "degree": degree })).unwrap();
        let d = s.doc().unwrap();
        let Some(EntityKind::Spline(sp)) = d.model.iter().last().map(|e| &e.kind) else { panic!("no spline") };
        assert_eq!(sp.degree, want);
        let ext = d.extents(&s.space());
        assert!(ext.min.x <= 0.0 && ext.max.x >= 2999.0, "{ext:?}");
    }
}

/// Control points are capped like `spline.cv` (issue #719): one more than the cap is refused
/// without adding anything, and the cap itself still draws.
#[test]
fn programmatic_spline_control_count_is_limited() {
    let control = |n: i32| -> Vec<[f64; 2]> { (0..n).map(|i| [f64::from(i), f64::from(i % 7)]).collect() };
    let mut s = Session::new();
    assert!(s.execute("spline", &json!({ "control": control(20_001) })).is_err());
    assert_eq!(s.doc().unwrap().model.len(), 0);
    s.execute("spline", &json!({ "control": control(20_000) })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
}
