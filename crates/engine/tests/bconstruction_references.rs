//! Construction guides remain in the definition, without leaking into reference operations.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle, RawTag, Space};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::snap::{mode, osnap};
use serde_json::json;

fn reference() -> (Session, Handle, Handle, Handle) {
    let mut s = Session::new();
    let regular = s.execute("line", &json!({ "points": [[0, 0], [100, 100]] })).unwrap()["handles"][0].clone();
    let guide = s.execute("line", &json!({ "points": [[0, 50], [100, 50]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Guided", "base": [0, 0], "handles": [regular, guide] })).unwrap();
    let reference = s.doc().unwrap().model.handles()[0];
    let block = Arc::make_mut(s.doc_mut().unwrap().blocks.get_mut("Guided").unwrap());
    let stored = block.entities.handles();
    let (regular, guide) = (stored[0], stored[1]);
    assert!(block.entities.modify(guide, |entity| {
        entity.common.xdata = vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }];
    }));
    (s, reference, regular, guide)
}

#[test]
fn invisible_guides_inside_reference_bounds_are_not_snap_targets() {
    for nested in [false, true] {
        let (mut s, reference, _, _) = reference();
        if nested {
            s.execute("block", &json!({ "name": "Outer", "base": [0, 0], "handles": [reference.hex()] })).unwrap();
        }
        let d = s.doc().unwrap();
        // The regular diagonal still covers this point's bounding box: a bounds-only filter
        // cannot hide the guide from the separate block-leaf snapping traversal.
        assert!(d.extents(&Space::Model).contains(Vec2::new(0.0, 50.0)));
        assert!(osnap(d, &Space::Model, Vec2::new(0.0, 0.0), 0.25, mode::END, None, false).is_some());
        assert!(osnap(d, &Space::Model, Vec2::new(0.0, 50.0), 0.25, mode::END, None, false).is_none(), "nested={nested}");
    }
}

#[test]
fn exploding_a_reference_does_not_materialize_its_construction_guides() {
    let (mut s, reference, _, _) = reference();
    s.execute("explode", &json!({ "handles": [reference.hex()] })).unwrap();
    let model = &s.doc().unwrap().model;
    assert_eq!(model.len(), 1);
    let EntityKind::Line(line) = &model.iter().next().unwrap().kind else { panic!("the regular line must remain") };
    assert_eq!((line.a.xy(), line.b.xy()), (Vec2::ZERO, Vec2::new(100.0, 100.0)));
    assert_eq!(s.doc().unwrap().block("Guided").unwrap().entities.len(), 2, "the definition keeps its guide");
}

#[test]
fn construction_style_cannot_be_changed_by_property_commands() {
    let (mut s, _, regular, guide) = reference();
    s.execute("bedit", &json!({ "name": "Guided" })).unwrap();
    s.execute("layer.new", &json!({ "name": "Other" })).unwrap();
    s.execute("properties.set", &json!({ "handles": [regular.hex()], "color": 3 })).unwrap();
    for params in [json!({ "color": 1 }), json!({ "layer": "Other" }), json!({ "linetype": "ByBlock" })] {
        let before = s.state().unwrap().clone();
        let mut params = params;
        params["handles"] = json!([regular.hex(), guide.hex()]);
        assert!(s.execute("properties.set", &params).is_err(), "accepted {params}");
        assert!(Arc::ptr_eq(&before.doc, &s.state().unwrap().doc), "a rejected mixed edit is atomic");
        assert_eq!(before.undo.len(), s.state().unwrap().undo.len());
    }
    let before = s.state().unwrap().clone();
    assert!(s.execute("matchprop", &json!({ "source": regular.hex(), "targets": [guide.hex()] })).is_err());
    assert!(Arc::ptr_eq(&before.doc, &s.state().unwrap().doc));
    assert_eq!(before.undo.len(), s.state().unwrap().undo.len());
}

#[test]
fn the_existing_explicit_dxf_codec_preserves_construction_metadata() {
    let (s, _, _, guide) = reference();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "synthetic.dxf").unwrap();
    let reopened = cadcraft_io::read(&bytes, "synthetic.dxf").unwrap();
    let entity = reopened.block("Guided").unwrap().entities.get(guide).unwrap();
    assert_eq!(entity.common.xdata, s.doc().unwrap().block("Guided").unwrap().entities.get(guide).unwrap().common.xdata);
    assert_eq!(reopened.block("Guided").unwrap().entities.len(), 2);
}
