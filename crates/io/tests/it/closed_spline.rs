//! A closed fit-point spline is periodic: DXF flags it closed + periodic (group 70 bits 1 and 2)
//! and it reads back as the same smooth closed curve.

use cadcraft_doc::{Common, Drawing, EntityKind, Space};
use cadcraft_geom::{Spline, Vec2};

#[test]
fn closed_fit_spline_roundtrips_as_periodic() {
    let fit = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)];
    let sp = Spline::from_fit_points_closed(&fit);
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Spline(sp.clone())).unwrap();
    let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let spline = &text[text.find("SPLINE").unwrap()..];
    assert!(spline.contains("70\r\n11\r\n"), "planar + closed + periodic");
    let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
    let Some(EntityKind::Spline(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no spline") };
    assert!(r.closed && r.is_periodic());
    assert_eq!(r.fit.len(), 4);
    for i in 0..=20 {
        let t = i as f64 / 20.0;
        assert!(r.eval(t).near(sp.eval(t), 1e-9));
    }
}
