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

#[test]
fn publish_single_pdf_respects_each_layout_page_setup_and_subset() {
    let mut s = session();
    s.execute("layout.new", &json!({"name": "Layout3"})).unwrap();
    s.execute("pagesetup", &json!({ "layout": "Layout1", "paper": "A3", "landscape": true, "fit": true })).unwrap();
    s.execute("pagesetup", &json!({ "layout": "Layout2", "paper": "A4", "landscape": false, "fit": false, "scale": 0.5 })).unwrap();

    let dir = std::env::temp_dir().join(format!("cadcraft-publish-single-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("subset.pdf");
    let r = s.execute("publish", &json!({ "path": out.to_string_lossy(), "layouts": ["Layout1", "Layout2"], "single_pdf": true })).unwrap();
    assert_eq!(r["pages"].as_u64(), Some(2));
    let raw = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    let boxes: Vec<(f64, f64)> = raw
        .match_indices("/MediaBox")
        .filter_map(|(i, _)| {
            let rest = &raw[i + 9..];
            let start = rest.find('[')?;
            let end = rest[start..].find(']')?;
            let nums: Vec<f64> = rest[start + 1..start + end].split_whitespace().filter_map(|t| t.parse::<f64>().ok()).collect();
            (nums.len() == 4).then(|| (nums[2] - nums[0], nums[3] - nums[1]))
        })
        .collect();
    assert_eq!(boxes.len(), 2, "expected 2 pages in combined PDF, got {boxes:?}");
    // A3 landscape = 420 x 297 mm (~1190.55 x 841.89 pt), A4 portrait = 210 x 297 mm (~595.28 x 841.89 pt).
    let pt_per_mm = 72.0 / 25.4;
    assert!((boxes[0].0 - 420.0 * pt_per_mm).abs() < 2.0 && (boxes[0].1 - 297.0 * pt_per_mm).abs() < 2.0, "page 1 A3: {:?}", boxes[0]);
    assert!((boxes[1].0 - 210.0 * pt_per_mm).abs() < 2.0 && (boxes[1].1 - 297.0 * pt_per_mm).abs() < 2.0, "page 2 A4: {:?}", boxes[1]);
    std::fs::remove_dir_all(&dir).ok();
}
