//! MTEXT paragraph formatting and columns in the layout (#532): `\p` indents, tabs, lists,
//! alignment, column flow and column breaks, and hostile codes.

use cadcraft_fonts::{MTextColumnSpec, MTextLayout, MTextParams, layout_mtext_with};
use cadcraft_geom::Vec2;

fn lay(s: &str, width: f64, columns: Option<MTextColumnSpec>) -> MTextLayout {
    layout_mtext_with(s, &MTextParams { width, columns, ..MTextParams::new(Vec2::ZERO, 1.0) })
}

fn xs(l: &MTextLayout) -> Vec<f64> {
    l.strokes.iter().flatten().map(|p| p.x).collect()
}

fn min_x(l: &MTextLayout) -> f64 {
    xs(l).into_iter().fold(f64::MAX, f64::min)
}

fn max_x(l: &MTextLayout) -> f64 {
    xs(l).into_iter().fold(f64::MIN, f64::max)
}

/// Leftmost x of the geometry of line `n` (0 = first; baselines 5/3 apart below -1).
fn line_min_x(l: &MTextLayout, n: usize) -> f64 {
    let base = -1.0 - n as f64 * 5.0 / 3.0;
    l.strokes.iter().flatten().filter(|p| p.y >= base - 0.4 && p.y <= base + 1.3).map(|p| p.x).fold(f64::MAX, f64::min)
}

#[test]
fn left_indent_moves_every_line() {
    let plain = lay("indented text", 0.0, None);
    let ind = lay("\\pxl2;indented text", 0.0, None);
    assert!((min_x(&ind) - min_x(&plain) - 2.0).abs() < 1e-9);
}

#[test]
fn native_absolute_indents_are_independent_of_text_height() {
    for height in [2.0, 4.0] {
        let params = MTextParams::new(Vec2::ZERO, height);
        let plain = layout_mtext_with("LABEL", &params);
        for (code, offset) in [("\\pi10;", 10.0), ("\\pxi10;", 10.0 * height), ("\\pxl2;\\pi3;", 2.0 * height + 3.0)] {
            let indented = layout_mtext_with(&format!("{code}LABEL"), &params);
            assert!((min_x(&indented) - min_x(&plain) - offset).abs() < 1e-9, "height={height}, {code}");
        }
        let absolute_tab = layout_mtext_with("\\pt10;a\tb", &params);
        assert!(xs(&absolute_tab).iter().any(|x| (*x - 10.0).abs() < 0.3));
    }
}

#[test]
fn hanging_indent_list_aligns_wrapped_lines_after_the_label() {
    // Label at 0, text at the tab stop 3, wrapped lines at the left indent 3.
    let l = lay("\\pxi-3,l3,t3;1.\tthe quick brown fox jumps over the lazy dog", 12.0, None);
    assert!(l.lines.len() >= 2, "{:?}", l.lines);
    assert!(line_min_x(&l, 0).abs() < 0.3, "label at the left edge");
    assert!((line_min_x(&l, 1) - 3.0).abs() < 0.3, "wrapped line at the indent: {}", line_min_x(&l, 1));
    let after_tab = l.strokes.iter().flatten().filter(|p| p.y > -1.2 && p.x > 1.8).map(|p| p.x).fold(f64::MAX, f64::min);
    assert!((after_tab - 3.0).abs() < 0.3, "{after_tab}");
}

#[test]
fn tabs_go_to_the_stops_then_every_default_interval() {
    let l = lay("\\pxt5;a\tb\tc", 0.0, None);
    let xs = xs(&l);
    // `b` at the stop 5, `c` at the next default stop (8 = 2 × 4 text heights).
    assert!(xs.iter().any(|x| (*x - 5.0).abs() < 0.3));
    assert!(xs.iter().any(|x| (*x - 8.0).abs() < 0.3));
    assert!(!xs.iter().any(|x| *x > 1.0 && *x < 4.9));
}

#[test]
fn paragraph_alignment_overrides_the_attachment() {
    let right = lay("\\pqr;ab", 10.0, None);
    assert!(max_x(&right) > 9.0 && max_x(&right) <= 10.0 + 1e-9, "{}", max_x(&right));
    let centre = lay("\\pqc;ab", 10.0, None);
    let c = (min_x(&centre) + max_x(&centre)) / 2.0;
    assert!((c - 5.0).abs() < 0.5, "{c}");
}

