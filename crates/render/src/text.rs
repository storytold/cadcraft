//! Filled font glyphs: bounded font-unit meshes shared across placements and zoom rebuilds.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use cadcraft_fonts::{TextGlyph, ttf};
use cadcraft_geom::{Bounds2, Mat3, Vec2};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{FillOptions, FillTessellator, FillVertex};

const MAX_POINTS: usize = 16_384;
const MAX_ENTRIES: usize = 8192;
const MAX_CACHE_BYTES: usize = 64 << 20;
const MAX_SIMPLIFY_STEPS: usize = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Detail {
    Full,
    /// A power-of-two tolerance in font units, rounded toward finer detail.
    Reduced(i16),
    /// An em occupies at most two pixels in every direction: show the ink's extent.
    Box,
}

type Key = (usize, usize, u32, u16, Detail);

pub(super) struct Mesh {
    contours: Arc<[Vec<Vec2>]>,
    pub triangles: Vec<Vec2>,
}

struct Entry {
    // Keep the bytes alive: registering a replacement font must never reuse this key's address.
    _face: ttf::Face,
    mesh: Arc<Mesh>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
}

/// A read-write lock: display lists are built on all cores and nearly every lookup is a hit.
fn cache() -> &'static RwLock<Cache> {
    static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Cache::default()))
}

/// World-space placement of font units, including the block/viewport transform.
pub(super) fn placement(g: &TextGlyph, xf: Mat3) -> Mat3 {
    xf.then_before(Mat3 { a: g.x.x / 1000.0, b: g.x.y / 1000.0, c: g.y.x / 1000.0, d: g.y.y / 1000.0, e: g.origin.x, f: g.origin.y })
}

/// Whether a glyph whose ink spans `ink` (font units, 1000 per em; see [`ttf::glyph_bounds`])
/// can reach into `view` when placed by `xf`: the placed corners of its ink box overlap the
/// area. The box is the outline's own, so flourishes, tall stacks and large operators that
/// reach far outside the em are kept, and testing it costs four points however long the
/// outline is. A glyph without ink shows nothing.
pub(super) fn may_show(xf: Mat3, ink: Bounds2, view: &Bounds2) -> bool {
    if ink.is_empty() {
        return false;
    }
    let corners = [ink.min, Vec2::new(ink.max.x, ink.min.y), ink.max, Vec2::new(ink.min.x, ink.max.y)];
    let placed = corners.map(|c| xf.apply(c));
    // A placement that isn't finite is left to the mesh code, which handles it. Bounds skip
    // non-finite points, so every corner is checked before they are built.
    if placed.iter().any(|p| !p.is_finite()) {
        return true;
    }
    let b = Bounds2::from_points(placed);
    b.is_empty() || b.intersects(view)
}

fn detail(xf: Mat3, pixel: f64) -> Detail {
    // The Frobenius norm bounds the largest stretch, including shear, mirroring and unequal
    // block scales. A font-unit error below pixel / stretch is below a pixel in every direction.
    let stretch = xf.a.hypot(xf.b).hypot(xf.c.hypot(xf.d));
    if !(pixel.is_finite() && pixel > 0.0 && stretch.is_finite() && stretch > 0.0) {
        return Detail::Full;
    }
    if stretch * 1000.0 <= pixel * 2.0 {
        return Detail::Box;
    }
    let tolerance = pixel * 0.25 / stretch;
    if !tolerance.is_finite() || tolerance < 0.0625 { Detail::Full } else { Detail::Reduced(tolerance.log2().floor().clamp(-4.0, 16.0) as i16) }
}

/// Most meshes each thread keeps in front of the shared cache.
const MAX_LOCAL_ENTRIES: usize = 4096;
/// Most memory each thread's meshes hold: display lists are built on every core, and CJK text at
/// full detail makes large meshes, so a count alone doesn't bound it. A mesh above a quarter of
/// it is read from the shared cache each time instead.
const MAX_LOCAL_BYTES: usize = 16 << 20;

#[derive(Default)]
struct Local {
    entries: HashMap<Key, Entry>,
    bytes: usize,
}

thread_local! {
    /// This thread's meshes, in front of the shared [`cache`]: display lists are built on all
    /// cores, and every placed glyph reading the shared lock made text-heavy drawings (CJK text
    /// is all filled glyphs) wait on it (#662, #666). Entries keep their face alive like the
    /// shared ones, so a key's address is never reused while it is here.
    static LOCAL: std::cell::RefCell<Local> = std::cell::RefCell::new(Local::default());
}

