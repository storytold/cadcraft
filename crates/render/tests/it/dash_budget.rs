//! A small file of many long lines on a pattern of mostly gaps (#1923, F-05 f). Laying a run out
//! visits every element of every period, two million here, but draws one dash in 256 of them, so
//! the point budget (`MAX_DISPLAY_POINTS`) never stops the build: each line used to cost two
//! million steps. The build's dash budget lays a few dozen out and draws the rest continuous.

use cadcraft_doc::{Common, DashElement, Drawing, EntityKind, Handle, Line, Linetype, Space};
use cadcraft_geom::Vec3;
use cadcraft_render::{DisplayList, Options, build_plot};

const LINES: usize = 1000;

/// `LINES` lines 1,999 long on a pattern of 255 gaps and a dash, each 0.001: 7,808 periods of 256
/// elements a line, just under the two million one run may be laid out over.
fn sparse() -> Drawing {
    let mut d = Drawing::new_imperial();
    let pattern = (0..256).map(|i| DashElement::dash(if i == 255 { 0.001 } else { -0.001 })).collect();
    d.linetypes.push(Linetype { name: "SPARSE".into(), description: String::new(), pattern });
    for row in 0..LINES {
        let y = row as f64;
        let line = EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(1999.0, y, 0.0) });
        d.add(&Space::Model, Common { linetype: "SPARSE".into(), ..Default::default() }, line).unwrap();
    }
    d
}

/// The primitives drawn for each top-level entity, in drawing order.
fn prims_per_entity(l: &DisplayList) -> Vec<usize> {
    let mut out: Vec<(Handle, usize)> = Vec::new();
    for p in &l.prims {
        match out.last_mut() {
            Some((h, n)) if *h == p.handle => *n += 1,
            _ => out.push((p.handle, 1)),
        }
    }
    out.into_iter().map(|(_, n)| n).collect()
}

#[test]
fn many_long_runs_of_a_pattern_of_gaps_stop_at_the_dash_budget() {
    let l = build_plot(&sparse(), &Space::Model, &Options::default());
    let each = prims_per_entity(&l);
    let dashed = each.iter().filter(|n| **n > 1).count();
    assert_eq!(each.len(), LINES);
    // Every line used to be laid out; a few dozen fit the budget and the rest are one stroke.
    assert!(dashed > 0 && dashed * 4 < LINES, "{dashed} of {LINES} lines dashed");
    assert!(each.iter().skip(dashed).all(|n| *n == 1), "{each:?}");
    assert!(l.truncated, "the lines drawn continuous are reported");
}
