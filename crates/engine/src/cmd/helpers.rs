//! Shared geometry helpers for commands.

use cadcraft_doc::{EntityKind, Line};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

pub fn v3(p: Vec2) -> Vec3 {
    p.to3(0.0)
}

pub fn line(a: Vec2, b: Vec2) -> EntityKind {
    EntityKind::Line(Line { a: v3(a), b: v3(b) })
}

pub fn circle(c: Vec2, r: f64) -> EntityKind {
    EntityKind::Circle(cadcraft_doc::Circle { center: v3(c), radius: r.abs() })
}

pub fn arc(a: &cadcraft_geom::Arc) -> EntityKind {
    EntityKind::Arc(cadcraft_doc::Arc { center: v3(a.center), radius: a.radius, start: a.start, end: a.end })
}

pub fn lwpoly(vs: Vec<PolyVertex>, closed: bool) -> EntityKind {
    EntityKind::LwPolyline(cadcraft_doc::LwPolyline { vertices: vs, closed, const_width: 0.0, elevation: 0.0, plinegen: false })
}

pub fn rect_vertices(a: Vec2, b: Vec2) -> Vec<PolyVertex> {
    [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].into_iter().map(PolyVertex::new).collect()
}

/// Regular polygon vertices: inscribed (vertex at `radius`) or circumscribed (edge midpoint at `radius`).
pub fn polygon_vertices(center: Vec2, sides: usize, radius_pt: Vec2, inscribed: bool) -> Vec<PolyVertex> {
    let n = sides.clamp(3, 1024);
    let d = radius_pt - center;
    let r = d.len();
    let a0 = d.angle();
    let step = cadcraft_geom::TAU / n as f64;
    // Circumscribed: the picked point is an edge's midpoint and the first vertex is the end of
    // that edge counter-clockwise from it, as AutoCAD 2024 numbers them.
    let (r, a0) = if inscribed { (r, a0) } else { (r / (step / 2.0).cos(), a0 + step / 2.0) };
    (0..n).map(|i| PolyVertex::new(Vec2::polar(center, r, a0 + step * i as f64))).collect()
}

/// The radius point for a polygon whose radius is typed: the bottom edge is horizontal in both
/// modes (inscribed: a vertex half a side's angle past straight down; circumscribed: the bottom
/// edge's midpoint straight down).
pub fn polygon_typed_radius_point(center: Vec2, sides: usize, radius: f64, inscribed: bool) -> Vec2 {
    let down = -std::f64::consts::FRAC_PI_2;
    let a = if inscribed { down + std::f64::consts::PI / sides.clamp(3, 1024) as f64 } else { down };
    Vec2::polar(center, radius, a)
}

/// Polygon from an edge (first two vertices).
pub fn polygon_from_edge(p1: Vec2, p2: Vec2, sides: usize) -> Vec<PolyVertex> {
    let n = sides.clamp(3, 1024);
    let step = cadcraft_geom::TAU / n as f64;
    let mut pts = vec![p1, p2];
    let len = p1.dist(p2);
    let mut ang = p1.angle_to(p2);
    for _ in 2..n {
        ang += step;
        let last = pts.last().copied().unwrap_or(p2);
        pts.push(Vec2::polar(last, len, ang));
    }
    pts.into_iter().map(PolyVertex::new).collect()
}

/// Rectangle vertices from corner `a` to the opposite corner `b`, with a fillet radius or
/// chamfer distances at every corner, in AutoCAD 2024's order: from `a` along X to `b`'s X, then
/// `b`, then back along Y. A chamfer cuts the edge into each corner by its first distance and the
/// edge out of it by its second; each fillet arc starts on the edge into its corner. A fillet or
/// chamfer that doesn't fit leaves the corners square.
pub fn rect_with_corners(a: Vec2, b: Vec2, fillet: f64, chamfer: (f64, f64)) -> Vec<PolyVertex> {
    let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
    let short = (b.x - a.x).abs().min((b.y - a.y).abs());
    let (d_in, d_out, bulge) = if fillet > 0.0 && fillet * 2.0 <= short + 1e-12 {
        // Counter-clockwise round the corners (a positive bulge) when `b` is up and right of `a`, or
        // down and left.
        let sign = if (b.x - a.x) * (b.y - a.y) >= 0.0 { 1.0 } else { -1.0 };
        (fillet, fillet, sign * cadcraft_geom::arc_to_bulge(std::f64::consts::FRAC_PI_2))
    } else if chamfer.0 > 0.0 && chamfer.1 > 0.0 && chamfer.0 + chamfer.1 <= short + 1e-12 {
        (chamfer.0, chamfer.1, 0.0)
    } else {
        return corners.into_iter().map(PolyVertex::new).collect();
    };
    let mut out = Vec::with_capacity(8);
    for (i, cur) in corners.iter().enumerate() {
        let (Some(prev), Some(next)) = (corners.get((i + 3) % 4), corners.get((i + 1) % 4)) else { continue };
        out.push(PolyVertex::with_bulge(*cur + (*prev - *cur).normalized() * d_in, bulge));
        out.push(PolyVertex::new(*cur + (*next - *cur).normalized() * d_out));
    }
    out
}

// ---------- the user's support files ----------

/// Largest support file (linetype, hatch pattern or alias file) read.
pub const MAX_SUPPORT_FILE_BYTES: u64 = 8 << 20;

/// The text of a support file the user gives: a path in `{path_key}` (read from disk; the web has
/// no file system) or the contents in `{text_key}`. Returns the text and a name for messages, or
/// `None` when neither is given. Files over [`MAX_SUPPORT_FILE_BYTES`] are refused; bytes that
/// aren't UTF-8 are read as replacement characters.
pub fn support_text(cmd: &str, p: &serde_json::Value, path_key: &str, text_key: &str) -> crate::Result<Option<(String, String)>> {
    if let Some(text) = p.get(text_key).and_then(serde_json::Value::as_str) {
        if text.len() as u64 > MAX_SUPPORT_FILE_BYTES {
            return Err(super::bad(cmd, format!("`{text_key}` is larger than {} MB", MAX_SUPPORT_FILE_BYTES >> 20)));
        }
        return Ok(Some((text.to_string(), "the given text".into())));
    }
    match p.get(path_key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(path)) => Ok(Some((read_support_file(cmd, path)?, path.clone()))),
        Some(_) => Err(super::bad(cmd, format!("`{path_key}` must be a file path"))),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_support_file(cmd: &str, path: &str) -> crate::Result<String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| super::bad(cmd, format!("{path}: {e}")))?;
    let mut bytes = Vec::new();
    f.take(MAX_SUPPORT_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| super::bad(cmd, format!("{path}: {e}")))?;
    if bytes.len() as u64 > MAX_SUPPORT_FILE_BYTES {
        return Err(super::bad(cmd, format!("{path}: larger than {} MB", MAX_SUPPORT_FILE_BYTES >> 20)));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_arch = "wasm32")]
fn read_support_file(cmd: &str, _path: &str) -> crate::Result<String> {
    Err(super::bad(cmd, "paths are not available on the web; pass the file's contents"))
}
