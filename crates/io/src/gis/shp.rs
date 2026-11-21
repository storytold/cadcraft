//! ESRI Shapefile geometry (`.shp`), from ESRI's published Shapefile Technical Description
//! (1998): a 100-byte header, then records of a big-endian number and length followed by the
//! little-endian shape.

use cadcraft_geom::Vec3;

use super::{Budget, Feature, Polygon, Skipped, Source, area2};
use crate::{IoError, Result};

const FILE_CODE: i32 = 9994;
const HEADER: usize = 100;

/// Whether `bytes` start like a shapefile (file code 9994, big-endian).
pub(super) fn is_shapefile(bytes: &[u8]) -> bool {
    be_i32(bytes, 0) == Some(FILE_CODE)
}

fn bytes4(b: &[u8], at: usize) -> Option<[u8; 4]> {
    b.get(at..at.checked_add(4)?)?.try_into().ok()
}

fn be_i32(b: &[u8], at: usize) -> Option<i32> {
    bytes4(b, at).map(i32::from_be_bytes)
}

fn le_i32(b: &[u8], at: usize) -> Option<i32> {
    bytes4(b, at).map(i32::from_le_bytes)
}

fn le_f64(b: &[u8], at: usize) -> Option<f64> {
    let s: [u8; 8] = b.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(f64::from_le_bytes(s))
}

fn count(b: &[u8], at: usize) -> Option<usize> {
    le_i32(b, at).and_then(|n| usize::try_from(n).ok())
}

/// `n` points of 16 bytes from `at`, with heights from a Z array at `z_at` when there is one.
fn points(b: &[u8], at: usize, n: usize, z_at: Option<usize>) -> Option<Vec<Option<Vec3>>> {
    let end = at.checked_add(n.checked_mul(16)?)?;
    if end > b.len() {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = at + i * 16;
        let (x, y) = (le_f64(b, p)?, le_f64(b, p + 8)?);
        let z = z_at.and_then(|z| le_f64(b, z.checked_add(i.checked_mul(8)?)?)).filter(|z| z.is_finite()).unwrap_or(0.0);
        out.push((x.is_finite() && y.is_finite()).then(|| Vec3::new(x, y, z)));
    }
    Some(out)
}

/// The Z array after a point list ending at `points_end`: past the Z range, when the record
/// holds it whole.
fn z_array(b: &[u8], points_end: usize, n: usize) -> Option<usize> {
    let at = points_end.checked_add(16)?;
    (at.checked_add(n.checked_mul(8)?)? <= b.len()).then_some(at)
}

enum Shape {
    Null,
    Points(Vec<Vec3>),
    Parts { polygon: bool, parts: Vec<Vec<Vec3>> },
    Skip(&'static str),
}

fn shape(c: &[u8]) -> Option<Shape> {
    let kind = le_i32(c, 0)?;
    let has_z = matches!(kind, 11 | 13 | 15 | 18);
    Some(match kind {
        0 => Shape::Null,
        1 | 11 | 21 => {
            let (x, y) = (le_f64(c, 4)?, le_f64(c, 12)?);
            let z = if kind == 11 { le_f64(c, 20).filter(|z| z.is_finite()).unwrap_or(0.0) } else { 0.0 };
            if !(x.is_finite() && y.is_finite()) {
                return Some(Shape::Skip("points without coordinates"));
            }
            Shape::Points(vec![Vec3::new(x, y, z)])
        }
        8 | 18 | 28 => {
            let n = count(c, 36)?;
            let end = 40usize.checked_add(n.checked_mul(16)?)?;
            let z = if has_z { z_array(c, end, n) } else { None };
            Shape::Points(points(c, 40, n, z)?.into_iter().flatten().collect())
        }
        3 | 5 | 13 | 15 | 23 | 25 => {
            let (np, n) = (count(c, 36)?, count(c, 40)?);
            let at = 44usize.checked_add(np.checked_mul(4)?)?;
            let end = at.checked_add(n.checked_mul(16)?)?;
            if end > c.len() {
                return None;
            }
            let z = if has_z { z_array(c, end, n) } else { None };
            let pts = points(c, at, n, z)?;
            let mut starts = Vec::with_capacity(np);
            for i in 0..np {
                starts.push(count(c, 44 + i * 4)?);
            }
            let mut parts = Vec::with_capacity(np);
            for (i, &s) in starts.iter().enumerate() {
                let e = starts.get(i + 1).copied().unwrap_or(n);
                let part = pts.get(s..e)?;
                parts.push(part.iter().flatten().copied().collect());
            }
            Shape::Parts { polygon: matches!(kind, 5 | 15 | 25), parts }
        }
        31 => Shape::Skip("multipatch shapes"),
        _ => Shape::Skip("shapes of unknown types"),
    })
}

/// A ring without its closing vertex and repeated vertices; `None` when fewer than three remain.
fn ring(mut pts: Vec<Vec3>) -> Option<Vec<Vec3>> {
    pts.dedup();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    (pts.len() >= 3).then_some(pts)
}

/// Whether `p` lies inside `ring` (even-odd rule).
fn inside(p: Vec3, ring: &[Vec3]) -> bool {
    let mut inside = false;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

fn bounds(ring: &[Vec3]) -> (f64, f64, f64, f64) {
    ring.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |(x0, y0, x1, y1), p| {
        (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y))
    })
}

