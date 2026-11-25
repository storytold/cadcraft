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
    assert!(d.read_notes.iter().any(|n| n.contains("1 white or near-white fill")), "{:?}", d.read_notes);
}

#[test]
fn a_near_white_fill_is_left_out_like_a_white_one() {
    // 245, 245, 245 table shading is left out; a light grey (230) is a fill.
    let d = import("0.96 0.96 0.96 rg 0 0 50 50 re f 0.9 0.9 0.9 rg 100 100 50 50 re f");
    let all = kinds(&d);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].common.color, Color::True(Rgb(230, 230, 230)));
    assert!(d.read_notes.iter().any(|n| n.contains("1 white or near-white fill")), "{:?}", d.read_notes);
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
fn text_that_climbs_a_step_each_glyph_leaves_its_first_baseline() {
    // Each glyph is 0.08 em above the one before: close enough to join it, but after a few the
    // glyphs are well off the baseline the text is placed on.
    let d = import("BT /F1 10 Tf 100 100 Td (A) Tj 0.8 Ts (B) Tj 1.6 Ts (C) Tj 2.4 Ts (D) Tj 3.2 Ts (E) Tj 4 Ts (F) Tj ET");
    assert_eq!(texts(&d).concat(), "ABCDEF");
    let rises: Vec<f64> = kinds(&d).into_iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.insert.y) } else { None }).collect();
    assert!(rises.len() > 1 && rises.windows(2).all(|w| w[1] > w[0]), "{:?}: {rises:?}", texts(&d));
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

fn texts(d: &Drawing) -> Vec<String> {
    kinds(d).into_iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None }).collect()
}

#[test]
fn text_starting_on_a_clip_edge_is_kept() {
    // A cell clipped to a box whose left side runs a hair right of the text's start, as a plotter
    // draws it: a viewer shows the whole word.
    let d = import("q 100.001 200 200 14 re W n BT /F1 10 Tf 100 202 Td (MATERIAL) Tj ET Q");
    assert_eq!(texts(&d), ["MATERIAL"]);
    // A label turned to read downwards with its start just off a page clip.
    let d = import("q 0.12 0.12 599 799 re W n BT /F1 10 Tf 0 -1 1 0 -0.5 300 Tm (B) Tj ET Q");
    assert_eq!(texts(&d), ["B"]);
    // Text wholly outside the clip is still left out, and a glyph is cut at the clip.
    let d = import("q 100 200 200 14 re W n BT /F1 10 Tf 400 202 Td (Gone) Tj ET BT /F1 10 Tf 88 202 Td (XYZ) Tj ET Q");
    assert_eq!(texts(&d), ["Z"]);
}

