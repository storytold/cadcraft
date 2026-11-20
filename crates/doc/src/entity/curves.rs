use super::{Deserialize, PolyVertex, Serialize, Vec3};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub p: Vec3,
    #[serde(default)]
    pub angle: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Vec3,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub center: Vec3,
    pub radius: f64,
    /// Radians, CCW.
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub center: Vec3,
    pub major: Vec3,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LwPolyline {
    pub vertices: Vec<PolyVertex>,
    pub closed: bool,
    #[serde(default)]
    pub const_width: f64,
    #[serde(default)]
    pub elevation: f64,
    /// Polyline linetype generation (PLINEGEN).
    #[serde(default)]
    pub plinegen: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polyline3d {
    pub points: Vec<Vec3>,
    pub closed: bool,
}

/// SOLID / TRACE: up to four corners (DXF order 1,2,4,3 when drawn).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub corners: [Vec3; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face3d {
    pub corners: [Vec3; 4],
    #[serde(default)]
    pub hidden_edges: u8,
}

impl Face3d {
    /// The edges drawn: side i (corner i to the next) unless bit i of `hidden_edges` hides it,
    /// without the zero-length side of a triangle.
    pub fn visible_edges(&self) -> Vec<(Vec3, Vec3)> {
        let c = self.corners;
        (0..4).filter(|i| self.hidden_edges & (1 << i) == 0).map(|i| (c[i], c[(i + 1) % 4])).filter(|(a, b)| a != b).collect()
    }
}

/// POLYLINE polyface mesh (DXF flag 64): its vertices, and faces naming three or four of them
/// by 1-based index as DXF stores them (0: no fourth vertex; a negative index hides the edge
/// that starts at that vertex).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PolyfaceMesh {
    pub vertices: Vec<Vec3>,
    pub faces: Vec<[i32; 4]>,
}

impl PolyfaceMesh {
    /// The faces as 3D faces (what EXPLODE makes). Faces with fewer than three vertices or an
    /// index outside the vertex list are skipped.
    pub fn faces3d(&self) -> Vec<Face3d> {
        self.faces.iter().filter_map(|f| self.face3d(f)).collect()
    }
    /// One face record as a 3D face: `None` when it names fewer than three vertices before a 0,
    /// or one outside the vertex list.
    pub fn face3d(&self, f: &[i32; 4]) -> Option<Face3d> {
        let mut corners: Vec<Vec3> = Vec::with_capacity(4);
        let mut hidden = 0u8;
        for &ix in f.iter().take_while(|&&ix| ix != 0) {
            let at = usize::try_from(ix.unsigned_abs()).ok().and_then(|n| n.checked_sub(1));
            let p = *at.and_then(|n| self.vertices.get(n))?;
            if ix < 0 {
                hidden |= 1 << corners.len();
            }
            corners.push(p);
        }
        match corners.as_slice() {
            // A triangle's closing edge is the 3DFACE's fourth (the third is degenerate).
            [a, b, c] => Some(Face3d { corners: [*a, *b, *c, *c], hidden_edges: hidden | if hidden & 4 != 0 { 8 } else { 0 } }),
            [a, b, c, d] => Some(Face3d { corners: [*a, *b, *c, *d], hidden_edges: hidden }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RayLine {
    pub base: Vec3,
    pub dir: Vec3,
}