pub(super) fn mesh(g: &TextGlyph, xf: Mat3, pixel: f64) -> Arc<Mesh> {
    let detail = detail(xf, pixel);
    let key = (g.face.bytes.as_ptr() as usize, g.face.bytes.len(), g.face.index, g.gid, detail);
    if let Some(m) = LOCAL.with(|l| l.borrow().entries.get(&key).map(|e| e.mesh.clone())) {
        return m;
    }
    let m = mesh_shared(g, key, detail);
    LOCAL.with(|l| {
        let l = &mut *l.borrow_mut();
        remember(&mut l.entries, &mut l.bytes, key, || Entry { _face: g.face.clone(), mesh: m.clone() }, mesh_bytes(&m));
    });
    m
}

/// Keep `value` (about `bytes` of memory) in a per-thread cache holding `total` bytes: started
/// over when it would pass [`MAX_LOCAL_ENTRIES`] or [`MAX_LOCAL_BYTES`], and a value above a
/// quarter of the budget isn't kept at all.
fn remember<T>(entries: &mut HashMap<Key, T>, total: &mut usize, key: Key, value: impl FnOnce() -> T, bytes: usize) {
    if bytes > MAX_LOCAL_BYTES / 4 {
        return;
    }
    if entries.len() >= MAX_LOCAL_ENTRIES || total.saturating_add(bytes) > MAX_LOCAL_BYTES {
        entries.clear();
        *total = 0;
    }
    if entries.insert(key, value()).is_none() {
        *total = total.saturating_add(bytes);
    }
}

/// About how much memory a cached mesh holds (its points and triangles as allocated).
fn mesh_bytes(mesh: &Mesh) -> usize {
    let points = mesh.contours.iter().map(Vec::capacity).sum::<usize>().saturating_add(mesh.triangles.capacity());
    points
        .saturating_mul(std::mem::size_of::<Vec2>())
        .saturating_add(mesh.contours.len().saturating_mul(std::mem::size_of::<Vec<Vec2>>()))
        .saturating_add(std::mem::size_of::<Entry>() + std::mem::size_of::<Key>() + std::mem::size_of::<Mesh>())
}

/// [`mesh`] through the shared cache.
fn mesh_shared(g: &TextGlyph, key: Key, detail: Detail) -> Arc<Mesh> {
    if let Some(entry) = cache().read().unwrap_or_else(PoisonError::into_inner).entries.get(&key) {
        return entry.mesh.clone();
    }
    let contours = ttf::glyph_contours(&g.face, g.gid);
    let points = contours.iter().map(Vec::len).try_fold(0usize, usize::checked_add);
    // Font data is untrusted; don't hand unbounded or invalid paths to the tessellator.
    if points.is_none_or(|n| n > MAX_POINTS) || contours.iter().flatten().any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()) > 1.0e6) {
        return Arc::new(Mesh { contours: Arc::default(), triangles: Vec::new() });
    }
    let contours = match detail {
        Detail::Full => contours,
        Detail::Reduced(band) => contours.iter().map(|c| simplify(c, 2f64.powi(i32::from(band)))).collect::<Vec<_>>().into(),
        Detail::Box => {
            let bb = Bounds2::from_points(contours.iter().flatten().copied());
            if bb.is_empty() {
                Arc::default()
            } else {
                vec![vec![bb.min, Vec2::new(bb.max.x, bb.min.y), bb.max, Vec2::new(bb.min.x, bb.max.y), bb.min]].into()
            }
        }
    };
    let mesh = Arc::new(Mesh { triangles: triangulate(&contours), contours });
    let bytes = mesh_bytes(&mesh);
    if bytes <= MAX_CACHE_BYTES {
        let mut c = cache().write().unwrap_or_else(PoisonError::into_inner);
        if c.entries.len() >= MAX_ENTRIES || c.bytes.saturating_add(bytes) > MAX_CACHE_BYTES {
            c.entries.clear();
            c.bytes = 0;
        }
        if !c.entries.contains_key(&key) {
            c.bytes = c.bytes.saturating_add(bytes);
            c.entries.insert(key, Entry { _face: g.face.clone(), mesh: mesh.clone() });
        }
    }
    mesh
}

