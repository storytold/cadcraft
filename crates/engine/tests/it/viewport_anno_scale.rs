//! A layout viewport's own annotation scale, apart from its viewport scale (gaps F12): inside the
//! viewport CANNOSCALE is the viewport's, `viewport.set {annoScale}` sets it alone, a locked
//! viewport keeps its scale when the annotation scale changes, and `viewport.syncscale` brings the
//! two back together.

use cadcraft_doc::{EntityKind, Viewport};
use cadcraft_engine::Session;
use serde_json::{Value, json};

fn viewport(s: &Session, h: &str) -> Viewport {
    let d = s.doc().unwrap();
    let e = d.layout("Layout1").unwrap().entities.iter().find(|e| e.handle.hex() == h).unwrap();
    match &e.kind {
        EntityKind::Viewport(v) => v.clone(),
        _ => panic!("not a viewport"),
    }
}

fn scale(s: &Session, h: &str) -> f64 {
    let v = viewport(s, h);
    v.height / v.view_height
}

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) {
    s.execute("setvar", &json!({ "name": name, "value": value })).unwrap();
}

/// A metric drawing with a 1:50 viewport on Layout1, entered with MSPACE.
fn session() -> (Session, String) {
    let mut s = Session::new();
    s.new_drawing(true);
    s.viewport_px = (1600.0, 900.0);
    s.execute("line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let r = s.execute("mview", &json!({"p1": [20, 20], "p2": [120, 100]})).unwrap();
    let h = r["viewports"][0].as_str().unwrap().to_string();
    s.execute("viewport.set", &json!({"handle": h, "scale": "1:50", "center": [0, 0]})).unwrap();
    s.execute("mspace", &json!({"handle": h})).unwrap();
    (s, h)
}

#[test]
fn inside_a_viewport_the_annotation_scale_is_the_viewports() {
    let (mut s, h) = session();
    // By default it matches the viewport scale; the Model tab keeps its own.
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:50"));
    assert_eq!(s.doc().unwrap().header.str("CANNOSCALE", "1:1"), "1:1");

    // Set apart from the viewport scale.
    let r = s.execute("viewport.set", &json!({"annoScale": "1:100"})).unwrap();
    assert_eq!(r[0]["annoScale"], json!("1:100"), "{r}");
    assert_eq!(r[0]["scalesDiffer"], json!(true), "{r}");
    assert!((scale(&s, &h) - 0.02).abs() < 1e-12);
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:100"));
    assert!((getvar(&mut s, "CANNOSCALEVALUE").as_f64().unwrap() - 0.01).abs() < 1e-12);

    // New annotative objects drawn in the viewport support its annotation scale.
    s.execute("style", &json!({ "name": "Notes", "annotative": true, "current": true })).unwrap();
    let t = s.execute("text", &json!({ "at": [0, 0], "text": "NOTE", "height": 2.5 })).unwrap();
    let th = cadcraft_doc::Handle::parse_hex(t["handle"].as_str().unwrap()).unwrap();
    let scales: Vec<String> = s.doc().unwrap().entity(th).unwrap().common.anno_scales.iter().map(|x| x.name.clone()).collect();
    assert_eq!(scales, ["1:100"]);

    // Undo takes the annotation scale back with the rest.
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert!(viewport(&s, &h).anno_scale.is_none());

    // Back on paper, CANNOSCALE is the drawing's again.
    s.execute("viewport.set", &json!({"handle": h, "annoScale": "1:100"})).unwrap();
    s.execute("pspace", &json!({})).unwrap();
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:1"));
}

#[test]
fn cannoscale_moves_an_unlocked_viewport_but_not_a_locked_one() {
    let (mut s, h) = session();
    setvar(&mut s, "CANNOSCALE", json!("1:20"));
    assert!((scale(&s, &h) - 0.05).abs() < 1e-12, "{}", scale(&s, &h));
    assert!(!s.doc().unwrap().viewport_scales_differ(&viewport(&s, &h)));
    assert_eq!(s.doc().unwrap().header.str("CANNOSCALE", "1:1"), "1:1", "the Model tab's scale is left alone");

    s.execute("viewport.set", &json!({"handle": h, "locked": true})).unwrap();
    setvar(&mut s, "CANNOSCALE", json!("1:10"));
    assert!((scale(&s, &h) - 0.05).abs() < 1e-12, "a locked viewport keeps its scale");
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:10"));
    assert!(s.doc().unwrap().viewport_scales_differ(&viewport(&s, &h)));

    // Synchronizing the viewport scale needs an unlocked viewport; the other way round doesn't.
    assert!(s.execute("viewport.syncscale", &json!({})).is_err());
    let r = s.execute("viewport.syncscale", &json!({"to": "viewport"})).unwrap();
    assert_eq!(r[0]["annoScale"], json!("1:20"), "{r}");
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:20"));

    s.execute("viewport.set", &json!({"handle": h, "locked": false, "annoScale": "1:100"})).unwrap();
    let r = s.execute("viewport.syncscale", &json!({})).unwrap();
    assert_eq!(r[0]["scalesDiffer"], json!(false), "{r}");
    assert!((scale(&s, &h) - 0.01).abs() < 1e-12, "{}", scale(&s, &h));
}

#[test]
fn a_new_viewport_scale_brings_the_annotation_scale_with_it() {
    let (mut s, h) = session();
    s.execute("viewport.set", &json!({"annoScale": "1:100"})).unwrap();
    s.execute("viewport.set", &json!({"scale": "1:10"})).unwrap();
    assert!(viewport(&s, &h).anno_scale.is_none());
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:10"));
    // A viewport scale the list lacks is named after its ratio.
    s.execute("viewport.set", &json!({"scale": 1.0 / 37.5})).unwrap();
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:37.5"));
    // Both in one call: the annotation scale given wins.
    s.execute("viewport.set", &json!({"scale": "1:50", "annoScale": "1:20"})).unwrap();
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:20"));
    assert!((scale(&s, &h) - 0.02).abs() < 1e-12);
    s.execute("viewport.set", &json!({"annoScale": null})).unwrap();
    assert_eq!(getvar(&mut s, "CANNOSCALE"), json!("1:50"));
}

#[test]
fn a_scale_a_viewport_uses_stays_in_the_list() {
    let (mut s, _) = session();
    s.execute("scalelistedit", &json!({"add": {"name": "Detail", "ratio": "1:3"}})).unwrap();
    s.execute("viewport.set", &json!({"annoScale": "Detail"})).unwrap();
    assert!(s.execute("scalelistedit", &json!({"delete": "Detail"})).is_err());
    let r = s.execute("scalelistedit", &json!({"reset": true})).unwrap();
    assert!(r["scales"].as_array().unwrap().iter().any(|x| x["name"] == "Detail"), "{r}");
}

#[test]
fn hostile_viewport_annotation_scales_are_refused() {
    let (mut s, h) = session();
    for bad in [json!("No such scale"), json!(5), json!(["1:50"]), json!({"name": "1:50"}), json!(""), json!("\u{0}")] {
        assert!(s.execute("viewport.set", &json!({"handle": h, "annoScale": bad})).is_err(), "{bad}");
    }
    for bad in [json!("x"), json!(3), json!(null)] {
        let r = s.execute("viewport.syncscale", &json!({"to": bad}));
        // null reads as the default.
        assert_eq!(r.is_err(), !bad.is_null(), "{bad}");
    }
    assert!(viewport(&s, &h).anno_scale.is_none());
    // No viewport to act on.
    s.execute("pspace", &json!({})).unwrap();
    s.set_selection(Vec::new());
    assert!(s.execute("viewport.syncscale", &json!({})).is_err());
    assert!(s.execute("viewport.syncscale", &json!({"handle": "ZZZZ"})).is_err());
    // A viewport whose size went bad shows annotation at 1:1 rather than failing.
    let mut d = s.doc().unwrap().clone();
    let vp = Viewport { height: 0.0, view_height: f64::NAN, anno_scale: Some(cadcraft_doc::AnnoScale::new("bad", 0.0, 1.0)), ..Default::default() };
    assert_eq!(d.viewport_anno_scale(&vp).name, "1:1");
    assert!(!d.viewport_scales_differ(&vp));
    d.scales.clear();
    let vp = Viewport { height: 1.0, view_height: f64::MAX, ..Default::default() };
    assert!(d.viewport_anno_scale(&vp).is_valid());
}
