//! A drawing opened from the window doesn't wait for its PDF pages' lines: they are read on a
//! worker thread, and object snaps find them once read. Its own test program, for its slow line
//! reader.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::read_file;
use cadcraft_engine::cmd::underlay::{PdfHooks, page_lines, set_pdf};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, PdfUnderlay, Space};

#[test]
fn opening_from_the_window_does_not_wait_for_the_lines() {
    set_pdf(PdfHooks {
        page: |_, _| Ok((1, 297.0, 210.0, None)),
        render: |_, _, side| Ok((side, side, vec![255; (side * side * 4) as usize])),
        // A slow reader, as a large PDF is.
        lines: |_, _| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            Ok(vec![vec![(0.1, 0.5), (0.9, 0.5)]])
        },
        region: |_, _, _, w, h| Ok((w, h, vec![255; (w * h * 4) as usize])),
        layers: |_| Ok(Vec::new()),
        with_layers: |bytes, _| Ok(bytes.to_vec()),
    });
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-lines-after-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Sheet.pdf");
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    let stored = pdf.to_string_lossy().to_string();
    let mut d = Drawing::new_metric();
    let u = PdfUnderlay { scale: 25.4, path: stored.clone(), ..PdfUnderlay::default() };
    d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    let dxf = dir.join("Plan.dxf");
    std::fs::write(&dxf, cadcraft_io::write(&d, "Plan.dxf").unwrap()).unwrap();

    let mut s = Session::empty();
    let f = read_file(&dxf).unwrap();
    let t = std::time::Instant::now();
    s.finish_open(f).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(1000), "the window waited {:?} for the lines", t.elapsed());
    assert!(page_lines(&stored, 1, "").is_none(), "not read yet");
    let t = std::time::Instant::now();
    while page_lines(&stored, 1, "").is_none() && t.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(page_lines(&stored, 1, "").map(|l| l.len()), Some(1), "read on a worker thread");
    let _ = std::fs::remove_dir_all(&dir);
}
