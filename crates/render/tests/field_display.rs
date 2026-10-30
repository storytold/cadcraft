//! Fields are drawn as their values, on a grey background while FIELDDISPLAY is 1 and never
//! when plotting (#532).

use cadcraft_doc::fields;
use cadcraft_doc::{Common, Drawing, EntityKind, HVal, Space, Text};
use cadcraft_geom::Vec3;
use cadcraft_render::{Options, build, build_plot};

fn drawing(value: &str) -> Drawing {
    let mut d = Drawing::new_metric();
    let t = Text {
        insert: Vec3::new(0.0, 0.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.3,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t.clone())).unwrap();
    let mt = serde_json::from_value(serde_json::json!({"insert": {"x": 0.0, "y": 10.0, "z": 0.0}, "height": 2.5, "contents": value})).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mt)).unwrap();
    d
}

#[test]
fn values_on_a_grey_background_on_screen_only() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    let with_field = drawing(&format!("A {code}"));
    let plain = drawing("A PLATE");
    let o = Options::default();
    let f = build(&with_field, &Space::Model, &o);
    let p = build(&plain, &Space::Model, &o);
    // The same glyphs as the plain value, plus a grey box (two triangles) per text.
    assert_eq!(f.verts.len(), p.verts.len());
    assert_eq!(f.tris.len(), p.tris.len() + 12);
    assert!(build_plot(&with_field, &Space::Model, &o).tris.len() == p.tris.len());
    let mut off = with_field.clone();
    off.header.set("FIELDDISPLAY", HVal::Int(0));
    assert_eq!(build(&off, &Space::Model, &o).tris.len(), p.tris.len());
}

#[test]
fn layout_matching_fields_keep_glyphs_and_background_upright() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    let upright = |value: &str, rotation: f64| {
        let mut d = drawing(value);
        let entities: Vec<_> = d.model.iter().map(|e| (**e).clone()).collect();
        for mut e in entities {
            e.common.xdata =
                vec![cadcraft_doc::RawTag { code: 1001, value: "AcadAnnoPO".into() }, cadcraft_doc::RawTag { code: 1070, value: "1".into() }];
            if let EntityKind::Text(t) = &mut e.kind {
                t.rotation = rotation;
            }
            if let EntityKind::MText(t) = &mut e.kind {
                t.rotation = rotation;
            }
            d.model.push(e);
        }
        d
    };
    let points = |d: &Drawing, plot: bool| {
        let list = if plot { build_plot(d, &Space::Model, &Options::default()) } else { build(d, &Space::Model, &Options::default()) };
        list.prims.iter().flat_map(|p| list.points(p).iter().copied()).collect::<Vec<_>>()
    };
    let rotated = upright(&code, std::f64::consts::PI);
    let zero = upright(&code, 0.0);
    let plain = upright("PLATE", 0.0);
    assert!(!points(&rotated, false).is_empty());
    assert_eq!(points(&rotated, false), points(&zero, false), "glyphs and field backgrounds ignore stored rotation");
    assert_eq!(points(&rotated, true), points(&plain, true), "plotting draws the displayed field value upright without a background");
}
