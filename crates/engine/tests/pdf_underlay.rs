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
        // A diagonal and a horizontal line crossing it at the page's middle.
        lines: |bytes, _| {
            let off = |n: &str| bytes.windows(5 + n.len()).any(|w| w == format!("%OFF:{n}").as_bytes());
            let mut l = Vec::new();
            if !off("MURS") {
                l.push(vec![(0.2, 0.2), (0.6, 0.6)]);
            }
            if !off("COTES") {
                l.push(vec![(0.1, 0.5), (0.9, 0.5)]);
            }
            Ok(l)
        },
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

/// A drawing opened before its PDF is reachable: once the PDF is back, the page gets its size
/// and object snaps find its lines, without reopening the drawing.
#[test]
fn object_snaps_find_the_page_lines_once_the_pdf_is_back() {
    use cadcraft_engine::cmd::underlay::retry_sizes;
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    real_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-snap-later-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Later.pdf");
    let dxf = dir.join("Later.dxf");
    // Written straight to the file, so this PDF's lines were never read (they are kept per file,
    // page and PDF layers for the whole process).
    let mut d = cadcraft_engine::doc::Drawing::new_metric();
    let u = cadcraft_engine::doc::PdfUnderlay {
        insert: cadcraft_geom::Vec3::new(3000.0, 0.0, 0.0),
        scale: 25.4,
        path: pdf.to_string_lossy().to_string(),
        ..cadcraft_engine::doc::PdfUnderlay::default()
    };
    d.add(&Space::Model, cadcraft_engine::doc::Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    std::fs::write(&dxf, cadcraft_io::write(&d, "Later.dxf").unwrap()).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    let at = |fx: f64, fy: f64| cadcraft_geom::Vec2::new(3000.0 + fx * 297.0, fy * 210.0);
    let snap = |s: &Session| osnap(s.doc().unwrap(), &Space::Model, at(0.5, 0.5) + cadcraft_geom::Vec2::new(0.7, -0.4), 3.0, mode::INT, None, false);
    assert!(snap(&s).is_none(), "no PDF: no lines");
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    assert!(retry_sizes(&mut s));
    let hit = snap(&s).map(|h| (h.mode, h.point));
    assert!(hit.is_some_and(|(m, p)| m == mode::INT && p.dist(at(0.5, 0.5)) < 1e-6), "{hit:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The canvas's way: the size read on a worker (`page_size`), set on the UI thread
/// (`set_sizes`), and the page's lines read in the background, then found by object snaps.
#[test]
fn object_snaps_find_the_page_lines_read_in_the_background() {
    use cadcraft_engine::cmd::underlay::{SizeRead, page_size, set_sizes, unsized_pages};
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    real_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-snap-bg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Background.pdf");
    let dxf = dir.join("Background.dxf");
    let mut d = cadcraft_engine::doc::Drawing::new_metric();
    let u = cadcraft_engine::doc::PdfUnderlay {
        insert: cadcraft_geom::Vec3::new(5000.0, 0.0, 0.0),
        scale: 25.4,
        path: pdf.to_string_lossy().to_string(),
        ..cadcraft_engine::doc::PdfUnderlay::default()
    };
    d.add(&Space::Model, cadcraft_engine::doc::Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    std::fs::write(&dxf, cadcraft_io::write(&d, "Background.dxf").unwrap()).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    let todo = unsized_pages(&s);
    assert_eq!(todo.pages.len(), 1);
    let sizes: std::collections::HashMap<(String, u32), cadcraft_geom::Vec2> = todo
        .pages
        .into_iter()
        .filter_map(|(stored, page)| match page_size(&stored, todo.document.as_deref(), page, None) {
            SizeRead::Found(size) => Some(((stored, page), size)),
            _ => None,
        })
        .collect();
    assert!(set_sizes(&mut s, todo.drawing, todo.document.as_deref(), &sizes));
    let at = cadcraft_geom::Vec2::new(5000.0 + 0.5 * 297.0, 0.5 * 210.0);
    let mut hit = None;
    for _ in 0..200 {
        hit = osnap(s.doc().unwrap(), &Space::Model, at + cadcraft_geom::Vec2::new(0.7, -0.4), 3.0, mode::INT, None, false).map(|h| h.point);
        if hit.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(hit.is_some_and(|p| p.dist(at) < 1e-6), "{hit:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn object_snaps_find_the_lines_drawn_on_the_page() {
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    let path = pdf_file("Snap.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path, "at": [1000, 0], "scale": 2})).unwrap();
    let d = s.doc().unwrap();
    let (w, h) = (297.0 * 2.0, 210.0 * 2.0);
    let at = |fx: f64, fy: f64| cadcraft_geom::Vec2::new(1000.0 + fx * w, fy * h);
    let near = |p: cadcraft_geom::Vec2| p + cadcraft_geom::Vec2::new(0.7, -0.4);
    let snap = |p, m| osnap(d, &Space::Model, near(p), 3.0, m, None, false).map(|h| (h.mode, h.point));
    let close = |a: cadcraft_geom::Vec2, b: cadcraft_geom::Vec2| a.dist(b) < 1e-6;

    let (m, p) = snap(at(0.2, 0.2), mode::END).unwrap();
    assert!(m == mode::END && close(p, at(0.2, 0.2)), "{p:?}");
    let (m, p) = snap(at(0.4, 0.4), mode::MID).unwrap();
    assert!(m == mode::MID && close(p, at(0.4, 0.4)), "{p:?}");
    let (m, p) = snap(at(0.5, 0.5), mode::INT).unwrap();
    assert!(m == mode::INT && close(p, at(0.5, 0.5)), "{p:?}");
    // Far from any line of the page: nothing.
    assert!(snap(at(0.8, 0.2), mode::END | mode::MID | mode::INT).is_none());
}

#[test]
fn object_snaps_find_the_page_lines_through_a_layout_viewport() {
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    let path = pdf_file("Snap-layout.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path, "at": [1000, 0], "scale": 2})).unwrap();
    s.execute("layout", &json!({"option": "set", "name": "Layout1"})).unwrap();
    s.execute("mview", &json!({"p1": [10, 10], "p2": [200, 150]})).unwrap();
    s.execute("zoom", &json!({"extents": true})).unwrap();
    let d = s.doc().unwrap();
    let paper = Space::Paper("Layout1".into());
    let vp = d
        .space(&paper)
        .unwrap()
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 => Some(v.clone()),
            _ => None,
        })
        .unwrap();
    // Model point → paper through the viewport.
    let k = vp.height / vp.view_height;
    let (w, h) = (297.0 * 2.0, 210.0 * 2.0);
    let on_paper = |fx: f64, fy: f64| vp.center.xy() + (cadcraft_geom::Vec2::new(1000.0 + fx * w, fy * h) - vp.view_center) * k;
    let near = |p: cadcraft_geom::Vec2| p + cadcraft_geom::Vec2::new(0.3, -0.2);
    let snap = |p, m| osnap(d, &paper, near(p), 2.0, m, None, false).map(|h| (h.mode, h.point));
    let close = |a: cadcraft_geom::Vec2, b: cadcraft_geom::Vec2| a.dist(b) < 1e-6;

    let (m, p) = snap(on_paper(0.2, 0.2), mode::END).unwrap();
    assert!(m == mode::END && close(p, on_paper(0.2, 0.2)), "{p:?} vs {:?}", on_paper(0.2, 0.2));
    let (m, p) = snap(on_paper(0.5, 0.5), mode::INT).unwrap();
    assert!(m == mode::INT && close(p, on_paper(0.5, 0.5)), "{p:?}");
    assert!(snap(on_paper(0.8, 0.2), mode::END | mode::MID | mode::INT).is_none());
}

#[test]
fn pdfclip_clips_to_a_rectangle_or_polygon_and_follows_the_underlay() {
    use cadcraft_geom::Vec2;
    hooks();
    let path = pdf_file("Clip.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let bounds = |s: &Session| {
        let d = s.doc().unwrap();
        let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
        let b = entity_bounds(d, e, 0);
        (b.min, b.max)
    };
    let close = |a: Vec2, b: Vec2| a.dist(b) < 1e-6;

    s.execute("pdfclip", &json!({"handle": h, "rect": [[150, 40], [50, 100]]})).unwrap();
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip.len() == 4);
    let (lo, hi) = bounds(&s);
    assert!(close(lo, Vec2::new(50.0, 40.0)) && close(hi, Vec2::new(150.0, 100.0)), "the frame is the boundary: {lo:?} {hi:?}");

    // The boundary moves, turns and scales with the underlay.
    s.execute("move", &json!({"handles": [h], "from": [0, 0], "to": [1000, 500]})).unwrap();
    let (lo, hi) = bounds(&s);
    assert!(close(lo, Vec2::new(1050.0, 540.0)) && close(hi, Vec2::new(1150.0, 600.0)), "{lo:?} {hi:?}");
    s.execute("scale", &json!({"handles": [h], "base": [1000, 500], "factor": 2})).unwrap();
    let (lo, hi) = bounds(&s);
    assert!(close(lo, Vec2::new(1100.0, 580.0)) && close(hi, Vec2::new(1300.0, 700.0)), "{lo:?} {hi:?}");

    // OFF shows the whole page again, keeping the boundary; ON applies it again.
    s.execute("pdfclip", &json!({"handle": h, "on": false})).unwrap();
    let u = the_underlay(&s);
    assert!(!u.clipping && u.clip.len() == 4);
    let (lo, hi) = bounds(&s);
    assert!(close(lo, Vec2::new(1000.0, 500.0)) && close(hi - lo, Vec2::new(594.0, 420.0)), "{lo:?} {hi:?}");
    s.execute("pdfclip", &json!({"handle": h, "on": true})).unwrap();
    assert!(the_underlay(&s).clipping);

    s.execute("pdfclip", &json!({"handle": h, "polygon": [[1100, 600], [1300, 600], [1200, 800]]})).unwrap();
    assert_eq!(the_underlay(&s).clip.len(), 3);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(the_underlay(&s).clip.len(), 4, "undone like any change");
    s.execute("pdfclip", &json!({"handle": h, "delete": true})).unwrap();
    let u = the_underlay(&s);
    assert!(!u.clipping && u.clip.is_empty());

    for bad in [
        json!({"handle": h}),
        json!({"handle": h, "rect": [[0, 0], [10, 0]]}),
        json!({"handle": h, "rect": [[0, 0]]}),
        json!({"handle": h, "polygon": [[0, 0], [1, 1]]}),
        json!({"handle": h, "polygon": [[0, 0], [1, 1], [2, 2]]}),
        json!({"handle": h, "rect": [[0, 0], [1, 1]], "delete": true}),
        json!({"handle": h, "on": "yes"}),
        json!({"handle": h, "delete": false}),
    ] {
        assert!(s.execute("pdfclip", &bad).is_err(), "{bad}");
    }
    let line = s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    assert!(s.execute("pdfclip", &json!({"handle": line, "delete": true})).is_err(), "not an underlay");
    s.execute("layer.set", &json!({"name": "0", "locked": true})).unwrap();
    assert!(s.execute("pdfclip", &json!({"handle": h, "rect": [[0, 0], [10, 10]]})).is_err(), "locked layer");
}

#[test]
fn pdfclip_typed_rectangle_and_polygon() {
    hooks();
    let path = pdf_file("Clip-typed.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path})).unwrap();
    for line in ["pdfclip", "l", "", "", "", "10,10", "60,40"] {
        s.cmdline(line).unwrap();
    }
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip.len() == 4, "{:?}", s.log);
    for line in ["pdfclip", "l", "", "off"] {
        s.cmdline(line).unwrap();
    }
    assert!(!the_underlay(&s).clipping);
    // Polygonal, with a point taken back.
    for line in ["pdfclip", "l", "", "n", "p", "0,0", "100,0", "500,500", "u", "100,100", "0,100", ""] {
        s.cmdline(line).unwrap();
    }
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip.len() == 4, "{:?}", u.clip);
    for line in ["pdfclip", "l", "", "d"] {
        s.cmdline(line).unwrap();
    }
    assert!(the_underlay(&s).clip.is_empty());
}

#[test]
fn object_snaps_find_page_lines_only_inside_the_clip_boundary() {
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    let path = pdf_file("Clip-snap.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let (w, hh) = (297.0, 210.0);
    let at = |fx: f64, fy: f64| cadcraft_geom::Vec2::new(fx * w, fy * hh);
    // Keep the page's right half: the diagonal's lower end (0.2, 0.2) is cut away, the lines'
    // crossing (0.5, 0.5) kept.
    s.execute("pdfclip", &json!({"handle": h, "rect": [[0.45 * w, 0.0], [w, hh]]})).unwrap();
    let d = s.doc().unwrap();
    let snap =
        |p: cadcraft_geom::Vec2, m| osnap(d, &Space::Model, p + cadcraft_geom::Vec2::new(0.7, -0.4), 3.0, m, None, false).map(|h| (h.mode, h.point));
    assert!(snap(at(0.2, 0.2), mode::END).is_none(), "outside the boundary");
    let (m, p) = snap(at(0.5, 0.5), mode::INT).unwrap();
    assert!(m == mode::INT && p.dist(at(0.5, 0.5)) < 1e-6, "{p:?}");
    // The diagonal now ends on the boundary.
    let (m, p) = snap(at(0.45, 0.45), mode::END).unwrap();
    assert!(m == mode::END && p.dist(at(0.45, 0.45)) < 1e-6, "{p:?}");
}

#[test]
fn pdfclip_inverted_hides_the_inside() {
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    let path = pdf_file("Clip-invert.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let (w, hh) = (297.0, 210.0);
    let at = |fx: f64, fy: f64| cadcraft_geom::Vec2::new(fx * w, fy * hh);
    // Hide the middle of the page: the lines' crossing (0.5, 0.5) goes, the diagonal's lower end
    // (0.2, 0.2) stays.
    s.execute("pdfclip", &json!({"handle": h, "rect": [[0.4 * w, 0.4 * hh], [0.7 * w, 0.7 * hh]], "invert": true})).unwrap();
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip_inverted);
    {
        let d = s.doc().unwrap();
        let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
        let b = entity_bounds(d, e, 0);
        assert!(b.min.dist(at(0.0, 0.0)) < 1e-6 && b.max.dist(at(1.0, 1.0)) < 1e-6, "inverted: the whole page's extents");
        let snap = |p: cadcraft_geom::Vec2, m| {
            osnap(d, &Space::Model, p + cadcraft_geom::Vec2::new(0.7, -0.4), 3.0, m, None, false).map(|h| (h.mode, h.point))
        };
        assert!(snap(at(0.5, 0.5), mode::INT).is_none(), "hidden inside the boundary");
        let (m, p) = snap(at(0.2, 0.2), mode::END).unwrap();
        assert!(m == mode::END && p.dist(at(0.2, 0.2)) < 1e-6, "{p:?}");
        // The diagonal is cut where it enters the boundary.
        let (m, p) = snap(at(0.4, 0.4), mode::END).unwrap();
        assert!(m == mode::END && p.dist(at(0.4, 0.4)) < 1e-6, "{p:?}");
    }
    s.execute("pdfclip", &json!({"handle": h, "invert": false})).unwrap();
    assert!(!the_underlay(&s).clip_inverted);
    assert!(s.execute("pdfclip", &json!({"handle": h, "invert": "yes"})).is_err());
    assert!(s.execute("pdfclip", &json!({"handle": h, "on": true, "invert": true})).is_err());

    // Typed: "Invert clip" before the boundary.
    for line in ["pdfclip", "l", "", "", "i", "r", "10,10", "60,40"] {
        s.cmdline(line).unwrap();
    }
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip_inverted, "{:?}", s.log);
}

#[test]
fn an_underlay_in_a_block_is_drawn_and_snapped_through_each_reference() {
    use cadcraft_engine::snap::{mode, osnap};
    use cadcraft_geom::Vec2;
    hooks();
    let path = pdf_file("Block.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({"name": "PLAN", "base": [0, 0], "handles": [h], "keep": "delete"})).unwrap();
    s.execute("insert", &json!({"name": "PLAN", "at": [1000, 0]})).unwrap();
    s.execute("insert", &json!({"name": "PLAN", "at": [0, 1000], "scale": 2, "rotation": 90})).unwrap();
    let d = s.doc().unwrap();
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    let mut origins: Vec<Vec2> = list.underlays.iter().map(|u| u.origin).collect();
    origins.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(origins.len(), 2, "one page per reference");
    assert!(origins[0].dist(Vec2::new(0.0, 1000.0)) < 1e-6 && origins[1].dist(Vec2::new(1000.0, 0.0)) < 1e-6, "{origins:?}");
    for lu in &list.underlays {
        assert!(cadcraft_render::pdf_underlay(d, lu.handle).is_some(), "its file is found in the block");
    }
    let turned = list.underlays.iter().find(|u| u.origin.x < 1.0).unwrap();
    assert!(turned.u.dist(Vec2::new(0.0, 594.0)) < 1e-6, "scaled and turned with the reference: {:?}", turned.u);

    // Snaps on the page lines, through the first reference.
    let at = |fx: f64, fy: f64| Vec2::new(1000.0 + fx * 297.0, fy * 210.0);
    let (m, p) = osnap(d, &Space::Model, at(0.5, 0.5) + Vec2::new(0.7, -0.4), 3.0, mode::INT, None, false).map(|h| (h.mode, h.point)).unwrap();
    assert!(m == mode::INT && p.dist(at(0.5, 0.5)) < 1e-6, "{p:?}");
}

/// A rectangle on a page attached at 500,0, scale 50.8 (2 on paper), rotation 30°, corners
/// given at 400,100 and 800,400: aligned with the page and cut to its edges, it keeps page
/// corners (0, 3.86634) and (9.05133, 2.68903), a band whose extents are 401.79,118.30 -
/// 829.90,400.
#[test]
fn a_rectangle_on_a_turned_page_is_aligned_with_the_page_and_cut_to_it() {
    use cadcraft_geom::Vec2;
    hooks();
    let path = pdf_file("Clip-turned.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h =
        s.execute("pdfattach", &json!({"path": path, "at": [500, 0], "scale": 2, "rotation": 30})).unwrap()["handle"].as_str().unwrap().to_string();
    s.execute("pdfclip", &json!({"handle": h, "rect": [[400, 100], [800, 400]]})).unwrap();
    let u = the_underlay(&s);
    let (lo, hi) = u.clip.iter().fold((Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))
    });
    assert!(lo.dist(Vec2::new(0.0, 2.68903)) < 1e-4 && hi.dist(Vec2::new(9.05133, 3.86634)) < 1e-4, "{lo:?} {hi:?}");
    let d = s.doc().unwrap();
    let e = d.space(&Space::Model).into_iter().flat_map(|st| st.iter()).find(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).unwrap();
    let b = entity_bounds(d, e, 0);
    assert!(b.min.dist(Vec2::new(401.79, 118.30)) < 0.01 && b.max.dist(Vec2::new(829.90, 400.0)) < 0.01, "{b:?}");
    // Wholly off the page: refused.
    assert!(s.execute("pdfclip", &json!({"handle": h, "rect": [[-900, -900], [-800, -800]]})).is_err());
}

#[test]
fn pdfframe_and_pdfosnap_switch_the_frame_and_the_snaps() {
    use cadcraft_engine::snap::{mode, osnap};
    hooks();
    let path = pdf_file("Frame.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    s.execute("pdfattach", &json!({"path": path})).unwrap();
    let prims = |s: &Session| {
        let d = s.doc().unwrap();
        let o = cadcraft_render::Options::default();
        (cadcraft_render::build(d, &Space::Model, &o).prims.len(), cadcraft_render::build_plot(d, &Space::Model, &o).prims.len())
    };
    let (screen, plot) = prims(&s);
    assert!(screen > 0 && plot > 0, "frame shown and plotted by default (PDFFRAME 1)");
    s.execute("setvar", &json!({"name": "PDFFRAME", "value": 0})).unwrap();
    assert_eq!(prims(&s), (0, 0), "PDFFRAME 0: no frame");
    s.execute("setvar", &json!({"name": "PDFFRAME", "value": 2})).unwrap();
    assert_eq!(prims(&s), (screen, 0), "PDFFRAME 2: shown, not plotted");
    assert!(s.execute("setvar", &json!({"name": "PDFFRAME", "value": 3})).is_err());
    // The page itself stays whatever the frame.
    assert_eq!(cadcraft_render::build_plot(s.doc().unwrap(), &Space::Model, &cadcraft_render::Options::default()).underlays.len(), 1);

    let at = cadcraft_geom::Vec2::new(0.5 * 297.0, 0.5 * 210.0) + cadcraft_geom::Vec2::new(0.7, -0.4);
    assert!(osnap(s.doc().unwrap(), &Space::Model, at, 3.0, mode::INT, None, false).is_some());
    s.execute("setvar", &json!({"name": "PDFOSNAP", "value": 0})).unwrap();
    let off = mode::INT | s.settings.snap_extras();
    assert!(osnap(s.doc().unwrap(), &Space::Model, at, 3.0, off, None, false).is_none(), "PDFOSNAP 0: no snaps on the page");
    assert!(s.execute("setvar", &json!({"name": "PDFOSNAP", "value": 2})).is_err());
    // A session setting: another drawing follows it, and no drawing saves it.
    s.new_drawing(true);
    assert_eq!(s.execute("getvar", &json!({"name": "PDFOSNAP"})).unwrap()["value"], json!(0));
    assert!(s.doc().unwrap().header.get("PDFOSNAP").is_none());
}

#[test]
fn pdfclip_takes_a_polyline_as_its_boundary() {
    hooks();
    let path = pdf_file("Clip-pline.pdf", b"%PDF-1.7 fake");
    let mut s = Session::empty();
    s.new_drawing(true);
    let h = s.execute("pdfattach", &json!({"path": path})).unwrap()["handle"].as_str().unwrap().to_string();
    let pl = s.execute("pline", &json!({"vertices": [[10, 10], [100, 10], [100, 80], [10, 80]], "closed": true})).unwrap();
    let pl = pl["handle"].as_str().or(pl["handles"][0].as_str()).unwrap().to_string();
    s.execute("pdfclip", &json!({"handle": h, "polyline": pl})).unwrap();
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip.len() == 4, "{:?}", u.clip);
    let line = s.execute("line", &json!({"points": [[0, 0], [1, 1]]})).unwrap()["handles"][0].as_str().unwrap().to_string();
    assert!(s.execute("pdfclip", &json!({"handle": h, "polyline": line})).is_err(), "not a polyline");
    assert!(s.execute("pdfclip", &json!({"handle": h, "polyline": pl, "rect": [[0, 0], [1, 1]]})).is_err());

    // Typed: the underlay preselected, Select polyline, then the polyline (the last object).
    s.execute("pdfclip", &json!({"handle": h, "delete": true})).unwrap();
    s.execute("erase", &json!({"handles": [line]})).unwrap();
    s.set_selection(vec![cadcraft_engine::doc::Handle::parse_hex(&h).unwrap()]);
    for t in ["pdfclip", "", "s", "l"] {
        s.cmdline(t).unwrap();
    }
    let u = the_underlay(&s);
    assert!(u.clipping && u.clip.len() == 4, "{:?} {:?}", u.clip, s.log);
}
