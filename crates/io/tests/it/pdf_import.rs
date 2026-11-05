//! PDFIMPORT: the vector content of a PDF page as editable objects. The PDFs are written here by
//! hand (a content stream and the standard Helvetica) or by CADCraft's own plotter.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, Entity, EntityKind, HAlign, Line, Lineweight, Space, Text};
use cadcraft_geom::Vec3;
use cadcraft_io::pdf_read::{MM_PER_PT, PdfImportOptions, read};
use serde_json::json;

/// A one-page PDF, 600 × 800 points, drawing `content` with Helvetica as `/F1`.
fn page(content: &str) -> Vec<u8> {
    pages(&[content])
}

fn pages(contents: &[&str]) -> Vec<u8> {
    pages_by(contents, Some("(Autodesk AutoCAD 2024 DWG To PDF)"))
}

/// Like [`pages`], with the `/Producer` value (a PDF string) the file's information names, or none.
fn pages_by(contents: &[&str], producer: Option<&str>) -> Vec<u8> {
    let n = contents.len();
    let kids: String = (0..n).map(|i| format!("{} 0 R ", 10 + 2 * i)).collect();
    let mut pdf = format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [{kids}] /Count {n} >> endobj\n\
         3 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> endobj\n"
    );
    for (i, c) in contents.iter().enumerate() {
        pdf.push_str(&format!(
            "{} 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >> endobj\n\
             {} 0 obj << /Length {} >> stream\n{c}\nendstream endobj\n",
            10 + 2 * i,
            11 + 2 * i,
            11 + 2 * i,
            c.len()
        ));
    }
    match producer {
        Some(p) => pdf.push_str(&format!("4 0 obj << /Producer {p} >> endobj\ntrailer << /Root 1 0 R /Info 4 0 R >>\n%%EOF\n")),
        None => pdf.push_str("trailer << /Root 1 0 R >>\n%%EOF\n"),
    }
    pdf.into_bytes()
}

fn import(content: &str) -> Drawing {
    read(&page(content), &PdfImportOptions::default()).unwrap().0
}

