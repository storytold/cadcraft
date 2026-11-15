use cadcraft_geom::{Bounds2, Camera3D, Mat3};

use super::common::Handle;
use super::{Color, Deserialize, Serialize, Vec2, Vec3, standard, yes};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub center: Vec3,
    pub width: f64,
    pub height: f64,
    pub view_center: Vec2,
    pub view_height: f64,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub frozen_layers: Vec<String>,
    /// Per-viewport layer colour overrides (VP Color in the Layer Properties Manager).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_colors: Vec<(String, Color)>,
    /// The paper-space entity whose outline clips the viewport (a closed polyline, circle,
    /// ellipse or spline; DXF group 340). `None`: the rectangle `center`/`width`/`height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Handle>,
    /// Display on (MVIEW ON/OFF). An off viewport shows its border but no model space.
    #[serde(default = "yes")]
    pub on: bool,
    /// The view twist (DVIEW TWist inside the viewport; DXF group 51): radians counterclockwise,
    /// the model view seen through the viewport is turned on the sheet by it. 0 is untwisted.
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub twist: f64,
    /// 3D camera frame for model space inside this viewport (`Camera3D::TOP` for 2D plan views).
    #[serde(default, skip_serializing_if = "is_top_camera")]
    pub camera: Camera3D,
    /// Elevation `Z` of the 3D camera target (`0.0` for planar 2D views).
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub target_z: f64,
    /// Per-viewport grid display flag (AutoCAD `AcDbViewport::isGridOn`, DXF group 90 bit 512).
    /// Newly created layout viewports default to `false` (off).
    #[serde(default, skip_serializing_if = "is_false")]
    pub grid: bool,
    /// Keep an independent UCS per viewport (`UCSVP`; DXF group 71). AutoCAD defaults to `true`.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub ucs_per_viewport: bool,
    /// Shade plot mode (`AcDbViewport::shadePlot`; DXF group 170): `0` As Displayed,
    /// `1` Legacy wireframe, `2` Legacy hidden, `3` Rendered.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub shade_plot: u8,
    /// Render mode / visual style (DXF group 281): `0` 2D Wireframe, `1` 3D Wireframe,
    /// `2` Hidden, `3` Flat Shaded, `4` Gouraud Shaded, `5` Flat Shaded with Edges,
    /// `6` Gouraud Shaded with Edges.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub render_mode: u8,
}
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_top_camera(c: &Camera3D) -> bool {
    *c == Camera3D::TOP
}
impl Viewport {
    /// Display labels for `shade_plot` (`0..=3`) matching AutoCAD's Properties palette.
    pub const SHADE_PLOT_NAMES: [&'static str; 4] = ["As Displayed", "Legacy wireframe", "Legacy hidden", "Rendered"];

    /// Display label for this viewport's `shade_plot` mode (`0..=3`).
    pub fn shade_plot_name(&self) -> &'static str {
        Self::SHADE_PLOT_NAMES.get(usize::from(self.shade_plot)).copied().unwrap_or(Self::SHADE_PLOT_NAMES[0])
    }