#[test]
fn text_inside_a_clip_with_no_area_is_left_out() {
    // A 1000 point 0 is 556 points wide, so its middle lies on x = 300, where the two clips touch.
    let content = |clips: &str| format!("q {clips} BT /F1 1000 Tf 22 100 Td (0) Tj ET Q");
    let points = PdfImportOptions { unit: 1.0, ..Default::default() };
    let (d, _) = read(&page(&content("0 0 300 800 re W n")), &points).unwrap();
    assert_eq!(texts(&d), ["0"], "a glyph whose middle lies on the clip's edge is kept");
    let (d, _) = read(&page(&content("0 0 300 800 re W n 300 0 300 800 re W n")), &points).unwrap();
    assert!(texts(&d).is_empty(), "{:?}", texts(&d));
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
fn any_pdf_is_imported_whatever_made_it() {
    // A plot that another program merged or printed again no longer names the CAD program.
    let content = ["0 0 m 10 10 l S"];
    for producer in [Some("(Skia/PDF m120)"), Some("(Example PDF Tool)"), Some("<FEFF004400570047>"), None] {
        let (d, _) = read(&pages_by(&content, producer), &PdfImportOptions::default()).unwrap();
        assert_eq!(kinds(&d).len(), 1, "{producer:?}");
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

/// A one-page PDF whose page draws the form `/X` once; the form's content is `form`, and it may
/// draw `/X` itself.
fn form_page(form: &str) -> String {
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [10 0 R] /Count 1 >> endobj\n\
         10 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 11 0 R /Resources << /XObject << /X 5 0 R >> >> >> endobj\n\
         11 0 obj << /Length 5 >> stream\n/X Do\nendstream endobj\n\
         5 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /XObject << /X 5 0 R >> >> /Length {} >> stream\n{form}\nendstream endobj\n\
         4 0 obj << /Producer (AutoCAD) >> endobj\n\
         trailer << /Root 1 0 R /Info 4 0 R >>\n%%EOF\n",
        form.len()
    )
}

fn lines_of(d: &Drawing) -> usize {
    kinds(d).iter().filter(|e| matches!(e.kind, EntityKind::Line(_))).count()
}

/// A form that draws itself twice asks for about 2^50 nested interpretations unless the interpreter
/// limits how often forms nest (craft-libs' patched hayro-interpret does).
#[test]
fn a_form_that_draws_itself_twice_is_stopped() {
    // The same page with a form that doesn't call itself draws its line once.
    let (d, _) = read(form_page("0 0 m 1 1 l S").as_bytes(), &PdfImportOptions::default()).unwrap();
    assert_eq!(lines_of(&d), 1);
    let pdf = form_page("/X Do /X Do 0 0 m 1 1 l S");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read(pdf.as_bytes(), &PdfImportOptions::default()).map(|(d, _)| lines_of(&d)));
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the self-calling form was not stopped");
    // The page still reads, with the form's own line drawn and the nesting cut off.
    let lines = result.unwrap_or_else(|e| panic!("{e}"));
    // The top-level form plus 50_000 nested paints (`MAX_NESTED_PAINTS` in craft-libs' hayro-interpret).
    assert!((1..=50_001).contains(&lines), "{lines} lines");
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
fn fill_only_text_before_fill_and_stroke_text_does_not_double_it() {
    // Note is only filled; Label's strokes must find Label's fills behind Note's.
    let d = import("BT /F1 12 Tf 50 200 Td (Note) Tj ET BT /F1 12 Tf 50 100 Td 2 Tr (Label) Tj ET");
    assert_eq!(texts(&d), ["Note", "Label"]);
}

#[test]
fn text_drawn_as_fill_and_stroke_is_one_text_when_it_splits_into_runs() {
    // The whole TJ is filled before it is stroked: fill Left, fill Right, stroke Left, stroke Right.
    let d = import("BT /F1 12 Tf 50 100 Td 2 Tr [(Left) -30000 (Right)] TJ ET");
    assert_eq!(texts(&d), ["Left", "Right"]);
    // Text that is only stroked is kept.
    let d = import("BT /F1 12 Tf 50 100 Td 1 Tr [(Left) -30000 (Right)] TJ ET");
    assert_eq!(texts(&d), ["Left", "Right"]);
}

#[test]
fn the_same_text_filled_again_in_another_colour_is_kept() {
    // A black label, then the same label in red over it, with nothing drawn between.
    let d = import("BT /F1 12 Tf 20 20 Td (Label) Tj ET 1 0 0 rg BT /F1 12 Tf 20 20 Td (Label) Tj ET");
    assert_eq!(texts(&d), ["Label", "Label"]);
    assert_eq!(kinds(&d).last().map(|e| e.common.color), Some(Color::True(Rgb(255, 0, 0))));
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

/// A one-page PDF drawing `content` with Helvetica as `/F1`, whose ToUnicode CMap maps the code
/// `<41>` to `to` (a hex string of UTF-16 code units).
fn page_with_to_unicode(content: &str, to: &str) -> Vec<u8> {
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CMapName /Test def /CMapType 2 def\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n\
         1 beginbfchar <41> <{to}> endbfchar\n\
         endcmap CMapName currentdict /CMap defineresource pop end end"
    );
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [10 0 R] /Count 1 >> endobj\n\
         3 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 5 0 R >> endobj\n\
         5 0 obj << /Length {} >> stream\n{cmap}\nendstream endobj\n\
         10 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 11 0 R /Resources << /Font << /F1 3 0 R >> >> >> endobj\n\
         11 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n\
         trailer << /Root 1 0 R >>\n%%EOF\n",
        cmap.len(),
        content.len()
    )
    .into_bytes()
}

#[test]
fn one_glyph_gives_at_most_32_characters_and_a_page_a_bounded_text() {
    // `<41>` (an A) stands for 16 000 Bs.
    let long = "0042".repeat(16_000);
    let d = read(&page_with_to_unicode("BT /F1 12 Tf 100 200 Td (A) Tj ET", &long), &PdfImportOptions::default()).unwrap().0;
    assert_eq!(texts(&d), ["B".repeat(32)]);
    // 125 000 of them, each kerned back onto the one before: more text than a page may hold, so
    // the import stops with a reason instead of growing the text.
    let content = format!("BT /F1 12 Tf 100 200 Td [{}] TJ ET", "(A) 667 ".repeat(125_000));
    let e = read(&page_with_to_unicode(&content, &long), &PdfImportOptions::default()).unwrap_err().to_string();
    assert!(e.contains("characters of text on the page"), "{e}");
}

#[test]
fn text_with_a_scale_too_large_for_numbers_is_skipped_not_added() {
    // Ten times 1e30 makes 1e300; a larger font size pushes the em square past what a number holds,
    // and a horizontal scaling of 1000 % its width alone (the baseline's direction).
    let big = "1000000000000000000000000000000 0 0 1000000000000000000000000000000 0 0 cm ".repeat(10);
    for (size, scaling) in [1, 100, 10_000, 1_000_000, 100_000_000].into_iter().flat_map(|s| [(s, 100), (s, 1000)]) {
        let r = read(&page(&format!("{big} BT /F1 {size} Tf {scaling} Tz (AB) Tj ET")), &PdfImportOptions::default());
        let (d, _) = r.unwrap_or_else(|e| panic!("size {size}, scaling {scaling}: {e}"));
        for e in kinds(&d) {
            if let EntityKind::Text(t) = &e.kind {
                let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite();
                assert!(finite(t.insert) && t.align_pt.is_none_or(finite) && t.rotation.is_finite() && t.height.is_finite(), "{t:?}");
            }
        }
    }
}

#[test]
fn a_curve_after_a_close_starts_where_the_subpath_did() {
    let d = import("0 0 m 100 0 l 100 100 l h 0 50 50 100 100 100 c S");
    let all = kinds(&d);
    assert_eq!(all.len(), 2, "{all:?}");
    let EntityKind::LwPolyline(p) = &all[1].kind else { panic!("{:?}", all[1].kind) };
    let (first, last) = (p.vertices.first().unwrap().p, p.vertices.last().unwrap().p);
    assert!(near(first.x, 0.0) && near(first.y, 0.0), "{first:?}");
    assert!(near(last.x, 100.0 * MM_PER_PT) && near(last.y, 100.0 * MM_PER_PT), "{last:?}");
}

#[test]
fn crop_origin_user_units_and_page_rotation_are_applied() {
    let pdf = String::from_utf8(page("100 200 m 150 200 l S")).unwrap();
    let pdf = pdf.replace("/MediaBox [0 0 600 800]", "/MediaBox [0 0 600 800] /CropBox [100 200 300 400] /UserUnit 2 /Rotate 90");
    let (d, _) = read(pdf.as_bytes(), &PdfImportOptions::default()).unwrap();
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    let EntityKind::Line(line) = &all[0].kind else { panic!("expected a line") };
    assert!(near(line.a.x, line.b.x), "rotation makes the line vertical: {line:?}");
    assert!(near((line.b.y - line.a.y).abs(), 100.0 * MM_PER_PT), "UserUnit doubles the length: {line:?}");
    assert!(line.a.x >= 0.0 && line.a.y >= 0.0 && line.b.x >= 0.0 && line.b.y >= 0.0);
}

#[test]
fn the_page_boundary_and_nested_clips_both_limit_the_import() {
    let d = import("q -100 -100 800 1000 re W n q 10 10 40 40 re W n -50 30 m 100 30 l S Q -100 60 m 700 60 l S Q");
    let all = kinds(&d);
    assert_eq!(all.len(), 2);
    let lines: Vec<_> = all.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l) } else { None }).collect();
    assert!(lines[0].a.x > lines[1].a.x && lines[0].b.x < lines[1].b.x, "inner clip narrows the first line");
    assert!(near(lines[1].a.x, 0.0) && near(lines[1].b.x, 600.0 * MM_PER_PT), "page clips the restored outer line");
}