fn kinds(d: &Drawing) -> Vec<&Entity> {
    d.model.iter().map(|e| &**e).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_stroked_line_is_a_line_with_its_colour_weight_and_layer() {
    let d = import("1 0 0 RG 2 w 10 20 m 110 20 l S");
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    let e = all[0];
    let EntityKind::Line(l) = &e.kind else { panic!("{:?}", e.kind) };
    assert!(near(l.a.x, 10.0 * MM_PER_PT) && near(l.a.y, 20.0 * MM_PER_PT), "{l:?}");
    assert!(near(l.b.x, 110.0 * MM_PER_PT) && near(l.b.y, 20.0 * MM_PER_PT), "{l:?}");
    assert_eq!(e.common.color, Color::True(Rgb(255, 0, 0)));
    assert_eq!(e.common.layer, "PDF_Geometry");
    // 2 pt = 0.7 mm.
    assert_eq!(e.common.lineweight, Lineweight::Mm100(70));
}

#[test]
fn black_is_the_automatic_colour() {
    let d = import("0 G 0 0 m 10 10 l S");
    assert_eq!(kinds(&d)[0].common.color, Color::Index(7));
}

#[test]
fn a_rectangle_is_a_closed_polyline() {
    let d = import("10 10 50 30 re S");
    let EntityKind::LwPolyline(p) = &kinds(&d)[0].kind else { panic!() };
    assert!(p.closed);
    assert_eq!(p.vertices.len(), 4);
    assert!(p.vertices.iter().all(|v| v.bulge == 0.0));
}

#[test]
fn a_circle_drawn_as_beziers_keeps_its_arcs() {
    let k = 0.552_284_75 * 50.0;
    let c = format!(
        "100 100 m 100 {a} {b} 150 150 150 c {c} 150 200 {a} 200 100 c 200 {d} {c} 50 150 50 c {b} 50 100 {d} 100 100 c h S",
        a = 100.0 + k,
        b = 150.0 - k,
        c = 150.0 + k,
        d = 100.0 - k + 0.0
    );
    // Four quarter arcs: out of the first point, counter-clockwise.
    let content = format!(
        "150 100 m 150 {k1} {k2} 150 100 150 c {k3} 150 50 {k1} 50 100 c 50 {k4} {k3} 50 100 50 c {k2} 50 150 {k4} 150 100 c h S",
        k1 = 100.0 + k,
        k2 = 100.0 + k,
        k3 = 100.0 - k,
        k4 = 100.0 - k
    );
    let _ = c;
    let d = import(&content);
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    let EntityKind::LwPolyline(p) = &all[0].kind else { panic!("{:?}", all[0].kind) };
    assert!(p.closed);
    assert_eq!(p.vertices.len(), 4, "{:?}", p.vertices);
    for v in &p.vertices {
        assert!((v.bulge - (std::f64::consts::PI / 8.0).tan()).abs() < 1e-3, "counter-clockwise quarter arcs: {v:?}");
    }
}

#[test]
fn a_filled_shape_is_a_solid_hatch_and_a_white_one_is_left_out() {
    let d = import("0 0 1 rg 0 0 100 50 re f 1 1 1 rg 200 200 40 40 re f");
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    let EntityKind::Hatch(h) = &all[0].kind else { panic!("{:?}", all[0].kind) };
    assert!(h.solid && h.loops.len() == 1 && h.loops[0].vertices.len() == 4);
    assert_eq!(all[0].common.color, Color::True(Rgb(0, 0, 255)));
    assert_eq!(all[0].common.layer, "PDF_Solid Fills");
    assert!(d.read_notes.iter().any(|n| n.contains("1 white fill")), "{:?}", d.read_notes);
}

#[test]
fn a_hole_stays_a_second_loop() {
    let d = import("0 0 1 rg 0 0 100 100 re 25 25 50 50 re f*");
    let EntityKind::Hatch(h) = &kinds(&d)[0].kind else { panic!() };
    assert_eq!(h.loops.len(), 2);
}

#[test]
fn text_is_one_text_object_per_baseline() {
    let d = import("BT /F1 12 Tf 100 200 Td (Hello World) Tj ET");
    let all = kinds(&d);
    assert_eq!(all.len(), 1, "{all:?}");
    let EntityKind::Text(t) = &all[0].kind else { panic!("{:?}", all[0].kind) };
    assert_eq!(t.value, "Hello World");
    assert!(near(t.insert.x, 100.0 * MM_PER_PT) && near(t.insert.y, 200.0 * MM_PER_PT), "{t:?}");
    assert!(near(t.height, 12.0 * MM_PER_PT * 0.72), "{t:?}");
    assert!(t.rotation.abs() < 1e-9);
    assert_eq!(t.halign, HAlign::Fit);
    let end = t.align_pt.unwrap();
    assert!(end.x > t.insert.x + 10.0 * MM_PER_PT && near(end.y, t.insert.y), "{t:?}");
    assert_eq!(all[0].common.layer, "PDF_Text");
}

#[test]
fn rotated_text_keeps_its_angle() {
    // A 90 degree text matrix.
    let d = import("BT /F1 10 Tf 0 1 -1 0 300 300 Tm (Up) Tj ET");
    let EntityKind::Text(t) = &kinds(&d)[0].kind else { panic!() };
    assert_eq!(t.value, "Up");
    assert!((t.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{t:?}");
}

#[test]
fn words_far_apart_on_a_line_are_separate_texts() {
    let d = import("BT /F1 10 Tf 50 100 Td (Left) Tj 400 0 Td (Right) Tj ET");
    let texts: Vec<_> = kinds(&d).into_iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert_eq!(texts, ["Left", "Right"]);
}

#[test]
fn options_leave_out_text_fills_and_weights() {
    let bytes = page("BT /F1 12 Tf 10 10 Td (Hi) Tj ET 0 0 1 rg 0 0 5 5 re f 2 w 0 0 m 9 9 l S");
    let (d, pages) = read(&bytes, &PdfImportOptions { text: false, fills: false, lineweights: false, ..Default::default() }).unwrap();
    assert_eq!(pages, 1);
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].common.lineweight, Lineweight::ByLayer);
}

#[test]
fn a_rectangular_clip_cuts_a_line_and_drops_what_lies_outside() {
    let d = import("q 100 100 100 100 re W n 0 150 m 300 150 l S 0 0 m 10 10 l S Q");
    let all = kinds(&d);
    assert_eq!(all.len(), 1, "{all:?}");
    let EntityKind::Line(l) = &all[0].kind else { panic!("{:?}", all[0].kind) };
    assert!(near(l.a.x, 100.0 * MM_PER_PT) && near(l.b.x, 200.0 * MM_PER_PT), "{l:?}");
    assert!(near(l.a.y, 150.0 * MM_PER_PT) && near(l.b.y, 150.0 * MM_PER_PT), "{l:?}");
}

#[test]
fn a_rectangular_clip_cuts_a_fill_to_its_corners() {
    let d = import("q 100 100 100 100 re W n 0 0 1 rg 50 50 200 200 re f Q 0 1 0 rg 0 0 10 10 re f");
    let hatches: Vec<_> = kinds(&d).into_iter().filter_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h) } else { None }).collect();
    assert_eq!(hatches.len(), 2);
    let xs: Vec<f64> = hatches[0].loops[0].vertices.iter().map(|v| v.p.x).collect();
    let ys: Vec<f64> = hatches[0].loops[0].vertices.iter().map(|v| v.p.y).collect();
    assert!(xs.iter().all(|x| *x >= 100.0 * MM_PER_PT - 1e-6 && *x <= 200.0 * MM_PER_PT + 1e-6), "{xs:?}");
    assert!(ys.iter().all(|y| *y >= 100.0 * MM_PER_PT - 1e-6 && *y <= 200.0 * MM_PER_PT + 1e-6), "{ys:?}");
    // After the clip is restored, the next fill is whole again.
    assert_eq!(hatches[1].loops[0].vertices.len(), 4);
}