    /// Parse a shade plot mode from a numeric code (`0..=3`) or label.
    pub fn parse_shade_plot(s: &str) -> Option<u8> {
        let trimmed = s.trim();
        if let Ok(n) = trimmed.parse::<u8>()
            && n <= 3
        {
            return Some(n);
        }
        Self::SHADE_PLOT_NAMES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(trimmed))
            .map(|i| i as u8)
    }

    /// Display label for this viewport's `render_mode` (DXF group 281).
    pub fn render_mode_name(&self) -> &'static str {
        match self.render_mode {
            0 => "2D Wireframe",
            1 => "3D Wireframe",
            2 => "Hidden",
            3 => "Flat Shaded",
            4 => "Gouraud Shaded",
            5 => "Flat Shaded with Edges",
            6 => "Gouraud Shaded with Edges",
            _ => "2D Wireframe",
        }
    }

    /// Whether this viewport has per-viewport frozen layers or layer color overrides.
    pub fn has_layer_overrides(&self) -> bool {
        !self.frozen_layers.is_empty() || !self.layer_colors.is_empty()
    }
    /// The view twist, usable: 0 when it isn't finite.
    pub fn view_twist(&self) -> f64 {
        if self.twist.is_finite() { self.twist } else { 0.0 }
    }
    /// The camera target elevation Z, usable: 0 when it isn't finite.
    pub fn view_target_z(&self) -> f64 {
        if self.target_z.is_finite() { self.target_z } else { 0.0 }
    }
    /// Full 3D camera target in model world coordinates.
    pub fn target(&self) -> Vec3 {
        Vec3::new(self.view_center.x, self.view_center.y, self.view_target_z())
    }
    /// Effective 3D camera orientation including any planar `twist`.
    pub fn effective_camera(&self) -> Camera3D {
        self.camera.with_twist(self.view_twist())
    }
    /// Paper units per model unit.
    pub fn scale(&self) -> f64 {
        self.height / self.view_height
    }
    /// Change this viewport's paper rectangle `(center_xy, width, height)` while keeping the
    /// displayed model geometry pinned at the same paper position and scale (`height / view_height`),
    /// regardless of whether the viewport is display-locked.
    pub fn resize_border_keeping_camera(&mut self, center_xy: Vec2, width: f64, height: f64) -> bool {
        if !(center_xy.is_finite()
            && width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
            && self.height.is_finite()
            && self.height > 0.0
            && self.view_height.is_finite()
            && self.view_height > 0.0)
        {
            return false;
        }
        let scale0 = self.scale();
        if !(scale0.is_finite() && scale0 > 1e-12) {
            return false;
        }
        let d_uv = (center_xy - self.center.xy()) / scale0;
        let eff = self.effective_camera();
        let new_target = self.target() + eff.x * d_uv.x + eff.y * d_uv.y;
        let new_view_height = self.view_height * (height / self.height);
        if !(new_target.is_finite() && new_view_height.is_finite() && new_view_height > 0.0) {
            return false;
        }
        self.view_center = new_target.xy();
        self.target_z = new_target.z;
        self.view_height = new_view_height;
        self.center.x = center_xy.x;
        self.center.y = center_xy.y;
        self.width = width;
        self.height = height;
        true
    }
    /// Where 3D model point `m` projects onto the paper sheet through this viewport.
    pub fn model_to_paper_3d(&self, m: Vec3) -> Vec2 {
        let eff = self.effective_camera();
        if eff.is_top_plan() && self.view_target_z() == 0.0 {
            return self.center.xy() + (m.xy() - self.view_center).rotate(self.view_twist()) * self.scale();
        }
        self.center.xy() + eff.project_3d(m, self.target()) * self.scale()
    }
    /// Where model point `m` (on `Z = 0`) is on the sheet.
    pub fn model_to_paper(&self, m: Vec2) -> Vec2 {
        self.model_to_paper_3d(m.to3(0.0))
    }
    /// The model point at sheet point `p` (on `Z = 0`, falling back to image-plane unprojection
    /// for edge-on views such as Front/Left).
    pub fn paper_to_model(&self, p: Vec2) -> Vec2 {
        let eff = self.effective_camera();
        let uv = (p - self.center.xy()) / self.scale();
        if eff.is_top_plan() {
            return self.view_center + uv.rotate(-self.view_twist());
        }
        let (ray_o, ray_d) = eff.ray_at_target(uv, self.target());
        if !eff.is_edge_on_xy()
            && let Some(hit) = cadcraft_geom::intersect_ray_plane(ray_o, ray_d, Vec3::ZERO, Vec3::Z)
        {
            return hit.xy();
        }
        eff.unproject_to_xy(uv, self.view_center)
    }
    /// Conservative model-space XY bounds visible inside paper rectangle `area` (capped cleanly for
    /// near-edge-on 3D views so viewport culling stays bounded).
    pub fn model_cull_bounds(&self, paper_area: Bounds2) -> Bounds2 {
        let eff = self.effective_camera();
        if eff.is_top_plan() {
            return Bounds2::from_points(paper_area.corners().map(|p| self.paper_to_model(p)));
        }
        let diag = paper_area.width().hypot(paper_area.height()) / self.scale().abs().max(1e-12);
        let cap = diag * 3.0;
        let capped = Bounds2::new(self.view_center - Vec2::new(cap, cap), self.view_center + Vec2::new(cap, cap));
        if eff.z.z.abs() <= 0.15 {
            return capped;
        }
        let b = Bounds2::from_points(paper_area.corners().map(|p| self.paper_to_model(p)));
        let lo = Vec2::new(b.min.x.clamp(capped.min.x, capped.max.x), b.min.y.clamp(capped.min.y, capped.max.y));
        let hi = Vec2::new(b.max.x.clamp(capped.min.x, capped.max.x), b.max.y.clamp(capped.min.y, capped.max.y));
        Bounds2::new(lo.min(self.view_center), hi.max(self.view_center))
    }
    /// [`Viewport::model_to_paper`] as a 2D affine matrix (projecting `Z = 0` geometry onto paper).
    pub fn model_to_paper_matrix(&self) -> Mat3 {
        let s = self.scale();
        let eff = self.effective_camera();
        if eff.is_top_plan() && self.view_target_z() == 0.0 {
            return Mat3::translate(self.center.xy())
                .then_before(Mat3::scale(s, s))
                .then_before(Mat3::rotate(self.view_twist()))
                .then_before(Mat3::translate(-self.view_center));
        }
        let o = self.model_to_paper(Vec2::ZERO);
        Mat3 { a: s * eff.x.x, b: s * eff.y.x, c: s * eff.x.y, d: s * eff.y.y, e: o.x, f: o.y }
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            center: Vec3::ZERO,
            width: 1.0,
            height: 1.0,
            view_center: Vec2::ZERO,
            view_height: 1.0,
            id: 0,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
            twist: 0.0,
            camera: Camera3D::TOP,
            target_z: 0.0,
            grid: false,
            ucs_per_viewport: true,
            shade_plot: 0,
            render_mode: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub insert: Vec3,
    /// One pixel's width (U) and height (V) vectors in drawing units.
    pub u: Vec3,
    pub v: Vec3,
    /// Image size in pixels.
    pub size: Vec2,
    /// The image file (its IMAGEDEF object's path).
    pub path: String,
    /// The image definition's name (its key in the drawing's image dictionary).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Clip boundary in pixel coordinates (y down, counted from pixel centres: the centre of the
    /// top-left pixel is the origin, so the whole image is -0.5 to size - 0.5): two opposite
    /// corners of a rectangle, or a polygon's vertices. Empty: the whole image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied.
    #[serde(default)]
    pub clipping: bool,
    /// Display flags: 1 show the image, 2 show it when not aligned with the screen, 4 use the
    /// clip boundary, 8 transparency on.
    #[serde(default = "image_display")]
    pub display: u16,
    /// Brightness, contrast and fade, 0..=100.
    #[serde(default = "image_fifty")]
    pub brightness: u8,
    #[serde(default = "image_fifty")]
    pub contrast: u8,
    #[serde(default)]
    pub fade: u8,
    /// The definition's default size of one pixel and its resolution unit (0 none, 2 cm, 5 inch).
    #[serde(default = "image_pixel")]
    pub pixel_size: Vec2,
    #[serde(default)]
    pub resolution_units: u8,
}

