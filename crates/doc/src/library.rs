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
        // CADCraft's own drawing (2026-10-18): small closed stones (triangles of 12-34 mm sides,
        // angles 35° or more) among scattered dots, at the density a concrete hatch usually has
        // on architectural drawings (one stone per ~12 000 mm², a dot per ~3 500 mm² in an ISO
        // drawing at scale 1).
        // No line is taken from any vendor's pattern file. Every family repeats on one lattice
        // (540 mm, 0) / (190 mm, 450 mm), each stone side along a lattice direction, so the three
        // sides of a stone repeat together and it stays closed. 20 stones and 70 dots per tile.
        HatchPattern {
            name: "AR-CONC",
            description: "Random dot and stone pattern",
            lines: vec![
                pl(0.0, (20.481011, 9.635334), (7.480315, 17.716535), &[1.18168, -20.078162]),
                pl(247.109448, (21.662691, 9.635334), (10.9615, 19.585621), &[1.068911, -18.162073]),
                pl(307.874984, (20.481011, 9.635334), (13.052281, 16.781504), &[1.247521, -21.196877]),
                pl(247.109448, (19.07861, 2.645564), (10.9615, 19.585621), &[0.619114, -18.61187]),
                pl(127.874984, (18.837792, 2.075206), (13.052281, 16.781504), &[0.722565, -21.721833]),
                pl(180.0, (19.07861, 2.645564), (7.480315, 17.716535), &[0.68443, -20.575412]),
                pl(67.109448, (10.457133, 9.277081), (10.9615, 19.585621), &[0.877543, -18.35344]),
                pl(333.178019, (10.798473, 10.085518), (18.972555, 9.592865), &[0.597223, -38.666413]),
                pl(31.651273, (10.457133, 9.277081), (15.66439, 11.156058), &[1.027079, -32.734915]),
                pl(31.651273, (3.050302, 8.149389), (15.66439, 11.156058), &[0.689582, -33.072412]),
                pl(153.178019, (3.637314, 8.511246), (18.972555, 9.592865), &[0.801952, -38.461683]),
                pl(100.080598, (3.050302, 8.149389), (19.854913, 10.465822), &[0.735061, -35.253584]),
                pl(211.651273, (16.536125, 5.170862), (15.66439, 11.156058), &[1.220917, -32.541077]),
                pl(333.178019, (15.49681, 4.530188), (18.972555, 9.592865), &[0.709935, -38.5537]),
                pl(247.109448, (16.536125, 5.170862), (10.9615, 19.585621), &[1.04316, -18.187824]),
                pl(307.874984, (6.927619, 10.461012), (13.052281, 16.781504), &[0.706048, -21.738351]),
                pl(211.651273, (7.36109, 9.903692), (15.66439, 11.156058), &[1.062072, -32.699922]),
                pl(247.109448, (6.927619, 10.461012), (10.9615, 19.585621), &[1.209922, -18.021062]),
                pl(31.651273, (15.592914, 15.996194), (15.66439, 11.156058), &[0.621023, -33.140971]),
                pl(153.178019, (16.121565, 16.322074), (18.972555, 9.592865), &[0.722221, -38.541414]),
                pl(100.080598, (15.592914, 15.996194), (19.854913, 10.465822), &[0.66198, -35.326665]),
                pl(280.080598, (9.685289, 17.107032), (19.854913, 10.465822), &[0.912763, -35.075882]),
                pl(153.178019, (9.845053, 16.208359), (18.972555, 9.592865), &[0.995825, -38.267811]),
                pl(211.651273, (9.685289, 17.107032), (15.66439, 11.156058), &[0.85629, -32.905705]),
                pl(153.178019, (7.584278, 13.166973), (18.972555, 9.592865), &[0.644726, -38.61891]),
                pl(280.080598, (7.008916, 13.457886), (19.854913, 10.465822), &[0.590949, -35.397696]),
                pl(211.651273, (7.584278, 13.166973), (15.66439, 11.156058), &[0.554386, -33.207608]),
                pl(127.874984, (5.550531, 4.253355), (13.052281, 16.781504), &[1.03043, -21.413968]),
                pl(247.109448, (4.917908, 5.066727), (10.9615, 19.585621), &[0.882901, -18.348083]),
                pl(180.0, (5.550531, 4.253355), (7.480315, 17.716535), &[0.976047, -20.283796]),
                pl(247.109448, (9.798774, 2.144405), (10.9615, 19.585621), &[1.238914, -17.99207]),
                pl(31.651273, (9.316871, 1.003056), (15.66439, 11.156058), &[1.087522, -32.674473]),
                pl(307.874984, (9.798774, 2.144405), (13.052281, 16.781504), &[0.722966, -21.721432]),
                pl(31.651273, (19.772647, 4.666939), (15.66439, 11.156058), &[1.1044, -32.657595]),
                pl(127.874984, (20.712776, 5.24647), (13.052281, 16.781504), &[0.734186, -21.710212]),
                pl(67.109448, (19.772647, 4.666939), (10.9615, 19.585621), &[1.258142, -17.972842]),
                pl(333.178019, (15.27065, 10.074253), (18.972555, 9.592865), &[0.802463, -38.461173]),
                pl(211.651273, (15.986778, 9.712166), (15.66439, 11.156058), &[0.690021, -33.071973]),
                pl(280.080598, (15.27065, 10.074253), (19.854913, 10.465822), &[0.735529, -35.253116]),
                pl(31.651273, (0.612588, 17.042707), (15.66439, 11.156058), &[0.933964, -32.82803]),
                pl(153.178019, (1.407632, 17.532803), (18.972555, 9.592865), &[1.086157, -38.177478]),
                pl(100.080598, (0.612588, 17.042707), (19.854913, 10.465822), &[0.995561, -34.993085]),
                pl(307.874984, (8.960817, 5.655419), (13.052281, 16.781504), &[1.057289, -21.387109]),
                pl(67.109448, (9.60993, 4.820846), (10.9615, 19.585621), &[0.905914, -18.325069]),
                pl(0.0, (8.960817, 5.655419), (7.480315, 17.716535), &[1.001488, -20.258355]),
                pl(0.0, (19.206725, 14.739017), (7.480315, 17.716535), &[0.655568, -20.604274]),
                pl(127.874984, (19.862293, 14.739017), (13.052281, 16.781504), &[0.692095, -21.752303]),
                pl(67.109448, (19.206725, 14.739017), (10.9615, 19.585621), &[0.593007, -18.637977]),
                pl(211.651273, (13.119025, 15.340647), (15.66439, 11.156058), &[1.167, -32.594994]),
                pl(307.874984, (12.125607, 14.728266), (13.052281, 16.781504), &[0.775802, -21.668596]),
                pl(247.109448, (13.119025, 15.340647), (10.9615, 19.585621), &[1.329457, -17.901526]),
                pl(31.651273, (3.773733, 12.39632), (15.66439, 11.156058), &[0.651956, -33.110039]),
                pl(153.178019, (4.328715, 12.738433), (18.972555, 9.592865), &[0.758194, -38.505441]),
                pl(100.080598, (3.773733, 12.39632), (19.854913, 10.465822), &[0.694953, -35.293692]),
                pl(31.651273, (16.206885, 12.356449), (15.66439, 11.156058), &[1.083079, -32.678915]),
                pl(153.178019, (17.128864, 12.924792), (18.972555, 9.592865), &[1.259571, -38.004065]),
                pl(100.080598, (16.206885, 12.356449), (19.854913, 10.465822), &[1.15451, -34.834135]),
                pl(67.109448, (12.849427, 5.722968), (10.9615, 19.585621), &[0.853249, -18.377735]),
                pl(211.651273, (13.181317, 6.509024), (15.66439, 11.156058), &[0.998645, -32.763349]),
                pl(153.178019, (12.849427, 5.722968), (18.972555, 9.592865), &[0.580689, -38.682947]),
                pl(
                    19.510799,
                    (1.403172, 0.497187),
                    (33.006905, 7.100458),
                    &[
                        0.0, -2.71837, 0.0, -2.122911, 0.0, -2.390294, 0.0, -3.002194, 0.0, -1.894831, 0.0, -4.06954, 0.0, -1.559613, 0.0, -2.89308,
                        0.0, -2.040845, 0.0, -3.324317, 0.0, -5.628361, 0.0, -2.660667, 0.0, -2.096617, 0.0, -7.476064, 0.0, -3.29102, 0.0,
                        -2.316796, 0.0, -1.70561, 0.0, -1.854847,
                    ],
                ),
                pl(
                    144.027613,
                    (-6.861953, 4.98045),
                    (38.763978, 6.243966),
                    &[
                        0.0, -4.052168, 0.0, -1.36175, 0.0, -9.967512, 0.0, -1.280637, 0.0, -4.170196, 0.0, -1.41432, 0.0, -2.367455, 0.0, -1.486285,
                        0.0, -4.62738, 0.0, -4.002436, 0.0, -2.663116, 0.0, -1.934293, 0.0, -1.991448, 0.0, -2.637094, 0.0, -1.58119, 0.0, -2.780019,
                        0.0, -2.049279, 0.0, -9.955783,
                    ],
                ),
                pl(
                    50.572198,
                    (2.533777, 3.081621),
                    (18.435492, 5.473881),
                    &[
                        0.0, -3.332314, 0.0, -2.666002, 0.0, -3.562468, 0.0, -4.494214, 0.0, -5.221485, 0.0, -3.224885, 0.0, -3.111831, 0.0,
                        -3.642425, 0.0, -7.151893, 0.0, -3.462364, 0.0, -3.165007, 0.0, -3.544732, 0.0, -1.308374, 0.0, -2.351488, 0.0, -5.583112,
                        0.0, -5.037525, 0.0, -7.948599,
                    ],
                ),
                pl(
                    162.531977,
                    (-3.708719, 1.167079),
                    (20.279437, 6.381641),
                    &[
                        0.0, -1.113427, 0.0, -2.237281, 0.0, -1.527796, 0.0, -6.108525, 0.0, -6.203357, 0.0, -4.227832, 0.0, -1.593644, 0.0,
                        -4.181987, 0.0, -3.165928, 0.0, -2.379037, 0.0, -1.551491, 0.0, -6.066391, 0.0, -2.645402, 0.0, -4.557707, 0.0, -2.419574,
                        0.0, -1.646997, 0.0, -7.394611,
                    ],
                ),
            ],
        },
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
