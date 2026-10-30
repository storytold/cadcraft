use cadcraft_geom::Bounds2;

use super::{Deserialize, Serialize, Vec2};

/// A 2D view of a space: world centre and visible height in drawing units, and the view twist
/// (VIEWTWIST, DVIEW TWist): how far the view is turned on screen, in radians counterclockwise
/// (the drawing appears turned by `twist`; 0 is the plain plan view).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub center: Vec2,
    pub height: f64,
    #[serde(default)]
    pub twist: f64,
}

impl Default for View {
    fn default() -> Self {
        View { center: Vec2::new(6.0, 4.5), height: 10.0, twist: 0.0 }
    }
}

impl View {
    /// Box `b` (world) as a view turned by `twist` shows it: the world centre of the box it
    /// makes on screen, and that box's width and height (`b`'s own when there is no twist).
    pub fn twisted_box(b: Bounds2, twist: f64) -> (Vec2, f64, f64) {
        if twist == 0.0 || !twist.is_finite() {
            return (b.center(), b.width(), b.height());
        }
        let r = Bounds2::from_points(b.corners().map(|p| p.rotate(twist)));
        (r.center().rotate(-twist), r.width(), r.height())
    }
}

pub(crate) fn usable_view(v: &View) -> bool {
    v.center.is_finite() && v.height.is_finite() && v.height > 1e-12 && v.twist.is_finite()
}

/// Tiled model viewports (VPORTS): the session side of the drawing's `*Active` configuration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelTiles {
    /// The tile corners these views belong to (when the drawing's tiles change, by VPORTS or
    /// undo, its views take over).
    pub rects: Vec<(Vec2, Vec2)>,
    /// Each tile's view; the current tile shows the model space view itself.
    pub views: Vec<View>,
    /// The current tile.
    pub active: usize,
}

/// One tile of the model space drawing area, for the UI ([`Session::model_tiles`]): its corners
/// as fractions of the drawing area (0,0 lower left) and its view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelTile {
    pub min: Vec2,
    pub max: Vec2,
    pub view: View,
    pub active: bool,
}