#[test]
fn text_and_shapes_keep_their_paint_order() {
    let d = import("BT /F1 12 Tf 20 20 Td (Below) Tj ET 1 0 0 rg 10 10 100 40 re f BT /F1 12 Tf 20 80 Td (Above) Tj ET");
    let all = kinds(&d);
    assert!(matches!(all[0].kind, EntityKind::Text(_)));
    assert!(matches!(all[1].kind, EntityKind::Hatch(_)));
    assert!(matches!(all[2].kind, EntityKind::Text(_)));
    assert!(d.read_notes.iter().any(|n| n.contains("Standard font")));
}

#[test]
fn the_same_text_drawn_again_over_a_shape_is_kept() {
    // A label, a fill over it, then the label again on top: the second one is what shows.
    let d = import("BT /F1 12 Tf 20 20 Td (Label) Tj ET 1 0 0 rg 10 10 100 40 re f BT /F1 12 Tf 20 20 Td (Label) Tj ET");
    assert_eq!(texts(&d), ["Label", "Label"]);
    assert!(matches!(kinds(&d).last().map(|e| &e.kind), Some(EntityKind::Text(_))));
}

#[test]
fn diagonal_edges_are_not_mistaken_for_a_rectangular_clip() {
    let d = import("q 0 0 m 100 100 l 100 0 l 0 100 l h W n 0 50 m 150 50 l S Q");
    assert!(d.read_notes.iter().any(|n| n.contains("clips by a shape other than a rectangle")));
}

