use super::Vec2;

/// The arrowheads drawn in code (DIMBLK names; our own geometry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrowhead {
    ClosedFilled,
    ClosedBlank,
    Closed,
    Dot,
    DotSmall,
    DotBlank,
    Oblique,
    ArchTick,
    Open,
    Open90,
    Open30,
    Origin,
    Origin2,
    Small,
    BoxFilled,
    BoxBlank,
    DatumFilled,
    DatumBlank,
    Integral,
    None,
}

impl Arrowhead {
    /// Parse a DIMBLK value ("" / "_DOT" / "Architectural tick" / "closed blank"…).
    pub fn parse(name: &str) -> Arrowhead {
        let n: String = name.trim().trim_start_matches('_').chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        match n.as_str() {
            "" | "closedfilled" | "filled" => Arrowhead::ClosedFilled,
            "closedblank" | "blank" => Arrowhead::ClosedBlank,
            "closed" => Arrowhead::Closed,
            "dot" => Arrowhead::Dot,
            "dotsmall" => Arrowhead::DotSmall,
            "dotblank" => Arrowhead::DotBlank,
            "oblique" => Arrowhead::Oblique,
            "archtick" | "architecturaltick" | "tick" => Arrowhead::ArchTick,
            "open" => Arrowhead::Open,
            "open90" | "rightangle" => Arrowhead::Open90,
            "open30" => Arrowhead::Open30,
            "origin" | "originindicator" => Arrowhead::Origin,
            "origin2" | "originindicator2" => Arrowhead::Origin2,
            "small" => Arrowhead::Small,
            "boxfilled" => Arrowhead::BoxFilled,
            "boxblank" | "box" => Arrowhead::BoxBlank,
            "datumfilled" | "datumtrianglefilled" => Arrowhead::DatumFilled,
            "datumblank" | "datumtriangle" => Arrowhead::DatumBlank,
            "integral" => Arrowhead::Integral,
            "none" => Arrowhead::None,
            _ => Arrowhead::ClosedFilled,
        }
    }
    /// Ticks and obliques: the dimension line runs through the tip and extends by DIMDLE.
    pub fn is_tick(self) -> bool {
        matches!(self, Arrowhead::Oblique | Arrowhead::ArchTick | Arrowhead::Integral | Arrowhead::None)
    }
    /// How far back from the tip the dimension line stops (blank heads are not crossed).
    pub fn inset(self, size: f64) -> f64 {
        match self {
            Arrowhead::ClosedBlank | Arrowhead::DatumBlank => size,
            Arrowhead::DotBlank => size / 4.0,
            Arrowhead::Origin | Arrowhead::Origin2 => size / 2.0,
            Arrowhead::BoxBlank => size / 4.0,
            _ => 0.0,
        }
    }
}

/// Arrowhead geometry: open polylines and filled triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrowGeom {
    pub lines: Vec<Vec<Vec2>>,
    pub tris: Vec<Vec2>,
}

fn circle_pts(c: Vec2, r: f64) -> Vec<Vec2> {
    (0..=24).map(|i| c + Vec2::from_angle(f64::from(i) * cadcraft_geom::TAU / 24.0) * r).collect()
}

fn disc(c: Vec2, r: f64) -> Vec<Vec2> {
    let mut t = Vec::with_capacity(72);
    for i in 0..24 {
        let a0 = f64::from(i) * cadcraft_geom::TAU / 24.0;
        let a1 = f64::from(i + 1) * cadcraft_geom::TAU / 24.0;
        t.extend_from_slice(&[c, c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r]);
    }
    t
}

