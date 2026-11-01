//! PLOT chooses the output format from the extension of `path` when no device is named (#2556),
//! like EXPORT does; a device that writes another format is refused.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::cmd::plotters::{PlotterStore, set_plotter_store};
use serde_json::json;

fn session() -> Session {
    set_plotter_store(PlotterStore::memory());
    set_io(IoHooks {
        read: |_, _| Err("no reader in this test".into()),
        write: |_, _| Err("no writer in this test".into()),
        plot: Some(|d, space, opts| cadcraft_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    s
}

fn magic(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path).unwrap().into_iter().take(5).collect()
}

#[test]
fn the_extension_picks_png_svg_or_pdf() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-plot-format-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cases: [(&str, &[u8], &str); 4] =
        [("a.png", b"\x89PNG\r", "png"), ("b.svg", b"<svg ", "svg"), ("c.pdf", b"%PDF-", "pdf"), ("D.PNG", b"\x89PNG\r", "png")];
    for (name, head, format) in cases {
        let out = dir.join(name);
        let r = s.execute("plot", &json!({ "path": out.to_string_lossy(), "layout": "Model" })).unwrap();
        assert_eq!(r["format"], format, "{name}: the reply names the format");
        assert_eq!(magic(&out), head, "{name}: the file holds that format");
    }
    // No path: PDF data as before. An extension that names no format is refused, like EXPORT.
    assert_eq!(s.execute("plot", &json!({ "layout": "Model" })).unwrap()["format"], "pdf");
    let other = dir.join("e.plt");
    let e = s.execute("plot", &json!({ "path": other.to_string_lossy(), "layout": "Model" })).unwrap_err().to_string();
    assert!(e.contains("png") && e.contains("svg") && e.contains("pdf"), "{e}");
    assert!(!other.exists(), "nothing is written under a name that is not a format");
    // EXPORTPDF always writes PDF.
    let pdf = dir.join("f.png");
    s.execute("exportpdf", &json!({ "path": pdf.to_string_lossy(), "layout": "Model" })).unwrap();
    assert_eq!(magic(&pdf), b"%PDF-");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_device_that_disagrees_with_the_extension_is_refused() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-plot-format-mismatch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("m.pdf");
    let e = s.execute("plot", &json!({ "path": out.to_string_lossy(), "layout": "Model", "device": "CADCraft PNG" })).unwrap_err();
    assert!(e.to_string().contains("CADCraft PNG") && e.to_string().contains("pdf"), "{e}");
    assert!(!out.exists(), "nothing is written");
    // A device that agrees works, and "None" leaves the extension in charge.
    let ok = dir.join("m.png");
    let r = s.execute("plot", &json!({ "path": ok.to_string_lossy(), "layout": "Model", "device": "CADCraft PNG" })).unwrap();
    assert_eq!((r["format"].as_str(), magic(&ok)), (Some("png"), b"\x89PNG\r".to_vec()));
    let none = dir.join("n.svg");
    s.execute("plot", &json!({ "path": none.to_string_lossy(), "layout": "Model", "device": "None" })).unwrap();
    assert_eq!(magic(&none), b"<svg ");
    std::fs::remove_dir_all(&dir).ok();
}
