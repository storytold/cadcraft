//! A drawing's PDF underlays get their page size from their PDF when it is opened (a file stores
//! none), and their pages are read for showing them.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::underlay::{PdfHooks, set_pdf, turn_lightness, underlay_pixels};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, PdfUnderlay, Space};
use cadcraft_geom::Vec3;
use serde_json::json;

/// A fake reader: every file starting with `%PDF` has 3 A4 landscape pages, drawn white.
fn hooks() {
    set_pdf(PdfHooks {
        page: |bytes, page| {
            if !bytes.starts_with(b"%PDF") {
                return Err("not a readable PDF file".into());
            }
            if page > 3 { Err(format!("the PDF has no page {page} (it has 3)")) } else { Ok((3, 297.0, 210.0, None)) }
        },
        render: |_, _, side| Ok((side, side, vec![255; (side * side * 4) as usize])),
        region: |_, _, _, w, h| Ok((w, h, vec![255; (w * h * 4) as usize])),
        layers: |_| Ok(Vec::new()),
        with_layers: |bytes, _| Ok(bytes.to_vec()),
    });
}

fn real_io() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn the_underlay(s: &Session) -> PdfUnderlay {
    s.state()
        .unwrap()
        .doc
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::PdfUnderlay(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn an_opened_underlay_gets_its_size_from_its_pdf_and_finds_it_beside_the_drawing() {
    hooks();
    real_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-open-{}", std::process::id()));
    let far = dir.join("elsewhere");
    std::fs::create_dir_all(&far).unwrap();
    let pdf = far.join("Plan.pdf");
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    let dxf = dir.join("Site.dxf");
    let mut d = Drawing::new_metric();
    let u = PdfUnderlay {
        insert: Vec3::new(5.0, 6.0, 0.0),
        scale: 50.0 * 25.4,
        fade: 20,
        path: pdf.to_string_lossy().to_string(),
        ..PdfUnderlay::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    std::fs::write(&dxf, cadcraft_io::write(&d, "Site.dxf").unwrap()).unwrap();

    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    let u = the_underlay(&s);
    assert!((u.size.x - 297.0 / 25.4).abs() < 1e-12 && (u.size.y - 210.0 / 25.4).abs() < 1e-12, "the size is read from the PDF: {u:?}");
    assert_eq!((u.insert.x, u.scale, u.fade, u.page), (5.0, 50.0 * 25.4, 20, 1));
    assert!(!s.state().unwrap().is_dirty(), "reading the size is no change");
    let (w, h, px) = underlay_pixels(&u.path, Some(&dxf.to_string_lossy()), 1, 64, &[]).unwrap();
    assert_eq!((w, h, px.len()), (64, 64, 64 * 64 * 4));

    // The PDF moved beside the drawing: found there by its name.
    std::fs::rename(&pdf, dir.join("Plan.pdf")).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    assert!((the_underlay(&s).size.x - 297.0 / 25.4).abs() < 1e-12);

    // Gone: the underlay is kept without a size, and reported.
    std::fs::remove_file(dir.join("Plan.pdf")).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    assert_eq!(the_underlay(&s).size.x, 0.0);
    assert!(s.log.iter().any(|l| l.contains("PDF underlay not shown")), "{:?}", s.log);
    // Opening a drawing names no command.
    assert!(!s.log.iter().any(|l| l.contains("pdfattach")), "{:?}", s.log);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turning_the_lightness_over_keeps_hues() {
    let mut px = vec![255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 200, 200, 200, 255];
    turn_lightness(&mut px);
    assert_eq!(px, vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 55, 55, 55, 255]);
}