#[test]
fn justified_lines_fill_the_width_but_the_last() {
    let l = lay("\\pqj;aa bb cc dd ee ff gg hh ii jj kk", 10.0, None);
    assert!(l.lines.len() >= 2);
    let first_line_max = l.strokes.iter().flatten().filter(|p| p.y > -1.3).map(|p| p.x).fold(f64::MIN, f64::max);
    assert!(first_line_max > 9.5, "{first_line_max}");
}

#[test]
fn text_flows_through_static_columns() {
    let spec = MTextColumnSpec { count: 3, gutter: 2.0, height: 6.0, ..Default::default() };
    // Two words a line, four lines a column (1 + 3 × 5/3 = 6): 20 words fill two and a half.
    let l = lay(&"word ".repeat(20), 10.0, Some(spec.clone()));
    assert!((l.bounds.width() - 34.0).abs() < 1e-9);
    assert!((l.bounds.height() - 6.0).abs() < 1e-9);
    for col in 0..3 {
        let x0 = col as f64 * 12.0;
        assert!(xs(&l).iter().any(|x| *x >= x0 && *x <= x0 + 10.0), "column {col} has text");
    }
    // The gutters stay empty.
    assert!(!xs(&l).iter().any(|x| *x > 10.0 + 1e-9 && *x < 12.0 - 1e-9));
    // Text past the last static column runs on below it.
    let over = lay(&"word ".repeat(40), 10.0, Some(spec));
    assert!(over.bounds.height() > 6.0 && (over.bounds.width() - 34.0).abs() < 1e-9);
}

#[test]
fn dynamic_columns_add_columns_and_column_breaks_force_one() {
    let spec = MTextColumnSpec { dynamic: true, gutter: 1.0, height: 3.0, ..Default::default() };
    // Two lines fit in 3 (1 + 5/3): five paragraphs take three columns.
    let l = lay("one\\Ptwo\\Pthree\\Pfour\\Pfive", 5.0, Some(spec.clone()));
    assert!((l.bounds.width() - 17.0).abs() < 1e-9, "{:?}", l.bounds);
    let b = lay("one\\Ntwo", 5.0, Some(spec));
    assert!((b.bounds.width() - 11.0).abs() < 1e-9, "the column break starts a second column");
}

#[test]
fn reversed_columns_run_right_to_left() {
    let spec = MTextColumnSpec { count: 2, height: 1.0, reversed: true, ..Default::default() };
    let l = lay("first\\Nsecond", 10.0, Some(spec));
    assert_eq!(l.lines, vec!["first", "second"]);
    let first = l.strokes.iter().flatten().filter(|p| p.x >= 10.0).count();
    let second = l.strokes.iter().flatten().filter(|p| p.x < 10.0).count();
    assert!(first > 0 && second > 0 && min_x(&l) >= 0.0);
}

#[test]
fn hostile_paragraph_codes_do_not_panic() {
    assert_eq!(cadcraft_fonts::ParaProps::default().length(2.0, u8::MAX, 3.0), 6.0);
    let huge_tabs = format!("\\pt{};a\tb", (0..100_000).map(|i| i.to_string()).collect::<Vec<_>>().join(","));
    let many = "\\pl1;x\\P".repeat(20_000);
    for s in [
        "\\p",
        "\\pxi",
        "\\pi-1e308,l1e308,r-5;x",
        "\\pt;\t\t\t",
        "\\pqz,t-3,cx,r;a\tb",
        "\\pxl99999999;a b c",
        "\t",
        "\\N\\N\\N",
        "{\\pl2;\\P}}}\\p;",
        huge_tabs.as_str(),
        many.as_str(),
    ] {
        let _ = lay(s, 5.0, None);
        let _ = lay(s, 5.0, Some(MTextColumnSpec { dynamic: true, height: 1.0, ..Default::default() }));
        let _ = lay(
            s,
            1.0,
            Some(MTextColumnSpec { count: 1000, gutter: f64::NAN, height: f64::INFINITY, heights: vec![f64::NAN, -1.0], ..Default::default() }),
        );
    }
}