/// Iterative Douglas-Peucker on a closed contour. Never collapse a hole to a line or recurse
/// on font data; preserve the original if the work budget is exhausted.
fn simplify(contour: &[Vec2], tolerance: f64) -> Vec<Vec2> {
    if contour.len() < 5 {
        return contour.to_vec();
    }
    let mut points = contour.to_vec();
    if let (Some(&first), Some(&last)) = (points.first(), points.last())
        && first != last
    {
        points.push(first);
    }
    let last = points.len().saturating_sub(1);
    let mut keep = vec![false; points.len()];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut pending = vec![(0usize, last)];
    let mut steps = 0usize;
    while let Some((start, end)) = pending.pop() {
        let (Some(&a), Some(&b)) = (points.get(start), points.get(end)) else { continue };
        let ab = b - a;
        let len2 = ab.len2();
        let mut furthest = None;
        let mut distance = tolerance;
        for (i, &p) in points.iter().enumerate().take(end).skip(start + 1) {
            steps += 1;
            if steps > MAX_SIMPLIFY_STEPS {
                return contour.to_vec();
            }
            let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let d = p.dist(a + ab * t);
            if d > distance {
                distance = d;
                furthest = Some(i);
            }
        }
        if let Some(i) = furthest {
            if let Some(k) = keep.get_mut(i) {
                *k = true;
            }
            pending.extend([(start, i), (i, end)]);
        }
    }
    let result: Vec<_> = points.into_iter().zip(keep).filter_map(|(p, keep)| keep.then_some(p)).collect();
    if result.len() >= 4 { result } else { contour.to_vec() }
}

/// Monotone tessellation with the nonzero winding rule that TrueType and OpenType outlines are
/// defined with: overlapping contours stay filled (Arial Unicode's 진 draws its final ㄴ twice,
/// which even-odd cancelled out) and holes are contours wound the other way. Work in font units
/// so f32 precision is independent of drawing coordinates; the placement stays f64. The existing
/// slab filler remains the fallback when tessellation fails (including its u16 vertex limit).
fn triangulate(contours: &[Vec<Vec2>]) -> Vec<Vec2> {
    let mut path = Path::builder();
    for contour in contours {
        let Some(&first) = contour.first().filter(|_| contour.len() >= 3) else { continue };
        path.begin(point(first.x as f32, first.y as f32));
        for p in contour.iter().skip(1) {
            path.line_to(point(p.x as f32, p.y as f32));
        }
        path.end(true);
    }
    let mut buffers: VertexBuffers<Vec2, u16> = VertexBuffers::new();
    let result = FillTessellator::new().tessellate_path(
        &path.build(),
        &FillOptions::non_zero(),
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| {
            let p = v.position();
            Vec2::new(f64::from(p.x), f64::from(p.y))
        }),
    );
    if result.is_err() {
        return crate::fill::triangulate_evenodd(contours);
    }
    buffers.indices.into_iter().filter_map(|i| buffers.vertices.get(usize::from(i)).copied()).collect()
}

#[cfg(test)]
mod view_tests {
    use super::*;

    /// A glyph's own ink box decides whether it can show: ink reaching well past its em (a
    /// flourish, a tall stack, a large operator) is kept when the view sees only that part.
    #[test]
    fn glyph_ink_beyond_the_em_keeps_the_glyph_in_view() {
        // An em of one drawing unit with its origin at (0, 0).
        let xf = Mat3 { a: 0.001, b: 0.0, c: 0.0, d: 0.001, e: 0.0, f: 0.0 };
        let ink = Bounds2::new(Vec2::new(-3000.0, -3000.0), Vec2::new(4000.0, 4000.0));
        let overhang = Bounds2::new(Vec2::new(3.0, 3.0), Vec2::new(3.5, 3.5));
        assert!(may_show(xf, ink, &overhang), "the view sees only the ink outside the em");
        let past = Bounds2::new(Vec2::new(4.5, 4.5), Vec2::new(5.0, 5.0));
        assert!(!may_show(xf, ink, &past), "a view past the ink leaves the glyph out");
        let em = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(1000.0, 1000.0));
        assert!(!may_show(xf, em, &overhang), "an em-sized glyph is left out of the same view");
        assert!(!may_show(xf, Bounds2::EMPTY, &overhang), "a glyph without ink shows nothing");
        // One corner lands within f64, the far one overflows: the glyph is left to the mesh code.
        let huge = Mat3 { a: 1.0e305, b: 0.0, c: 0.0, d: 1.0e305, e: 0.0, f: 0.0 };
        let wide = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(1.0e4, 1.0e4));
        let far = Bounds2::new(Vec2::new(-10.0, -10.0), Vec2::new(-5.0, -5.0));
        assert!(may_show(huge, wide, &far), "an overflowing corner keeps the glyph");
    }
}