#[test]
fn a_shape_outside_its_clip_is_left_out() {
    let d = import("q 100 100 50 50 re W n 0 0 m 10 10 l S Q");
    assert_eq!(kinds(&d).len(), 0);
}

#[test]
fn another_clip_shape_is_not_applied_and_the_read_note_says_so() {
    let d = import("q 100 100 m 200 100 l 150 200 l h W n 0 0 m 300 300 l S Q");
    assert_eq!(kinds(&d).len(), 1);
    assert!(d.read_notes.iter().any(|n| n.contains("clips by a shape other than a rectangle")), "{:?}", d.read_notes);
}

#[test]
fn the_unit_sets_the_scale() {
    let (d, _) = read(&page("0 0 m 72 0 l S"), &PdfImportOptions { unit: 1.0 / 72.0, ..Default::default() }).unwrap();
    let EntityKind::Line(l) = &kinds(&d)[0].kind else { panic!() };
    assert!(near(l.b.x, 1.0), "{l:?}");
}

#[test]
fn a_page_is_chosen_by_number() {
    let bytes = pages(&["0 0 m 10 0 l S", "0 0 m 20 0 l S", "0 0 m 30 0 l S"]);
    let (d, n) = read(&bytes, &PdfImportOptions { page: 2, ..Default::default() }).unwrap();
    assert_eq!(n, 3);
    let EntityKind::Line(l) = &kinds(&d)[0].kind else { panic!() };
    assert!(near(l.b.x, 20.0 * MM_PER_PT));
    let e = read(&bytes, &PdfImportOptions { page: 4, ..Default::default() }).unwrap_err().to_string();
    assert!(e.contains("page 4") && e.contains("3 pages"), "{e}");
    assert!(read(&bytes, &PdfImportOptions { page: 0, ..Default::default() }).is_err());
}

#[test]
fn a_file_that_is_not_a_pdf_is_an_error_not_a_panic() {
    assert!(read(b"%PDF-1.7 garbage", &PdfImportOptions::default()).is_err());
    assert!(read(b"", &PdfImportOptions::default()).is_err());
    assert!(read(&page("0 0 m 1 1 l S"), &PdfImportOptions { unit: 0.0, ..Default::default() }).is_err());
}

#[test]
fn only_a_pdf_made_from_a_dwg_is_imported() {
    let content = ["0 0 m 10 10 l S"];
    let err = |producer| read(&pages_by(&content, producer), &PdfImportOptions::default()).unwrap_err().to_string();
    // A browser's or a word processor's PDF names itself in the refusal.
    let e = err(Some("(Skia/PDF m120)"));
    assert!(e.contains("made from a DWG") && e.contains("Skia/PDF m120"), "{e}");
    // One that doesn't say what made it is refused too.
    assert!(err(None).contains("doesn't say what made it"));
    // The names of CAD programs pass, whatever the case; so does a UTF-16 string ("DWG").
    for ok in ["(AutoCAD PDF \\(General Documentation\\))", "(CADCRAFT)", "(BricsCAD)", "<FEFF004400570047>"] {
        assert!(read(&pages_by(&content, Some(ok)), &PdfImportOptions::default()).is_ok(), "{ok}");
    }
}

#[test]
fn nonsense_content_does_not_panic() {
    for c in ["q q q", "1 2 3 re re re f", "BT (x) Tj ET", "0 0 m 1e300 1e300 l 1e300 -1e300 l S", "Q Q Q h h S"] {
        let _ = read(&page(c), &PdfImportOptions::default());
    }
}

