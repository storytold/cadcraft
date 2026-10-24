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

#[test]
fn layout_pdf_and_png_place_geometry_from_the_same_printable_origin() {
    let mut d = Drawing::new_metric();
    let page = &mut d.layouts[1].page;
    page.width_mm = 210.0;
    page.height_mm = 297.0;
    page.landscape = true;
    page.margins_mm = [7.5, 20.0, 7.5, 20.0];
    page.offset_mm = [3.0, -2.0];
    let space = Space::Paper(d.layouts[1].name.clone());
    d.add(&space, Common::default(), EntityKind::Line(Line { a: Vec3::new(3.0, 4.0, 0.0), b: Vec3::new(8.0, 4.0, 0.0) })).unwrap();
    let bytes = cadcraft_io::plot(&d, &space, &json!({"compress": false})).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let points: Vec<Vec<f64>> = text
        .lines()
        .filter(|l| l.ends_with(" m") || l.ends_with(" l"))
        .map(|l| l.split_whitespace().take(2).map(|v| v.parse::<f64>().unwrap() * 25.4 / 72.0).collect())
        .collect();
    assert_eq!(points.len(), 2);
    for (p, x) in points.iter().zip([26.0, 31.0]) {
        assert!((p[0] - x).abs() < 0.01 && (p[1] - 9.5).abs() < 0.01, "{p:?}");
    }
    let png = cadcraft_io::plotter::plot(&d, &space, &json!({"output": "png", "resolution": 100})).unwrap();
    let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let dark: Vec<_> = image.pixels().iter().enumerate().filter(|(_, p)| p.red() < 128).map(|(i, _)| i).collect();
    assert!(!dark.is_empty());
    let width = image.width() as usize;
    let left_mm = dark.iter().map(|i| i % width).min().unwrap() as f64 * 25.4 / 100.0;
    let bottom_mm = (image.height() as usize - dark.iter().map(|i| i / width).max().unwrap()) as f64 * 25.4 / 100.0;
    assert!((left_mm - 26.0).abs() < 0.5 && (bottom_mm - 9.5).abs() < 0.5, "{left_mm}, {bottom_mm}");
}
