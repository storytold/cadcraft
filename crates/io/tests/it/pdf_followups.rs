//! Plot follow-ups (#626): model-space plots use the plot device's paper margins, PNG and SVG
//! plots draw plot style ends, joins and fills as PDF does, and hyperlinks a drawing file gives
//! its entities (`PE_URL` extended data) become PDF link annotations.

use cadcraft_doc::{Common, Drawing, EndStyle, EntityKind, Line, PlotStyle, PlotStyleTable, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{plotter, read_dxf, write_dxf};
use serde_json::{Value, json};

fn line(d: &mut Drawing, a: (f64, f64), b: (f64, f64)) {
    let k = EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
    d.add(&Space::Model, Common::default(), k).unwrap();
}

fn pdf_doc(d: &Drawing, opts: Value) -> crate::pdf_read::Doc {
    let mut o = json!({"compress": false});
    if let (Some(m), Some(e)) = (o.as_object_mut(), opts.as_object()) {
        m.extend(e.clone());
    }
    crate::pdf_read::read(&plotter::plot(d, &Space::Model, &o).unwrap())
}

/// The plot area's clip rectangle (`x y w h`) of the first page.
fn clip(doc: &crate::pdf_read::Doc) -> Vec<f64> {
    crate::pdf_read::clip_rect(doc.content()).map(|r| vec![r[0], r[1], r[2] - r[0], r[3] - r[1]]).unwrap_or_default()
}

/// The link targets of the first page.
fn uris(doc: &crate::pdf_read::Doc) -> Vec<String> {
    doc.pages[0].links.iter().map(|l| l.uri.clone()).collect()
}

#[test]
fn model_space_pdf_uses_the_device_margins() {
    let mut d = Drawing::new_metric();
    line(&mut d, (0.0, 0.0), (100.0, 50.0));
    let mm = 72.0 / 25.4;
    // No device: the page setup's printable area, as before.
    let plain = clip(&pdf_doc(&d, json!({})));
    assert!(plain.first().is_some_and(|x| (x - 20.0 * mm).abs() > 1.0), "{plain:?}");
    // A device paper with 20 mm margins: the printable area starts 20 mm in.
    let r = clip(&pdf_doc(&d, json!({"width": 210, "height": 297, "margins": [20, 20, 20, 20]})));
    assert_eq!(r.len(), 4, "{r:?}");
    assert!((r[0] - 20.0 * mm).abs() < 0.01 && (r[1] - 20.0 * mm).abs() < 0.01, "{r:?}");
    let (w, h) = (r[2].min(r[3]), r[2].max(r[3]));
    assert!((w - 170.0 * mm).abs() < 0.01 && (h - 257.0 * mm).abs() < 0.01, "{r:?}");
    // Hostile margins are ignored.
    for bad in [json!([1e9, 0, 0, 0]), json!([1, 2, 3]), json!("wide"), json!([f64::NAN, 1, 1, 1])] {
        assert_eq!(clip(&pdf_doc(&d, json!({"margins": bad}))), plain, "{bad}");
    }
}

/// A drawing with two lines in a 2 mm wide style with round ends, and a plot style table holding it.
fn styled() -> Drawing {
    let mut d = Drawing::new_metric();
    line(&mut d, (0.0, 0.0), (100.0, 0.0));
    line(&mut d, (0.0, 40.0), (100.0, 60.0));
    let round = PlotStyle { name: "Round".into(), lineweight: Some(200), end: EndStyle::Round, ..PlotStyle::default() };
    d.plot_style_tables.push(PlotStyleTable::named("round.stb", "", vec![round]));
    let hs: Vec<_> = d.model.iter().map(|e| e.handle).collect();
    for h in hs {
        d.modify_entity(h, |e| e.common.plot_style = "Round".into()).unwrap();
    }
    d
}

#[test]
fn png_and_svg_plots_draw_plot_style_ends() {
    let d = styled();
    let png = |table: &str| plotter::plot(&d, &Space::Model, &json!({"output": "png", "resolution": 72, "plotStyleTable": table})).unwrap();
    assert_ne!(png("round.stb"), png("None"), "round ends change the image");

    let svg = |table: &str| String::from_utf8(plotter::plot(&d, &Space::Model, &json!({"output": "svg", "plotStyleTable": table})).unwrap()).unwrap();
    let plain = svg("None");
    assert!(plain.contains("<polyline") && !plain.contains("<path"), "{plain}");
    let round = svg("round.stb");
    assert!(round.contains("<path") && !round.contains("<polyline"), "wide lines with round ends are outlines: {round}");
}

/// DXF text of `d` with `PE_URL` extended data on each LINE, one URL per line in order.
fn with_pe_urls(d: &Drawing, urls: &[&str]) -> String {
    let text = write_dxf(d);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<String> = Vec::new();
    let mut urls = urls.iter();
    let mut in_line = false;
    // Extended data goes last in an entity, before the next `0` group.
    for pair in lines.chunks(2) {
        let [code, value] = pair else { continue };
        if *code == "0" {
            if in_line && let Some(url) = urls.next() {
                for (c, v) in [("1001", "PE_URL"), ("1000", url), ("1002", "{"), ("1000", "A link"), ("1002", "}")] {
                    out.push(c.to_string());
                    out.push(v.to_string());
                }
            }
            in_line = *value == "LINE";
        }
        out.push(code.to_string());
        out.push(value.to_string());
    }
    out.join("\n") + "\n"
}

#[test]
fn pe_url_hyperlinks_become_link_annotations() {
    let mut d = Drawing::new_metric();
    line(&mut d, (0.0, 0.0), (100.0, 0.0));
    line(&mut d, (0.0, 20.0), (100.0, 20.0));
    line(&mut d, (0.0, 40.0), (100.0, 40.0));
    let text = with_pe_urls(&d, &["https://example.com/plan", "javascript:alert(1)", "data:text/html,x"]);
    assert_eq!(text.matches("PE_URL").count(), 3, "the test drawing carries the links");
    let read = read_dxf(text.as_bytes()).unwrap();
    let pdf = uris(&pdf_doc(&read, json!({})));
    assert_eq!(pdf, ["https://example.com/plan"], "only the web link");
    // Turned off, and given explicitly for the same object (the explicit one wins).
    assert!(uris(&pdf_doc(&read, json!({"hyperlinks": false}))).is_empty());
    let first = read.model.iter().next().unwrap().handle.hex();
    let own = uris(&pdf_doc(&read, json!({"links": [{"handle": first, "url": "https://example.org/own"}]})));
    assert!(own.contains(&"https://example.org/own".to_string()) && !own.contains(&"https://example.com/plan".to_string()), "{own:?}");
}
