//! Isometric drafting: the isometric snap lattice and grid (SNAPSTYL 1), the current isometric
//! plane (SNAPISOPAIR, ISOPLANE), ortho along the plane's axes and isometric circles.
//!
//! Isometric axes run at 30°, 90° and 150°. Each plane is drawn on two of them: Left on 90° and
//! 150°, Top on 30° and 150°, Right on 30° and 90°. Lengths along the axes are true lengths, so
//! the snap lattice is triangular: its points sit the snap Y spacing apart along every axis.

use cadcraft_geom::{Bounds2, Ellipse, Vec2};
use serde::{Deserialize, Serialize};

use crate::Settings;

/// cos 30°.
const COS30: f64 = 0.866_025_403_784_438_6;
/// The most lines one family of the isometric grid draws.
pub const MAX_GRID_LINES: usize = 2000;

/// The current isometric plane (SNAPISOPAIR 0, 1, 2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IsoPlane {
    #[default]
    Left,
    Top,
    Right,
}

impl IsoPlane {
    pub const ALL: [IsoPlane; 3] = [IsoPlane::Left, IsoPlane::Top, IsoPlane::Right];

    /// SNAPISOPAIR.
    pub fn index(self) -> i64 {
        match self {
            IsoPlane::Left => 0,
            IsoPlane::Top => 1,
            IsoPlane::Right => 2,
        }
    }

    pub fn from_index(i: i64) -> Option<IsoPlane> {
        IsoPlane::ALL.get(usize::try_from(i).ok()?).copied()
    }

    /// The plane a keyword or name means (`left`, `t`…), case-insensitive.
    pub fn parse(text: &str) -> Option<IsoPlane> {
        let t = text.trim().to_ascii_lowercase();
        if t.is_empty() {
            return None;
        }
        IsoPlane::ALL.into_iter().find(|p| p.name().to_ascii_lowercase().starts_with(&t))
    }

    pub fn name(self) -> &'static str {
        match self {
            IsoPlane::Left => "Left",
            IsoPlane::Top => "Top",
            IsoPlane::Right => "Right",
        }
    }

    /// The plane F5 / Ctrl+E switches to: Left, Top, Right, Left…
    pub fn next(self) -> IsoPlane {
        match self {
            IsoPlane::Left => IsoPlane::Top,
            IsoPlane::Top => IsoPlane::Right,
            IsoPlane::Right => IsoPlane::Left,
        }
    }

    /// The directions (radians) of the plane's two axes.
    pub fn axes(self) -> [f64; 2] {
        let d = f64::to_radians;
        match self {
            IsoPlane::Left => [d(90.0), d(150.0)],
            IsoPlane::Top => [d(30.0), d(150.0)],
            IsoPlane::Right => [d(30.0), d(90.0)],
        }
    }
}

/// The plane's axis directions when isometric drafting is on, else the X and Y axes (radians).
pub fn cursor_axes(s: &Settings) -> [f64; 2] {
    if s.isodraft { s.isoplane.axes() } else { [0.0, std::f64::consts::FRAC_PI_2] }
}

/// The isometric lattice spacing: the snap Y spacing (a positive finite number, or 1).
fn spacing(d: f64) -> Option<f64> {
    (d.is_finite() && d > 1e-12).then_some(d)
}

/// The nearest point of the isometric lattice of `d` spacing through `origin`, turned by `angle`
/// (radians) about it. The lattice's points are `i·(d cos 30°, d sin 30°) + j·(0, d)`.
pub fn lattice_snap(p: Vec2, d: f64, origin: Vec2, angle: f64) -> Vec2 {
    let Some(d) = spacing(d) else { return p };
    let angle = if angle.is_finite() { angle } else { 0.0 };
    let q = (p - origin).rotate(-angle);
    let (u, v) = (Vec2::new(d * COS30, d * 0.5), Vec2::new(0.0, d));
    // Lattice coordinates; the nearest point is a corner of the cell holding `q`.
    let i = q.x / u.x;
    let j = (q.y - i * u.y) / d;
    if !(i.is_finite() && j.is_finite()) {
        return p;
    }
    let (i0, j0) = (i.floor(), j.floor());
    let mut best = (f64::INFINITY, q);
    for (di, dj) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let c = u * (i0 + di) + v * (j0 + dj);
        let dd = c.dist(q);
        if dd < best.0 {
            best = (dd, c);
        }
    }
    let s = origin + best.1.rotate(angle);
    if s.is_finite() { s } else { p }
}