/// An arrowhead at `tip` pointing along `dir` (unit), `size` = DIMASZ × scale.
pub fn arrowhead(kind: Arrowhead, tip: Vec2, dir: Vec2, size: f64) -> ArrowGeom {
    let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::X } else { dir };
    let size = if size.is_finite() { size.abs() } else { 0.0 };
    let n = dir.perp();
    let wings = |half_angle: f64, len: f64| {
        let back = -dir;
        (tip + back.rotate(half_angle) * len, tip + back.rotate(-half_angle) * len)
    };
    let mut g = ArrowGeom::default();
    match kind {
        Arrowhead::ClosedFilled => g.tris = arrow(tip, dir, size),
        Arrowhead::Small => g.tris = arrow(tip, dir, size / 2.0),
        Arrowhead::ClosedBlank | Arrowhead::Closed => {
            let back = tip - dir * size;
            let w = n * (size / 6.0);
            g.lines.push(vec![tip, back + w, back - w, tip]);
            if kind == Arrowhead::Closed {
                g.lines.push(vec![back, tip]);
            }
        }
        Arrowhead::Dot => g.tris = disc(tip, size / 4.0),
        Arrowhead::DotSmall => g.tris = disc(tip, size / 8.0),
        Arrowhead::DotBlank => g.lines.push(circle_pts(tip, size / 4.0)),
        Arrowhead::Origin => g.lines.push(circle_pts(tip, size / 2.0)),
        Arrowhead::Origin2 => {
            g.lines.push(circle_pts(tip, size / 2.0));
            g.lines.push(circle_pts(tip, size / 4.0));
        }
        Arrowhead::Oblique => {
            let t = canonical(dir + n).normalized() * (size / 2.0);
            g.lines.push(vec![tip - t, tip + t]);
        }
        Arrowhead::ArchTick => {
            // A heavy 45° stroke: a thin filled parallelogram.
            let t = canonical(dir + n).normalized() * (size / 2.0);
            let w = t.perp().normalized() * (size / 16.0);
            let (a, b) = (tip - t, tip + t);
            g.tris = vec![a - w, b - w, b + w, a - w, b + w, a + w];
        }
        Arrowhead::Integral => {
            // A small integral-sign wave through the tip.
            let t = canonical(dir + n).normalized();
            let pts = (-8..=8)
                .map(|i| {
                    let f = f64::from(i) / 8.0;
                    tip + t * (f * size / 2.0) + t.perp() * ((f * std::f64::consts::PI).sin() * size * 0.12)
                })
                .collect();
            g.lines.push(pts);
        }
        Arrowhead::Open => {
            let (a, b) = wings((1.0f64 / 6.0).atan(), size);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::Open90 => {
            let (a, b) = wings(std::f64::consts::FRAC_PI_4, size / std::f64::consts::SQRT_2);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::Open30 => {
            let (a, b) = wings(15f64.to_radians(), size);
            g.lines.push(vec![a, tip, b]);
        }
        Arrowhead::BoxFilled | Arrowhead::BoxBlank => {
            let h = size / 4.0;
            let (u, v) = (dir * h, n * h);
            let q = [tip - u - v, tip + u - v, tip + u + v, tip - u + v];
            if kind == Arrowhead::BoxFilled {
                g.tris = vec![q[0], q[1], q[2], q[0], q[2], q[3]];
            } else {
                g.lines.push(vec![q[0], q[1], q[2], q[3], q[0]]);
            }
        }
        Arrowhead::DatumFilled | Arrowhead::DatumBlank => {
            let apex = tip - dir * size;
            let w = n * (size / 2.0);
            if kind == Arrowhead::DatumFilled {
                g.tris = vec![tip + w, tip - w, apex];
            } else {
                g.lines.push(vec![tip + w, tip - w, apex, tip + w]);
            }
        }
        Arrowhead::None => {}
    }
    g
}

/// A closed filled arrowhead at `tip` pointing along `dir` (unit), as one triangle.
pub fn arrow(tip: Vec2, dir: Vec2, size: f64) -> Vec<Vec2> {
    let back = tip - dir * size;
    let n = dir.perp() * (size / 6.0);
    vec![tip, back + n, back - n]
}

/// Ticks lean the same way at both ends of a dimension line whichever way they point.
fn canonical(v: Vec2) -> Vec2 {
    if v.x < -1e-12 || (v.x.abs() <= 1e-12 && v.y < 0.0) { -v } else { v }
}

/// A DIMTSZ tick: an oblique stroke centred on `at`.
pub(super) fn tick(at: Vec2, dir: Vec2, half: f64) -> Vec<Vec2> {
    let d = canonical(dir);
    let t = (d + d.perp()).normalized() * half;
    vec![at - t, at + t]
}