fn image_display() -> u16 {
    7
}

fn image_fifty() -> u8 {
    50
}

fn image_pixel() -> Vec2 {
    Vec2::new(1.0, 1.0)
}

impl Default for Image {
    fn default() -> Self {
        Image {
            insert: Vec3::ZERO,
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(1.0, 1.0),
            path: String::new(),
            name: String::new(),
            clip: Vec::new(),
            clipping: false,
            display: image_display(),
            brightness: image_fifty(),
            contrast: image_fifty(),
            fade: 0,
            pixel_size: image_pixel(),
            resolution_units: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub text: String,
    #[serde(default)]
    pub merged: Option<(u32, u32)>,
    /// The cell's own cell style; empty: its row's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// A formula (`=Sum(A1:B3)`); `text` holds its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The cell's own text height (DXF 140 in the cell record), before the table's scale;
    /// `None`: its cell style's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_height: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub insert: Vec3,
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<Vec<TableCell>>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "default_text_h")]
    pub text_height: f64,
    /// First row is a title (merged across, larger text).
    #[serde(default)]
    pub title: bool,
    /// The row after the title holds column headers.
    #[serde(default)]
    pub header: bool,
    /// The cell style of each row; missing or empty: by position (Title, Header, Data).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_styles: Vec<String>,
    /// Table breaking (TABLEBREAK): the table drawn in several parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<crate::TableBreak>,
    /// The CSV file the cells come from (DATALINK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_link: Option<crate::DataLink>,
}

