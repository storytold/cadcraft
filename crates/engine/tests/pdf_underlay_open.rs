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
fn an_underlay_whose_pdf_comes_back_gets_its_size_without_reopening() {
    use cadcraft_engine::cmd::underlay::retry_sizes;
    hooks();
    real_io();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-later-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Later.pdf");
    let dxf = dir.join("Later.dxf");
    let mut d = Drawing::new_metric();
    let u = PdfUnderlay { scale: 25.4, path: pdf.to_string_lossy().to_string(), ..PdfUnderlay::default() };
    d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    std::fs::write(&dxf, cadcraft_io::write(&d, "Later.dxf").unwrap()).unwrap();
    // Opened before its PDF is there (a network share not yet reachable): no size.
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    assert_eq!(the_underlay(&s).size.x, 0.0);
    let revision = s.state().unwrap().revision;
    // Still missing: nothing changes, nothing is reported again.
    let lines = s.log.len();
    assert!(!retry_sizes(&mut s));
    assert_eq!((s.state().unwrap().revision, s.log.len()), (revision, lines));
    // The PDF arrives: the size is read, the drawing is redrawn (a new revision) and isn't modified.
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    assert!(retry_sizes(&mut s));
    assert!((the_underlay(&s).size.x - 297.0 / 25.4).abs() < 1e-12, "{:?}", the_underlay(&s));
    assert!(s.state().unwrap().revision > revision);
    assert!(!s.state().unwrap().is_dirty(), "reading the size is no change");
    assert!(!retry_sizes(&mut s), "nothing left to size");
    // A drawing edited meanwhile stays edited.
    std::fs::remove_file(&pdf).unwrap();
    let mut s = Session::empty();
    s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    assert!(retry_sizes(&mut s));
    assert!(s.state().unwrap().is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_pdf_is_not_read_again_until_it_changes() {
    use cadcraft_engine::cmd::underlay::{SizeRead, page_size};
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Damaged.pdf");
    let stored = pdf.to_string_lossy().to_string();
    assert_eq!(page_size(&stored, None, 1, None), SizeRead::Missing, "not there yet");
    std::fs::write(&pdf, b"not a pdf").unwrap();
    let SizeRead::Unreadable(since, why) = page_size(&stored, None, 1, None) else { panic!("a damaged file") };
    assert!(since.is_some() && why.contains("Damaged.pdf"), "{why}");
    // Not changed since: not read again.
    assert_eq!(page_size(&stored, None, 1, since), SizeRead::Unchanged);
    // Replaced by a good one (a later modification time): read, and found.
    let later = since.unwrap() + std::time::Duration::from_secs(2);
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    std::fs::File::options().write(true).open(&pdf).unwrap().set_modified(later).unwrap();
    let SizeRead::Found(size) = page_size(&stored, None, 1, since) else { panic!("read again once changed") };
    assert!((size.x - 297.0 / 25.4).abs() < 1e-12);
    // A page the PDF doesn't have: unreadable too.
    assert!(matches!(page_size(&stored, None, 9, None), SizeRead::Unreadable(..)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turning_the_lightness_over_keeps_hues() {
    let mut px = vec![255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 200, 200, 200, 255];
    turn_lightness(&mut px);
    assert_eq!(px, vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 55, 55, 55, 255]);
}
