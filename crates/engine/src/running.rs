use super::{Arc, Drawing, Handle, Interactive, Serialize, Vec2};

/// An interactive command in progress.
pub struct Running {
    pub id: String,
    pub machine: Box<dyn Interactive>,
    pub before: Arc<Drawing>,
    pub selection_before: Vec<Handle>,
}

/// A pending window/crossing selection started by a click on empty space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PendingWindow {
    pub corner: Vec2,
    /// Target-plane 3D anchor of `corner`, preserving screen UV in 3D/edge-on views.
    pub corner_3d: cadcraft_geom::Vec3,
    pub during_command: bool,
}

impl PendingWindow {
    pub fn new(corner: Vec2, during_command: bool) -> Self {
        Self { corner, corner_3d: corner.to3(0.0), during_command }
    }
}
