//! A PDF page converted on the command line keeps its geometry and its fill rules through a DXF
//! or DWG save and reopen.

use std::process::{Command, Output};

use serde_json::Value;

const MM_PER_PT: f64 = 25.4 / 72.0;

/// A line, a nonzero fill of two nested squares wound the same way (all of it filled), an
/// even-odd fill of the same squares (the inner one a hole) and a nonzero fill of two
/// overlapping squares (split into triangles).
const CONTENT: &str = "1 0 0 RG 2 w 10 10 m 100 10 l S\n\
    0 0 1 rg 200 200 m 300 200 l 300 300 l 200 300 l h 220 220 m 280 220 l 280 280 l 220 280 l h f\n\
    0 1 0 rg 400 400 m 500 400 l 500 500 l 400 500 l h 420 420 m 480 420 l 480 480 l 420 480 l h f*\n\
    1 0 1 rg 50 600 m 150 600 l 150 700 l 50 700 l h 100 650 m 200 650 l 200 750 l 100 750 l h f\n";

fn page(content: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
         3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R >> endobj\n\
         4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n\
         trailer << /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A fresh scratch directory that is removed on drop, also when an assertion fails.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cc-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).expect("create scratch dir");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The model space of `file` with each object's geometry.
fn inspect(file: &str) -> Value {
    let out = cli(&["run", file, "--cmd", r#"drawing.inspect {"entities":true}"#]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{file}: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(stdout.lines().last().unwrap_or_default()).unwrap_or_else(|e| panic!("{file}: {e}: {stdout}"))
}

fn inside(ring: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    let mut odd = false;
    for (i, &(ax, ay)) in ring.iter().enumerate() {
        let (bx, by) = ring[(i + 1) % ring.len()];
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            odd = !odd;
        }
    }
    odd
}

/// Whether a hatch covers the page point `at` (in PDF points), by odd parity as hatches are drawn.
fn filled(drawing: &Value, at: (f64, f64)) -> bool {
    let at = (at.0 * MM_PER_PT, at.1 * MM_PER_PT);
    let entities = drawing["entities"].as_array().cloned().unwrap_or_default();
    entities.iter().filter(|e| e["type"] == "Hatch").any(|hatch| {
        let loops = hatch["geometry"]["loops"].as_array().cloned().unwrap_or_default();
        let rings = loops.iter().map(|l| {
            l["vertices"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| (v["p"]["x"].as_f64().unwrap_or(0.0), v["p"]["y"].as_f64().unwrap_or(0.0)))
                .collect()
        });
        rings.filter(|ring: &Vec<(f64, f64)>| inside(ring, at)).count() % 2 == 1
    })
}

#[test]
fn a_pdf_page_keeps_its_fills_through_dxf_and_dwg() {
    let scratch = Scratch::new("pdf-import");
    let pdf = scratch.0.join("plan.pdf");
    std::fs::write(&pdf, page(CONTENT)).unwrap();
    let pdf = pdf.to_str().unwrap().to_string();
    let mut files = vec![pdf.clone()];
    for ext in ["dxf", "dwg"] {
        let out = scratch.0.join(format!("plan.{ext}"));
        let out = out.to_str().unwrap().to_string();
        let run = cli(&["convert", &pdf, &out]);
        assert!(run.status.success(), "convert to {ext}: {}", String::from_utf8_lossy(&run.stderr));
        files.push(out);
    }
    for file in &files {
        let d = inspect(file);
        assert_eq!(d["counts"]["Line"], 1, "{file}: {}", d["counts"]);
        assert_eq!(d["counts"]["Hatch"], 3, "{file}: {}", d["counts"]);
        assert!(filled(&d, (251.3, 247.7)), "{file}: the nonzero fill keeps its centre");
        assert!(filled(&d, (210.2, 205.9)), "{file}: the nonzero fill keeps its rim");
        assert!(!filled(&d, (451.3, 447.7)), "{file}: the even-odd fill keeps its hole");
        assert!(filled(&d, (410.2, 405.9)), "{file}: the even-odd fill keeps its rim");
        assert!(!filled(&d, (350.4, 351.7)), "{file}: nothing is filled between the shapes");
        assert!(filled(&d, (125.3, 675.2)), "{file}: the overlap of the crossing squares stays filled");
        assert!(filled(&d, (60.4, 610.3)) && filled(&d, (190.2, 740.1)), "{file}: both crossing squares stay filled");
        assert!(!filled(&d, (60.4, 740.1)), "{file}: the corner beside the crossing squares stays empty");
    }
}
