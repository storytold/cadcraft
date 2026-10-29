//! CADCraft's own standard libraries: linetypes and hatch patterns.
//!
//! These definitions were written for CADCraft (CC0 / public domain as part of this codebase).
//! The names follow the industry-common names users expect (DASHED, CENTER, ANSI31…); the
//! dash and spacing values are our own.

use crate::{DashElement, Drawing, Linetype};

pub mod files;

/// One ISO 128-20 element length, in millimetres for a 1.00 mm pen (`d`): a dot is at most half
/// the pen width, a gap 3d, a short dash 6d, a dash 12d, a long dash 24d and a long gap 18d.
/// The library is in inches, so these are divided by 25.4: a metric drawing (which loads the
/// library multiplied by 25.4, see [`standard_linetypes_for`]) gets exactly the ISO lengths.
const ISO_DOT: f64 = 0.5 / 25.4;
const ISO_GAP: f64 = -3.0 / 25.4;
const ISO_SHORT: f64 = 6.0 / 25.4;
const ISO_DASH: f64 = 12.0 / 25.4;
const ISO_LONG: f64 = 24.0 / 25.4;
const ISO_SPACE: f64 = -18.0 / 25.4;

/// A text linetype: a dash, then `text` centred in a gap of `gap` each side.
fn text_linetype(name: &str, desc: &str, dash: f64, text: &str, gap: f64, half_width: f64) -> Linetype {
    Linetype {
        name: name.into(),
        description: desc.into(),
        pattern: vec![
            DashElement::dash(dash),
            DashElement {
                text: Some(text.into()),
                style: Some("Standard".into()),
                scale: 0.1,
                offset: cadcraft_geom::Vec2::new(-half_width, -0.05),
                ..DashElement::dash(-gap)
            },
            DashElement::dash(-gap),
        ],
    }
}

