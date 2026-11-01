//! Annotative text with "Match orientation to layout" (`AcadAnnoPO` xdata = 1) draws upright,
//! ignoring its stored rotation (#2498).

use cadcraft_doc::{Common, Drawing, EntityKind, RawTag, Space, Text};
use cadcraft_geom::{Bounds2, Vec3};
use cadcraft_render::{Options, build};

fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.into() }
}

fn bounds(rotation: f64, match_orientation: Option<&str>) -> Bounds2 {
    let mut d = Drawing::new_metric();
    let mut common = Common::default();
    if let Some(flag) = match_orientation {
        common.xdata = vec![
            tag(1001, "AcadAnnotative"),
            tag(1000, "AnnotativeData"),
            tag(1002, "{"),
            tag(1070, "1"),
            tag(1070, "1"),
            tag(1002, "}"),
            tag(1001, "AcadAnnoPO"),
            tag(1070, flag),
        ];
    }
    let text = Text {
        insert: Vec3::new(10.0, 5.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: "TITLE".into(),
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, common, EntityKind::Text(text)).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    let mut b = Bounds2::EMPTY;
    for p in &l.prims {
        for q in l.points(p) {
            b.add(*q);
        }
    }
    b
}

fn same(a: &Bounds2, b: &Bounds2) -> bool {
    (a.min.x - b.min.x).abs() < 1e-9 && (a.min.y - b.min.y).abs() < 1e-9 && (a.max.x - b.max.x).abs() < 1e-9 && (a.max.y - b.max.y).abs() < 1e-9
}

#[test]
fn match_orientation_ignores_the_stored_rotation() {
    let upright = bounds(0.0, None);
    assert!(!upright.is_empty());
    assert!(same(&bounds(std::f64::consts::PI, Some("1")), &upright));
}

#[test]
fn stored_rotation_applies_without_the_flag() {
    let upright = bounds(0.0, None);
    assert!(!same(&bounds(std::f64::consts::PI, None), &upright));
    assert!(!same(&bounds(std::f64::consts::PI, Some("0")), &upright));
}