fn default_text_h() -> f64 {
    0.18
}

#[cfg(test)]
mod viewport_3d_tests {
    use super::*;

    #[test]
    fn viewport_3d_projection_and_edge_on_cull_bounds() {
        let cam = Camera3D::preset("se_isometric").expect("se_isometric");
        let vp = Viewport {
            center: Vec3::new(100.0, 80.0, 0.0),
            width: 80.0,
            height: 60.0,
            view_center: Vec2::new(10.0, 20.0),
            view_height: 30.0,
            id: 2,
            camera: cam,
            target_z: 12.0,
            ..Viewport::default()
        };
        let p = Vec2::new(18.0, 24.0);
        let paper = vp.model_to_paper(p);
        assert!(paper.near(vp.model_to_paper_matrix().apply(p), 1e-9));
        let back = vp.paper_to_model(paper);
        assert!(back.near(p, 1e-9), "3D roundtrip failed: {back:?} vs {p:?}");

        let front = Viewport {
            camera: Camera3D::preset("front").expect("front"),
            target_z: 15.0,
            ..vp
        };
        let area = Bounds2::new(Vec2::new(60.0, 50.0), Vec2::new(140.0, 110.0));
        let cull = front.model_cull_bounds(area);
        assert!(!cull.is_empty() && cull.min.is_finite() && cull.max.is_finite());
        assert!(cull.contains(front.view_center));
        assert!(cull.width() <= 350.0 && cull.height() <= 350.0, "cull blew up: {cull:?}");
    }

    #[test]
    fn resize_border_keeping_camera_preserves_projection_and_scale() {
        let top = Viewport {
            center: Vec3::new(100.0, 80.0, 0.0),
            width: 80.0,
            height: 60.0,
            view_center: Vec2::new(10.0, 20.0),
            view_height: 30.0,
            id: 2,
            locked: true,
            ..Viewport::default()
        };
        let twisted = Viewport { twist: 0.7, ..top.clone() };
        let front = Viewport {
            camera: Camera3D::preset("front").expect("front"),
            target_z: 15.0,
            ..top.clone()
        };
        let iso = Viewport {
            camera: Camera3D::preset("se_isometric").expect("se_isometric"),
            target_z: 12.0,
            ..top.clone()
        };
        let probes = [Vec3::ZERO, Vec3::new(10.0, 20.0, 0.0), Vec3::new(-5.5, 42.0, 17.0)];
        for orig in [top, twisted, front, iso] {
            let mut resized = orig.clone();
            assert!(resized.resize_border_keeping_camera(Vec2::new(125.0, 95.0), 130.0, 90.0));
            assert!((resized.scale() - orig.scale()).abs() < 1e-12);
            for pt in probes {
                assert!(
                    resized.model_to_paper_3d(pt).near(orig.model_to_paper_3d(pt), 1e-9),
                    "point {pt:?} shifted on paper for {:?}: {:?} vs {:?}",
                    orig.camera.view_name(),
                    resized.model_to_paper_3d(pt),
                    orig.model_to_paper_3d(pt),
                );
            }
        }
        let mut degenerate = Viewport::default();
        assert!(!degenerate.resize_border_keeping_camera(Vec2::ZERO, 0.0, 10.0));
        assert!(!degenerate.resize_border_keeping_camera(Vec2::ZERO, 10.0, -5.0));
    }
}
