//! The "Default" lineweight is the width LWDEFAULT names (issue #292).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Lineweight, Space};
use cadcraft_geom::Vec3;
use cadcraft_render::{Kind, Options, build};

/// Lineweights (mm) of a ByLayer line on layer 0 (lineweight Default) and an explicit 0.50 mm line.
fn widths(lwdefault: Option<i64>) -> Vec<f32> {
    let mut d = Drawing::new_metric();
    if let Some(v) = lwdefault {
        d.header.set_i64("LWDEFAULT", v);
    }
    let line = |y: f64| EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) });
    d.add(&Space::Model, Common::default(), line(0.0)).unwrap();
    d.add(&Space::Model, Common { lineweight: Lineweight::Mm100(50), ..Default::default() }, line(1.0)).unwrap();
    let l = build(&d, &Space::Model, &Options { lineweights: true, ..Default::default() });
    l.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| p.lw).collect()
}

#[test]
fn default_lineweight_follows_lwdefault() {
    assert_eq!(widths(None), [0.25, 0.5], "unset: 0.25 mm");
    assert_eq!(widths(Some(70)), [0.7, 0.5]);
    assert_eq!(widths(Some(0)), [0.0, 0.5]);
    assert_eq!(widths(Some(-3)), [0.25, 0.5], "out of range: 0.25 mm");
    assert_eq!(widths(Some(5000)), [0.25, 0.5]);
}