#[test]
fn a_truncated_or_damaged_file_is_never_a_panic() {
    let whole = page("1 0 0 RG 2 w 10 20 m 110 20 l S 0 0 1 rg 0 0 50 50 re f BT /F1 12 Tf 100 200 Td (Hello) Tj ET");
    for len in (0..whole.len()).step_by(3) {
        let _ = read(&whole[..len], &PdfImportOptions::default());
    }
    for at in (0..whole.len()).step_by(2) {
        for byte in [0x00, 0xFF, b'<', b'0'] {
            let mut damaged = whole.clone();
            damaged[at] = byte;
            let _ = read(&damaged, &PdfImportOptions::default());
        }
    }
}

#[test]
fn a_page_with_a_great_many_objects_is_read_or_refused_not_hung() {
    let content = "0 0 m 1 1 l S ".repeat(60_000);
    let (d, _) = read(&page(&content), &PdfImportOptions::default()).unwrap();
    assert_eq!(d.model.iter().count(), 60_000);
}

#[test]
fn the_general_reader_does_not_take_a_pdf() {
    // INSERT, XREF, COMPARE, RECOVER and templates read drawings through `read`; a PDF is taken
    // only by the engine's OPEN, IMPORT and PDFIMPORT, through `pdf_read`.
    let e = cadcraft_io::read(&page("0 0 m 10 10 l S"), "plan.pdf").unwrap_err().to_string();
    assert!(e.contains("unsupported file type"), "{e}");
}

/// A form that draws itself twice asks for about 2^50 nested interpretations unless the interpreter
/// limits how often forms nest (craft-libs' patched hayro-interpret does).
#[test]
fn a_form_that_draws_itself_twice_is_stopped() {
    let form = "/X Do /X Do 0 0 m 1 1 l S";
    let pdf = format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [10 0 R] /Count 1 >> endobj\n\
         10 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 11 0 R /Resources << /XObject << /X 5 0 R >> >> >> endobj\n\
         11 0 obj << /Length 5 >> stream\n/X Do\nendstream endobj\n\
         5 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /XObject << /X 5 0 R >> >> /Length {} >> stream\n{form}\nendstream endobj\n\
         4 0 obj << /Producer (AutoCAD) >> endobj\n\
         trailer << /Root 1 0 R /Info 4 0 R >>\n%%EOF\n",
        form.len()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read(pdf.as_bytes(), &PdfImportOptions::default()).is_ok() || true);
    });
    assert!(rx.recv_timeout(std::time::Duration::from_secs(20)).is_ok(), "the self-calling form was not stopped");
}

#[test]
fn a_huge_curve_is_bounded_and_an_out_of_range_one_is_left_out() {
    // A million points across: its flattening is capped, not millions of segments.
    let d = import("1000000 0 0 1000000 0 0 cm 0 0 m 0 1 1 1 1 0 c S");
    let verts: usize = kinds(&d).iter().map(|e| if let EntityKind::LwPolyline(p) = &e.kind { p.vertices.len() } else { 2 }).sum();
    assert!(verts < 5_000, "{verts} vertices");
    // 1e20 points across: out of any drawing's range, named in a read note, and nothing runs away.
    let d = import("100000000000000000000 0 0 100000000000000000000 0 0 cm 0 0 m 0 1 1 1 1 0 c S");
    assert_eq!(kinds(&d).len(), 0);
    assert!(d.read_notes.iter().any(|n| n.contains("out-of-range")), "{:?}", d.read_notes);
}

#[test]
fn text_drawn_as_fill_and_stroke_is_one_text() {
    // Render mode 2 draws each glyph twice at one origin: fill, then stroke.
    let d = import("BT /F1 12 Tf 100 200 Td 2 Tr (Hello) Tj ET");
    let texts: Vec<String> =
        kinds(&d).into_iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect();
    assert_eq!(texts, ["Hello"]);
}

#[test]
fn a_plot_of_a_drawing_imports_back_as_its_geometry() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) })).unwrap();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Text(Text {
            insert: Vec3::new(10.0, 20.0, 0.0),
            align_pt: None,
            height: 5.0,
            value: "Plan".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Left,
            valign: Default::default(),
        }),
    )
    .unwrap();
    let pdf = cadcraft_io::plot(&d, &Space::Model, &json!({ "compress": false, "paper": "A4", "fit": true })).unwrap();
    let (back, _) = read(&pdf, &PdfImportOptions::default()).unwrap();
    let lines = back.model.iter().filter(|e| matches!(e.kind, EntityKind::Line(_) | EntityKind::LwPolyline(_))).count();
    assert!(lines >= 1, "{:?}", back.model.iter().map(|e| &e.kind).collect::<Vec<_>>());
}
