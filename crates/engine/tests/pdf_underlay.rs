//! PDFATTACH: a page of a PDF file under the drawing, at its size on paper in drawing units.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::underlay::{AttachPreset, PdfHooks, set_pdf, start_attach, stored_path};
use cadcraft_engine::doc::{EntityKind, Space, entity_bounds};
use serde_json::json;

/// A fake reader: every file has 3 A4 landscape pages; a file whose bytes name a `/Measure` has a
/// measuring scale in millimetres (0.35282 per point, a common one). Hiding a PDF layer
/// appends a `%OFF:<name>` note to the bytes.
fn hooks() {
    set_pdf(PdfHooks {
        page: |bytes, page| {
            if !bytes.starts_with(b"%PDF") {
                return Err("not a readable PDF file".into());
            }
            let measure = bytes.windows(8).any(|w| w == b"/Measure").then_some(0.35282);
            if page > 3 { Err(format!("the PDF has no page {page} (it has 3)")) } else { Ok((3, 297.0, 210.0, measure)) }
        },
        render: |_, _, side| Ok((side, side, vec![255; (side * side * 4) as usize])),
        region: |_, _, _, w, h| Ok((w, h, vec![255; (w * h * 4) as usize])),
        layers: |_| Ok(vec![("MURS".into(), true), ("COTES".into(), true)]),
        with_layers: |bytes, v| {
            let mut b = bytes.to_vec();
            for (n, on) in v {
                if !on {
                    b.extend_from_slice(format!("%OFF:{n}").as_bytes());
                }
            }
            Ok(b)
        },
    });
}

fn pdf_file(name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("cadcraft-pdfattach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(name);
    std::fs::write(&f, bytes).unwrap();
    f.to_string_lossy().to_string()
}

#[test]
fn a_page_is_attached_at_its_size_on_paper() {
    hooks();
    let path = pdf_file("Plan.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let r = s.execute("pdfattach", &json!({"path": path, "page": 2, "at": [100, 50]})).unwrap();
    assert_eq!(r["pages"], 3);
    assert!((r["width"].as_f64().unwrap() - 297.0).abs() < 1e-9, "{r}");
    assert!((r["height"].as_f64().unwrap() - 210.0).abs() < 1e-9, "{r}");
    let d = s.doc().unwrap();
    let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
    let EntityKind::PdfUnderlay(u) = &e.kind else { unreachable!() };
    assert_eq!((u.page, u.path.as_str()), (2, path.as_str()));
    // Stored as files store the underlay scale: the page in inches at scale 1, here 25.4 for paper size.
    assert!((u.size.x - 297.0 / 25.4).abs() < 1e-12 && (u.scale - 25.4).abs() < 1e-12, "{u:?}");
    let b = entity_bounds(d, e, 0);
    assert!((b.min.x - 100.0).abs() < 1e-9 && (b.max.x - 397.0).abs() < 1e-9 && (b.max.y - 260.0).abs() < 1e-9, "{b:?}");
    // It is undone like any other object.
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().space(&Space::Model).into_iter().flat_map(|st| st.iter()).any(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))));
}

#[test]
fn a_rotation_that_is_not_a_number_is_refused() {
    hooks();
    let path = pdf_file("Rotation.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    for bad in [json!("90"), json!(true), json!([90]), json!({"deg": 90})] {
        let r = s.execute("pdfattach", &json!({"path": path, "rotation": bad}));
        assert!(r.as_ref().is_err_and(|e| e.to_string().contains("rotation")), "{bad}: {r:?}");
    }
    assert!(!s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))), "nothing attached");
    // A number, or none (0), is taken.
    assert!(s.execute("pdfattach", &json!({"path": path, "rotation": 90})).is_ok());
    assert!(s.execute("pdfattach", &json!({"path": path, "rotation": null})).is_ok());
}

