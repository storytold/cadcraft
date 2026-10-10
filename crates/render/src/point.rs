//! POINT display figures: point style (`PDMODE`) and size (`PDSIZE`).

use cadcraft_geom::{Circle, Vec2};

/// The figure a POINT draws.
#[derive(Debug, Default, PartialEq)]
pub struct Figure {
    /// A one-pixel dot at the point.
    pub dot: bool,
    /// Line work (polylines).
    pub lines: Vec<Vec<Vec2>>,
}

/// Figure size in drawing units: a positive `PDSIZE` is absolute, a negative one a percentage
/// of `view_height`, and 0 is 5 % of `view_height`.
pub fn size(pdsize: f64, view_height: f64) -> f64 {
    if pdsize.is_finite() && pdsize > 0.0 {
        return pdsize;
    }
    let percent = if pdsize.is_finite() && pdsize < 0.0 { -pdsize } else { 5.0 };
    view_height * percent / 100.0
}

/// Whether `PDMODE` `mode` draws a figure with a size (anything but a dot or nothing).
pub fn sized(mode: i64) -> bool {
    matches!(mode & 31, 2..=4) || mode & 96 != 0
}

/// The figure for `PDMODE` `mode`, `size` across, at `at`, turned by `angle` (radians). The low
/// bits pick dot (0), nothing (1), plus (2), cross (3) or a tick upwards (4); +32 adds a
/// circle and +64 a square. Without a usable size every visible figure falls back to a dot.
pub fn figure(mode: i64, size: f64, at: Vec2, angle: f64, tol: f64) -> Figure {
    let base = mode & 31;
    let mut f = Figure::default();
    let r = size / 2.0;
    if !(r.is_finite() && r > 0.0) {
        f.dot = base != 1;
        return f;
    }
    let angle = if angle.is_finite() { angle } else { 0.0 };
    let p = |x: f64, y: f64| at + Vec2::new(x, y).rotate(angle);
    match base {
        1 => {}
        2 => f.lines.extend([vec![p(-r, 0.0), p(r, 0.0)], vec![p(0.0, -r), p(0.0, r)]]),
        3 => f.lines.extend([vec![p(-r, -r), p(r, r)], vec![p(-r, r), p(r, -r)]]),
        4 => f.lines.push(vec![at, p(0.0, r)]),
        _ => f.dot = true,
    }
    if mode & 32 != 0 {
        let mut pts = Vec::new();
        Circle { center: at, radius: r }.tessellate(tol.max(r * 1e-4), &mut pts);
        f.lines.push(pts);
    }
    if mode & 64 != 0 {
        f.lines.push(vec![p(-r, -r), p(r, -r), p(r, r), p(-r, r), p(-r, -r)]);
    }
    f
}
