//! COPY and MIRROR of a layout viewport keep its own UCS and its unmodelled settings (#1316): they
//! belong to the viewport, like its view.

use cadcraft_doc::{EntityKind, Handle, RawTag, Space, Viewport, ViewportUcs};
use cadcraft_engine::Session;
use cadcraft_geom::Vec3;
use serde_json::json;

fn viewports(s: &Session) -> Vec<(Handle, Viewport)> {
    let d = s.doc().unwrap();
    d.layout("Layout1")
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 => Some((e.handle, v.clone())),
            _ => None,
        })
        .collect()
}

fn ucs() -> ViewportUcs {
    ViewportUcs {
        origin: Vec3::new(100.0, 50.0, 0.0),
        x_axis: Vec3::new(0.0, 1.0, 0.0),
        y_axis: Vec3::new(-1.0, 0.0, 0.0),
        elevation: 2.5,
        ortho: 0,
        named: Some("SITE".into()),
        base: None,
    }
}

fn extra() -> Vec<RawTag> {
    vec![RawTag { code: 1, value: "monochrome.ctb".into() }, RawTag { code: 15, value: "10.0".into() }, RawTag { code: 170, value: "2".into() }]
}

/// Layout1 with one viewport that has its own UCS and settings.
fn session() -> (Session, Handle) {
    let mut s = Session::new();
    s.new_drawing(true);
    s.execute("line", &json!({"points": [[0, 0], [100, 100]]})).unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let old: Vec<String> = viewports(&s).iter().map(|(h, _)| h.hex()).collect();
    if !old.is_empty() {
        s.execute("erase", &json!({"handles": old})).unwrap();
    }
    s.execute("mview", &json!({"p1": [10, 10], "p2": [110, 80]})).unwrap();
    let (h, _) = viewports(&s).into_iter().next().unwrap();
    let d = s.doc_mut().unwrap();
    let store = d.space_mut(&Space::Paper("Layout1".into())).unwrap();
    let mut e = (**store.get(h).unwrap()).clone();
    if let EntityKind::Viewport(v) = &mut e.kind {
        v.ucs = Some(ucs());
        v.ucs_per_viewport = true;
        v.extra = extra();
    }
    assert!(store.replace(e));
    (s, h)
}

fn assert_kept(v: &Viewport) {
    assert_eq!(v.ucs, Some(ucs()));
    assert!(v.ucs_per_viewport);
    assert_eq!(v.extra, extra());
}

#[test]
fn copy_keeps_a_viewport_s_ucs_and_settings() {
    let (mut s, h) = session();
    s.execute("copy", &json!({"handles": [h.hex()], "delta": [150, 0]})).unwrap();
    let vps = viewports(&s);
    assert_eq!(vps.len(), 2);
    for (_, v) in &vps {
        assert_kept(v);
    }
}

#[test]
fn mirror_keeps_a_viewport_s_ucs_and_settings() {
    let (mut s, h) = session();
    s.execute("mirror", &json!({"handles": [h.hex()], "p1": [0, 100], "p2": [10, 100]})).unwrap();
    for (_, v) in &viewports(&s) {
        assert_kept(v);
    }
}
