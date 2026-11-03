//! PDFIMPORT end to end: a sample drawing is plotted to PDF by `convert`, that PDF is brought into
//! a new drawing by the `pdfimport` command, and the drawing is saved and read as text.

fn invoke(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cadcraft_cli::run(args, &mut out, &mut err);
    (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
}

fn count(dxf: &str, kind: &str) -> usize {
    dxf.lines().collect::<Vec<_>>().windows(2).filter(|w| w[0].trim() == "0" && w[1].trim() == kind).count()
}

#[test]
fn a_plotted_sample_comes_back_as_geometry_on_the_pdf_layers() {
    let dir = std::env::temp_dir().join(format!("cc-pdfimport-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (sample, pdf, out) = (dir.join("plan.dxf"), dir.join("plan.pdf"), dir.join("back.dxf"));
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();

    let (code, _, err) = invoke(&["sample", "floorplan", &s(&sample)]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = invoke(&["convert", &s(&sample), &s(&pdf)]);
    assert_eq!(code, 0, "{err}");

    let cmd = format!("pdfimport {}", serde_json::json!({ "path": s(&pdf) }));
    let (code, stdout, err) = invoke(&["run", "--metric", "--cmd", &cmd, "--save", &s(&out)]);
    assert_eq!(code, 0, "{err}\n{stdout}");
    assert!(stdout.contains("objects imported from page 1 of 1 of plan.pdf"), "{stdout}");

    let back = std::fs::read_to_string(&out).unwrap();
    let geometry = count(&back, "LINE") + count(&back, "LWPOLYLINE");
    assert!(geometry >= 20, "the plan's walls and symbols: {geometry}");
    // The Standard style is the stroke font, which a plot draws as outlines, not text.
    let kinds: Vec<String> = ["LINE", "LWPOLYLINE", "TEXT", "HATCH"].iter().map(|k| format!("{k}={}", count(&back, k))).collect();
    assert!(back.contains("PDF_Geometry"), "{kinds:?}");

    // A second page that isn't there is a failure with the page count in it.
    let cmd = format!("pdfimport {}", serde_json::json!({ "path": s(&pdf), "page": 2 }));
    let (code, _, err) = invoke(&["run", "--metric", "--cmd", &cmd]);
    assert_eq!(code, 1);
    assert!(err.contains("page 2 doesn't exist"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
