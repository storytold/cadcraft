//! A page's lines, for object snaps, are read once at a time: two drawings whose PDF comes back
//! together don't read it twice in parallel. Its own test program, for its slow line reader.

use std::sync::atomic::{AtomicUsize, Ordering};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::underlay::{PdfHooks, SizeRead, page_size, set_pdf, set_sizes, unsized_pages};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, PdfUnderlay, Space};
use serde_json::json;

static LINE_READS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn a_page_s_lines_are_not_read_twice_at_the_same_time() {
    set_pdf(PdfHooks {
        page: |_, _| Ok((1, 297.0, 210.0, None)),
        render: |_, _, side| Ok((side, side, vec![255; (side * side * 4) as usize])),
        // A slow reader, as a large PDF is.
        lines: |_, _| {
            LINE_READS.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(300));
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
    let dir = std::env::temp_dir().join(format!("cadcraft-pdf-lines-once-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("Shared.pdf");
    let mut d = Drawing::new_metric();
    let u = PdfUnderlay { scale: 25.4, path: pdf.to_string_lossy().to_string(), ..PdfUnderlay::default() };
    d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    // Two drawings on the same PDF, opened while it isn't there.
    let mut s = Session::empty();
    for name in ["One.dxf", "Two.dxf"] {
        let dxf = dir.join(name);
        std::fs::write(&dxf, cadcraft_io::write(&d, name).unwrap()).unwrap();
        s.execute("open", &json!({"path": dxf.to_string_lossy()})).unwrap();
    }
    std::fs::write(&pdf, b"%PDF-1.7 fake").unwrap();
    // Its size is read for each in turn; each sets it, and asks for the page's lines in the background.
    for i in 0..2 {
        s.active = i;
        let todo = unsized_pages(&s);
        let sizes = todo
            .pages
            .iter()
            .filter_map(|(stored, page)| match page_size(stored, todo.document.as_deref(), *page, None) {
                SizeRead::Found(size) => Some(((stored.clone(), *page), size)),
                _ => None,
            })
            .collect();
        assert!(set_sizes(&mut s, todo.drawing, todo.document.as_deref(), &sizes));
    }
    std::thread::sleep(std::time::Duration::from_millis(900));
    assert_eq!(LINE_READS.load(Ordering::SeqCst), 1, "read once, not once per drawing");
    let _ = std::fs::remove_dir_all(&dir);
}
