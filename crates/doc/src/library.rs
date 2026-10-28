//! CADCraft's own standard libraries: linetypes and hatch patterns.
//!
//! These definitions were written for CADCraft (CC0 / public domain as part of this codebase).
//! The names follow the industry-common names users expect (DASHED, CENTER, ANSI31…); the
//! dash and spacing values are our own.

use crate::{DashElement, Drawing, Linetype};

pub mod files;

/// The standard linetype library, defined in inches. Load it into a drawing with
/// [`standard_linetypes_for`], which scales it for metric drawings.
pub fn standard_linetypes() -> Vec<Linetype> {
    let s = |name: &str, desc: &str, d: &[f64]| Linetype::simple(name, desc, d);
    vec![
        s("BORDER", "Border __ __ . __ __ . __ __ .", &[0.5, -0.25, 0.5, -0.25, 0.0, -0.25]),
        s("CENTER", "Center ____ _ ____ _ ____", &[1.25, -0.25, 0.25, -0.25]),
        s("CENTER2", "Center (.5x) ___ _ ___ _", &[0.75, -0.125, 0.125, -0.125]),
        s("CENTERX2", "Center (2x) ________  __  ________", &[2.5, -0.5, 0.5, -0.5]),
        s("DASHDOT", "Dash dot __ . __ . __ .", &[0.5, -0.25, 0.0, -0.25]),
        s("DASHDOT2", "Dash dot (.5x) _._._._.", &[0.25, -0.125, 0.0, -0.125]),
        s("DASHED", "Dashed __ __ __ __", &[0.5, -0.25]),
        s("DASHED2", "Dashed (.5x) _ _ _ _ _", &[0.25, -0.125]),
        s("DASHEDX2", "Dashed (2x) ____  ____  ____", &[1.0, -0.5]),
        s("DIVIDE", "Divide ____ . . ____ . .", &[0.5, -0.25, 0.0, -0.25, 0.0, -0.25]),
        s("DOT", "Dot . . . . . . . .", &[0.0, -0.25]),
        s("DOT2", "Dot (.5x) ........", &[0.0, -0.125]),
        s("HIDDEN", "Hidden __ __ __ __", &[0.25, -0.125]),
        s("HIDDEN2", "Hidden (.5x) _ _ _ _ _", &[0.125, -0.0625]),
        s("HIDDENX2", "Hidden (2x) ____ ____ ____", &[0.5, -0.25]),
        s("PHANTOM", "Phantom ______  __  __  ______", &[1.25, -0.25, 0.25, -0.25, 0.25, -0.25]),
        s("PHANTOM2", "Phantom (.5x) ___ _ _ ___", &[0.625, -0.125, 0.125, -0.125, 0.125, -0.125]),
        Linetype {
            handle: Default::default(),
            name: "FENCELINE1".into(),
            description: "Fenceline circle ----0-----0----".into(),
            pattern: vec![
                DashElement::dash(0.25),
                // The text goes where its element ends: centred between the two gaps.
                DashElement {
                    text: Some("o".into()),
                    style: Some("Standard".into()),
                    scale: 0.1,
                    offset: cadcraft_geom::Vec2::new(-0.035, -0.035),
                    ..DashElement::dash(-0.1)
                },
                DashElement::dash(-0.1),
                DashElement::dash(0.5),
            ],
        },
        s("ZIGZAG", "Zig zag /\\/\\/\\/\\/", &[0.0001, -0.2, 0.0001, -0.2]),
    ]
}

/// The standard linetypes as loaded into `d`. In a metric drawing (`MEASUREMENT` 1) every length
/// (dashes, gaps, text and shape size, offsets) is multiplied by 25.4, so the patterns measure
/// in millimetres what they measure in inches in an imperial drawing (DASHED: 12.7 on, 6.35
/// off) and LTSCALE 1 suits both, as CAD programs load ISO-scaled definitions for metric
/// drawings. Definitions already in a drawing are not changed.
pub fn standard_linetypes_for(d: &Drawing) -> Vec<Linetype> {
    let lib = standard_linetypes();
    if d.header.i64("MEASUREMENT", 0) != 1 {
        return lib;
    }
    lib.into_iter().map(|lt| scaled(lt, 25.4)).collect()
}

/// `lt` with every length multiplied by `k`.
fn scaled(mut lt: Linetype, k: f64) -> Linetype {
    for e in &mut lt.pattern {
        e.length *= k;
        e.offset = e.offset * k;
        if e.text.is_some() || e.shape.is_some() {
            e.scale *= k;
        }
    }
    lt
}