#[cfg(test)]
mod local_cache_tests {
    use super::*;

    /// Each thread's meshes stay within [`MAX_LOCAL_BYTES`], and a mesh too large for it isn't kept.
    #[test]
    fn a_threads_meshes_stay_within_their_byte_budget() {
        let (mut entries, mut total) = (HashMap::new(), 0usize);
        let key = |i: u16| (0usize, 0usize, 0u32, i, Detail::Full);
        let big = MAX_LOCAL_BYTES / 8;
        for i in 0..100 {
            remember(&mut entries, &mut total, key(i), || (), big);
            assert!(total <= MAX_LOCAL_BYTES, "{total}");
            assert_eq!(total, entries.len() * big);
        }
        remember(&mut entries, &mut total, key(500), || (), MAX_LOCAL_BYTES / 2);
        assert!(!entries.contains_key(&key(500)), "a mesh above a quarter of the budget isn't kept");
    }
}

/// [`triangulate`] for a glyph outline already placed in drawing coordinates (complex-script
/// runs carry no [`TextGlyph`]): tessellated relative to its first point so f32 keeps the
/// precision. Outlines past the limits [`mesh`] applies keep the slab filler.
pub(super) fn triangulate_placed(contours: &[Vec<Vec2>]) -> Vec<Vec2> {
    let Some(&origin) = contours.iter().flatten().next() else { return Vec::new() };
    let points = contours.iter().map(Vec::len).try_fold(0usize, usize::checked_add);
    let out_of_range = |p: &Vec2| {
        let d = *p - origin;
        !p.is_finite() || d.x.abs().max(d.y.abs()) > 1.0e6
    };
    if !points.is_some_and(|n| n <= MAX_POINTS) || contours.iter().flatten().any(out_of_range) {
        return crate::fill::triangulate_evenodd(contours);
    }
    let local: Vec<Vec<Vec2>> = contours.iter().map(|c| c.iter().map(|&p| p - origin).collect()).collect();
    triangulate(&local).into_iter().map(|p| p + origin).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<Vec2> {
        // Clockwise, as TrueType draws outer contours.
        vec![Vec2::new(x, y), Vec2::new(x, y + s), Vec2::new(x + s, y + s), Vec2::new(x + s, y), Vec2::new(x, y)]
    }

    fn area(tris: &[Vec2]) -> f64 {
        tris.chunks(3)
            .filter_map(|t| match t {
                [a, b, c] => Some(((*b - *a).x * (*c - *a).y - (*b - *a).y * (*c - *a).x).abs() / 2.0),
                _ => None,
            })
            .sum()
    }

    #[test]
    fn duplicated_contour_stays_filled() {
        // A composite glyph that draws one component twice (Arial Unicode's 진 and its ㄴ).
        let s = square(0.0, 0.0, 10.0);
        assert!((area(&triangulate(&[s.clone(), s])) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn overlapping_contours_fill_their_union() {
        let tris = triangulate(&[square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0)]);
        assert!((area(&tris) - 175.0).abs() < 1e-6);
    }

    #[test]
    fn counter_wound_contour_is_still_a_hole() {
        let mut hole = square(4.0, 4.0, 2.0);
        hole.reverse();
        assert!((area(&triangulate(&[square(0.0, 0.0, 10.0), hole])) - 96.0).abs() < 1e-6);
    }

    #[test]
    fn placed_outline_far_from_the_origin_keeps_its_place_and_fill() {
        let s = square(5.0e5, -3.0e5, 2.0);
        let tris = triangulate_placed(&[s.clone(), s]);
        assert!((area(&tris) - 4.0).abs() < 1e-6);
        assert!(tris.iter().all(|p| (5.0e5..=5.0e5 + 2.0).contains(&p.x) && (-3.0e5..=-3.0e5 + 2.0).contains(&p.y)));
    }

    #[test]
    fn hostile_placed_outline_never_panics() {
        assert!(triangulate_placed(&[]).is_empty());
        let _ = triangulate_placed(&[vec![Vec2::new(f64::NAN, 0.0), Vec2::X, Vec2::Y]]);
        let _ = triangulate_placed(&[vec![Vec2::ZERO, Vec2::new(1.0e300, 0.0), Vec2::Y]]);
    }
}
