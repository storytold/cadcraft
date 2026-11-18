//! Reading an SVG whose paths flatten into a huge number of retained vertices must be rejected by
//! the document-wide generated-geometry budget, not amplified into gigabytes (CWE-400, row 31).

use cadcraft_io::read;

/// A path with many elliptical-arc subpaths under a non-uniform group transform; each subpath
/// flushes to its own retained polyline, so enough of them must exceed the document vertex budget.
fn arc_bomb(arcs: usize) -> Vec<u8> {
    // `scale(1 2)` makes the transform non-uniform, forcing per-sample flattening rather than a
    // single bulge arc. Each `M … a …` is a separate subpath that is flushed and retained.
    let mut d = String::new();
    for i in 0..arcs {
        let y = i as i32;
        d.push_str(&format!("M 0 {y} a 1000 500 0 1 1 1 1 "));
    }
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><g transform="scale(1 2)"><path d="{d}"/></g></svg>"#);
    svg.into_bytes()
}

/// A small transformed-arc input remains accepted within the generated-geometry budget.
#[test]
fn a_modest_arc_svg_still_reads() {
    // A handful of arcs is well within budget and parses fine.
    let d = read(&arc_bomb(4), "a.svg").expect("a small arc SVG should read");
    assert!(d.model.iter().count() >= 1, "expected some geometry");
}

/// Repeated flattened arcs fail with a geometry-budget error before retaining excessive vertices.
#[test]
fn an_arc_heavy_svg_is_rejected_by_the_vertex_budget() {
    // ~1000 arcs x up to 10_000 chords each would far exceed the 8M vertex budget.
    let err = read(&arc_bomb(30000), "a.svg");
    assert!(err.unwrap_err().to_string().contains("SVG: too much generated geometry"));
}
