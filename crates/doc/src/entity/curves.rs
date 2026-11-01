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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RayLine {
    pub base: Vec3,
    pub dir: Vec3,
}