#[test]
fn a_path_that_doubles_back_along_an_edge_is_not_a_rectangular_clip() {
    // Every point lies on the box's edges, but the corner at (0, 100) is never reached.
    let d = import("q 0 0 m 100 0 l 100 100 l 100 0 l h W n 0 50 m 150 50 l S Q");
    assert!(d.read_notes.iter().any(|n| n.contains("clips by a shape other than a rectangle")), "{:?}", d.read_notes);
    let EntityKind::Line(line) = &kinds(&d)[0].kind else { panic!("expected a line") };
    assert!(line.b.x > 100.0 * MM_PER_PT, "the line was cut at the box: {line:?}");
}

#[test]
fn a_clip_of_separate_segments_is_empty_not_a_rectangle() {
    // Two open segments reach the four corners of a box, but neither encloses any area.
    let d = import("q 0 0 m 100 0 l 100 100 m 0 100 l W n 0 50 m 150 50 l S Q");
    assert!(kinds(&d).is_empty(), "{:?}", kinds(&d));
    assert!(!d.read_notes.iter().any(|n| n.contains("clips by a shape other than a rectangle")), "{:?}", d.read_notes);
}

#[test]
fn raster_only_pages_are_identified_without_fabricated_geometry() {
    let d = import("q 100 0 0 100 0 0 cm BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID FF0000> EI Q");
    assert!(d.model.is_empty());
    assert!(d.read_notes.iter().any(|n| n.contains("raster image")), "{:?}", d.read_notes);
}

#[test]
fn form_transforms_and_bounding_boxes_are_respected() {
    let form = "0 0 m 100 0 l S";
    let mut pdf = String::from_utf8(page("/Fm Do"))
        .unwrap()
        .replace("/Resources << /Font << /F1 3 0 R >> >>", "/Resources << /Font << /F1 3 0 R >> /XObject << /Fm 6 0 R >> >>");
    let object = format!(
        "6 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Matrix [2 0 0 2 10 30] /Length {} >> stream\n{form}\nendstream endobj\n",
        form.len()
    );
    pdf = pdf.replace("trailer <<", &format!("{object}trailer <<"));
    let (d, _) = read(pdf.as_bytes(), &PdfImportOptions::default()).unwrap();
    let all = kinds(&d);
    assert_eq!(all.len(), 1);
    let EntityKind::Line(line) = &all[0].kind else { panic!("expected a line") };
    assert!(near(line.a.x, 10.0 * MM_PER_PT) && near(line.b.x, 50.0 * MM_PER_PT), "{line:?}");
    assert!(near(line.a.y, 30.0 * MM_PER_PT) && near(line.b.y, line.a.y), "{line:?}");
}

#[test]
fn changed_dash_appearance_is_reported() {
    let d = import("[5 3] 0 d 10 10 m 100 10 l S");
    assert!(!d.model.is_empty());
    assert!(d.read_notes.iter().any(|n| n.contains("dashed paths")), "{:?}", d.read_notes);
}
