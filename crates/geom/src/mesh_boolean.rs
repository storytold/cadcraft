//! Bounded BSP operations on closed, consistently oriented triangle meshes.
use crate::{Mesh3, MeshError, Vec3};
use std::collections::HashMap;

const MAX_FACES: usize = 16_384;
const MAX_PIECES: usize = 65_536;
// Work units are vertex/plane classifications and vertex comparisons. The bound stops runaway
// inputs after a few seconds of a release build; a box with several faceted bosses and holes
// stays far below it.
const MAX_WORK: usize = 256_000_000;

#[derive(Clone)]
struct Poly {
    points: Vec<Vec3>,
    normal: Vec3,
    w: f64,
    /// The input triangle this piece was cut from (an index into the boolean's originals).
    source: usize,
}
impl Poly {
    fn new(points: Vec<Vec3>) -> Option<Self> {
        let a = *points.first()?;
        let mut normal = Vec3::ZERO;
        for i in 1..points.len().saturating_sub(1) {
            normal = normal + (points[i] - a).cross(points[i + 1] - a);
        }
        if normal.len() <= 1e-18 {
            return None;
        }
        normal = normal.normalized();
        Some(Self { w: normal.dot(a), normal, points, source: usize::MAX })
    }
    fn flip(&mut self) {
        self.points.reverse();
        self.normal = -self.normal;
        self.w = -self.w;
    }
}
#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    w: f64,
}
struct Budget {
    tolerance: f64,
    work: usize,
}
impl Budget {
    fn tick(&mut self, amount: usize) -> Result<(), MeshError> {
        self.work = self.work.checked_add(amount).ok_or(MeshError("boolean work limit"))?;
        if self.work > MAX_WORK { Err(MeshError("boolean work limit")) } else { Ok(()) }
    }
}
fn split(
    plane: Plane,
    p: Poly,
    coplanar_front: &mut Vec<Poly>,
    coplanar_back: &mut Vec<Poly>,
    front: &mut Vec<Poly>,
    back: &mut Vec<Poly>,
    budget: &mut Budget,
) -> Result<(), MeshError> {
    budget.tick(p.points.len())?;
    let dist: Vec<f64> = p.points.iter().map(|v| plane.normal.dot(*v) - plane.w).collect();
    let types: Vec<u8> = dist
        .iter()
        .map(|d| {
            if *d > budget.tolerance {
                1
            } else if *d < -budget.tolerance {
                2
            } else {
                0
            }
        })
        .collect();
    let kind = types.iter().fold(0, |a, b| a | b);
    match kind {
        0 => {
            if plane.normal.dot(p.normal) > 0.0 {
                coplanar_front.push(p)
            } else {
                coplanar_back.push(p)
            }
        }
        1 => front.push(p),
        2 => back.push(p),
        _ => {
            let mut f = Vec::new();
            let mut b = Vec::new();
            for i in 0..p.points.len() {
                let j = (i + 1) % p.points.len();
                let a = p.points[i];
                let z = p.points[j];
                let t = types[i];
                let u = types[j];
                if t != 2 {
                    f.push(a);
                }
                if t != 1 {
                    b.push(a);
                }
                if (t | u) == 3 {
                    let denom = dist[i] - dist[j];
                    if denom.abs() <= f64::EPSILON {
                        return Err(MeshError("unstable boolean split"));
                    }
                    let v = a + (z - a) * (dist[i] / denom);
                    f.push(v);
                    b.push(v);
                }
            }
            if let Some(mut q) = Poly::new(f) {
                q.source = p.source;
                front.push(q);
            }
            if let Some(mut q) = Poly::new(b) {
                q.source = p.source;
                back.push(q);
            }
        }
    }
    Ok(())
}
/// The bounding box of a solid (grown by a margin) and whether its tree is inverted.
#[derive(Clone, Copy)]
struct Region {
    min: Vec3,
    max: Vec3,
    inverted: bool,
}
impl Region {
    fn of(polys: &[Poly], margin: f64) -> Self {
        let mut min = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut max = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for v in polys.iter().flat_map(|p| &p.points) {
            min = Vec3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
            max = Vec3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
        }
        let m = Vec3::new(margin, margin, margin);
        Region { min: min - m, max: max + m, inverted: false }
    }
    fn flipped(self) -> Self {
        Region { inverted: !self.inverted, ..self }
    }
    /// Whether the bounding box of `p` overlaps this box.
    fn touches(&self, p: &Poly) -> bool {
        let (mut lo, mut hi) =
            (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
        for v in &p.points {
            lo = Vec3::new(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z));
            hi = Vec3::new(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z));
        }
        lo.x <= self.max.x && hi.x >= self.min.x && lo.y <= self.max.y && hi.y >= self.min.y && lo.z <= self.max.z && hi.z >= self.min.z
    }
}
/// One BSP node: its splitting plane, the polygons on that plane, and its children (indices
/// into [`Tree::nodes`]).
#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Poly>,
    front: Option<usize>,
    back: Option<usize>,
}
/// A BSP tree kept in an arena and walked with explicit work lists, never by recursion: the
/// tree of a convex solid is a chain as deep as it has faces, which would overflow the stack.
struct Tree {
    nodes: Vec<Node>,
}
impl Default for Tree {
    fn default() -> Self {
        Tree { nodes: vec![Node::default()] }
    }
}
impl Tree {
    fn child(&mut self, at: usize, front: bool) -> Result<usize, MeshError> {
        let existing = self.nodes.get(at).and_then(|n| if front { n.front } else { n.back });
        if let Some(c) = existing {
            return Ok(c);
        }
        if self.nodes.len() >= MAX_PIECES * 2 {
            return Err(MeshError("boolean complexity limit"));
        }
        let c = self.nodes.len();
        self.nodes.push(Node::default());
        if let Some(n) = self.nodes.get_mut(at) {
            if front {
                n.front = Some(c);
            } else {
                n.back = Some(c);
            }
        }
        Ok(c)
    }
    fn build(&mut self, polys: Vec<Poly>, budget: &mut Budget) -> Result<(), MeshError> {
        let mut work = vec![(0usize, polys)];
        while let Some((at, polys)) = work.pop() {
            if polys.is_empty() {
                continue;
            }
            if polys.len() > MAX_PIECES {
                return Err(MeshError("boolean complexity limit"));
            }
            let Some(node) = self.nodes.get_mut(at) else { return Err(MeshError("boolean complexity limit")) };
            let plane = match (node.plane, polys.first()) {
                (Some(p), _) => p,
                (None, Some(first)) => *node.plane.insert(Plane { normal: first.normal, w: first.w }),
                (None, None) => continue,
            };
            let (mut f, mut b, mut cf, mut cb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut f, &mut b, budget)?;
            }
            node.polygons.extend(cf);
            node.polygons.extend(cb);
            if node.polygons.len() > MAX_PIECES {
                return Err(MeshError("boolean complexity limit"));
            }
            if !f.is_empty() {
                work.push((self.child(at, true)?, f));
            }
            if !b.is_empty() {
                work.push((self.child(at, false)?, b));
            }
        }
        Ok(())
    }
    fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polygons {
                p.flip();
            }
            if let Some(ref mut p) = n.plane {
                p.normal = -p.normal;
                p.w = -p.w;
            }
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }
    /// The parts of `polys` outside this solid (in front of its surface).
    fn clip(&self, polys: Vec<Poly>, budget: &mut Budget) -> Result<Vec<Poly>, MeshError> {
        let mut out = Vec::new();
        let mut work = vec![(0usize, polys)];
        while let Some((at, polys)) = work.pop() {
            let Some(node) = self.nodes.get(at) else { continue };
            let Some(plane) = node.plane else {
                out.extend(polys);
                continue;
            };
            let (mut f, mut b, mut cf, mut cb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut f, &mut b, budget)?;
            }
            f.extend(cf);
            b.extend(cb);
            match node.front {
                Some(c) => work.push((c, f)),
                None => out.extend(f),
            }
            if let Some(c) = node.back {
                work.push((c, b));
            }
            if out.len() > MAX_PIECES || work.len() > MAX_PIECES * 2 {
                return Err(MeshError("boolean complexity limit"));
            }
        }
        Ok(out)
    }
    /// Clip this tree's polygons to `other`, a closed solid whose bounding box is `region`.
    /// A polygon clear of that box is wholly outside the solid, so it skips the BSP walk (which
    /// would only cut it into pieces along unrelated planes): it is kept, or dropped when the
    /// solid is inverted (its outside is then the inside).
    fn clip_to(&mut self, other: &Tree, region: &Region, budget: &mut Budget) -> Result<(), MeshError> {
        for n in &mut self.nodes {
            let (near, far): (Vec<Poly>, Vec<Poly>) = std::mem::take(&mut n.polygons).into_iter().partition(|p| region.touches(p));
            n.polygons = other.clip(near, budget)?;
            if !region.inverted {
                n.polygons.extend(far);
            }
        }
        Ok(())
    }
    fn collect(&self, out: &mut Vec<Poly>) -> Result<(), MeshError> {
        for n in &self.nodes {
            out.extend(n.polygons.iter().cloned());
            if out.len() > MAX_PIECES {
                return Err(MeshError("boolean complexity limit"));
            }
        }
        Ok(())
    }
}
fn input(mesh: &Mesh3, tol: f64) -> Result<Vec<Poly>, MeshError> {
    if mesh.triangles.len() > MAX_FACES || mesh.vertices.len() > MAX_FACES * 3 {
        return Err(MeshError("boolean input size limit"));
    }
    if mesh.vertices.iter().any(|p| !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12 || p.z.abs() > 1e12) {
        return Err(MeshError("invalid mesh coordinates"));
    }
    let mut edges = HashMap::new();
    let mut out = Vec::new();
    let origin = mesh.vertices.first().copied().unwrap_or(Vec3::ZERO);
    let mut volume = 0.0;
    for t in &mesh.triangles {
        let (Some(a), Some(b), Some(c)) = (mesh.vertices.get(t[0]), mesh.vertices.get(t[1]), mesh.vertices.get(t[2])) else {
            return Err(MeshError("invalid triangle index"));
        };
        if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] || (*b - *a).cross(*c - *a).len() <= tol * tol {
            return Err(MeshError("degenerate triangle"));
        }
        for (i, j) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let e = edges.entry((i.min(j), i.max(j))).or_insert((0, 0));
            e.0 += 1;
            e.1 += if i < j { 1 } else { -1 };
        }
        volume += (*a - origin).dot((*b - origin).cross(*c - origin)) / 6.0;
        out.push(Poly::new(vec![*a, *b, *c]).ok_or(MeshError("degenerate triangle"))?);
    }
    if edges.values().any(|e| *e != (2, 0)) {
        return Err(MeshError("mesh must be closed and consistently oriented"));
    }
    if !out.is_empty() && (!volume.is_finite() || volume <= tol * tol * tol) {
        return Err(MeshError("mesh has invalid orientation or volume"));
    }
    Ok(out)
}
fn area(points: &[Vec3]) -> f64 {
    let Some(a) = points.first().copied() else { return 0.0 };
    let mut n = Vec3::ZERO;
    for w in points.windows(2).skip(1) {
        if let [b, c] = w {
            n = n + (*b - a).cross(*c - a);
        }
    }
    n.len() * 0.5
}
/// Put back whole input triangles: when every piece of one survived (their areas add up to the
/// triangle's), the triangle replaces them, so faces the other solid only cut along unrelated
/// planes don't come out as many slivers.
fn rejoin(polys: Vec<Poly>, originals: &[Poly]) -> Vec<Poly> {
    let mut out = Vec::with_capacity(polys.len());
    let mut groups: std::collections::BTreeMap<usize, (f64, Vec<Poly>)> = std::collections::BTreeMap::new();
    for p in polys {
        if p.source < originals.len() {
            let g = groups.entry(p.source).or_insert_with(|| (0.0, Vec::new()));
            g.0 += area(&p.points);
            g.1.push(p);
        } else {
            out.push(p);
        }
    }
    for (source, (sum, pieces)) in groups {
        let first = pieces.first().map(|p| p.normal).unwrap_or(Vec3::ZERO);
        match originals.get(source) {
            Some(o)
                if pieces.len() > 1
                    && (sum - area(&o.points)).abs() <= area(&o.points) * 1e-9
                    && pieces.iter().all(|p| p.normal.dot(first) > 0.0) =>
            {
                let mut q = o.clone();
                if q.normal.dot(first) < 0.0 {
                    q.flip();
                }
                out.push(q);
            }
            _ => out.extend(pieces),
        }
    }
    out
}
fn output(polys: Vec<Poly>, originals: &[Poly], budget: &mut Budget) -> Result<Mesh3, MeshError> {
    let polys = rejoin(polys, originals);
    let mut points: Vec<Vec3> = Vec::new();
    let mut boundaries: Vec<Vec<usize>> = Vec::new();
    for p in polys {
        let mut row = Vec::new();
        for v in p.points {
            budget.tick(points.len())?;
            let index = points.iter().position(|q| (*q - v).len() <= budget.tolerance).unwrap_or_else(|| {
                points.push(v);
                points.len() - 1
            });
            if row.last().copied() != Some(index) {
                row.push(index);
            }
        }
        if row.len() > 1 && row.first() == row.last() {
            row.pop();
        }
        if row.len() >= 3 {
            boundaries.push(row);
        }
    }
    let original = points.len();
    let mut triangles = Vec::new();
    for row in boundaries {
        // The outline with every vertex that lies on one of its edges (a neighbour's corner),
        // so the triangles meet without T-junctions.
        let mut ring = Vec::new();
        for j in 0..row.len() {
            let a = row[j];
            let b = row[(j + 1) % row.len()];
            let delta = points[b] - points[a];
            let length = delta.len();
            if length <= budget.tolerance {
                continue;
            }
            let unit = delta * (1.0 / length);
            let mut on = vec![(0.0, a), (length, b)];
            budget.tick(original)?;
            for (k, p) in points.iter().take(original).enumerate() {
                if k == a || k == b {
                    continue;
                }
                let d = *p - points[a];
                let along = d.dot(unit);
                if along > budget.tolerance && along < length - budget.tolerance && (d - unit * along).len() <= budget.tolerance {
                    on.push((along, k));
                }
            }
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            on.dedup_by(|x, y| (x.0 - y.0).abs() <= budget.tolerance);
            ring.extend(on.iter().take(on.len() - 1).map(|x| x.1));
        }
        if ring.len() < 3 {
            continue;
        }
        if let [a, b, c] = ring[..] {
            triangles.push([a, b, c]);
            continue;
        }
        // A convex piece with extra points: fan from its centre.
        let center = ring.iter().fold(Vec3::ZERO, |a, i| a + points[*i]) * (1.0 / ring.len() as f64);
        let center_id = points.len();
        points.push(center);
        for j in 0..ring.len() {
            let (x, y) = (ring[j], ring[(j + 1) % ring.len()]);
            if (points[x] - center).cross(points[y] - center).len() > budget.tolerance * budget.tolerance {
                triangles.push([center_id, x, y]);
            }
        }
    }
    if triangles.len() > MAX_PIECES * 8 {
        return Err(MeshError("boolean output size limit"));
    }
    let result = Mesh3 { vertices: points, triangles };
    input(&result, budget.tolerance)?;
    Ok(result)
}
#[derive(Clone, Copy)]
enum Op {
    Union,
    Subtract,
    Intersect,
}
fn boolean(a: &Mesh3, b: &Mesh3, op: Op) -> Result<Mesh3, MeshError> {
    let mut min = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut max = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in a.vertices.iter().chain(&b.vertices) {
        if !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12 || p.z.abs() > 1e12 {
            return Err(MeshError("invalid mesh coordinates"));
        }
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }
    let scale = if a.vertices.is_empty() && b.vertices.is_empty() { 1.0 } else { (max - min).len().max(1.0) };
    let mut budget = Budget { tolerance: scale * 1e-9, work: 0 };
    let mut pa = input(a, budget.tolerance)?;
    let mut pb = input(b, budget.tolerance)?;
    if pa.is_empty() || pb.is_empty() {
        return Ok(match op {
            Op::Union => {
                if pa.is_empty() {
                    b.clone()
                } else {
                    a.clone()
                }
            }
            Op::Subtract => {
                if pb.is_empty() {
                    a.clone()
                } else {
                    Mesh3 { vertices: vec![], triangles: vec![] }
                }
            }
            Op::Intersect => Mesh3 { vertices: vec![], triangles: vec![] },
        });
    }
    // Shift the BSP close to the origin to retain precision for georeferenced drawings.
    for p in pa.iter_mut().chain(pb.iter_mut()) {
        for v in &mut p.points {
            *v = *v - min;
        }
        p.w = p.normal.dot(p.points[0]);
    }
    for (k, p) in pa.iter_mut().chain(pb.iter_mut()).enumerate() {
        p.source = k;
    }
    let originals: Vec<Poly> = pa.iter().chain(&pb).cloned().collect();
    // The margin keeps polygons that touch the other solid's box within reach of its planes.
    let margin = budget.tolerance * 1e3;
    let (ra, rb) = (Region::of(&pa, margin), Region::of(&pb, margin));
    let mut x = Tree::default();
    let mut y = Tree::default();
    x.build(pa, &mut budget)?;
    y.build(pb, &mut budget)?;
    // The classic BSP sequence; the final step joins both polygon sets directly instead of
    // building them into one tree, which would split them again along each other's planes.
    let mut polys = Vec::new();
    match op {
        Op::Union => {
            x.clip_to(&y, &rb, &mut budget)?;
            y.clip_to(&x, &ra, &mut budget)?;
            y.invert();
            y.clip_to(&x, &ra, &mut budget)?;
            y.invert();
            x.collect(&mut polys)?;
            y.collect(&mut polys)?;
        }
        Op::Subtract => {
            x.invert();
            x.clip_to(&y, &rb, &mut budget)?;
            y.clip_to(&x, &ra.flipped(), &mut budget)?;
            y.invert();
            y.clip_to(&x, &ra.flipped(), &mut budget)?;
            y.invert();
            x.invert();
            y.invert();
            x.collect(&mut polys)?;
            y.collect(&mut polys)?;
        }
        Op::Intersect => {
            x.invert();
            y.clip_to(&x, &ra.flipped(), &mut budget)?;
            y.invert();
            x.clip_to(&y, &rb.flipped(), &mut budget)?;
            y.clip_to(&x, &ra.flipped(), &mut budget)?;
            x.invert();
            y.invert();
            x.collect(&mut polys)?;
            y.collect(&mut polys)?;
        }
    }
    if polys.len() > MAX_PIECES {
        return Err(MeshError("boolean complexity limit"));
    }
    let mut result = output(polys, &originals, &mut budget)?;
    for v in &mut result.vertices {
        *v = *v + min;
    }
    Ok(result)
}
impl Mesh3 {
    /// Union of closed, consistently oriented manifold meshes.
    pub fn union(&self, other: &Self) -> Result<Self, MeshError> {
        boolean(self, other, Op::Union)
    }
    /// Subtract the second solid from the first.
    pub fn subtract(&self, other: &Self) -> Result<Self, MeshError> {
        boolean(self, other, Op::Subtract)
    }
    /// Intersect two solids; a disjoint result is an empty mesh.
    pub fn intersect(&self, other: &Self) -> Result<Self, MeshError> {
        boolean(self, other, Op::Intersect)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn volume(m: &Mesh3) -> f64 {
        m.triangles.iter().map(|[a, b, c]| m.vertices[*a].dot(m.vertices[*b].cross(m.vertices[*c])) / 6.0).sum()
    }
    fn cube(x: f64, y: f64, z: f64, size: f64) -> Result<Mesh3, MeshError> {
        Mesh3::box_solid(Vec3::new(x, y, z), Vec3::new(x + size, y + size, z + size))
    }
    fn close(mesh: Mesh3, expected: f64) -> Result<(), MeshError> {
        input(&mesh, 1e-8)?;
        assert!((volume(&mesh) - expected).abs() < 1e-6, "expected {expected}, got {}", volume(&mesh));
        Ok(())
    }
    #[test]
    fn boxes() -> Result<(), MeshError> {
        let a = cube(0.0, 0.0, 0.0, 2.0)?;
        let b = cube(1.0, 1.0, 1.0, 2.0)?;
        close(a.union(&b)?, 15.0)?;
        close(a.intersect(&b)?, 1.0)?;
        close(a.subtract(&b)?, 7.0)?;
        let c = cube(4.0, 0.0, 0.0, 2.0)?;
        close(a.union(&c)?, 16.0)?;
        close(a.subtract(&c)?, 8.0)?;
        close(a.intersect(&c)?, 0.0)?;
        let inner = cube(0.5, 0.5, 0.5, 1.0)?;
        close(a.union(&inner)?, 8.0)?;
        close(a.intersect(&inner)?, 1.0)?;
        close(a.subtract(&inner)?, 7.0)?;
        let touching = cube(2.0, 0.0, 0.0, 2.0)?;
        close(a.union(&touching)?, 16.0)?;
        close(a.intersect(&touching)?, 0.0)?;
        close(a.subtract(&touching)?, 8.0)?;
        close(a.union(&a)?, 8.0)?;
        close(a.intersect(&a)?, 8.0)?;
        close(a.subtract(&a)?, 0.0)?;
        Ok(())
    }
    /// Regressions: a convex solid's BSP is a chain as deep as its face count (a default sphere
    /// used to hit the old recursion cap), and repeated booleans must not multiply slivers.
    #[test]
    fn faceted_solids_and_chained_booleans() -> Result<(), MeshError> {
        let a = Mesh3::sphere(Vec3::ZERO, 10.0, 32, 16)?;
        let b = Mesh3::sphere(Vec3::new(7.0, 3.0, 2.0), 9.0, 32, 16)?;
        let u = a.union(&b)?;
        input(&u, 1e-8)?;
        assert!(volume(&u) > volume(&a) && volume(&u) < volume(&a) + volume(&b));
        let i = a.intersect(&b)?;
        input(&i, 1e-8)?;
        assert!((volume(&u) + volume(&i) - volume(&a) - volume(&b)).abs() < 1e-6 * volume(&u));
        // An open-top housing: shell, four corner bosses, two floor holes.
        let ngon = |r: f64, n: f64| 0.5 * n * r * r * (std::f64::consts::TAU / n).sin();
        let mut body = Mesh3::box_solid(Vec3::ZERO, Vec3::new(80.0, 50.0, 30.0))?
            .subtract(&Mesh3::box_solid(Vec3::new(3.0, 3.0, 3.0), Vec3::new(77.0, 47.0, 40.0))?)?;
        for (x, y) in [(8.0, 8.0), (72.0, 8.0), (72.0, 42.0), (8.0, 42.0)] {
            body = body.union(&Mesh3::cylinder(Vec3::new(x, y, 2.0), 3.5, 22.0, 24)?)?;
        }
        for x in [25.0, 55.0] {
            body = body.subtract(&Mesh3::cylinder(Vec3::new(x, 25.0, -1.0), 4.0, 6.0, 24)?)?;
        }
        let expected = 80.0 * 50.0 * 30.0 - 74.0 * 44.0 * 27.0 + 4.0 * 21.0 * ngon(3.5, 24.0) - 2.0 * 3.0 * ngon(4.0, 24.0);
        input(&body, 1e-8)?;
        assert!((volume(&body) - expected).abs() < 1e-9 * expected, "expected {expected}, got {}", volume(&body));
        assert!(body.triangles.len() < 4000, "{} triangles", body.triangles.len());
        Ok(())
    }
    #[test]
    fn translated_and_varied_overlap() -> Result<(), MeshError> {
        for &(origin, shift) in &[(0.0, 0.25), (0.0, 1.5), (1e9, 0.5)] {
            let a = cube(origin, origin, origin, 2.0)?;
            let b = cube(origin + shift, origin, origin, 2.0)?;
            let overlap = (2.0 - shift) * 4.0;
            let result = a.intersect(&b)?;
            input(&result, 1e-8)?;
            let base = result.vertices.first().copied().unwrap_or(Vec3::ZERO);
            let volume: f64 = result
                .triangles
                .iter()
                .map(|[i, j, k]| (result.vertices[*i] - base).dot((result.vertices[*j] - base).cross(result.vertices[*k] - base)) / 6.0)
                .sum();
            assert!((volume - overlap).abs() < 1e-5, "{volume} vs {overlap}");
        }
        Ok(())
    }
    #[test]
    fn curved_solid_intersection() -> Result<(), MeshError> {
        let a = Mesh3::sphere(Vec3::ZERO, 2.0, 16, 8)?;
        let b = cube(-1.0, -1.0, -1.0, 2.0)?;
        let intersection = a.intersect(&b)?;
        close(intersection, 8.0)?;
        let shifted = cube(0.0, -1.0, -1.0, 2.0)?;
        let cut = a.intersect(&shifted)?;
        input(&cut, 1e-8)?;
        assert!(volume(&cut) > 0.0 && volume(&cut) < 8.0);
        Ok(())
    }
    #[test]
    fn rejects_hostile_meshes() -> Result<(), MeshError> {
        let a = cube(0.0, 0.0, 0.0, 1.0)?;
        let mut b = a.clone();
        b.triangles[0][0] = usize::MAX;
        assert!(a.union(&b).is_err());
        b = a.clone();
        b.vertices[0].x = f64::NAN;
        assert!(a.union(&b).is_err());
        b = a.clone();
        b.triangles.pop();
        assert!(a.union(&b).is_err());
        b = a.clone();
        for t in &mut b.triangles {
            t.swap(1, 2);
        }
        assert!(a.union(&b).is_err());
        Ok(())
    }
}