/// Grid snap for a cursor update: with snap on and isometric style, the cursor snapped to the
/// isometric lattice, and the settings with grid snap off so the rectangular grid leaves it be.
/// Otherwise both unchanged, also when PolarSnap (SNAPTYPE 1) is the snap type: snap mode then
/// steps along the tracking paths instead.
pub(crate) fn grid_snap(doc: &cadcraft_doc::Drawing, space: &cadcraft_doc::Space, mut s: Settings, p: Vec2) -> (Vec2, Settings) {
    if !(s.snapmode && s.isodraft) || s.polarsnap {
        return (p, s);
    }
    let (origin, angle) = super::grid_frame_in(doc, space);
    s.snapmode = false;
    (lattice_snap(p, s.snapunit.y, origin, angle), s)
}

/// The plane axis (unit vector, either way along it) nearest the direction `base`→`p`.
fn nearest_axis(plane: IsoPlane, d: Vec2) -> Vec2 {
    let mut best = (f64::NEG_INFINITY, Vec2::X);
    for a in plane.axes() {
        let dir = Vec2::from_angle(a);
        let dot = d.dot(dir);
        let (k, dir) = if dot < 0.0 { (-dot, dir * -1.0) } else { (dot, dir) };
        if k > best.0 {
            best = (k, dir);
        }
    }
    best.1
}

/// Ortho without snapping: `p` constrained relative to `base` to the nearest axis of the current
/// isometric plane (or to horizontal/vertical when isometric drafting is off).
pub fn constrain(s: &Settings, base: Vec2, p: Vec2) -> Vec2 {
    // Along the current UCS's axes (`s.ucs_angle`), the isometric ones turned with it.
    if !s.isodraft {
        return super::ortho_at(base, p, s.ucs_angle);
    }
    let a = if s.ucs_angle.is_finite() { s.ucs_angle } else { 0.0 };
    let dir = nearest_axis(s.isoplane, (p - base).rotate(-a)).rotate(a);
    let r = base + dir * (p - base).dot(dir);
    if r.is_finite() { r } else { p }
}

/// Ortho at the cursor: [`constrain`], and with isometric snap on, whole snap steps along the
/// axis from `base`.
pub fn ortho(s: &Settings, base: Vec2, p: Vec2) -> Vec2 {
    let r = constrain(s, base, p);
    match spacing(s.snapunit.y).filter(|_| s.isodraft && s.snapmode) {
        Some(d) => {
            let t = r.dist(base);
            let dir = (r - base).normalized();
            let snapped = base + dir * ((t / d).round() * d);
            if snapped.is_finite() { snapped } else { r }
        }
        None => r,
    }
}

/// The coordinates of `v` in the plane's axis basis (`v = a·axis0 + b·axis1`).
fn plane_coords(plane: IsoPlane, v: Vec2) -> Option<(f64, f64)> {
    let [a0, a1] = plane.axes().map(Vec2::from_angle);
    let det = a0.cross(a1);
    if det.abs() < 1e-12 {
        return None;
    }
    let a = v.cross(a1) / det;
    let b = a0.cross(v) / det;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

/// The radius, measured in the isometric plane, of the isocircle about `center` through `p`.
pub fn iso_radius(plane: IsoPlane, center: Vec2, p: Vec2) -> Option<f64> {
    let (a, b) = plane_coords(plane, p - center)?;
    let r = a.hypot(b);
    (r.is_finite() && r > 1e-12).then_some(r)
}

/// A circle of `radius` in the isometric plane, as drawn: an ellipse whose minor axis runs along
/// the isometric axis the plane doesn't use, with ratio 1/√3 and semi-major axis radius·√(3/2).
pub fn isocircle(plane: IsoPlane, center: Vec2, radius: f64) -> Option<Ellipse> {
    if !(radius.is_finite() && radius > 1e-12 && center.is_finite()) {
        return None;
    }
    let other = match plane {
        IsoPlane::Left => 30.0f64,
        IsoPlane::Top => 90.0,
        IsoPlane::Right => 150.0,
    };
    let major = Vec2::from_angle((other + 90.0).to_radians()) * (radius * 1.5f64.sqrt());
    Some(Ellipse::full(center, major, 1.0 / 3.0f64.sqrt()))
}

/// One line of the isometric grid, clipped to the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLine {
    pub a: Vec2,
    pub b: Vec2,
    pub major: bool,
}