/// The standard linetype library, defined in inches. Load it into a drawing with
/// [`standard_linetypes_for`], which scales it for metric drawings.
pub fn standard_linetypes() -> Vec<Linetype> {
    let s = |name: &str, desc: &str, d: &[f64]| Linetype::simple(name, desc, d);
    let (dot, gap, short, dash, long, space) = (ISO_DOT, ISO_GAP, ISO_SHORT, ISO_DASH, ISO_LONG, ISO_SPACE);
    vec![
        s("BORDER", "Border __ __ . __ __ . __ __ .", &[0.5, -0.25, 0.5, -0.25, 0.0, -0.25]),
        s("BORDER2", "Border, half size: two dashes and a dot", &[0.25, -0.125, 0.25, -0.125, 0.0, -0.125]),
        s("BORDERX2", "Border, double size: two dashes and a dot", &[1.0, -0.5, 1.0, -0.5, 0.0, -0.5]),
        s("CENTER", "Center ____ _ ____ _ ____", &[1.25, -0.25, 0.25, -0.25]),
        s("CENTER2", "Center (.5x) ___ _ ___ _", &[0.75, -0.125, 0.125, -0.125]),
        s("CENTERX2", "Center (2x) ________  __  ________", &[2.5, -0.5, 0.5, -0.5]),
        s("DASHDOT", "Dash dot __ . __ . __ .", &[0.5, -0.25, 0.0, -0.25]),
        s("DASHDOT2", "Dash dot (.5x) _._._._.", &[0.25, -0.125, 0.0, -0.125]),
        s("DASHDOTX2", "Dash dot, double size", &[1.0, -0.5, 0.0, -0.5]),
        s("DASHED", "Dashed __ __ __ __", &[0.5, -0.25]),
        s("DASHED2", "Dashed (.5x) _ _ _ _ _", &[0.25, -0.125]),
        s("DASHEDX2", "Dashed (2x) ____  ____  ____", &[1.0, -0.5]),
        s("DIVIDE", "Divide ____ . . ____ . .", &[0.5, -0.25, 0.0, -0.25, 0.0, -0.25]),
        s("DIVIDE2", "Divide, half size: a dash and two dots", &[0.25, -0.125, 0.0, -0.125, 0.0, -0.125]),
        s("DIVIDEX2", "Divide, double size: a dash and two dots", &[1.0, -0.5, 0.0, -0.5, 0.0, -0.5]),
        s("DOT", "Dot . . . . . . . .", &[0.0, -0.25]),
        s("DOT2", "Dot (.5x) ........", &[0.0, -0.125]),
        s("DOTX2", "Dot, double spacing", &[0.0, -0.5]),
        s("HIDDEN", "Hidden __ __ __ __", &[0.25, -0.125]),
        s("HIDDEN2", "Hidden (.5x) _ _ _ _ _", &[0.125, -0.0625]),
        s("HIDDENX2", "Hidden (2x) ____ ____ ____", &[0.5, -0.25]),
        s("PHANTOM", "Phantom ______  __  __  ______", &[1.25, -0.25, 0.25, -0.25, 0.25, -0.25]),
        s("PHANTOM2", "Phantom (.5x) ___ _ _ ___", &[0.625, -0.125, 0.125, -0.125, 0.125, -0.125]),
        s("PHANTOMX2", "Phantom, double size: a long dash and two short ones", &[2.5, -0.5, 0.5, -0.5, 0.5, -0.5]),
        // ISO 128-20 line types 02 to 15 for a 1.00 mm pen (W100), built from the standard's
        // element lengths above.
        s("ISO02W100", "ISO 128 type 02: dashed", &[dash, gap]),
        s("ISO03W100", "ISO 128 type 03: dashed, long gaps", &[dash, space]),
        s("ISO04W100", "ISO 128 type 04: long dash, dot", &[long, gap, dot, gap]),
        s("ISO05W100", "ISO 128 type 05: long dash, two dots", &[long, gap, dot, gap, dot, gap]),
        s("ISO06W100", "ISO 128 type 06: long dash, three dots", &[long, gap, dot, gap, dot, gap, dot, gap]),
        s("ISO07W100", "ISO 128 type 07: dotted", &[dot, gap]),
        s("ISO08W100", "ISO 128 type 08: long dash, short dash", &[long, gap, short, gap]),
        s("ISO09W100", "ISO 128 type 09: long dash, two short dashes", &[long, gap, short, gap, short, gap]),
        s("ISO10W100", "ISO 128 type 10: dash, dot", &[dash, gap, dot, gap]),
        s("ISO11W100", "ISO 128 type 11: two dashes, dot", &[dash, gap, dash, gap, dot, gap]),
        s("ISO12W100", "ISO 128 type 12: dash, two dots", &[dash, gap, dot, gap, dot, gap]),
        s("ISO13W100", "ISO 128 type 13: two dashes, two dots", &[dash, gap, dash, gap, dot, gap, dot, gap]),
        s("ISO14W100", "ISO 128 type 14: dash, three dots", &[dash, gap, dot, gap, dot, gap, dot, gap]),
        s("ISO15W100", "ISO 128 type 15: two dashes, three dots", &[dash, gap, dash, gap, dot, gap, dot, gap, dot, gap]),
        Linetype {
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
        text_linetype("GAS_LINE", "Gas pipe: dashes broken by the word GAS", 0.5, "GAS", 0.2, 0.12),
        text_linetype("HOT_WATER_SUPPLY", "Hot water supply pipe: dashes broken by HW", 0.5, "HW", 0.15, 0.08),
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

/// The standard hatch pattern library: the patterns first drafted for CADCraft, then the
/// material, masonry, paving and symbol patterns added with it (`more_patterns`).
pub fn standard_patterns() -> Vec<HatchPattern> {
    LIBRARY.clone()
}

/// The library, built once: every lookup of a hatch's pattern while drawing reads it.
static LIBRARY: std::sync::LazyLock<Vec<HatchPattern>> = std::sync::LazyLock::new(|| {
    let mut v = first_patterns();
    v.extend(more_patterns());
    v
});

fn first_patterns() -> Vec<HatchPattern> {
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

fn gcd(a: i32, b: i32) -> i32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// A segment of a [`motif`]: its two ends in grid steps inside the motif's cell.
type Seg = ((i32, i32), (i32, i32));

/// The edges of the closed polygon through `pts` (grid steps), as [`motif`] segments.
fn ring(pts: &[(i32, i32)]) -> Vec<Seg> {
    pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect()
}

/// Dots at `pts` (grid steps), as [`motif`] segments.
fn dots(pts: &[(i32, i32)]) -> Vec<Seg> {
    pts.iter().map(|p| (*p, *p)).collect()
}

/// Line families that repeat a drawing of short segments on a square lattice of side `cell`.
/// Each segment runs between two points given in grid steps of `cell / steps` (a segment whose
/// ends meet is a dot). Its direction is a whole step (p, q), so the lattice repeats along it
/// every `cell × |(p, q)|` and its parallel copies are `cell / |(p, q)|` apart: one family per
/// segment, offset to the next copy by a lattice step D with det((p, q), D) = 1.
fn motif(cell: f64, steps: i32, segments: &[Seg]) -> Vec<PatternLine> {
    let unit = cell / f64::from(steps.max(1));
    segments
        .iter()
        .filter_map(|&((x0, y0), (x1, y1))| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let g = gcd(dx.abs(), dy.abs());
            let (p, q) = if g == 0 { (1, 0) } else { (dx / g, dy / g) };
            let (r, t) = (-8..=8).flat_map(|r| (-8..=8).map(move |t| (r, t))).find(|&(r, t)| p * t - q * r == 1)?;
            let len = f64::from(p * p + q * q).sqrt();
            let (ux, uy) = (f64::from(p) / len, f64::from(q) / len);
            let (sx, sy) = (f64::from(r) * cell, f64::from(t) * cell);
            let period = cell * len;
            let dash = f64::from(g) * unit * len;
            Some(PatternLine {
                angle: f64::from(q).atan2(f64::from(p)).to_degrees(),
                origin: (f64::from(x0) * unit, f64::from(y0) * unit),
                delta: (sx * ux + sy * uy, sy * ux - sx * uy),
                dashes: vec![dash, dash - period],
            })
        })
        .collect()
}

/// A zigzag of period `p` (along x) and height `h`, repeated every `s` upwards (`2h / s` a whole
/// number): the rising strokes and the falling strokes are a family each, whose lines meet the
/// next stroke of their kind every two stroke lengths.
fn zigzag(p: f64, h: f64, s: f64) -> Vec<PatternLine> {
    let a = h.atan2(p / 2.0);
    let l = (p / 2.0).hypot(h);
    vec![
        pl(a.to_degrees(), (0.0, 0.0), (s * a.sin(), s * a.cos()), &[l, -l]),
        pl(-a.to_degrees(), (p / 2.0, h), (-s * a.sin(), s * a.cos()), &[l, -l]),
    ]
}

/// Courses `h` high of units `l` long, each course's joints half a unit along from the last
/// (running bond).
fn running_bond(h: f64, l: f64) -> Vec<PatternLine> {
    vec![pl(0.0, (0.0, 0.0), (0.0, h), &[]), pl(90.0, (0.0, 0.0), (0.0, l), &[h, -h]), pl(90.0, (l / 2.0, h), (0.0, l), &[h, -h])]
}

fn hp(name: &'static str, description: &'static str, lines: Vec<PatternLine>) -> HatchPattern {
    HatchPattern { name, description, lines }
}

/// The patterns added to the first library: materials, masonry and paving, floors, grids and
/// symbols. All are CADCraft's own designs (lengths in inches, like the rest of the library).
fn more_patterns() -> Vec<HatchPattern> {
    let diag = std::f64::consts::FRAC_1_SQRT_2;
    vec![
        // Materials.
        hp(
            "STEEL",
            "Steel section: paired diagonal lines",
            vec![pl(45.0, (0.0, 0.0), (0.0, 0.25), &[]), pl(45.0, (0.0, 0.05 / diag), (0.0, 0.25), &[])],
        ),
        hp("INSULATION", "Thermal insulation: zigzag bands", zigzag(0.25, 0.1875, 0.375)),
        hp("CONCRETE", "Concrete: aggregate triangles and dots", {
            let mut s = ring(&[(2, 2), (6, 3), (3, 6)]);
            s.extend(ring(&[(10, 9), (13, 8), (12, 12)]));
            s.extend(dots(&[(8, 3), (13, 4), (4, 12), (7, 10), (14, 14), (1, 8), (10, 5)]));
            motif(0.5, 16, &s)
        }),
        hp("GRAVEL", "Gravel: scattered pebbles", {
            let mut s = ring(&[(1, 1), (4, 0), (6, 2), (5, 5), (2, 5), (0, 3)]);
            s.extend(ring(&[(9, 9), (11, 8), (13, 10), (12, 12), (9, 12)]));
            s.extend(ring(&[(3, 10), (5, 10), (6, 12), (4, 13), (2, 12)]));
            s.extend(ring(&[(12, 1), (14, 2), (14, 4), (12, 4), (11, 2)]));
            motif(0.5, 16, &s)
        }),
        hp(
            "SAND",
            "Sand: fine irregular stipple",
            motif(0.25, 16, &dots(&[(1, 2), (6, 1), (11, 3), (14, 7), (3, 8), (8, 6), (12, 12), (5, 13), (9, 10), (2, 15)])),
        ),
        hp(
            "ROCK",
            "Rock: irregular cracks",
            motif(
                0.5,
                16,
                &[
                    ((1, 1), (4, 3)),
                    ((4, 3), (5, 6)),
                    ((5, 6), (9, 7)),
                    ((9, 7), (12, 6)),
                    ((12, 6), (15, 9)),
                    ((8, 9), (9, 13)),
                    ((2, 12), (6, 14)),
                    ((12, 1), (14, 4)),
                ],
            ),
        ),
        hp(
            "CLAY",
            "Clay or ceramic: line pairs with a dashed line",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.1875), &[]),
                pl(0.0, (0.0, 0.03125), (0.0, 0.1875), &[]),
                pl(0.0, (0.0, 0.09375), (0.0625, 0.1875), &[0.0625, -0.0625]),
            ],
        ),
        hp(
            "TIMBER",
            "Timber along the grain: broken lines",
            vec![pl(0.0, (0.0, 0.0), (0.35, 0.0625), &[0.8, -0.05, 0.3, -0.1]), pl(0.0, (0.2, 0.03125), (0.55, 0.125), &[0.5, -0.08, 0.15, -0.12])],
        ),
        hp(
            "GLASS",
            "Glass: groups of short diagonal strokes",
            motif(0.5, 16, &[((2, 2), (5, 5)), ((4, 2), (7, 5)), ((6, 2), (8, 4)), ((10, 9), (13, 12)), ((12, 9), (15, 12))]),
        ),
        hp(
            "WATER",
            "Water or liquid: staggered horizontal dashes",
            motif(
                0.5,
                16,
                &[
                    ((1, 2), (6, 2)),
                    ((9, 2), (11, 2)),
                    ((4, 6), (8, 6)),
                    ((11, 7), (15, 7)),
                    ((3, 10), (7, 10)),
                    ((10, 12), (15, 12)),
                    ((1, 14), (3, 14)),
                ],
            ),
        ),
        hp(
            "MARSH",
            "Marsh or wetland: ground lines with tufts",
            motif(
                0.5,
                16,
                &[
                    ((1, 4), (7, 4)),
                    ((4, 4), (4, 7)),
                    ((4, 4), (2, 6)),
                    ((4, 4), (6, 6)),
                    ((9, 12), (15, 12)),
                    ((12, 12), (12, 15)),
                    ((12, 12), (10, 14)),
                    ((12, 12), (14, 14)),
                    ((10, 3), (13, 3)),
                    ((2, 13), (5, 13)),
                ],
            ),
        ),
        // Masonry, paving and roofing.
        hp("BRICK_STACK", "Brick, stack bond", vec![pl(0.0, (0.0, 0.0), (0.0, 0.25), &[]), pl(90.0, (0.0, 0.0), (0.0, 0.5), &[])]),
        hp(
            "BRICK_ENGLISH",
            "Brick, English bond: stretcher and header courses",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.125), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.5), &[0.125, -0.125]),
                pl(90.0, (0.125, 0.125), (0.0, 0.25), &[0.125, -0.125]),
            ],
        ),
        hp(
            "BRICK_FLEMISH",
            "Brick, Flemish bond: stretchers and headers in every course",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.125), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.75), &[0.125, -0.125]),
                pl(90.0, (0.5, 0.0), (0.0, 0.75), &[0.125, -0.125]),
                pl(90.0, (0.125, 0.125), (0.0, 0.75), &[0.125, -0.125]),
                pl(90.0, (0.375, 0.125), (0.0, 0.75), &[0.125, -0.125]),
            ],
        ),
        hp(
            "ASHLAR",
            "Ashlar: coursed stone of two heights",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.375), &[]),
                pl(0.0, (0.0, 0.25), (0.0, 0.375), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.5), &[0.25, -0.125]),
                pl(90.0, (0.1875, 0.25), (0.0, 0.375), &[0.125, -0.25]),
            ],
        ),
        hp(
            "FLAGSTONE",
            "Flagstone or crazy paving: irregular stones",
            motif(
                1.0,
                8,
                &[
                    ((0, 0), (3, 1)),
                    ((3, 1), (6, 0)),
                    ((6, 0), (8, 0)),
                    ((0, 0), (1, 3)),
                    ((1, 3), (0, 5)),
                    ((0, 5), (0, 8)),
                    ((3, 1), (4, 4)),
                    ((1, 3), (4, 4)),
                    ((4, 4), (7, 5)),
                    ((7, 5), (8, 5)),
                    ((6, 0), (7, 5)),
                    ((0, 5), (3, 7)),
                    ((3, 7), (4, 4)),
                    ((3, 7), (3, 9)),
                    ((7, 5), (6, 8)),
                ],
            ),
        ),
        hp("SETTS", "Stone setts or cobbles, running bond", running_bond(0.125, 0.15)),
        hp("ROOF_TILES", "Roof tiles: overlapping courses", running_bond(0.15, 0.2)),
        hp(
            "SHINGLES",
            "Shingles or shakes: irregular joints",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.25), &[]),
                pl(90.0, (0.0, 0.0), (-0.75, 0.0625), &[0.25, -1.75]),
                pl(90.0, (0.3125, 0.0), (-0.75, 0.0625), &[0.25, -1.75]),
            ],
        ),
        // Floors and boarding.
        hp(
            "TILE",
            "Square floor tiles with joints",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.5), &[]),
                pl(0.0, (0.0, 0.03), (0.0, 0.5), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.5), &[]),
                pl(90.0, (0.03, 0.0), (0.0, 0.5), &[]),
            ],
        ),
        hp(
            "OCTAGON",
            "Octagonal tiles with square insets",
            motif(0.375, 6, &[((2, 0), (4, 0)), ((4, 0), (6, 2)), ((6, 4), (4, 6)), ((2, 6), (0, 4)), ((0, 2), (2, 0)), ((0, 2), (0, 4))]),
        ),
        hp(
            "HERRINGBONE",
            "Herringbone: bricks or blocks at right angles",
            vec![pl(0.0, (-0.125, 0.0), (0.125, 0.125), &[0.375, -0.125]), pl(90.0, (0.0, 0.0), (-0.125, 0.125), &[0.375, -0.125])],
        ),
        hp(
            "PARQUET",
            "Parquet, basket weave",
            vec![
                pl(0.0, (0.0, 0.0), (0.0, 0.375), &[]),
                pl(90.0, (0.0, 0.0), (0.0, 0.375), &[]),
                pl(0.0, (0.0, 0.125), (0.375, 0.375), &[0.375, -0.375]),
                pl(0.0, (0.0, 0.25), (0.375, 0.375), &[0.375, -0.375]),
                pl(90.0, (0.125, 0.375), (0.375, 0.375), &[0.375, -0.375]),
                pl(90.0, (0.25, 0.375), (0.375, 0.375), &[0.375, -0.375]),
            ],
        ),
        hp(
            "BOARDS",
            "Boards or decking: staggered butt joints",
            vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(90.0, (0.0, 0.0), (-0.375, 0.125), &[0.125, -0.875])],
        ),
        hp("GRATING", "Bar grating", vec![pl(90.0, (0.0, 0.0), (0.0, 0.0625), &[]), pl(0.0, (0.0, 0.0), (0.0, 0.5), &[])]),
        hp(
            "CHECKER_PLATE",
            "Checker plate: raised bars, alternating",
            motif(0.25, 8, &[((1, 1), (3, 3)), ((5, 5), (7, 7)), ((5, 3), (7, 1)), ((1, 7), (3, 5))]),
        ),
        // Grids and line patterns.
        hp("CROSSHATCH", "Crosshatch: diagonal square grid", vec![pl(45.0, (0.0, 0.0), (0.0, 0.25), &[]), pl(135.0, (0.0, 0.0), (0.0, 0.25), &[])]),
        hp("DIAMOND", "Diamond lattice", vec![pl(30.0, (0.0, 0.0), (0.0, 0.125), &[]), pl(150.0, (0.0, 0.0), (0.0, 0.125), &[])]),
        hp(
            "LATTICE",
            "Lattice or trellis: diagonal double lines",
            vec![
                pl(45.0, (0.0, 0.0), (0.0, 0.5 * diag), &[]),
                pl(45.0, (0.0, 0.025 / diag), (0.0, 0.5 * diag), &[]),
                pl(135.0, (0.0, 0.0), (0.0, 0.5 * diag), &[]),
                pl(135.0, (0.0, 0.025 / diag), (0.0, 0.5 * diag), &[]),
            ],
        ),
        hp("MESH", "Fine square mesh, wire cloth", vec![pl(0.0, (0.0, 0.0), (0.0, 0.0625), &[]), pl(90.0, (0.0, 0.0), (0.0, 0.0625), &[])]),
        hp(
            "GRID_DASHED",
            "Dashed square grid, open at the crossings",
            vec![pl(0.0, (0.03125, 0.0), (0.0, 0.25), &[0.1875, -0.0625]), pl(90.0, (0.0, 0.03125), (0.0, 0.25), &[0.1875, -0.0625])],
        ),
        hp("DOTGRID", "Dots on a square grid", vec![pl(0.0, (0.0, 0.0), (0.0, 0.125), &[0.0, -0.125])]),
        hp("DASH45", "Dashed diagonal lines", vec![pl(45.0, (0.0, 0.0), (0.0625, 0.125), &[0.125, -0.0625])]),
        hp("CHEVRON", "Chevrons: nested zigzag lines", zigzag(0.5, 0.25, 0.125)),
        hp("SAWTOOTH", "Sawtooth or corrugated sheet", zigzag(0.25, 0.0625, 0.125)),
        // Symbols.
        hp("TRIANGLES", "Small triangles", motif(0.25, 8, &ring(&[(1, 1), (7, 1), (4, 7)]))),
        hp("CROSS_DIAGONAL", "Diagonal crosses", motif(0.25, 8, &[((2, 2), (6, 6)), ((2, 6), (6, 2))])),
    ]
}

