//! The Model layout's plot settings say what CADCraft plots for model space: the drawing extents
//! fitted to the paper. Plot type 5 ("Layout information", DXF Reference, PLOTSETTINGS group 74)
//! means nothing for model space; another program's Plot dialog then showed no plot area and no
//! scale for Model.

use cadcraft_doc::*;
use cadcraft_geom::Vec3;

/// The (code, value) pairs of every `kind` record in DXF text.
fn records(text: &str, kind: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == kind;
            if inside {
                out.push(Vec::new());
            }
        } else if inside && let Some(rec) = out.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    out
}

/// A LAYOUT record's name (the code 1 after the `AcDbLayout` marker), plot type (74) and
/// standard scale type (75).
fn layout_plot(rec: &[(String, String)]) -> (String, Option<i64>, Option<i64>) {
    let at = rec.iter().position(|(c, v)| c == "100" && v == "AcDbLayout").unwrap_or(rec.len());
    let name = rec.iter().skip(at).find(|(c, _)| c == "1").map(|(_, v)| v.clone()).unwrap_or_default();
    let int = |code: &str| rec.iter().take(at).find(|(c, _)| c == code).and_then(|(_, v)| v.trim().parse().ok());
    (name, int("74"), int("75"))
}

fn plots(text: &str) -> Vec<(String, Option<i64>, Option<i64>)> {
    records(text, "LAYOUT").iter().map(|r| layout_plot(r)).collect()
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 50.0, 0.0) })).unwrap();
    d
}

#[test]
fn model_layout_plots_extents_fitted_and_layouts_their_sheet() {
    let p = plots(&cadcraft_io::write_dxf(&drawing()));
    let model = p.iter().find(|(n, _, _)| n == "Model").unwrap();
    assert_eq!((model.1, model.2), (Some(1), Some(0)), "Model: drawing extents, scaled to fit");
    // Paper-space layouts keep "Layout information" at 1:1.
    let layouts: Vec<_> = p.iter().filter(|(n, _, _)| n != "Model").collect();
    assert!(!layouts.is_empty());
    assert!(layouts.iter().all(|(_, t, s)| (*t, *s) == (Some(5), Some(16))), "{layouts:?}");
}

#[test]
fn model_layout_plot_type_survives_the_dwg_bridge() {
    let dwg = cadcraft_io::write(&drawing(), "x.dwg").unwrap();
    // What the DWG holds, as the bridge's own DXF.
    let dxf = String::from_utf8_lossy(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).to_string();
    let p = plots(&dxf);
    let model = p.iter().find(|(n, _, _)| n.eq_ignore_ascii_case("Model")).unwrap();
    assert_eq!(model.1, Some(1), "{p:?}");
    assert_ne!(p.iter().find(|(n, _, _)| !n.eq_ignore_ascii_case("Model")).and_then(|l| l.1), None);
}
