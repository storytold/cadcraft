//! Construction styling must not inherit a user's gapless redefinition of DASHED.

use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, Line, Linetype, RawTag, Space};
use cadcraft_geom::Vec3;
use cadcraft_render::{Kind, Options, build};

/// Construction dashes keep gaps when the drawing redefines dashed as solid.
#[test]
fn construction_dashes_keep_gaps_when_the_drawing_redefines_dashed_as_solid() {
    let mut d = Drawing::new_metric();
    d.block_editor = true;
    d.linetypes.push(Linetype::simple("DASHED", "User-defined adjacent strokes", &[0.5, 0.25]));
    let mut e = Entity::new(Handle(1), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) }));
    e.common.xdata = vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }];
    d.model.push(e);

    let list = build(&d, &Space::Model, &Options::default());
    assert!(list.prims.len() > 1, "construction has several visible dash strokes");
    let ink: f64 = list
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline)
        .map(|p| list.points(p).windows(2).map(|pair| pair[0].dist(pair[1])).sum::<f64>())
        .sum();
    assert!(ink > 0.0 && ink < 99.0, "a 100-unit construction line needs actual gaps, not contiguous pieces: ink={ink}");
}