pub fn pattern(name: &str) -> Option<HatchPattern> {
    LIBRARY.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
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

    #[test]
    fn library_names_are_unique_and_every_definition_is_drawable() {
        let pats = standard_patterns();
        assert!(pats.len() >= 60, "{}", pats.len());
        let mut names: Vec<String> = pats.iter().map(|p| p.name.to_ascii_uppercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), pats.len(), "duplicate pattern names");
        for p in &pats {
            assert!(!p.description.is_empty(), "{}", p.name);
            for l in &p.lines {
                let finite = [l.angle, l.origin.0, l.origin.1, l.delta.0, l.delta.1].iter().chain(&l.dashes).all(|v| v.is_finite());
                assert!(finite, "{}", p.name);
                // Parallel lines a positive distance apart, and a dash repeat that moves on.
                assert!(l.delta.1.abs() > 1e-6, "{}: lines on top of each other", p.name);
                assert!(l.dashes.is_empty() || l.dashes.iter().map(|d| d.abs()).sum::<f64>() > 1e-6, "{}", p.name);
            }
        }
        let lts = standard_linetypes();
        assert!(lts.len() >= 40, "{}", lts.len());
        let mut names: Vec<String> = lts.iter().map(|l| l.name.to_ascii_uppercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), lts.len(), "duplicate linetype names");
        assert!(lts.iter().all(|l| !l.pattern.is_empty() && l.pattern.iter().all(|e| e.length.is_finite())));
    }

    #[test]
    fn a_motif_gives_one_line_family_per_segment_repeating_on_its_cell() {
        // A dot, a horizontal stroke, a diagonal and a steep stroke on a 1 × 1 cell of 8 steps.
        let segs: Vec<Seg> = vec![((1, 1), (1, 1)), ((0, 2), (4, 2)), ((1, 1), (3, 3)), ((5, 1), (6, 4))];
        let fams = motif(1.0, 8, &segs);
        assert_eq!(fams.len(), segs.len());
        for (f, &((x0, y0), (x1, y1))) in fams.iter().zip(&segs) {
            // The dash is the segment's length; dash and gap together are one lattice period.
            let len = f64::from((x1 - x0).pow(2) + (y1 - y0).pow(2)).sqrt() / 8.0;
            assert!((f.dashes[0] - len).abs() < 1e-12, "{f:?}");
            let period = f.dashes[0] - f.dashes[1];
            let (dx, dy) = ((x1 - x0) as f64, (y1 - y0) as f64);
            let g = gcd((x1 - x0).abs(), (y1 - y0).abs()).max(1) as f64;
            let expect = if dx == 0.0 && dy == 0.0 { 1.0 } else { (dx / g).hypot(dy / g) };
            assert!((period - expect).abs() < 1e-12, "{f:?}");
            // Copies a positive distance apart, one cell's area per period: spacing × period = 1.
            assert!((f.delta.1 * period - 1.0).abs() < 1e-12, "{f:?}");
        }
    }

    #[test]
    fn iso_linetypes_have_the_iso_128_lengths_in_metric_drawings() {
        let lts = standard_linetypes_for(&Drawing::new_metric());
        let get = |n: &str| lts.iter().find(|l| l.name == n).map(|l| l.pattern.iter().map(|e| e.length).collect::<Vec<_>>());
        let close = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
        assert!(get("ISO02W100").is_some_and(|p| close(&p, &[12.0, -3.0])));
        assert!(get("ISO04W100").is_some_and(|p| close(&p, &[24.0, -3.0, 0.5, -3.0])));
        assert!(get("ISO15W100").is_some_and(|p| p.len() == 10));
        // A text linetype carries its text in the gap after the dash.
        let gas = standard_linetypes().into_iter().find(|l| l.name == "GAS_LINE").map(|l| l.pattern);
        assert!(gas.is_some_and(|p| p.get(1).and_then(|e| e.text.as_deref()) == Some("GAS")));
    }
}
