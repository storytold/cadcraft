//! A model-space PNG plot with `fit: false` and no `scale` plots at 1:1, as PDF does (issue #713).

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

/// The RGBA rows of an 8-bit RGBA PNG (the only kind the plotter writes).
fn decode_rgba(png: &[u8]) -> (usize, Vec<Vec<u8>>) {
    let be = |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
    let (w, h) = (be(16), be(20));
    assert_eq!((png[24], png[25]), (8, 6), "8-bit RGBA");
    let mut idat = Vec::new();
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = be(at);
        if &png[at + 4..at + 8] == b"IDAT" {
            idat.extend_from_slice(&png[at + 8..at + 8 + len]);
        }
        at += 12 + len;
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).unwrap();
    let stride = w * 4;
    let mut rows: Vec<Vec<u8>> = Vec::with_capacity(h);
    for y in 0..h {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let prev = if y > 0 { rows[y - 1].clone() } else { vec![0; stride] };
        let mut cur = line[1..].to_vec();
        for i in 0..stride {
            let a = if i >= 4 { i32::from(cur[i - 4]) } else { 0 };
            let b = i32::from(prev[i]);
            let c = if i >= 4 { i32::from(prev[i - 4]) } else { 0 };
            let add = match line[0] {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                f => panic!("PNG filter {f}"),
            };
            cur[i] = cur[i].wrapping_add(add as u8);
        }
        rows.push(cur);
    }
    (w, rows)
}

/// Plotted length in millimetres of a 100-unit horizontal line in model space, on an A4
/// landscape PNG at 100 dpi with 5 mm margins (printable area 287 × 200 mm).
fn plotted_mm(opts: Value) -> f64 {
    let mut d = Drawing::new_metric();
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) });
    d.add(&Space::Model, Common::default(), line).unwrap();
    let mut o =
        json!({"output": "png", "width": 210, "height": 297, "landscape": true, "margins": [5, 5, 5, 5], "resolution": 100, "lineweights": false});
    if let (Some(o), Some(e)) = (o.as_object_mut(), opts.as_object()) {
        o.extend(e.clone());
    }
    let (w, rows) = decode_rgba(&cadcraft_io::plotter::plot(&d, &Space::Model, &o).unwrap());
    let inked: Vec<usize> = (0..w).filter(|x| rows.iter().any(|r| r[x * 4] < 128)).collect();
    let (first, last) = (inked.first().unwrap(), inked.last().unwrap());
    (last - first) as f64 * 25.4 / 100.0
}

#[test]
fn model_png_without_fit_or_scale_plots_one_to_one() {
    // One pixel is 0.254 mm at 100 dpi.
    for (opts, mm) in [
        (json!({"fit": false}), 100.0),
        (json!({"fit": false, "scale": 1}), 100.0),
        (json!({"fit": false, "scale": 0.5}), 50.0),
        (json!({"fit": true}), 287.0),
        (json!({}), 287.0),
    ] {
        let got = plotted_mm(opts.clone());
        assert!((got - mm).abs() < 0.6, "{opts}: {got} mm, want {mm}");
    }
}