/// Every INSUNITS value sizes the page at its size on paper in that unit (DXF reference values;
/// the survey units are the US survey foot's, 1200/3937 m).
#[test]
fn every_drawing_unit_gets_the_page_at_its_size_on_paper() {
    hooks();
    let path = pdf_file("Units.pdf", b"%PDF-1.7 fake");
    let survey_ft = 1_200_000.0 / 3937.0;
    let units: [(i64, f64); 24] = [
        (1, 25.4),
        (2, 304.8),
        (3, 1_609_344.0),
        (4, 1.0),
        (5, 10.0),
        (6, 1000.0),
        (7, 1e6),
        (8, 25.4e-6),
        (9, 0.0254),
        (10, 914.4),
        (11, 1e-7),
        (12, 1e-6),
        (13, 1e-3),
        (14, 100.0),
        (15, 1e4),
        (16, 1e5),
        (17, 1e12),
        (18, 1.495_978_707e14),
        (19, 9.460_730_472_580_8e18),
        (20, 3.085_677_581_491_367e19),
        (21, survey_ft),
        (22, survey_ft / 12.0),
        (23, survey_ft * 3.0),
        (24, survey_ft * 5280.0),
    ];
    for (insunits, mm) in units {
        let mut s = Session::empty();
        s.new_drawing(true);
        s.doc_mut().unwrap().header.set_i64("INSUNITS", insunits);
        let r = s.execute("pdfattach", &json!({"path": path})).unwrap();
        let w = r["width"].as_f64().unwrap();
        assert!((w * mm / 297.0 - 1.0).abs() < 1e-9, "INSUNITS {insunits}: {w} units wide for 297 mm");
    }
}

#[test]
fn an_inch_drawing_gets_the_page_in_inches_and_scale_and_rotation_apply() {
    hooks();
    let path = pdf_file("Inch.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(false);
    let r = s.execute("pdfattach", &json!({"path": path, "scale": 2, "rotation": 90})).unwrap();
    assert!((r["width"].as_f64().unwrap() - 2.0 * 297.0 / 25.4).abs() < 1e-9);
    let d = s.doc().unwrap();
    let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
    let b = entity_bounds(d, e, 0);
    // Turned a quarter turn about the lower-left corner: it now spans x -h..0, y 0..w.
    assert!((b.min.x + 2.0 * 210.0 / 25.4).abs() < 1e-9 && b.max.x.abs() < 1e-9, "{b:?}");
    assert!((b.max.y - 2.0 * 297.0 / 25.4).abs() < 1e-9, "{b:?}");
}

#[test]
fn a_moved_underlay_keeps_its_page() {
    hooks();
    let path = pdf_file("Move.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("move", &json!({"handles": [h], "from": [0, 0], "to": [10, 20]})).unwrap();
    let d = s.doc().unwrap();
    let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
    let EntityKind::PdfUnderlay(u) = &e.kind else { unreachable!() };
    assert_eq!((u.insert.x, u.insert.y), (10.0, 20.0));
    assert_eq!(u.page, 1);
}

#[test]
fn bad_files_and_params_are_errors() {
    hooks();
    let good = pdf_file("Good.pdf", b"%PDF-1.7 fake");
    let junk = pdf_file("Junk.pdf", b"not a pdf");
    let mut s = Session::new();
    for p in [
        json!({}),
        json!({"path": ""}),
        json!({"path": "/no/such/file.pdf"}),
        json!({"path": junk}),
        json!({"path": good, "page": 4}),
        json!({"path": good, "page": 0}),
        json!({"path": good, "page": -1}),
        json!({"path": good, "page": 1e30}),
        json!({"path": good, "scale": 0}),
        json!({"path": good, "scale": -2}),
        json!({"path": good, "at": "here"}),
    ] {
        assert!(s.execute("pdfattach", &p).is_err(), "{p}");
    }
    assert!(!s.doc().unwrap().space(&Space::Model).into_iter().flat_map(|st| st.iter()).any(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))));
}