/// The isometric grid lines (30°, 90° and 150° families through the lattice of `d` spacing at
/// `origin`, turned by `angle`) that cross `view`; every `major_every`th line of a family is a
/// major line. Empty when a family would need more than [`MAX_GRID_LINES`] lines.
pub fn grid_lines(view: Bounds2, d: f64, origin: Vec2, angle: f64, major_every: u32) -> Vec<GridLine> {
    let Some(d) = spacing(d) else { return Vec::new() };
    // Neighbouring lines of a family are d·cos 30° apart.
    families(view, &[30.0, 90.0, 150.0], d * COS30, origin, angle, major_every)
}

/// The rectangular grid lines (lines along X and along Y, `d` apart, through `origin`, turned by
/// `angle`) that cross `view`; every `major_every`th line of a family is a major line. Empty when
/// a family would need more than [`MAX_GRID_LINES`] lines.
pub fn rect_grid_lines(view: Bounds2, d: f64, origin: Vec2, angle: f64, major_every: u32) -> Vec<GridLine> {
    let Some(d) = spacing(d) else { return Vec::new() };
    families(view, &[0.0, 90.0], d, origin, angle, major_every)
}

/// Lines along each of `degs` (turned by `angle`), `gap` apart, one through `origin`, clipped to
/// `view`.
fn families(view: Bounds2, degs: &[f64], gap: f64, origin: Vec2, angle: f64, major_every: u32) -> Vec<GridLine> {
    let angle = if angle.is_finite() { angle } else { 0.0 };
    if !(view.min.is_finite() && view.max.is_finite() && origin.is_finite()) {
        return Vec::new();
    }
    let corners = [view.min, Vec2::new(view.max.x, view.min.y), view.max, Vec2::new(view.min.x, view.max.y)];
    let major_every = i64::from(major_every.max(1));
    let mut out = Vec::new();
    for &deg in degs {
        let dir = Vec2::from_angle(deg.to_radians() + angle);
        let n = dir.perp();
        let off = origin.dot(n);
        let (lo, hi) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            let k = (c.dot(n) - off) / gap;
            (lo.min(k), hi.max(k))
        });
        let (k0, k1) = (lo.floor(), hi.ceil());
        if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > MAX_GRID_LINES as f64 {
            return Vec::new();
        }
        let (k0, k1) = (k0 as i64, k1 as i64);
        for k in k0..=k1 {
            let p0 = n * (off + k as f64 * gap);
            if let Some((a, b)) = clip(view, p0, dir) {
                out.push(GridLine { a, b, major: k.rem_euclid(major_every) == 0 });
            }
        }
    }
    out
}