/// A hatch pattern line family: angle (degrees), origin, offset (shift along, spacing), dashes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatternLine {
    pub angle: f64,
    pub origin: (f64, f64),
    pub delta: (f64, f64),
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HatchPattern {
    pub name: &'static str,
    pub description: &'static str,
    pub lines: Vec<PatternLine>,
}

fn pl(angle: f64, origin: (f64, f64), delta: (f64, f64), dashes: &[f64]) -> PatternLine {
    PatternLine { angle, origin, delta, dashes: dashes.to_vec() }
}

/// The standard hatch pattern library.
pub fn standard_patterns() -> Vec<HatchPattern> {
    vec![
        HatchPattern { name: "SOLID", description: "Solid fill", lines: vec![] },
        HatchPattern { name: "ANSI31", description: "Iron, brick, stone masonry", lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.125), &[])] },
        HatchPattern {
            name: "ANSI32",
            description: "Steel",
            lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.375), &[]), pl(45.0, (0.176777, 0.0), (0.0, 0.375), &[])],
        },
        HatchPattern {
            name: "ANSI33",
            description: "Bronze, brass, copper",
            lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.25), &[]), pl(45.0, (0.176777, 0.0), (0.0, 0.25), &[0.125, -0.0625])],
        },
        HatchPattern {
            name: "ANSI34",
            description: "Plastic, rubber",
            lines: (0..4).map(|i| pl(45.0, (0.176777 * f64::from(i) / 2.0, 0.0), (0.0, 0.75), &[])).collect(),
        },
        HatchPattern {
            name: "ANSI35",
            description: "Fire brick, refractory",
            lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.25), &[]), pl(45.0, (0.176777, 0.0), (0.0, 0.25), &[0.3125, -0.0625, 0.0, -0.0625])],
        },
        HatchPattern {
            name: "ANSI36",
            description: "Marble, slate, glass",
            lines: vec![pl(45.0, (0.0, 0.0), (0.21875, 0.125), &[0.3125, -0.0625, 0.0, -0.0625])],
        },
        HatchPattern {
            name: "ANSI37",
            description: "Lead, zinc, magnesium",
            lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(135.0, (0.0, 0.0), (0.0, 0.125), &[])],
        },
        HatchPattern {
            name: "ANSI38",
            description: "Aluminum",
            lines: vec![pl(45.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(135.0, (0.0, 0.0), (0.25, 0.125), &[0.3125, -0.1875])],
        },
        HatchPattern { name: "LINE", description: "Parallel horizontal lines", lines: vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[])] },
        HatchPattern {
            name: "NET",
            description: "Horizontal / vertical grid",
            lines: vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(90.0, (0.0, 0.0), (0.0, 0.125), &[])],
        },
        HatchPattern {
            name: "NET3",
            description: "Network pattern 0-60-120",
            lines: vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(60.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(120.0, (0.0, 0.0), (0.0, 0.125), &[])],
        },
        HatchPattern { name: "DOTS", description: "A series of dots", lines: vec![pl(0.0, (0.0, 0.0), (0.03125, 0.0625), &[0.0, -0.0625])] },
        HatchPattern { name: "DASH", description: "Dashed lines", lines: vec![pl(0.0, (0.0, 0.0), (0.125, 0.125), &[0.125, -0.125])] },
        HatchPattern {
            name: "BRICK",
            description: "Brick or masonry-type surface",
            lines: vec![
                // Running bond: courses 0.25 high, bricks 0.5 long. Each vertical family draws the
                // joints of every other course, 0.5 apart; the second is offset half a brick.
                pl(0.0, (0.0, 0.0), (0.0, 0.25), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.5), &[0.25, -0.25]),
                pl(90.0, (0.25, 0.0), (0.0, 0.5), &[-0.25, 0.25]),
            ],
        },
        HatchPattern {
            name: "SQUARE",
            description: "Small aligned squares",
            lines: vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[0.125, -0.125]), pl(90.0, (0.0, 0.0), (0.0, 0.125), &[0.125, -0.125])],
        },
        HatchPattern {
            name: "CROSS",
            description: "A series of crosses",
            lines: vec![pl(0.0, (0.0, 0.0), (0.25, 0.25), &[0.125, -0.375]), pl(90.0, (0.0625, -0.0625), (0.25, 0.25), &[0.125, -0.375])],
        },
        HatchPattern {
            name: "EARTH",
            description: "Earth or ground (subterranean)",
            lines: vec![
                pl(0.0, (0.0, 0.0), (0.25, 0.25), &[0.25, -0.25]),
                pl(0.0, (0.0, 0.09375), (0.25, 0.25), &[0.25, -0.25]),
                pl(0.0, (0.0, 0.1875), (0.25, 0.25), &[0.25, -0.25]),
                pl(90.0, (0.03125, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
                pl(90.0, (0.125, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
                pl(90.0, (0.21875, 0.21875), (0.25, 0.25), &[0.25, -0.25]),
            ],
        },
        HatchPattern {
            name: "GRASS",
            description: "Grass area",
            lines: vec![
                pl(90.0, (0.0, 0.0), (std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2), &[0.1875, -1.226]),
                pl(45.0, (0.0, 0.0), (0.0, 1.0), &[0.1875, -0.8125]),
                pl(135.0, (0.0, 0.0), (0.0, 1.0), &[0.1875, -0.8125]),
            ],
        },
        HatchPattern {
            name: "HONEY",
            description: "Honeycomb pattern",
            lines: vec![
                pl(0.0, (0.0, 0.0), (0.1875, 0.108253), &[0.125, -0.25]),
                pl(120.0, (0.0, 0.0), (0.1875, 0.108253), &[0.125, -0.25]),
                pl(60.0, (0.125, 0.0), (0.1875, 0.108253), &[0.125, -0.25]),
            ],
        },
        HatchPattern {
            name: "ZIGZAG",
            description: "Staircase effect",
            lines: vec![pl(0.0, (0.0, 0.0), (0.125, 0.125), &[0.125, -0.125]), pl(90.0, (0.125, 0.0), (0.125, 0.125), &[0.125, -0.125])],
        },
        // Drawn at the scale of the pattern CAD programs ship under this name (about 16x
        // coarser than first drawn here: AR-CONC hatches came out hundreds of times too dense).
        scaled_pattern(
            HatchPattern {
                name: "AR-CONC",
                description: "Random dot and stone pattern",
                lines: vec![
                    pl(50.0, (0.0, 0.0), (0.4, 0.25), &[0.15, -0.6]),
                    pl(355.0, (0.0, 0.0), (0.27, 0.3), &[0.12, -0.5]),
                    pl(100.0, (0.12, 0.05), (0.35, 0.33), &[0.1, -0.55]),
                    pl(0.0, (0.07, 0.02), (0.21, 0.17), &[0.0, -0.4]),
                ],
            },
            16.0,
        ),
        // Drawn at the scale of the pattern CAD programs ship under this name (about 16x
        // coarser than first drawn here: AR-CONC hatches came out hundreds of times too dense).
        scaled_pattern(
            HatchPattern {
                name: "AR-SAND",
                description: "Random dot pattern",
                lines: vec![
                    pl(37.5, (0.0, 0.0), (0.112, 0.0912), &[0.0, -0.22, 0.0, -0.14]),
                    pl(7.5, (0.0, 0.0), (0.157, 0.0724), &[0.0, -0.19, 0.0, -0.31]),
                    pl(-32.5, (0.0, 0.0), (0.091, 0.1312), &[0.0, -0.27, 0.0, -0.11]),
                ],
            },
            16.0,
        ),
    ]
}

pub fn pattern(name: &str) -> Option<HatchPattern> {
    standard_patterns().into_iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// The library pattern `name` as drawn in `d`. In a metric drawing (`MEASUREMENT` 1) every
/// length is multiplied by 25.4, as CAD programs use ISO-scaled pattern definitions there (as
/// for linetypes, see [`standard_linetypes_for`]): a hatch at scale 1 measures in millimetres
/// what it measures in inches in an imperial drawing.
pub fn pattern_for(d: &Drawing, name: &str) -> Option<HatchPattern> {
    let p = pattern(name)?;
    Some(if d.header.i64("MEASUREMENT", 0) == 1 { scaled_pattern(p, 25.4) } else { p })
}

/// `p` with every length (origins, offsets, dashes) multiplied by `k`.
fn scaled_pattern(mut p: HatchPattern, k: f64) -> HatchPattern {
    for l in &mut p.lines {
        l.origin = (l.origin.0 * k, l.origin.1 * k);
        l.delta = (l.delta.0 * k, l.delta.1 * k);
        for d in &mut l.dashes {
            *d *= k;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_drawings_draw_iso_scaled_patterns() {
        let imp = pattern_for(&Drawing::new_imperial(), "ANSI31").map(|p| p.lines[0].delta.1);
        let met = pattern_for(&Drawing::new_metric(), "ansi31").map(|p| p.lines[0].delta.1);
        assert_eq!(imp, Some(0.125));
        assert!(met.is_some_and(|m| (m - 0.125 * 25.4).abs() < 1e-12), "{met:?}");
        assert!(pattern_for(&Drawing::new_metric(), "NOPE").is_none());
    }
}