#[test]
fn scale_reference_calibrates_an_underlay_from_two_points_and_their_true_length() {
    hooks();
    let path = pdf_file("Calibrate.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path})).unwrap();
    // A plan printed at 1:50: two points 100 mm apart on the page are 5 m apart for real.
    for line in ["scale", "l", "", "0,0", "r", "0,0", "100,0", "5000"] {
        s.cmdline(line).unwrap();
    }
    let d = s.doc().unwrap();
    let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
    let EntityKind::PdfUnderlay(u) = &e.kind else { unreachable!() };
    assert!((u.scale - 50.0 * 25.4).abs() < 1e-9, "{}", u.scale);
    let b = entity_bounds(d, e, 0);
    assert!((b.max.x - 297.0 * 50.0).abs() < 1e-6, "{b:?}");
}

fn the_underlay(s: &Session) -> cadcraft_engine::doc::PdfUnderlay {
    let d = s.doc().unwrap();
    d.space(&Space::Model)
        .into_iter()
        .flat_map(|st| st.iter())
        .find_map(|e| match &e.kind {
            EntityKind::PdfUnderlay(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn pdfadjust_fades_and_adapts_colors_unless_the_layer_is_locked() {
    hooks();
    let path = pdf_file("Adjust.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let u = the_underlay(&s);
    assert_eq!((u.fade, u.adjust_colors), (0, true), "a new underlay is not faded and adapts to the background");

    s.execute("pdfadjust", &json!({"handles": [h], "fade": 60, "adjustColors": false})).unwrap();
    let u = the_underlay(&s);
    assert_eq!((u.fade, u.adjust_colors), (60, false));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(the_underlay(&s).fade, 0, "undone like any change");

    for bad in [
        json!({"handles": [h], "fade": 81}),
        json!({"handles": [h], "fade": -1}),
        json!({"handles": [h], "fade": 2.5}),
        json!({"handles": [h], "adjustColors": "yes"}),
    ] {
        assert!(s.execute("pdfadjust", &bad).is_err(), "{bad}");
    }
    let line = s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    assert!(s.execute("pdfadjust", &json!({"handles": [line], "fade": 10})).is_err(), "not an underlay");

    // Locked: the underlay can't be changed, like any object on a locked layer.
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    assert!(s.execute("pdfadjust", &json!({"handles": [h], "fade": 10})).is_err());
    assert_eq!(the_underlay(&s).fade, 0);
}

#[test]
fn pdfadjust_typed_at_the_command_line() {
    hooks();
    let path = pdf_file("Typed.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path})).unwrap();
    for line in ["pdfadjust", "l", "", "f", "30"] {
        s.cmdline(line).unwrap();
    }
    assert_eq!(the_underlay(&s).fade, 30);
    for line in ["pdfadjust", "l", "", "a", "n"] {
        s.cmdline(line).unwrap();
    }
    assert!(!the_underlay(&s).adjust_colors);
}

#[test]
fn pdfadjust_sets_contrast_and_monochrome() {
    hooks();
    let path = pdf_file("Contrast.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let u = the_underlay(&s);
    assert_eq!((u.contrast, u.monochrome), (100, false), "a new underlay keeps its page's contrast and colours");

    s.execute("pdfadjust", &json!({"handles": [h], "contrast": 40, "monochrome": true})).unwrap();
    let u = the_underlay(&s);
    assert_eq!((u.contrast, u.monochrome, u.fade), (40, true, 0), "the other settings are kept");
    for bad in [json!({"handles": [h], "contrast": 101}), json!({"handles": [h], "contrast": -5}), json!({"handles": [h], "monochrome": 1})] {
        assert!(s.execute("pdfadjust", &bad).is_err(), "{bad}");
    }

    for line in ["pdfadjust", "l", "", "c", "70"] {
        s.cmdline(line).unwrap();
    }
    assert_eq!(the_underlay(&s).contrast, 70);
    for line in ["pdfadjust", "l", "", "m", "n"] {
        s.cmdline(line).unwrap();
    }
    assert!(!the_underlay(&s).monochrome);
    // Out of range at the prompt: asked again, nothing changed.
    for line in ["pdfadjust", "l", "", "c", "150"] {
        s.cmdline(line).unwrap();
    }
    s.cancel();
    assert_eq!(the_underlay(&s).contrast, 70);
}

#[test]
fn contrast_and_monochrome_tone_the_page_pixels() {
    let px = vec![255, 255, 255, 0, 0, 0, 0, 255, 255, 0, 0, 255];
    let tone = |contrast: u8, monochrome: bool| {
        let mut p = px.clone();
        cadcraft_engine::doc::PdfUnderlay { contrast, monochrome, ..Default::default() }.tone(&mut p);
        p
    };
    assert_eq!(tone(100, false), px, "full contrast in colour: the page as it is");
    assert_eq!(tone(0, false), vec![128, 128, 128, 0, 128, 128, 128, 255, 128, 128, 128, 255], "no contrast: middle grey");
    assert_eq!(tone(50, false), vec![191, 191, 191, 0, 64, 64, 64, 255, 191, 64, 64, 255]);
    assert_eq!(tone(100, true), vec![255, 255, 255, 0, 0, 0, 0, 255, 76, 76, 76, 255], "red becomes its grey; transparency kept");
}

fn real_io() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

#[test]
fn a_saved_underlay_reopens_at_its_size_and_finds_its_pdf_beside_the_drawing() {
    hooks();
    real_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-save-{}", std::process::id()));
    let far = dir.join("elsewhere");
    std::fs::create_dir_all(&far).unwrap();
    let pdf = far.join("Plan.pdf");
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    let dxf = dir.join("Site.dxf");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": pdf.to_string_lossy(), "at": [5, 6], "scale": 50, "fade": 20})).unwrap();
    s.execute("saveas", &json!({"path": dxf.to_string_lossy()})).unwrap();

    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    let u = the_underlay(&s);
    assert!((u.size.x - 297.0 / 25.4).abs() < 1e-12 && (u.size.y - 210.0 / 25.4).abs() < 1e-12, "the size is read again from the PDF: {u:?}");
    assert_eq!((u.insert.x, u.scale, u.fade, u.page), (5.0, 50.0 * 25.4, 20, 1));
    assert!(!s.state().unwrap().is_dirty(), "reading the size is no change");

    // The PDF moved beside the drawing: found there by its name.
    std::fs::rename(&pdf, dir.join("Plan.pdf")).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    assert!((the_underlay(&s).size.x - 297.0 / 25.4).abs() < 1e-12);

    // Gone: the underlay is kept (and saved again) without a size, and reported.
    std::fs::remove_file(dir.join("Plan.pdf")).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    let u = the_underlay(&s);
    assert_eq!(u.size.x, 0.0);
    assert!(s.log.iter().any(|l| l.contains("PDF underlay not shown")), "{:?}", s.log);
    // Opening a drawing names no command.
    assert!(!s.log.iter().any(|l| l.contains("pdfattach")), "{:?}", s.log);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pdfattach_typed_asks_page_point_scale_and_rotation() {
    hooks();
    let path = pdf_file("Typed-attach.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.cmdline("pdfattach").unwrap();
    s.cmdline("/no/such.pdf").unwrap();
    assert!(s.running.is_some(), "a wrong file asks again");
    for line in [path.as_str(), "4", "2", "100,50", "0.5", "90"] {
        s.cmdline(line).unwrap();
    }
    assert!(s.running.is_none());
    let u = the_underlay(&s);
    assert_eq!((u.page, u.insert.x, u.insert.y, u.scale), (2, 100.0, 50.0, 0.5 * 25.4), "page 4 doesn't exist: asked again");
    assert!((u.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.doc().unwrap().model.iter().any(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))), "undone at once");

    // Enter takes the defaults: page 1 at 0,0, scale 1, no rotation.
    s.cmdline("pdfattach").unwrap();
    for line in [path.as_str(), "", "", "", ""] {
        s.cmdline(line).unwrap();
    }
    let u = the_underlay(&s);
    assert_eq!((u.page, u.insert.x, u.scale, u.rotation), (1, 0.0, 25.4, 0.0), "paper size in a millimetre drawing");
}

#[test]
fn scale_one_is_the_size_on_paper_and_files_keep_their_own_scale() {
    // A file's underlay scale 1 is the page in inches: an A4 PDF is 11.6944 × 8.2639 units in a
    // millimetre drawing and in an inch one. CADCraft's scale 1 is the page at its size on
    // paper; the underlay keeps the file's scale (25.4 in millimetres, 1 in inches).
    hooks();
    let path = pdf_file("Convention.pdf", b"%PDF-1.7 fake");
    for (metric, width, stored) in [(true, 297.0, 25.4), (false, 297.0 / 25.4, 1.0)] {
        let mut s = Session::empty();
        s.new_drawing(metric);
        let r = s.execute("pdfattach", &json!({"path": path, "scale": 1})).unwrap();
        assert!((r["width"].as_f64().unwrap() - width).abs() < 1e-9, "{r}");
        let u = the_underlay(&s);
        assert!((u.scale - stored).abs() < 1e-12 && (u.size.x - 297.0 / 25.4).abs() < 1e-12, "{u:?}");
    }
}

fn underlays(s: &Session) -> Vec<cadcraft_engine::doc::PdfUnderlay> {
    s.doc()
        .unwrap()
        .space(&Space::Model)
        .into_iter()
        .flat_map(|st| st.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::PdfUnderlay(u) => Some(u.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn several_pages_go_side_by_side() {
    hooks();
    let path = pdf_file("Pages.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let r = s.execute("pdfattach", &json!({"path": path, "pages": [1, 3]})).unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(2));
    let u = underlays(&s);
    assert_eq!((u[0].page, u[1].page), (1, 3));
    assert!((u[1].insert.x - 297.0 * 1.05).abs() < 1e-9 && u[1].insert.y == 0.0, "{:?}", u[1].insert);
    assert!(s.execute("pdfattach", &json!({"path": path, "pages": [1, 4]})).is_err(), "page 4 doesn't exist");
    assert!(s.execute("pdfattach", &json!({"path": path, "pages": []})).is_err());
    s.execute("undo", &json!({})).unwrap();
    assert!(underlays(&s).is_empty(), "both undone at once");
}

#[test]
fn the_path_is_stored_full_relative_or_as_the_file_name() {
    let (doc, pdf) = ("/plans/site/Site.dxf", "/plans/site/pdf/Plan.pdf");
    assert_eq!(stored_path(pdf, Some(doc), "full").unwrap(), pdf);
    assert_eq!(stored_path(pdf, Some(doc), "none").unwrap(), "Plan.pdf");
    assert_eq!(stored_path(pdf, Some(doc), "relative").unwrap(), "pdf/Plan.pdf");
    assert_eq!(stored_path("/plans/other/Plan.pdf", Some(doc), "relative").unwrap(), "../other/Plan.pdf");
    assert!(stored_path(pdf, None, "relative").is_err(), "an unsaved drawing has no folder");
    assert!(stored_path(pdf, Some(doc), "nope").is_err());
}

/// The frame shown while the insertion point is picked is the page as it will be attached: at
/// the dialog's scale and rotation.
#[test]
fn the_insertion_preview_has_the_dialog_s_scale_and_rotation() {
    use cadcraft_engine::doc::Line;
    hooks();
    let path = pdf_file("Preview.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let preset = AttachPreset { path, pages: vec![1], at: None, scale: Some(0.02), rotation: Some(90.0), path_type: "full".into() };
    start_attach(&mut s, preset).unwrap();
    let cursor = cadcraft_geom::Vec2::new(100.0, 50.0);
    let lines: Vec<Line> = s
        .preview(cursor)
        .into_iter()
        .filter_map(|e| match e.kind {
            EntityKind::Line(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 4);
    let pts: Vec<cadcraft_geom::Vec2> = lines.iter().flat_map(|l| [l.a.xy(), l.b.xy()]).collect();
    let b = cadcraft_geom::Bounds2::from_points(pts);
    // 297 × 210 mm at 1/50, turned a quarter turn about the cursor: x from -4.2 to 0, y 0 to 5.94.
    assert!((b.width() - 210.0 * 0.02).abs() < 1e-9 && (b.height() - 297.0 * 0.02).abs() < 1e-9, "{b:?}");
    assert!((b.max.x - cursor.x).abs() < 1e-9 && (b.min.y - cursor.y).abs() < 1e-9, "{b:?}");
}

/// Typed, at the scale prompt the frame is the page at the prompt's default scale (1, its size
/// on paper), not a point.
#[test]
fn the_scale_prompt_shows_the_page_at_its_size_on_paper() {
    use cadcraft_engine::doc::Line;
    hooks();
    let path = pdf_file("TypedScale.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    for input in ["pdfattach", path.as_str(), "1", "10,20"] {
        s.cmdline(input).unwrap();
    }
    assert!(s.current_prompt().is_some_and(|p| p.message.contains("scale")), "{:?}", s.current_prompt().map(|p| p.message.clone()));
    let pts: Vec<cadcraft_geom::Vec2> = s
        .preview(cadcraft_geom::Vec2::new(500.0, 500.0))
        .into_iter()
        .filter_map(|e| match e.kind {
            EntityKind::Line(l) => Some(l),
            _ => None,
        })
        .flat_map(|l: Line| [l.a.xy(), l.b.xy()])
        .collect();
    let b = cadcraft_geom::Bounds2::from_points(pts);
    assert!((b.width() - 297.0).abs() < 1e-9 && (b.height() - 210.0).abs() < 1e-9, "{b:?}");
    assert!((b.min.x - 10.0).abs() < 1e-9 && (b.min.y - 20.0).abs() < 1e-9, "{b:?}");
}

#[test]
fn the_dialog_fills_in_what_it_knows_and_the_screen_gives_the_rest() {
    hooks();
    let path = pdf_file("Dialog.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    // Insertion point on screen; scale and rotation given.
    let preset = AttachPreset { path: path.clone(), pages: vec![2], at: None, scale: Some(0.5), rotation: Some(90.0), path_type: "full".into() };
    start_attach(&mut s, preset).unwrap();
    assert!(s.running.is_some(), "asks for the insertion point");
    s.cmdline("10,20").unwrap();
    assert!(s.running.is_none(), "nothing else to ask");
    let u = underlays(&s);
    assert_eq!((u[0].page, u[0].insert.x, u[0].insert.y, u[0].scale), (2, 10.0, 20.0, 0.5 * 25.4));
    assert!((u[0].rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

    // Everything given: attached at once.
    let preset = AttachPreset {
        path: path.clone(),
        pages: vec![1],
        at: Some(cadcraft_geom::Vec2::new(5.0, 5.0)),
        scale: Some(1.0),
        rotation: Some(0.0),
        path_type: "full".into(),
    };
    start_attach(&mut s, preset).unwrap();
    assert!(s.running.is_none());
    assert_eq!(underlays(&s).len(), 2);
    // A later PDFATTACH asks everything again.
    s.cmdline("pdfattach").unwrap();
    assert!(s.current_prompt().is_some_and(|p| p.message.contains("path")), "{:?}", s.current_prompt().map(|p| p.message.clone()));
}

#[test]
fn a_page_with_a_measuring_scale_is_sized_by_it() {
    // At file scale 1, a page whose first viewport has a /Measure is its size in points times
    // that conversion factor (an A4 at 0.35282 mm per point: 297.074 wide), not its size in
    // inches, whatever the drawing's units.
    hooks();
    let path = pdf_file("Measured.pdf", b"%PDF-1.7 /VP [<</Measure <<>>>>]");
    for (metric, paper) in [(true, 297.0), (false, 297.0 / 25.4)] {
        let mut s = Session::empty();
        s.new_drawing(metric);
        let r = s.execute("pdfattach", &json!({"path": path, "scale": 50})).unwrap();
        assert!((r["width"].as_f64().unwrap() - 50.0 * paper).abs() < 1e-9, "{r}");
        let u = the_underlay(&s);
        let native = 297.0 / (25.4 / 72.0) * 0.35282;
        assert!((u.size.x - native).abs() < 1e-9, "{u:?}");
        // The file's scale: what the native page size is multiplied by.
        assert!((u.scale - 50.0 * paper / native).abs() < 1e-9, "{u:?}");
    }
}

/// ATARYS places its PDF plans on an `IMPORT` layer: an underlay goes on the current layer, and
/// that layer's switches hide its page (on screen and on plots) like any object's.
#[test]
fn an_underlay_goes_on_the_current_layer_and_follows_it() {
    hooks();
    let path = pdf_file("Layer.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("layer.new", &json!({"name": "IMPORT", "current": true})).unwrap();
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let d = s.doc().unwrap();
    let hh = cadcraft_engine::doc::Handle::parse_hex(&h).unwrap();
    assert_eq!(d.entity(hh).unwrap().common.layer, "IMPORT");
    let pages = |s: &Session| {
        let d = s.doc().unwrap();
        let o = cadcraft_render::Options::default();
        (cadcraft_render::build(d, &Space::Model, &o).underlays.len(), cadcraft_render::build_plot(d, &Space::Model, &o).underlays.len())
    };
    assert_eq!(pages(&s), (1, 1));
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    s.execute("layer.set", &json!({"name": "IMPORT", "on": false})).unwrap();
    assert_eq!(pages(&s), (0, 0), "layer off: no page");
    s.execute("layer.set", &json!({"name": "IMPORT", "on": true, "frozen": true})).unwrap();
    assert_eq!(pages(&s), (0, 0), "layer frozen: no page");
    s.execute("layer.set", &json!({"name": "IMPORT", "frozen": false, "plot": false})).unwrap();
    assert_eq!(pages(&s), (1, 0), "layer not plotted: shown, not plotted");
}
