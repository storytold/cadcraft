//! More of the standard hatch patterns, written for CADCraft (CC0 / public domain as part of this
//! codebase), like the rest of the library.
//!
//! The names are the industry-common names drawings reference; every geometry here is our own
//! design: a tile of segments we chose (a brick course, a stone outline, a star) repeated on a
//! lattice, or plain line families with our own spacing and dashes. The block and brick
//! patterns use published unit sizes, named next to each (the 8 x 8 x 16 in concrete masonry
//! unit, the US modular brick, the UK / ISO metric brick); the herringbone, parquet, roof and
//! other layouts are our own, described next to each. The ISO
//! line patterns use the dash and gap multiples of the line width that ISO 128 publishes for its
//! line types 02 to 15 (width 1.0 mm), with our own 4 mm spacing. Lengths are in inches, like
//! the rest of the library (metric drawings scale them by 25.4).

use super::{HatchPattern, PatternLine, pl};

type P = (f64, f64);

/// Most lattice steps tried when expressing a segment's direction on its tile's lattice.
const MAX_STEPS: i64 = 24;

/// One millimetre in the library's inches.
const MM: f64 = 1.0 / 25.4;

/// A tile repeated at every integer combination of `e1` and `e2` (counter-clockwise basis).
#[derive(Clone, Copy)]
struct Tile {
    e1: P,
    e2: P,
}

fn rect(w: f64, h: f64) -> Tile {
    Tile { e1: (w, 0.0), e2: (0.0, h) }
}

fn near_int(x: f64) -> bool {
    (x - x.round()).abs() < 1e-6
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// `(g, x, y)` with `a x + b y = g`.
fn egcd(a: i64, b: i64) -> (i64, i64, i64) {
    if b == 0 {
        (a, 1, 0)
    } else {
        let (g, x, y) = egcd(b, a % b);
        (g, y, x - (a / b) * y)
    }
}

impl Tile {
    fn det(&self) -> f64 {
        self.e1.0 * self.e2.1 - self.e1.1 * self.e2.0
    }

    /// The line family drawing segment `a`–`b` in every copy of the tile: lines along the
    /// segment's lattice direction, one dash the segment's length per lattice period. `None`
    /// when the direction is not a short lattice direction or the segment is longer than the
    /// period (it would overlap its own copies).
    fn seg(&self, a: P, b: P) -> Option<PatternLine> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let det = self.det();
        if !(len > 1e-12 && det > 1e-12) {
            return None;
        }
        let al = (dx * self.e2.1 - dy * self.e2.0) / det;
        let be = (self.e1.0 * dy - self.e1.1 * dx) / det;
        // Only the direction matters: the larger coefficient becomes 1.
        let m = al.abs().max(be.abs());
        let (al, be) = (al / m, be / m);
        let n = (1..=MAX_STEPS).find(|&n| near_int(al * n as f64) && near_int(be * n as f64))?;
        let (p, q) = ((al * n as f64).round() as i64, (be * n as f64).round() as i64);
        let g = gcd(p, q);
        if g == 0 {
            return None;
        }
        let (p, q) = (p / g, q / g);
        let d = (p as f64 * self.e1.0 + q as f64 * self.e2.0, p as f64 * self.e1.1 + q as f64 * self.e2.1);
        let period = d.0.hypot(d.1);
        if len > period + 1e-9 {
            return None;
        }
        // The next line over: a lattice vector w with p j - q i = 1, so cross(d, w) = det.
        let (g, x, y) = egcd(p, q);
        let (j, i) = if g < 0 { (-x, y) } else { (x, -y) };
        let w = (i as f64 * self.e1.0 + j as f64 * self.e2.0, i as f64 * self.e1.1 + j as f64 * self.e2.1);
        let u = (d.0 / period, d.1 / period);
        let along = w.0 * u.0 + w.1 * u.1;
        let gap = period - len;
        let dashes = if gap < 1e-9 { Vec::new() } else { vec![len, -gap] };
        Some(PatternLine { angle: d.1.atan2(d.0).to_degrees(), origin: a, delta: (along, det / period), dashes })
    }

    /// The families of the polyline through `pts` (closed back to the first point when `closed`).
    fn poly(&self, pts: &[P], closed: bool) -> Vec<Option<PatternLine>> {
        let mut v: Vec<Option<PatternLine>> = pts.windows(2).map(|w| self.seg(w[0], w[1])).collect();
        if closed && let (Some(&a), Some(&b)) = (pts.last(), pts.first()) {
            v.push(self.seg(a, b));
        }
        v
    }
}