const GRID_BIG: usize = 64;

/// Outer rings bucketed by bounding box on a grid, so a hole looks only at rings near its point.
struct RingIndex {
    x0: f64,
    y0: f64,
    cw: f64,
    ch: f64,
    n: usize,
    cells: Vec<Vec<usize>>,
    big: Vec<usize>,
    boxes: Vec<(f64, f64, f64, f64)>,
}

impl RingIndex {
    fn new(outer: &[Polygon]) -> Self {
        let boxes: Vec<_> = outer.iter().map(|p| bounds(&p.outer)).collect();
        let (x0, y0, x1, y1) = boxes.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| {
            (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
        });
        let n = (outer.len() as f64).sqrt().ceil().clamp(1.0, 1024.0) as usize;
        let cw = ((x1 - x0) / n as f64).max(f64::MIN_POSITIVE);
        let ch = ((y1 - y0) / n as f64).max(f64::MIN_POSITIVE);
        let mut ix = RingIndex { x0, y0, cw, ch, n, cells: vec![Vec::new(); n * n], big: Vec::new(), boxes };
        for i in 0..outer.len() {
            let (bx0, by0, bx1, by1) = ix.boxes[i];
            let (c0, c1, r0, r1) = (ix.col(bx0), ix.col(bx1), ix.row(by0), ix.row(by1));
            if (c1 - c0 + 1) * (r1 - r0 + 1) > GRID_BIG {
                ix.big.push(i);
                continue;
            }
            for r in r0..=r1 {
                for c in c0..=c1 {
                    ix.cells[r * n + c].push(i);
                }
            }
        }
        ix
    }

    fn col(&self, x: f64) -> usize {
        (((x - self.x0) / self.cw).max(0.0) as usize).min(self.n - 1)
    }

    fn row(&self, y: f64) -> usize {
        (((y - self.y0) / self.ch).max(0.0) as usize).min(self.n - 1)
    }

    fn candidates(&self, p: Vec3) -> impl Iterator<Item = usize> + '_ {
        let cell = self.row(p.y) * self.n + self.col(p.x);
        self.cells.get(cell).into_iter().flatten().chain(&self.big).copied().filter(move |&i| {
            let (x0, y0, x1, y1) = self.boxes[i];
            p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
        })
    }
}

/// A shapefile polygon's rings as polygons: clockwise rings are outer rings, counter-clockwise
/// ones holes of the outer ring around them. A hole no outer ring holds stands alone.
fn polygons(rings: Vec<Vec<Vec3>>) -> Vec<Polygon> {
    let (outer, holes): (Vec<_>, Vec<_>) = rings.into_iter().partition(|r| area2(r) <= 0.0);
    let mut out: Vec<Polygon> = outer.into_iter().map(|outer| Polygon { outer, holes: Vec::new() }).collect();
    if holes.is_empty() {
        return out;
    }
    let index = RingIndex::new(&out);
    let mut alone = Vec::new();
    for h in holes {
        let first = h.first().copied().unwrap_or(Vec3::ZERO);
        let best = index
            .candidates(first)
            .filter(|&i| inside(first, &out[i].outer))
            .min_by(|&a, &b| area2(&out[a].outer).abs().total_cmp(&area2(&out[b].outer).abs()));
        match best {
            Some(i) => out[i].holes.push(h),
            None => alone.push(Polygon { outer: h, holes: Vec::new() }),
        }
    }
    out.extend(alone);
    out
}

/// Read a `.shp` file's shapes, one feature per record in file order (null shapes included, so
/// the features line up with the attribute table's records).
pub(super) fn read(b: &[u8], budget: &mut Budget) -> Result<Source> {
    if !is_shapefile(b) || b.len() < HEADER {
        return Err(IoError::Format("not an ESRI shapefile (.shp)".into()));
    }
    let mut src = Source::default();
    let mut skipped = Skipped::default();
    let mut at = HEADER;
    while at.checked_add(8).is_some_and(|e| e <= b.len()) {
        let words = be_i32(b, at + 4).and_then(|n| usize::try_from(n).ok());
        let Some(c) = words.and_then(|w| w.checked_mul(2)).and_then(|len| b.get(at + 8..(at + 8).checked_add(len)?)) else {
            src.notes.push(format!("GIS: the shapefile ends inside record {}; the shapes before it were imported", src.features.len() + 1));
            break;
        };
        at += 8 + c.len();
        budget.take_feature()?;
        let mut f = Feature::default();
        match shape(c) {
            None => skipped.add("damaged records"),
            Some(Shape::Null) => skipped.add("null shapes"),
            Some(Shape::Skip(what)) => skipped.add(what),
            Some(Shape::Points(p)) => {
                budget.take_verts(p.len())?;
                f.points = p;
            }
            Some(Shape::Parts { polygon, parts }) => {
                budget.take_verts(parts.iter().map(Vec::len).sum())?;
                if polygon {
                    f.polygons = polygons(parts.into_iter().filter_map(ring).collect());
                } else {
                    f.lines = parts
                        .into_iter()
                        .map(|mut p| {
                            p.dedup();
                            p
                        })
                        .filter(|p| p.len() >= 2)
                        .collect();
                }
            }
        }
        src.features.push(f);
    }
    src.notes.extend(skipped.note("shapefile"));
    Ok(src)
}