/// The part of the line through `p` along unit `dir` inside `view` (Liang–Barsky).
fn clip(view: Bounds2, p: Vec2, dir: Vec2) -> Option<(Vec2, Vec2)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (pc, dc, lo, hi) in [(p.x, dir.x, view.min.x, view.max.x), (p.y, dir.y, view.min.y, view.max.y)] {
        if dc.abs() < 1e-15 {
            if pc < lo || pc > hi {
                return None;
            }
        } else {
            let (a, b) = ((lo - pc) / dc, (hi - pc) / dc);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0.is_finite() && t1.is_finite() && t1 > t0).then(|| (p + dir * t0, p + dir * t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_points_are_spacing_apart_along_each_axis() {
        let d = 2.0;
        for deg in [30.0f64, 90.0, 150.0] {
            let p = Vec2::from_angle(deg.to_radians()) * (3.0 * d);
            assert!(lattice_snap(p + Vec2::new(0.1, -0.05), d, Vec2::ZERO, 0.0).near(p, 1e-9));
        }
        // Halfway along a lattice step is not a lattice point.
        let h = Vec2::new(0.0, 3.0);
        let s = lattice_snap(h, d, Vec2::ZERO, 0.0);
        assert!(s.near(Vec2::new(0.0, 2.0), 1e-9) || s.near(Vec2::new(0.0, 4.0), 1e-9));
    }

    #[test]
    fn grid_lines_bounded() {
        let v = Bounds2 { min: Vec2::new(-10.0, -10.0), max: Vec2::new(10.0, 10.0) };
        let lines = grid_lines(v, 1.0, Vec2::ZERO, 0.0, 5);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| l.a.is_finite() && l.b.is_finite()));
        assert!(grid_lines(v, 1e-9, Vec2::ZERO, 0.0, 5).is_empty());
        assert!(grid_lines(v, f64::NAN, Vec2::ZERO, 0.0, 5).is_empty());
    }

    /// The rectangular grid: lines along X and Y through the snap base, `d` apart, every
    /// `major_every`th major, all turned by SNAPANG (in the World UCS too) and clipped to the view.
    #[test]
    fn rect_grid_lines_lie_in_the_snap_frame() {
        let v = Bounds2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(10.0, 10.0) };
        let lines = rect_grid_lines(v, 1.0, Vec2::ZERO, 0.0, 5);
        // 0..=10 along each axis.
        assert_eq!(lines.len(), 22);
        let inside = |p: Vec2| p.x >= -1e-9 && p.x <= 10.0 + 1e-9 && p.y >= -1e-9 && p.y <= 10.0 + 1e-9;
        assert!(lines.iter().all(|l| inside(l.a) && inside(l.b)));
        let vertical_major: Vec<f64> = lines.iter().filter(|l| (l.a.x - l.b.x).abs() < 1e-9 && l.major).map(|l| l.a.x).collect();
        assert_eq!(vertical_major.len(), 3, "x = 0, 5 and 10");
        assert!(vertical_major.iter().all(|x| (x / 5.0 - (x / 5.0).round()).abs() < 1e-9));
        // Turned 30° about a base at (0.5, 0.25): every line runs at 30° or 120°, and one of each
        // passes through the base.
        let (base, a) = (Vec2::new(0.5, 0.25), 30f64.to_radians());
        let turned = rect_grid_lines(v, 1.0, base, a, 5);
        assert!(!turned.is_empty());
        let (ux, uy) = (Vec2::from_angle(a), Vec2::from_angle(a).perp());
        for l in &turned {
            let d = (l.b - l.a).normalized();
            assert!(d.cross(ux).abs() < 1e-9 || d.cross(uy).abs() < 1e-9, "{l:?}");
            // Each line is a whole number of steps from the base across its direction.
            let k = (l.a - base).cross(d);
            assert!((k - k.round()).abs() < 1e-9, "{l:?}");
        }
        let through = |dir: Vec2| turned.iter().any(|l| (l.b - l.a).normalized().cross(dir).abs() < 1e-9 && (base - l.a).cross(dir).abs() < 1e-9);
        assert!(through(ux) && through(uy));
        // Too many lines, or no spacing: nothing drawn.
        assert!(rect_grid_lines(v, 1e-6, Vec2::ZERO, 0.0, 5).is_empty());
        assert!(rect_grid_lines(v, f64::NAN, Vec2::ZERO, 0.0, 5).is_empty());
        assert!(rect_grid_lines(v, 1.0, Vec2::new(f64::INFINITY, 0.0), 0.0, 5).is_empty());
    }

    /// A view far larger than any grid (a zoom to hostile extents) draws no grid instead of
    /// overflowing the line count.
    #[test]
    fn rect_grid_lines_survive_extreme_views() {
        let huge = Bounds2::new(Vec2::new(-1e300, -1e300), Vec2::new(1e300, 1e300));
        assert!(rect_grid_lines(huge, 1.0, Vec2::ZERO, 0.0, 5).is_empty());
        let far = Bounds2::new(Vec2::new(1e300, 1e300), Vec2::new(1e300 + 1.0, 1e300 + 1.0));
        assert!(rect_grid_lines(far, 1.0, Vec2::ZERO, 0.0, 5).len() <= 2 * (MAX_GRID_LINES + 1));
        let normal = Bounds2::new(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 10.0));
        assert_eq!(rect_grid_lines(normal, 1.0, Vec2::ZERO, 0.0, 5).len(), 22);
    }
}