/// Parallel lines at `angle`, `spacing` apart, the first `offset` across from the origin, each
/// shifted `shift` along itself from the last.
fn par(angle: f64, offset: f64, spacing: f64, shift: f64, dashes: &[f64]) -> Option<PatternLine> {
    let (s, c) = angle.to_radians().sin_cos();
    Some(pl(angle, (-s * offset, c * offset), (shift, spacing), dashes))
}

/// `lines` turned `deg` degrees about the origin.
fn rotated(lines: Vec<Option<PatternLine>>, deg: f64) -> Vec<Option<PatternLine>> {
    let (s, c) = deg.to_radians().sin_cos();
    lines
        .into_iter()
        .map(|l| {
            l.map(|mut l| {
                l.angle += deg;
                l.origin = (l.origin.0 * c - l.origin.1 * s, l.origin.0 * s + l.origin.1 * c);
                l
            })
        })
        .collect()
}

/// The patterns, and the names of any whose tile has a segment [`Tile::seg`] could not place
/// (a test keeps this empty).
pub(super) fn patterns_checked() -> (Vec<HatchPattern>, Vec<&'static str>) {
    let mut out = Vec::new();
    let mut missed = Vec::new();
    let mut add = |name: &'static str, description: &'static str, lines: Vec<Option<PatternLine>>| {
        if lines.iter().any(Option::is_none) {
            missed.push(name);
        }
        out.push(HatchPattern { name, description, lines: lines.into_iter().flatten().collect() });
    };

    // Own layout: unequal-leg L shapes (0.2 in by 0.12 in legs) on a 0.32 in by 0.24 in grid,
    // every row shifted half a step.
    add(
        "ANGLE",
        "Rows of small L shapes with one long and one short leg, every row shifted half a step (angle sections)",
        Tile { e1: (0.32, 0.0), e2: (0.16, 0.24) }.poly(&[(0.2, 0.0), (0.0, 0.0), (0.0, 0.12)], false),
    );
    let t = rect(0.45, 0.45);
    let mut v = vec![t.seg((0.0, 0.0), (0.45, 0.0)), t.seg((0.0, 0.0), (0.0, 0.45))];
    v.extend(t.poly(&[(0.1, 0.1), (0.35, 0.1), (0.35, 0.35), (0.1, 0.35)], true));
    add("BOX", "Square grid with a smaller square centred in every cell (box sections)", v);
    add(
        "BRASS",
        "Horizontal solid lines with a dashed line midway between each pair",
        vec![par(0.0, 0.0, 0.22, 0.0, &[]), par(0.0, 0.11, 0.22, 0.05, &[0.14, -0.07])],
    );
    let (c, t) = (1.0 / 3.0, rect(1.0, 2.0 / 3.0));
    let mut v = vec![par(0.0, 0.0, c, 0.0, &[]), t.seg((0.0, 0.0), (0.0, c)), t.seg((0.5, c), (0.5, 2.0 * c))];
    for (x, y) in [(0.6, c / 3.0), (0.6, 2.0 * c / 3.0), (0.1, 4.0 * c / 3.0), (0.1, 5.0 * c / 3.0)] {
        v.push(t.seg((x, y), (x + 0.3, y)));
    }
    add("BRSTONE", "Staggered courses of blocks, every block with two short strokes inside it", v);
    add(
        "CLAY",
        "Bands of three tight horizontal lines, a dashed line in each space between bands",
        vec![par(0.0, 0.0, 0.2, 0.0, &[]), par(0.0, 0.035, 0.2, 0.0, &[]), par(0.0, 0.07, 0.2, 0.0, &[]), par(0.0, 0.135, 0.2, 0.08, &[0.16, -0.08])],
    );
    let t = rect(0.24, 0.24);
    add(
        "CORK",
        "Horizontal lines with two short slanted strokes in every space between them",
        vec![par(0.0, 0.0, 0.24, 0.0, &[]), t.seg((0.17, 0.05), (0.07, 0.15)), t.seg((0.19, 0.1), (0.12, 0.17))],
    );
    let t = rect(0.56, 0.56);
    add(
        "DOLMIT",
        "Horizontal rock beds joined by short steep strokes that zig-zag from bed to bed",
        vec![par(0.0, 0.0, 0.28, 0.0, &[]), t.seg((0.1, 0.0), (0.24, 0.28)), t.seg((0.38, 0.0), (0.52, 0.28)), t.seg((0.38, 0.28), (0.24, 0.56))],
    );
    add(
        "FLEX",
        "Rows of straight runs, each broken by one small zig-zag wiggle",
        rect(0.6, 0.3).poly(&[(0.0, 0.0), (0.3, 0.0), (0.36, 0.06), (0.42, 0.0), (0.48, -0.06), (0.54, 0.0), (0.6, 0.0)], false),
    );
    let t = rect(0.48, 0.48);
    add(
        "GOST_GLASS",
        "Staggered pairs of short parallel strokes at 45 degrees (glass, after GOST 2.306)",
        vec![t.seg((0.0, 0.0), (0.14, 0.14)), t.seg((0.06, 0.0), (0.18, 0.12)), t.seg((0.24, 0.24), (0.38, 0.38)), t.seg((0.3, 0.24), (0.42, 0.36))],
    );
    add(
        "GOST_GROUND",
        "Groups of three tight 45-degree lines with wide spaces between (soil, after GOST 2.306)",
        vec![par(45.0, 0.0, 0.35, 0.0, &[]), par(45.0, 0.045, 0.35, 0.0, &[]), par(45.0, 0.09, 0.35, 0.0, &[])],
    );
    let t = rect(1.0, 0.24);
    add(
        "GOST_WOOD",
        "Long staggered horizontal strokes with short ones between, like grain (wood, after GOST 2.306)",
        vec![t.seg((0.0, 0.0), (0.7, 0.0)), t.seg((0.5, 0.12), (1.2, 0.12)), t.seg((0.8, 0.06), (0.9, 0.06))],
    );
    add(
        "GRATE",
        "Very close horizontal lines crossed by wider-spaced vertical lines",
        vec![par(0.0, 0.0, 0.04, 0.0, &[]), par(90.0, 0.0, 0.15, 0.0, &[])],
    );
    // Own layout: six stones of four to seven sides and three sizes, corners on a 0.05 in grid,
    // in a 1 in square tile.
    let t = rect(1.0, 1.0);
    let stones: [&[P]; 6] = [
        &[(0.05, 0.1), (0.2, 0.05), (0.35, 0.1), (0.4, 0.25), (0.3, 0.35), (0.1, 0.3)],
        &[(0.55, 0.05), (0.7, 0.1), (0.75, 0.2), (0.65, 0.3), (0.5, 0.25)],
        &[(0.85, 0.15), (0.95, 0.2), (0.9, 0.3), (0.8, 0.25)],
        &[(0.45, 0.45), (0.65, 0.4), (0.85, 0.5), (0.9, 0.7), (0.75, 0.85), (0.55, 0.8), (0.45, 0.65)],
        &[(0.1, 0.55), (0.25, 0.5), (0.3, 0.65), (0.2, 0.75), (0.05, 0.7)],
        &[(0.2, 0.85), (0.3, 0.85), (0.35, 0.95), (0.25, 0.95)],
    ];
    let v = stones.iter().flat_map(|s| t.poly(s, true)).collect();
    add("GRAVEL", "Loose stones of mixed sizes, each an irregular outline with four to seven sides", v);
    let (s, h) = (0.14, 0.14 * 3f64.sqrt());
    let hex = |cx: f64, cy: f64| -> Vec<P> {
        vec![
            (cx + s, cy),
            (cx + s / 2.0, cy + h / 2.0),
            (cx - s / 2.0, cy + h / 2.0),
            (cx - s, cy),
            (cx - s / 2.0, cy - h / 2.0),
            (cx + s / 2.0, cy - h / 2.0),
        ]
    };
    add("HEX", "Honeycomb of six-sided cells sharing their edges", rect(3.0 * s, 1.5 * h).poly(&hex(0.0, 0.0), true));
    add(
        "HOUND",
        "Horizontal and vertical dashes, each row stepped along, weaving a broken check",
        vec![par(0.0, 0.0, 0.08, 0.2, &[0.3, -0.1]), par(90.0, 0.0, 0.08, -0.2, &[0.3, -0.1])],
    );
    add(
        "INSUL",
        "Horizontal solid lines with two dashed lines in every space between them",
        vec![par(0.0, 0.0, 0.42, 0.0, &[]), par(0.0, 0.14, 0.42, 0.05, &[0.1, -0.1]), par(0.0, 0.28, 0.42, 0.05, &[0.1, -0.1])],
    );
    add(
        "MUDST",
        "Rows of long dashes with dots, alternating with rows of loose dots (mud and sand)",
        vec![par(0.0, 0.0, 0.22, 0.17, &[0.34, -0.09, 0.0, -0.09]), par(0.0, 0.11, 0.22, 0.06, &[0.0, -0.12, 0.0, -0.15])],
    );
    add("PLAST", "Groups of three tight horizontal lines", [0.0, 0.04, 0.08].iter().map(|&o| par(0.0, o, 0.27, 0.0, &[])).collect());
    add(
        "PLASTI",
        "Groups of three tight horizontal lines with one single line in each space between groups",
        [0.0, 0.045, 0.09, 0.2].iter().map(|&o| par(0.0, o, 0.3, 0.0, &[])).collect(),
    );
    add(
        "SACNCR",
        "Close 45-degree lines with a row of dots midway between each pair (concrete)",
        vec![par(45.0, 0.0, 0.1, 0.0, &[]), par(45.0, 0.05, 0.1, 0.035, &[0.0, -0.07])],
    );
    let (s, h) = (0.27, 0.27 * 3f64.sqrt() / 2.0);
    let t = rect(1.5 * s, 2.0 * h);
    let mut v = t.poly(&[(0.0, 0.0), (s, 0.0), (s / 2.0, h)], true);
    v.extend(t.poly(&[(0.0, 2.0 * h / 3.0), (s / 2.0, -h / 3.0), (s, 2.0 * h / 3.0)], true));
    add("STARS", "Six-pointed stars, each drawn as an upright and an inverted triangle overlapping", v);
    add(
        "STEEL",
        "Pairs of close 45-degree lines with wider spaces between pairs (steel sections)",
        vec![par(45.0, 0.0, 0.17, 0.0, &[]), par(45.0, 0.05, 0.17, 0.0, &[])],
    );
    let t = rect(0.56, 0.56);
    let mut v = Vec::new();
    for (x, y) in [(0.0, 0.0), (0.28, 0.28)] {
        v.push(t.seg((x, y), (x + 0.3, y)));
        let (tx, ty) = (x + 0.4, y);
        v.push(t.seg((tx, ty), (tx, ty + 0.1)));
        v.push(t.seg((tx, ty), (tx - 0.06, ty + 0.09)));
        v.push(t.seg((tx, ty), (tx + 0.06, ty + 0.09)));
    }
    add("SWAMP", "Short ground strokes, each with a small three-stem tuft of reeds beside it (marsh)", v);
    add(
        "TRANS",
        "Horizontal solid lines with a short-dashed line midway between each pair",
        vec![par(0.0, 0.0, 0.26, 0.0, &[]), par(0.0, 0.13, 0.26, 0.0, &[0.07, -0.05])],
    );
    let (s, h) = (0.2, 0.2 * 3f64.sqrt() / 2.0);
    let t = rect(2.0 * s, 2.0 * h);
    let mut v = t.poly(&[(0.0, 0.0), (s, 0.0), (s / 2.0, h)], true);
    v.extend(t.poly(&[(s, h), (2.0 * s, h), (1.5 * s, 2.0 * h)], true));
    add("TRIANG", "Staggered rows of small upright triangles with all sides equal", v);

    // Architectural patterns, drawn at real size: inches, or millimetres converted with `MM`.
    // In the block and brick patterns the origin is the lower left corner of one unit's face;
    // single-line patterns draw the joint centre lines, half a joint below and left of it.
    let face = |x: f64, y: f64, w: f64, h: f64| -> Vec<P> { vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)] };

    // Concrete masonry unit: 8 x 8 x 16 in nominal, 7-5/8 x 7-5/8 x 15-5/8 in actual, 3/8 in
    // joints, laid in running bond (each course shifted half a unit).
    let (bl, bh, j) = (16.0, 8.0, 0.375);
    let hj = j / 2.0;
    let t = rect(bl, 2.0 * bh);
    add(
        "AR-B816",
        "Concrete block wall in running bond: 16 in by 8 in blocks, joints as single lines",
        vec![par(0.0, -hj, bh, 0.0, &[]), t.seg((-hj, -hj), (-hj, bh - hj)), t.seg((bl / 2.0 - hj, bh - hj), (bl / 2.0 - hj, 2.0 * bh - hj))],
    );
    let mut v = t.poly(&face(0.0, 0.0, bl - j, bh - j), true);
    v.extend(t.poly(&face(bl / 2.0, bh, bl - j, bh - j), true));
    add("AR-B816C", "Concrete block wall in running bond: each 15-5/8 in by 7-5/8 in block face outlined, 3/8 in joints", v);
    // The same unit's 8 x 8 in end face (or an 8 x 8 x 8 in half unit), running bond.
    let t = rect(bh, 2.0 * bh);
    add(
        "AR-B88",
        "Concrete block wall in running bond: 8 in square block faces, joints as single lines",
        vec![par(0.0, -hj, bh, 0.0, &[]), t.seg((-hj, -hj), (-hj, bh - hj)), t.seg((bh / 2.0 - hj, bh - hj), (bh / 2.0 - hj, 2.0 * bh - hj))],
    );
    // US modular brick: 7-5/8 x 2-1/4 in face (3-5/8 in end), nominal 8 x 2-2/3 in (three
    // courses to 8 in), in English bond: a course of stretchers, then a course of headers whose
    // joints fall a quarter brick from the stretchers' joints.
    let (ml, mc, mh, mw) = (8.0, 8.0 / 3.0, 2.25, 3.625);
    let t = rect(ml, 2.0 * mc);
    let mut v = t.poly(&face(0.0, 0.0, ml - j, mh), true);
    for x in [ml / 4.0, 3.0 * ml / 4.0] {
        v.extend(t.poly(&face(x, mc, mw, mh), true));
    }
    add("AR-BRELM", "Brick wall in English bond: a course of long brick faces, then a course of brick ends, each face outlined", v);
    // UK / ISO metric brick: 215 x 65 mm face with 10 mm joints (225 x 75 mm module), in
    // stretcher bond (each course shifted half a brick).
    let (ul, uh, uj) = (225.0 * MM, 75.0 * MM, 5.0 * MM);
    let t = rect(ul, 2.0 * uh);
    add(
        "AR-BRSTD",
        "Brick wall in stretcher bond: 215 mm by 65 mm bricks with 10 mm joints, joints as single lines",
        vec![par(0.0, -uj, uh, 0.0, &[]), t.seg((-uj, -uj), (-uj, uh - uj)), t.seg((ul / 2.0 - uj, uh - uj), (ul / 2.0 - uj, 2.0 * uh - uj))],
    );
    // Own layout: herringbone of 2:1 bricks laid flat, the UK / ISO metric brick's 225 x 112.5 mm
    // module (215 x 102.5 mm plus 10 mm joints). One brick lies along x, the next stands
    // against its end; the pair repeats at (3u, u) and (-u, u). Every joint is the bottom or left
    // edge of a brick, drawn once; the whole is turned so the zig-zag runs across the sheet.
    let u = 112.5 * MM;
    let t = Tile { e1: (3.0 * u, u), e2: (-u, u) };
    let v = vec![t.seg((0.0, 0.0), (3.0 * u, 0.0)), t.seg((0.0, 0.0), (0.0, u)), t.seg((2.0 * u, 0.0), (2.0 * u, 2.0 * u))];
    add("AR-HBONE", "Herringbone paving: bricks twice as long as wide, alternately along and across, in zig-zag rows", rotated(v, -45.0));
    // Own layout: basket-weave parquet of 300 mm squares, each of four 75 mm strips, the strips
    // turning a right angle from square to square.
    let (sq, st) = (300.0 * MM, 75.0 * MM);
    let t = rect(2.0 * sq, 2.0 * sq);
    let mut v = vec![par(0.0, 0.0, sq, 0.0, &[]), par(90.0, 0.0, sq, 0.0, &[])];
    for k in 1..4 {
        let o = st * f64::from(k);
        v.push(t.seg((o, 0.0), (o, sq)));
        v.push(t.seg((sq + o, sq), (sq + o, 2.0 * sq)));
        v.push(t.seg((sq, o), (2.0 * sq, o)));
        v.push(t.seg((0.0, sq + o), (sq, sq + o)));
    }
    add("AR-PARQ1", "Basket-weave wood floor: squares of four strips, the strips turning a right angle from square to square", v);
    // Own layout: plain roof tiles seen from above, 165 mm wide at a 100 mm gauge, each course
    // shifted half a tile, with a line 8 mm below each course's lower edge for its shadow.
    let (tw, tg) = (165.0 * MM, 100.0 * MM);
    let t = rect(tw, 2.0 * tg);
    add(
        "AR-RROOF",
        "Tiled roof seen from above: courses of tiles with staggered joints, each lower edge shadowed by a close line",
        vec![
            par(0.0, 0.0, tg, 0.0, &[]),
            par(0.0, -8.0 * MM, tg, 0.0, &[]),
            t.seg((0.0, 0.0), (0.0, tg)),
            t.seg((tw / 2.0, tg), (tw / 2.0, 2.0 * tg)),
        ],
    );
    // Own layout: wooden shakes at a 230 mm exposure, uneven widths (90 to 225 mm), joints at
    // least 40 mm from those of the courses above and below, repeating every 1 m and 3 courses.
    let ex = 230.0 * MM;
    let t = rect(1000.0 * MM, 3.0 * ex);
    let mut v = vec![par(0.0, 0.0, ex, 0.0, &[])];
    let courses: [[f64; 6]; 3] =
        [[0.0, 150.0, 330.0, 460.0, 640.0, 820.0], [70.0, 240.0, 390.0, 560.0, 730.0, 900.0], [110.0, 285.0, 510.0, 685.0, 860.0, 950.0]];
    for (row, xs) in courses.iter().enumerate() {
        let y = ex * row as f64;
        for &x in xs {
            v.push(t.seg((x * MM, y), (x * MM, y + ex)));
        }
    }
    add("AR-RSHKE", "Wooden shake roof seen from above: courses of shakes of uneven widths with staggered joints", v);

    // ISO 128 line types 02 to 15 at line width 1.0 mm: dashes and gaps as multiples of it.
    const ISO: [(&str, &str, &[f64]); 14] = [
        ("ISO02W100", "Lines of 12 mm dashes with 3 mm gaps (ISO 128 type 02)", &[12.0, -3.0]),
        ("ISO03W100", "Lines of 12 mm dashes with 18 mm gaps (ISO 128 type 03)", &[12.0, -18.0]),
        ("ISO04W100", "Lines of 24 mm dashes, one dot in each gap (ISO 128 type 04)", &[24.0, -3.0, 0.5, -3.0]),
        ("ISO05W100", "Lines of 24 mm dashes, two dots in each gap (ISO 128 type 05)", &[24.0, -3.0, 0.5, -3.0, 0.5, -3.0]),
        ("ISO06W100", "Lines of 24 mm dashes, three dots in each gap (ISO 128 type 06)", &[24.0, -3.0, 0.5, -3.0, 0.5, -3.0, 0.5, -3.0]),
        ("ISO07W100", "Lines of dots 3 mm apart (ISO 128 type 07)", &[0.5, -3.0]),
        ("ISO08W100", "Lines of 24 mm dashes, one 6 mm dash in each gap (ISO 128 type 08)", &[24.0, -3.0, 6.0, -3.0]),
        ("ISO09W100", "Lines of 24 mm dashes, two 6 mm dashes in each gap (ISO 128 type 09)", &[24.0, -3.0, 6.0, -3.0, 6.0, -3.0]),
        ("ISO10W100", "Lines of 12 mm dashes, one dot in each gap (ISO 128 type 10)", &[12.0, -3.0, 0.5, -3.0]),
        ("ISO11W100", "Lines of 12 mm dash pairs, one dot after each pair (ISO 128 type 11)", &[12.0, -3.0, 12.0, -3.0, 0.5, -3.0]),
        ("ISO12W100", "Lines of 12 mm dashes, two dots in each gap (ISO 128 type 12)", &[12.0, -3.0, 0.5, -3.0, 0.5, -3.0]),
        ("ISO13W100", "Lines of 12 mm dash pairs, two dots after each pair (ISO 128 type 13)", &[12.0, -3.0, 12.0, -3.0, 0.5, -3.0, 0.5, -3.0]),
        ("ISO14W100", "Lines of 12 mm dashes, three dots in each gap (ISO 128 type 14)", &[12.0, -3.0, 0.5, -3.0, 0.5, -3.0, 0.5, -3.0]),
        (
            "ISO15W100",
            "Lines of 12 mm dash pairs, three dots after each pair (ISO 128 type 15)",
            &[12.0, -3.0, 12.0, -3.0, 0.5, -3.0, 0.5, -3.0, 0.5, -3.0],
        ),
    ];
    for (name, description, dashes) in ISO {
        let d: Vec<f64> = dashes.iter().map(|x| x * MM).collect();
        add(name, description, vec![par(0.0, 0.0, 4.0 * MM, 0.0, &d)]);
    }
    (out, missed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tile_segment_becomes_a_family() {
        let (p, missed) = patterns_checked();
        assert!(missed.is_empty(), "{missed:?}");
        assert_eq!(p.len(), 48);
        for p in &p {
            assert!(!p.lines.is_empty(), "{}", p.name);
            for l in &p.lines {
                let ok = [l.angle, l.origin.0, l.origin.1, l.delta.0, l.delta.1].iter().chain(&l.dashes).all(|x| x.is_finite());
                assert!(ok && l.delta.1 > 1e-6, "{} {l:?}", p.name);
            }
        }
    }

    #[test]
    fn a_tile_segment_repeats_once_per_tile() {
        // A horizontal segment half a tile long on a 1 x 1 tile: one dash per unit, lines 1 apart.
        let l = rect(1.0, 1.0).seg((0.0, 0.0), (0.5, 0.0));
        assert_eq!(l, Some(pl(0.0, (0.0, 0.0), (0.0, 1.0), &[0.5, -0.5])));
        // A diagonal of a 1 x 1 tile: the lines are 1/sqrt 2 apart, shifted half a diagonal.
        let l = rect(1.0, 1.0).seg((0.0, 0.0), (1.0, 1.0)).expect("diagonal");
        assert!((l.angle - 45.0).abs() < 1e-9 && (l.delta.1 - 0.5f64.sqrt()).abs() < 1e-9 && l.dashes.is_empty());
        assert!((l.delta.0.abs() - 0.5f64.sqrt()).abs() < 1e-9);
        // Longer than its period, or along an irrational direction: refused.
        assert!(rect(1.0, 1.0).seg((0.0, 0.0), (1.5, 0.0)).is_none());
        assert!(rect(1.0, 1.0).seg((0.0, 0.0), (1.0, 2f64.sqrt())).is_none());
    }
}
