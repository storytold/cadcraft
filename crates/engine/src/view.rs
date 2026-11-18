use cadcraft_geom::{Bounds2, Vec3};

use super::{Deserialize, Serialize, Vec2};

/// A view of a space: world centre (`center` + `target_z`), visible height in drawing units, and
/// the view twist (VIEWTWIST, DVIEW TWist): how far the view is turned on screen, in radians
/// counterclockwise (the drawing appears turned by `twist`; 0 is the plain plan view).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub center: Vec2,
    pub height: f64,
    #[serde(default)]
    pub twist: f64,
    /// Elevation (`Z`) of the 3D camera target point (`target() == Vec3(center.x, center.y, target_z)`).
    #[serde(default)]
    pub target_z: f64,
}

impl Default for View {
    fn default() -> Self {
        View { center: Vec2::new(6.0, 4.5), height: 10.0, twist: 0.0, target_z: 0.0 }
    }
}

impl View {
    /// Construct a planar view (`target_z == 0.0`).
    pub fn plan(center: Vec2, height: f64, twist: f64) -> Self {
        View { center, height, twist, target_z: 0.0 }
    }

    /// 3D camera target point `(center.x, center.y, target_z)`.
    pub fn target(&self) -> Vec3 {
        Vec3::new(self.center.x, self.center.y, self.target_z)
    }

    /// Return a copy of `self` aimed at 3D target `t`.
    pub fn with_target(mut self, t: Vec3) -> Self {
        self.center = t.xy();
        self.target_z = t.z;
        self
    }

    /// Return a copy of `self` panned by `(du, dv)` drawing units in the effective camera image
    /// plane (`cam.with_twist(self.twist)`), returning `self` unchanged if non-finite.
    pub fn pan_in_camera_plane(self, cam: cadcraft_geom::Camera3D, du: f64, dv: f64) -> Self {
        if !du.is_finite() || !dv.is_finite() {
            return self;
        }
        let eff = cam.with_twist(self.twist);
        let next_target = self.target() + eff.x * du + eff.y * dv;
        if !next_target.is_finite() {
            return self;
        }
        self.with_target(next_target)
    }

    /// Box `(b, z_range)` in 3D world space as a view with `cam` and `twist` shows it on screen:
    /// the 3D target point at the viewport centre, and the projected box's width and height in
    /// drawing units. Uses all 8 AABB corners in 3D/edge-on views (`GsView::zoomExtents`).
    pub fn projected_box_3d(
        b: Bounds2,
        z_range: (f64, f64),
        cam: cadcraft_geom::Camera3D,
        twist: f64,
    ) -> (Vec3, f64, f64) {
        let eff = cam.with_twist(twist);
        let z_mid = (z_range.0 + z_range.1) * 0.5;
        if eff.is_top_plan() {
            return (b.center().to3(z_mid), b.width(), b.height());
        }
        if (z_range.0 - z_range.1).abs() <= 1e-12 && z_range.0.abs() <= 1e-12 && eff.z.z.abs() > 1e-2 {
            let (center, w, h) = Self::projected_box(b, cam, twist);
            return (center.to3(0.0), w, h);
        }
        let c3 = b.center().to3(z_mid);
        let zs = if (z_range.0 - z_range.1).abs() <= 1e-12 { [z_range.0, z_range.0] } else { [z_range.0, z_range.1] };
        let mut r = Bounds2::EMPTY;
        for xy in b.corners() {
            r.add(eff.project_3d(xy.to3(zs[0]), c3));
            if zs[0] != zs[1] {
                r.add(eff.project_3d(xy.to3(zs[1]), c3));
            }
        }
        let uv_mid = r.center();
        let target = c3 + eff.x * uv_mid.x + eff.y * uv_mid.y;
        (target, r.width(), r.height())
    }

    /// Box `b` (world XY) as a view with camera `cam` and `twist` shows it on screen: the world
    /// XY centre to place at the viewport centre, and the projected box's width and height in
    /// drawing units.
    pub fn projected_box(b: Bounds2, cam: cadcraft_geom::Camera3D, twist: f64) -> (Vec2, f64, f64) {
        let eff = cam.with_twist(twist);
        if eff.is_top_plan() {
            return (b.center(), b.width(), b.height());
        }
        let r = Bounds2::from_points(b.corners().map(|p| eff.project_xy(p, Vec2::ZERO)));
        let center = eff.try_unproject_to_xy(r.center(), Vec2::ZERO).unwrap_or_else(|| b.center());
        (center, r.width(), r.height())
    }

    /// Box `b` (world) as a view turned by `twist` shows it: the world centre of the box it
    /// makes on screen, and that box's width and height (`b`'s own when there is no twist).
    pub fn twisted_box(b: Bounds2, twist: f64) -> (Vec2, f64, f64) {
        Self::projected_box(b, cadcraft_geom::Camera3D::TOP, twist)
    }
}

pub(crate) fn usable_view(v: &View) -> bool {
    v.center.is_finite() && v.height.is_finite() && v.height > 1e-12 && v.twist.is_finite() && v.target_z.is_finite()
}

/// The view drawing `d` was saved at, when it records a usable one: OPEN opens on it, and nothing
/// after the open (resolving xrefs, the UI's first frame) fits the extents over it.
pub(crate) fn saved_view(d: &cadcraft_doc::Drawing) -> Option<View> {
    d.header.saved_view().map(|(center, height)| View::plan(center, height, 0.0)).filter(usable_view)
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
