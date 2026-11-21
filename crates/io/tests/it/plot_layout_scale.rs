//! An explicit numeric `scale` on a non-fitted layout plot scales the geometry (issue #490).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

/// Plotted length in millimetres of the one 90-unit line in Layout1.
fn plotted_mm(opts: Value) -> f64 {
    let mut d = Drawing::new_metric();
    let line = EntityKind::Line(Line { a: Vec3::new(10.0, 10.0, 0.0), b: Vec3::new(100.0, 10.0, 0.0) });
    d.add(&Space::Paper("Layout1".into()), Common::default(), line).unwrap();
    let mut o = json!({"compress": false});
    if let (Some(o), Some(e)) = (o.as_object_mut(), opts.as_object()) {
        o.extend(e.clone());
    }
    let bytes = cadcraft_io::plot(&d, &Space::Paper("Layout1".into()), &o).unwrap();
    let doc = crate::pdf_read::read(&bytes);
    let xs: Vec<f64> =
        crate::pdf_read::drawn(doc.content()).iter().filter(|o| o.op == "m" || o.op == "l").filter_map(|o| o.nums.first().copied()).collect();
    assert_eq!(xs.len(), 2, "{}", doc.content());
    (xs[1] - xs[0]).abs() * 25.4 / 72.0
}

#[test]
fn layout_plot_honours_explicit_scale_without_fit() {
    for (scale, mm) in [(1.0, 90.0), (0.5, 45.0), (2.0, 180.0)] {
        let got = plotted_mm(json!({"fit": false, "scale": scale}));
        assert!((got - mm).abs() < 0.01, "scale {scale}: {got} mm, want {mm}");
    }
    let default = plotted_mm(json!({}));
    assert!((default - 90.0).abs() < 0.01, "default stays 1:1: {default}");
}
