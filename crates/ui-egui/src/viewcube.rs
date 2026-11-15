//! The navigation overlays on the drawing area: the 3D Beveled ViewCube (26 clickable zones:
//! 6 faces, 12 edges, 8 corners; rotating compass ring N/E/S/W; roll CW/CCW arrows; orthogonal
//! tilt triangles; Home button; WCS menu) and the 3-axis CADCross / UCS icon.

use cadcraft_geom::{Camera3D, Vec2, Vec3, build_beveled_cube, point_in_polygon};
use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::theme::Tokens;

/// Legacy constant kept for compatibility.
pub const NO_TWIST: &str = "ViewCube: rotating the view needs a view twist, which the 2D plan view doesn't support yet.";

/// Configurable ViewCube dimensions (ported from MATCAD `cubeOptions` in `_startup/MatCadApp_initParams.m`).
pub const RING_RADIUS: f32 = 58.0;
pub const RING_THICKNESS: f32 = 9.0;
/// Half-extent of the 3D ViewCube in screen/world units.
pub const CUBE_HALF: f64 = 26.0;
/// Bevel inset fraction (`0 < ew < 0.45`) of the 3D ViewCube edges/corners.
pub const CUBE_BEVEL: f64 = 0.22;
/// Radial distance (gap) between compass ring outer edge and roll arrows.
/// In MATCAD: `rollArrowGap = 0.08` vs `ringRo = 0.43` (~18.6% of ring outer radius).
pub const ROLL_ARROW_GAP: f32 = 20.0;
/// Radial thickness of the curved roll arrows (slender shaft).
pub const ROLL_ARROW_WIDTH: f32 = 4.8;
/// Angular span of roll arrows in degrees (MATCAD `rollArrowSpan = [39, 62]`).
pub const ROLL_ARROW_START_DEG: f32 = 62.0;
pub const ROLL_ARROW_END_DEG: f32 = 39.0;
/// Multiplier applied to egui's global `style.interaction.tooltip_delay` for ViewCube tooltips
/// (2.0 = twice the global delay; set to 1.0 to match the global delay directly).
pub const VIEWCUBE_TIP_DELAY_SCALE: f64 = 2.0;

/// Proportional UI scale factor derived from the viewport's smaller screen pixel dimension
/// (~17% relative to the nominal 120 px box, clamped between 35 px minimum and 120 px maximum).
pub(crate) fn viewport_scale(viewport_full: Rect) -> f32 {
    let min_dim = viewport_full.width().min(viewport_full.height()).max(0.0);
    ((min_dim * 0.17) / 120.0).clamp(35.0 / 120.0, 1.0)
}

/// Where the ViewCube's parts are on the canvas.
#[derive(Clone, Debug)]
pub struct Layout {
    pub center: Pos2,
    /// Proportional scale factor in `[35/120 ..= 1.0]` relative to full-size ViewCube.
    pub scale: f32,
    /// Visible screen clip rect limiting interaction and painting to the active viewport.
    pub clip: Rect,
    /// Radius of the compass ring's centre line.
    pub ring: f32,
    pub face: Rect,
    pub home: Rect,
    pub roll_ccw: Rect,
    pub roll_cw: Rect,
    /// 4 Orthogonal directional navigation triangles around the cube edges.
    pub dir_top: [Pos2; 3],
    pub dir_bottom: [Pos2; 3],
    pub dir_left: [Pos2; 3],
    pub dir_right: [Pos2; 3],
    /// Curved CW / CCW roll arrow polygons & arcs.
    pub cw_poly: Vec<Pos2>,
    pub cw_outer: Vec<Pos2>,
    pub cw_inner: Vec<Pos2>,
    pub cw_head: [Pos2; 3],
    pub ccw_poly: Vec<Pos2>,
    pub ccw_outer: Vec<Pos2>,
    pub ccw_inner: Vec<Pos2>,
    pub ccw_head: [Pos2; 3],
    /// The "WCS" coordinate-system menu under the cube.
    pub pill: Rect,
    /// Everything that belongs to the cube (the area kept away from the canvas).
    pub bounds: Rect,
}

impl Layout {
    /// Compute the ViewCube layout and pre-built roll arrow geometry for `canvas`.
    pub fn new(canvas: Rect) -> Self {
        Self::for_viewport(canvas, canvas)
    }

    /// Compute the ViewCube layout scaled to `viewport_full`'s pixel size and anchored/clipped
    /// within the visible screen rectangle `viewport_clip`.
    pub fn for_viewport(viewport_full: Rect, viewport_clip: Rect) -> Self {
        let s = viewport_scale(viewport_full);
        let clip = viewport_clip;
        let center = pos2(clip.right() - 92.0 * s, clip.top() + 100.0 * s);
        let ring = RING_RADIUS * s;
        let ring_outer = ring + (RING_THICKNESS * s) * 0.5;
        let r_inner = ring_outer + ROLL_ARROW_GAP * s;
        let r_outer = r_inner + ROLL_ARROW_WIDTH * s;
        let (dir_top, dir_bottom, dir_left, dir_right) = directional_triangles_for_cam(center, &Camera3D::TOP, s);
        let (cw_poly, cw_outer, cw_inner, cw_head) = build_curved_arrow_data(center, r_inner, r_outer, ROLL_ARROW_START_DEG, ROLL_ARROW_END_DEG, true);
        let (ccw_poly, ccw_outer, ccw_inner, ccw_head) = mirror_arrow_data(center, &cw_poly, &cw_outer, &cw_inner, &cw_head);
        let roll_cw = poly_bounding_rect(&cw_poly);
        let roll_ccw = poly_bounding_rect(&ccw_poly);
        let bounds = Rect::from_min_max(center - vec2(104.0 * s, 98.0 * s), center + vec2(88.0 * s, 88.0 * s));
        let pill_scale = s.clamp(0.55, 1.0);
        let pill_y = ring + 13.5 * s + 12.5 * pill_scale;
        Layout {
            center,
            scale: s,
            clip,
            ring,
            face: Rect::from_center_size(center, vec2(44.0 * s, 44.0 * s)),
            home: Rect::from_center_size(center + vec2(-86.0 * s, -82.0 * s), vec2(20.0 * s, 20.0 * s)),
            roll_ccw,
            roll_cw,
            dir_top,
            dir_bottom,
            dir_left,
            dir_right,
            cw_poly,
            cw_outer,
            cw_inner,
            cw_head,
            ccw_poly,
            ccw_outer,
            ccw_inner,
            ccw_head,
            pill: Rect::from_center_size(center + vec2(0.0, pill_y), vec2(56.0 * pill_scale, 16.0 * pill_scale)),
            bounds,
        }
    }

    /// Hit-test using default Top camera.
    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        self.part_at_cam(p, &Camera3D::TOP)
    }

    /// 4 orthogonal directional navigation triangles rotated synchronously with the active cube face
    /// (ported from MATCAD `createViewCubeGraphics.m` & `synchronize_viewcube.m`).
    pub fn dir_triangles(&self, cam: &Camera3D) -> ([Pos2; 3], [Pos2; 3], [Pos2; 3], [Pos2; 3]) {
        directional_triangles_for_cam(self.center, cam, self.scale)
    }

    /// Hit-test `p` against the 3D projected beveled cube, compass ring, roll buttons,
    /// directional triangles and home icon.
    pub fn part_at_cam(&self, p: Pos2, cam: &Camera3D) -> Option<Part> {
        if !self.clip.contains(p) {
            return None;
        }
        if self.home.contains(p) {
            return Some(Part::Home);
        }
        if point_in_poly(&self.ccw_poly, p) {
            return Some(Part::RollCcw);
        }
        if point_in_poly(&self.cw_poly, p) {
            return Some(Part::RollCw);
        }
        if cam.is_orthogonal() {
            let (dir_top, dir_bottom, dir_left, dir_right) = self.dir_triangles(cam);
            if point_in_tri(p, &dir_top) {
                return Some(Part::DirTop);
            }
            if point_in_tri(p, &dir_bottom) {
                return Some(Part::DirBottom);
            }
            if point_in_tri(p, &dir_left) {
                return Some(Part::DirLeft);
            }
            if point_in_tri(p, &dir_right) {
                return Some(Part::DirRight);
            }
        }
        if let Some(part_name) = hit_beveled_cube(self.center, cam, self.scale, p) {
            if part_name == "top" && cam.is_top_plan() {
                return Some(Part::Face);
            }
            return Some(Part::Cube(part_name));
        }
        let d = p.distance(self.center);
        let tol = 10.0 * self.scale;
        ((self.ring - tol..=self.ring + tol).contains(&d)).then_some(Part::Ring)
    }
}

/// Interactive part of the ViewCube widget under the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Face,
    Cube(&'static str),
    Home,
    Ring,
    RollCcw,
    RollCw,
    DirTop,
    DirBottom,
    DirLeft,
    DirRight,
}

impl Part {
    /// Hover tooltip text for this ViewCube part, returning `None` for the 6 main faces that
    /// already display their label on the cube.
    fn tip(self) -> Option<&'static str> {
        match self {
            Part::Face => None,
            Part::Cube("top" | "bottom" | "front" | "back" | "left" | "right") => None,
            Part::Cube(name) => Some(name),
            Part::Home => Some("Home: isometric / extents view"),
            Part::Ring => Some("Compass: drag to orbit around World Z"),
            Part::RollCcw => Some("Roll view 90° counter-clockwise"),
            Part::RollCw => Some("Roll view 90° clockwise"),
            Part::DirTop => Some("Tilt view 90° up"),
            Part::DirBottom => Some("Tilt view 90° down"),
            Part::DirLeft => Some("Tilt view 90° left"),
            Part::DirRight => Some("Tilt view 90° right"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TipTarget {
    ViewCube(Part),
    Wcs,
}

#[derive(Clone, Copy, Default)]
struct TipHoverState {
    target: Option<TipTarget>,
    entered_at: f64,
    shown: bool,
}

/// Gate a ViewCube tooltip behind `style.interaction.tooltip_delay * VIEWCUBE_TIP_DELAY_SCALE`
/// per hovered target so tooltips follow the global style while waiting longer on ViewCube controls
/// and resetting cleanly when moving between parts.
fn delayed_tip(
    resp: &egui::Response,
    hovered: Option<(TipTarget, &'static str)>,
    dragging: bool,
) -> Option<&'static str> {
    let ctx = &resp.ctx;
    let state_id = resp.id.with("viewcube_tip_delay");
    let clear = || ctx.data_mut(|d| d.remove_temp::<TipHoverState>(state_id));

    if dragging {
        clear();
        return None;
    }

    let Some((target, tip)) = hovered else {
        clear();
        return None;
    };

    let delay = f64::from(ctx.global_style().interaction.tooltip_delay) * VIEWCUBE_TIP_DELAY_SCALE.max(1.0);
    let (now, any_down, time_since_scroll, time_since_click, time_since_move) = ctx.input(|i| {
        (
            i.time,
            i.pointer.any_down(),
            i.time_since_last_scroll(),
            i.pointer.time_since_last_click(),
            i.pointer.time_since_last_movement(),
        )
    });
    if any_down || time_since_click < time_since_move + 0.1 {
        clear();
        return None;
    }

    let prev = ctx.data(|d| d.get_temp::<TipHoverState>(state_id));
    if let Some(state) = prev
        && state.target == Some(target)
        && state.shown
    {
        return Some(tip);
    }

    let entered_at = match prev {
        Some(state) if state.target == Some(target) => state.entered_at,
        _ => now,
    };
    let idle = f64::from(time_since_scroll.min(time_since_move).min(time_since_click));
    let elapsed = (now - entered_at).min(idle).max(0.0);

    if elapsed >= delay {
        ctx.data_mut(|d| {
            d.insert_temp(state_id, TipHoverState { target: Some(target), entered_at, shown: true });
        });
        return Some(tip);
    }

    ctx.data_mut(|d| {
        d.insert_temp(state_id, TipHoverState { target: Some(target), entered_at, shown: false });
    });
    ctx.request_repaint_after_secs((delay - elapsed).max(0.01) as f32);
    None
}

/// A projected 3D beveled-cube quad sorted back-to-front for painting and hit-testing.
struct ProjectedQuad {
    pts: [Pos2; 4],
    poly_world: [Vec2; 4],
    centroid: Vec3,
    part: &'static str,
    is_main_face: bool,
    depth: f64,
    facing: f64,
    shade: f32,
}

/// Project visible beveled-cube quads onto screen coordinates around `center` for `cam`,
/// culling backfaces and shallow edge-on slivers (`facing <= 0.075`) and sorting back-to-front.
fn project_cube_quads(center: Pos2, cam: &Camera3D, scale: f32) -> Vec<ProjectedQuad> {
    let quads = build_beveled_cube(CUBE_HALF * f64::from(scale), CUBE_BEVEL);
    let mut out = Vec::with_capacity(quads.len());
    for q in quads {
        // Backface culling: normal must face toward the viewer (-cam.z).
        // Use a ~4.3° threshold (0.075) so nearly edge-on faces at tiny tilt angles
        // do not appear as 1-pixel sliver planes at the top/bottom of the cube.
        let facing = -q.normal.dot(cam.z);
        if facing <= 0.075 {
            continue;
        }
        let mut pts = [Pos2::ZERO; 4];
        let mut poly_world = [Vec2::ZERO; 4];
        let mut centroid = Vec3::ZERO;
        for (idx, v) in q.verts.iter().enumerate() {
            centroid = centroid + *v * 0.25;
            let uv = cam.project_point(*v, Vec2::ZERO);
            if let Some(slot) = pts.get_mut(idx) {
                *slot = pos2(center.x + uv.x as f32, center.y - uv.y as f32);
            }
            if let Some(wslot) = poly_world.get_mut(idx) {
                *wslot = Vec2::new(uv.x, -uv.y);
            }
        }
        let depth = centroid.dot(cam.z);
        let shade = facing.clamp(0.25, 1.0) as f32;
        out.push(ProjectedQuad { pts, poly_world, centroid, part: q.part, is_main_face: q.is_main_face, depth, facing, shade });
    }
    // Painter's algorithm: furthest (largest dot with cam.z) drawn first.
    out.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Return the topmost visible beveled-cube zone (`"top"`, `"front_right"`, `"top_front_right"`, etc.) containing `p`.
fn hit_beveled_cube(center: Pos2, cam: &Camera3D, scale: f32, p: Pos2) -> Option<&'static str> {
    let local = Vec2::new(f64::from(p.x - center.x), f64::from(p.y - center.y));
    let quads = project_cube_quads(center, cam, scale);
    // Check front-to-back (reverse of draw order).
    for q in quads.iter().rev() {
        if point_in_polygon(&q.poly_world, local) {
            return Some(q.part);
        }
    }
    None
}

/// Surface-adapted colour palette for the ViewCube, compass ring, Home icon and WCS pill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCubePalette {
    pub is_light: bool,
    pub ring_fill: Color32,
    pub ring_hover: Color32,
    pub ring_outer_edge: Color32,
    pub ring_inner_edge: Color32,
    pub compass_text: Color32,
    pub arrow_fill: Color32,
    pub arrow_hover: Color32,
    pub arrow_edge: Color32,
    pub cube_edge: Color32,
    pub cube_hover: Color32,
    pub face_shade_base: f32,
    pub face_shade_range: f32,
    pub bevel_shade_base: f32,
    pub bevel_shade_range: f32,
    pub home_hover_bg: Color32,
    pub home_stroke: Color32,
    pub home_hover_stroke: Color32,
    pub wcs_bg: Color32,
    pub wcs_border: Option<Color32>,
    pub wcs_ink: Color32,
}

impl ViewCubePalette {
    /// Choose ViewCube colours suited to the canvas/paper colour `bg` behind the overlay.
    pub fn for_background(bg: Color32) -> Self {
        let is_light = crate::theme::luminance(bg) > 0.5;
        if is_light {
            Self {
                is_light: true,
                ring_fill: Color32::from_rgb(0xdc, 0xe1, 0xe8),
                ring_hover: Color32::from_rgb(0xb8, 0xcd, 0xe4),
                ring_outer_edge: Color32::from_rgb(0xb2, 0xba, 0xc6),
                ring_inner_edge: Color32::from_rgb(0xc2, 0xca, 0xd4),
                compass_text: Color32::from_rgb(0x2c, 0x33, 0x3d),
                arrow_fill: Color32::from_rgb(0xce, 0xd5, 0xdf),
                arrow_hover: Color32::from_rgb(0x6e, 0xa8, 0xe5),
                arrow_edge: Color32::from_rgb(0x90, 0x99, 0xa8),
                cube_edge: Color32::from_rgb(0x7c, 0x85, 0x93),
                cube_hover: Color32::from_rgb(0x6e, 0xa8, 0xe5),
                face_shade_base: 196.0,
                face_shade_range: 36.0,
                bevel_shade_base: 172.0,
                bevel_shade_range: 32.0,
                home_hover_bg: Color32::from_rgb(0xdc, 0xe3, 0xec),
                home_stroke: Color32::from_rgb(0x55, 0x5e, 0x6b),
                home_hover_stroke: Color32::from_rgb(0x1f, 0x26, 0x30),
                wcs_bg: Color32::from_rgb(0xdc, 0xe1, 0xe8),
                wcs_border: Some(Color32::from_rgb(0xb2, 0xba, 0xc6)),
                wcs_ink: Color32::from_rgb(0x2c, 0x33, 0x3d),
            }
        } else {
            Self {
                is_light: false,
                ring_fill: Color32::from_rgb(0x48, 0x50, 0x5c),
                ring_hover: Color32::from_rgb(0x5c, 0x67, 0x7c),
                ring_outer_edge: Color32::from_rgb(0x5c, 0x65, 0x72),
                ring_inner_edge: Color32::from_rgb(0x38, 0x3d, 0x46),
                compass_text: Color32::from_rgb(0xd2, 0xd6, 0xdc),
                arrow_fill: Color32::from_rgb(0x4a, 0x52, 0x60),
                arrow_hover: Color32::from_rgb(0x6e, 0xa8, 0xe5),
                arrow_edge: Color32::from_rgb(0x6e, 0x76, 0x86),
                cube_edge: Color32::from_rgb(0x52, 0x56, 0x5e),
                cube_hover: Color32::from_rgb(0x6e, 0xa8, 0xe5),
                face_shade_base: 130.0,
                face_shade_range: 45.0,
                bevel_shade_base: 105.0,
                bevel_shade_range: 35.0,
                home_hover_bg: Tokens::DARK.control_hover,
                home_stroke: Tokens::DARK.text_dim,
                home_hover_stroke: Tokens::DARK.text,
                wcs_bg: Color32::from_rgb(0x48, 0x50, 0x5c),
                wcs_border: None,
                wcs_ink: Tokens::DARK.canvas_ink,
            }
        }
    }
}

/// Paint the 3D Beveled ViewCube, rotating compass ring, roll buttons, directional triangles
/// and Home button, and handle clicks and orbit drags.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    show_with_bg(app, ui, l, Tokens::get().canvas);
}

/// Paint the 3D Beveled ViewCube adapted to the background surface colour `bg`.
pub fn show_with_bg(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout, bg: Color32) {
    let pal = ViewCubePalette::for_background(bg);
    let cam = app.session.state().map(|st| crate::canvas::model_camera(st).with_twist(st.view().twist)).unwrap_or_default();
    let s = l.scale;
    let interact_rect = l.bounds.intersect(l.clip);
    let resp = ui.interact(interact_rect, ui.scope_id().with("viewcube"), Sense::click_and_drag());
    let part = resp.hover_pos().and_then(|p| l.part_at_cam(p, &cam));
    let p = ui.painter_at(l.bounds.union(l.pill).intersect(l.clip));

    // 1. Rotating Compass Ring (always robust at all camera angles, ported from MATCAD synchronize_viewcube.m).
    let ring_col = if part == Some(Part::Ring) { pal.ring_hover } else { pal.ring_fill };
    let ring_w = (RING_THICKNESS * s).max(1.5);
    let edge_w = (s * 1.0).clamp(0.6, 1.0);
    p.circle_stroke(l.center, l.ring, Stroke::new(ring_w, ring_col));
    p.circle_stroke(l.center, l.ring + ring_w * 0.5, Stroke::new(edge_w, pal.ring_outer_edge));
    p.circle_stroke(l.center, l.ring - ring_w * 0.5, Stroke::new(edge_w, pal.ring_inner_edge));

    let dir_xy = cam.compass_dir_xy();
    let f = egui::FontId::proportional((13.5 * s).clamp(6.5, 13.5));
    let lc = pal.compass_text;
    for (label, wx, wy) in [("N", 0.0_f64, 1.0_f64), ("E", 1.0, 0.0), ("S", 0.0, -1.0), ("W", -1.0, 0.0)] {
        let u = wx * dir_xy.y - wy * dir_xy.x;
        let v = wx * dir_xy.x + wy * dir_xy.y;
        let pos = l.center + vec2(u as f32, -v as f32) * l.ring;
        p.text(pos, egui::Align2::CENTER_CENTER, crate::i18n::t(label), f.clone(), lc);
    }

    // 2. 4 Directional Navigation Triangles (visible only in orthogonal main views, rotating with the cube, ported from MATCAD).
    if cam.is_orthogonal() {
        let (dir_top, dir_bottom, dir_left, dir_right) = l.dir_triangles(&cam);
        for (tri, is_hot) in [
            (&dir_top, part == Some(Part::DirTop)),
            (&dir_bottom, part == Some(Part::DirBottom)),
            (&dir_left, part == Some(Part::DirLeft)),
            (&dir_right, part == Some(Part::DirRight)),
        ] {
            let fill = if is_hot { pal.arrow_hover } else { pal.arrow_fill };
            p.add(Shape::convex_polygon(tri.to_vec(), fill, Stroke::new(edge_w, pal.arrow_edge)));
        }
    }

    // 3. Curved 90° Roll Arrows (CCW top-left and CW top-right outside the ring, ported from MATCAD).
    let ccw_fill = if part == Some(Part::RollCcw) { pal.arrow_hover } else { pal.arrow_fill };
    paint_curved_arrow(&p, &l.ccw_poly, &l.ccw_outer, &l.ccw_inner, &l.ccw_head, ccw_fill, Stroke::new(edge_w, pal.arrow_edge));

    let cw_fill = if part == Some(Part::RollCw) { pal.arrow_hover } else { pal.arrow_fill };
    paint_curved_arrow(&p, &l.cw_poly, &l.cw_outer, &l.cw_inner, &l.cw_head, cw_fill, Stroke::new(edge_w, pal.arrow_edge));

    // 4. 3D Beveled Cube Quads (26 interactive zones).
    let hot_part = match part {
        Some(Part::Face) => Some("top"),
        Some(Part::Cube(name)) => Some(name),
        _ => None,
    };
    let quads = project_cube_quads(l.center, &cam, s);
    let edge_stroke = Stroke::new((0.9 * s).clamp(0.5, 0.9), pal.cube_edge);
    for q in &quads {
        let is_hot = hot_part == Some(q.part);
        let base: u8 = if q.is_main_face {
            (pal.face_shade_base + pal.face_shade_range * q.shade) as u8
        } else {
            (pal.bevel_shade_base + pal.bevel_shade_range * q.shade) as u8
        };
        let fill = if is_hot {
            pal.cube_hover
        } else {
            Color32::from_rgb(base, base.saturating_add(4), base.saturating_add(10))
        };
        // Draw quad fill as a 2-triangle Mesh + 4 individual edge segments so epaint's
        // convex_polygon miter-join feathering never shoots horizontal spikes at shallow angles.
        let mut qmesh = egui::Mesh::default();
        for &pt in &q.pts {
            qmesh.vertices.push(egui::epaint::Vertex { pos: pt, uv: egui::epaint::WHITE_UV, color: fill });
        }
        qmesh.indices.extend([0, 1, 2, 0, 2, 3]);
        p.add(Shape::mesh(qmesh));
        for i in 0..4 {
            p.line_segment([q.pts[i], q.pts[(i + 1) % 4]], edge_stroke);
        }
        if q.is_main_face {
            paint_face_text(&p, l.center, &cam, q, s);
        }
    }

    // 5. Home button at the top-left.
    paint_home(&p, l.home, part == Some(Part::Home), s, &pal);

    let resp = if part.is_some() {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    };
    let tip_target = part.and_then(|p| p.tip().map(|t| (TipTarget::ViewCube(p), t)));
    if let Some(tip) = delayed_tip(&resp, tip_target, app.canvas.viewcube_drag.is_some()) {
        egui::Tooltip::for_enabled(&resp)
            .at_pointer()
            .gap(12.0)
            .show(|ui| {
                ui.label(tip);
            });
    }

    // 6. ViewCube and Compass Ring Dragging:
    // Storing an `OrbitDrag` snapshot at drag start prevents horizon drift and rigidly orbits
    // `target0` about the resolved auto-target pivot.
    if resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(press_pos) = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos())
    {
        let is_ring = l.part_at_cam(press_pos, &cam) == Some(Part::Ring);
        app.canvas.viewcube_drag = crate::canvas::begin_canvas_orbit(app, press_pos, None).map(|drag| (drag, is_ring));
    }
    if resp.dragged_by(egui::PointerButton::Primary)
        && let Some((drag, is_ring)) = app.canvas.viewcube_drag
        && let Some(curr_pos) = resp.interact_pointer_pos()
    {
        if is_ring {
            // Angular delta around ring center (MATCAD move_viewcube.m lines 191-198)
            let a0 = -(drag.start.y - l.center.y).atan2(drag.start.x - l.center.x);
            let a1 = -(curr_pos.y - l.center.y).atan2(curr_pos.x - l.center.x);
            let daz_deg = f64::from((a1 - a0).to_degrees());
            crate::canvas::apply_canvas_orbit(app, &drag, daz_deg, 0.0);
        } else {
            // Cube drag: total pixel offset from drag start
            let delta = curr_pos - drag.start;
            let daz_deg = f64::from(delta.x) * 0.5;
            let del_deg = -f64::from(delta.y) * 0.5; // Negative because egui Y increases downwards
            crate::canvas::apply_canvas_orbit(app, &drag, daz_deg, del_deg);
        }
    }
    if resp.drag_stopped() {
        app.canvas.viewcube_drag = None;
    }

    if resp.clicked() {
        let pressed_on = resp.interact_pointer_pos().and_then(|o| l.part_at_cam(o, &cam));
        match pressed_on {
            Some(Part::Face) => {
                if !app.session.state().is_ok_and(|st| st.is_active_viewport_locked()) {
                    let _ = app.run("view.set", json!({"preset": "top"}));
                    let _ = app.run("zoom.extents", json!({}));
                }
            }
            Some(Part::Cube(preset)) => {
                let _ = app.run("view.set", json!({"preset": preset}));
            }
            Some(Part::Home) => {
                activate_home_view(app);
            }
            Some(Part::RollCcw) => {
                let _ = app.run("view.roll", json!({"angle": 90.0}));
            }
            Some(Part::RollCw) => {
                let _ = app.run("view.roll", json!({"angle": -90.0}));
            }
            Some(Part::DirTop) => {
                let _ = app.run("view.set", json!({"neighbor": "top"}));
            }
            Some(Part::DirBottom) => {
                let _ = app.run("view.set", json!({"neighbor": "bottom"}));
            }
            Some(Part::DirLeft) => {
                let _ = app.run("view.set", json!({"neighbor": "left"}));
            }
            Some(Part::DirRight) => {
                let _ = app.run("view.set", json!({"neighbor": "right"}));
            }
            Some(Part::Ring) => {
                app.session.echo(NO_TWIST);
            }
            None => {}
        }
    }

    let mut action = None;
    resp.context_menu(|ui| {
        if ui.button("Home (SE Isometric)").clicked() {
            action = Some(("view.set", json!({"preset": "top_front_right"})));
            ui.close();
        }
        if ui.button("Top (Plan View)").clicked() {
            action = Some(("plan", json!({})));
            ui.close();
        }
        if ui.button("Zoom Extents").clicked() {
            action = Some(("zoom.extents", json!({})));
            ui.close();
        }
        ui.separator();
        if ui.button("Hide ViewCube").clicked() {
            action = Some(("ui.toggle.viewcube", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = action {
        let _ = app.run(id, params);
    }
    // The WCS pill (painted by the canvas) opens the coordinate-system menu.
    let pill = ui.interact(l.pill.intersect(l.clip), ui.scope_id().with("viewcube_wcs"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
    let pill_target = pill.hovered().then_some((TipTarget::Wcs, "Coordinate system"));
    if let Some(tip) = delayed_tip(&pill, pill_target, app.canvas.viewcube_drag.is_some()) {
        egui::Tooltip::for_enabled(&pill).show(|ui| {
            ui.label(tip);
        });
    }
    ucs_menu(app, egui::Popup::menu(&pill));
}

/// Reset the camera to the configured ViewCube Home preset (default `"top"`) and fit the drawing
/// extents to the viewport.
fn activate_home_view(app: &mut CadApp) {
    let raw = app.ui.viewcube_home_preset.trim();
    let preset = if Camera3D::preset(raw).is_some() { raw.to_owned() } else { "top".to_owned() };
    let Ok(st) = app.session.state() else { return };
    if st.is_active_viewport_locked() {
        app.session.echo("The viewport is locked.");
        return;
    }
    if matches!(st.space, cadcraft_doc::Space::Model) || st.active_viewport().is_some() {
        let v = st.view();
        if v.twist.abs() > 1e-12 && st.active_viewport().is_none() {
            let _ = app.run(
                "view.set",
                json!({
                    "center": [v.center.x, v.center.y],
                    "height": v.height,
                    "twist": 0.0,
                    "preset": preset,
                }),
            );
        } else {
            let _ = app.run("view.set", json!({"preset": preset}));
        }
        let _ = app.run("zoom.extents", json!({}));
    }
}

/// A small house, drawn in code.
fn paint_home(p: &egui::Painter, r: Rect, hot: bool, scale: f32, pal: &ViewCubePalette) {
    if hot {
        p.rect_filled(r, 3.0 * scale, pal.home_hover_bg);
    }
    let st = Stroke::new((1.3 * scale).clamp(0.7, 1.3), if hot { pal.home_hover_stroke } else { pal.home_stroke });
    let c = r.center();
    let pt = |dx: f32, dy: f32| c + vec2(dx * scale, dy * scale);
    let roof = [pt(-6.5, 0.5), pt(0.0, -6.0), pt(6.5, 0.5)];
    p.line(roof.to_vec(), st);
    p.line(vec![pt(-4.5, -1.0), pt(-4.5, 6.0), pt(4.5, 6.0), pt(4.5, -1.0)], st);
    p.line_segment([pt(-1.2, 6.0), pt(-1.2, 2.5)], st);
    p.line_segment([pt(1.2, 6.0), pt(1.2, 2.5)], st);
    p.line_segment([pt(-1.2, 2.5), pt(1.2, 2.5)], st);
}

/// Project the localized main-face label directly onto the 3D cube face plane (`q.centroid`, `u_3d`, `v_3d`)
/// with a view-invariant 3D scale using the clean, thin proportional UI font.
fn paint_face_text(
    p: &egui::Painter,
    center: Pos2,
    cam: &Camera3D,
    q: &ProjectedQuad,
    ui_scale: f32,
) {
    // Skip drawing face text when the face is nearly edge-on (< ~12° to the line of sight).
    let facing = q.facing;
    if facing < 0.20 {
        return;
    }
    let key = match q.part {
        "top" => "TOP",
        "bottom" => "BOTTOM",
        "front" => "FRONT",
        "back" => "BACK",
        "left" => "LEFT",
        "right" => "RIGHT",
        other => other,
    };
    let label = crate::i18n::t(key);
    let (u_3d, v_3d) = cadcraft_geom::face_text_axes(q.part);

    let alpha = ((facing - 0.20) / 0.15).clamp(0.0, 1.0);
    let text_col = Color32::from_rgba_unmultiplied(0x22, 0x26, 0x2c, (240.0 * alpha) as u8);
    // Base font size scales down softly so small cubes keep crisp glyph rasterization.
    let base_font_size = (13.0 * ui_scale).clamp(6.0, 13.0);
    let galley = p.layout_no_wrap(label.to_owned(), egui::FontId::proportional(base_font_size), text_col);
    let [tex_w, tex_h] = p.ctx().fonts_mut(|f| f.font_image_size());
    if tex_w == 0 || tex_h == 0 {
        return;
    }
    let inv_w = 1.0 / (tex_w as f32);
    let inv_h = 1.0 / (tex_h as f32);

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for row in &galley.rows {
        for v in &row.visuals.mesh.vertices {
            let gx = row.pos.x + v.pos.x;
            let gy = row.pos.y + v.pos.y;
            min_x = min_x.min(gx);
            max_x = max_x.max(gx);
            min_y = min_y.min(gy);
            max_y = max_y.max(gy);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        return;
    }

    let span_x = (max_x - min_x).max(1.0);
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;

    // Constant 3D scale on the cube face (independent of camera angle):
    // 9.8 * ui_scale 3D units nominal font size, clamped so long labels (e.g. BOTTOM, HINTEN, RECHTS)
    // never exceed 86% of the scaled 3D inner face width (`inner_span`).
    let inner_span = (2.0 * (1.0 - CUBE_BEVEL) * CUBE_HALF) as f32 * ui_scale;
    let base_scale = (9.8 * ui_scale) / base_font_size;
    let max_w = inner_span * 0.86;
    let scale = base_scale.min(max_w / span_x);

    let mut mesh = egui::Mesh::default();
    for row in &galley.rows {
        let row_mesh = &row.visuals.mesh;
        let base_idx = mesh.vertices.len() as u32;
        mesh.indices.extend(row_mesh.indices.iter().map(|i| base_idx + *i));
        for v in &row_mesh.vertices {
            let gx = row.pos.x + v.pos.x;
            let gy = row.pos.y + v.pos.y;
            let lx = (gx - cx) * scale;
            let ly = -(gy - cy) * scale;
            let p3 = q.centroid + u_3d * f64::from(lx) + v_3d * f64::from(ly);
            let uv_screen = cam.project_point(p3, Vec2::ZERO);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(center.x + uv_screen.x as f32, center.y - uv_screen.y as f32),
                uv: pos2(v.uv.x * inv_w, v.uv.y * inv_h),
                color: text_col,
            });
        }
    }
    if !mesh.is_empty() {
        p.add(Shape::mesh(mesh));
    }
}

/// The 4 directional navigation triangles rotated with the active orthogonal cube face
/// (and any active plan-view twist via `Camera3D::with_twist`), ported from MATCAD
/// `createViewCubeGraphics.m` & `synchronize_viewcube.m`.
fn directional_triangles_for_cam(center: Pos2, cam: &Camera3D, scale: f32) -> ([Pos2; 3], [Pos2; 3], [Pos2; 3], [Pos2; 3]) {
    let face = match cam.view_name() {
        "Bottom" => "bottom",
        "Front" => "front",
        "Back" => "back",
        "Left" => "left",
        "Right" => "right",
        _ => "top",
    };
    let (u_3d, v_3d) = cadcraft_geom::face_text_axes(face);
    let project = |wx: f32, wy: f32| -> Pos2 {
        let p3 = u_3d * f64::from(wx) + v_3d * f64::from(wy);
        let uv = cam.project_point(p3, Vec2::ZERO);
        pos2(center.x + uv.x as f32, center.y - uv.y as f32)
    };
    let tip = (CUBE_HALF as f32 + 3.5) * scale;
    let base = tip + 11.0 * scale;
    let hw = 7.5 * scale;
    let top = [
        project(-hw, base),
        project(hw, base),
        project(0.0, tip),
    ];
    let bottom = [
        project(-hw, -base),
        project(hw, -base),
        project(0.0, -tip),
    ];
    let left = [
        project(-base, -hw),
        project(-base, hw),
        project(-tip, 0.0),
    ];
    let right = [
        project(base, -hw),
        project(base, hw),
        project(tip, 0.0),
    ];
    (top, bottom, left, right)
}

/// Build vertices for curved roll arrows (ported from MATCAD `buildCurvedArrowPoly.m`).
fn build_curved_arrow_data(
    center: Pos2,
    r_inner: f32,
    r_outer: f32,
    theta_start_deg: f32,
    theta_end_deg: f32,
    is_cw: bool,
) -> (Vec<Pos2>, Vec<Pos2>, Vec<Pos2>, [Pos2; 3]) {
    let theta_start = theta_start_deg.to_radians();
    let theta_end = theta_end_deg.to_radians();
    let n_pts = 10;
    let r_mid = (r_inner + r_outer) * 0.5;
    let head_len = 7.5_f32.to_radians();
    let head_w = (r_outer - r_inner) * 2.0;

    let mut outer_arc = Vec::with_capacity(n_pts);
    let mut inner_arc = Vec::with_capacity(n_pts);

    for i in 0..n_pts {
        let t = theta_start + (theta_end - theta_start) * (i as f32 / (n_pts - 1) as f32);
        outer_arc.push(pos2(center.x + r_outer * t.cos(), center.y - r_outer * t.sin()));
        inner_arc.push(pos2(center.x + r_inner * t.cos(), center.y - r_inner * t.sin()));
    }

    let (tip, base_outer, base_inner) = if is_cw {
        let tip_theta = theta_end - head_len;
        let tip = pos2(center.x + r_mid * tip_theta.cos(), center.y - r_mid * tip_theta.sin());
        let bo = pos2(center.x + (r_mid + head_w * 0.5) * theta_end.cos(), center.y - (r_mid + head_w * 0.5) * theta_end.sin());
        let bi = pos2(center.x + (r_mid - head_w * 0.5) * theta_end.cos(), center.y - (r_mid - head_w * 0.5) * theta_end.sin());
        (tip, bo, bi)
    } else {
        let tip_theta = theta_end + head_len;
        let tip = pos2(center.x + r_mid * tip_theta.cos(), center.y - r_mid * tip_theta.sin());
        let bo = pos2(center.x + (r_mid + head_w * 0.5) * theta_end.cos(), center.y - (r_mid + head_w * 0.5) * theta_end.sin());
        let bi = pos2(center.x + (r_mid - head_w * 0.5) * theta_end.cos(), center.y - (r_mid - head_w * 0.5) * theta_end.sin());
        (tip, bo, bi)
    };

    let mut poly = Vec::with_capacity(n_pts * 2 + 3);
    poly.extend(outer_arc.iter().copied());
    poly.push(base_outer);
    poly.push(tip);
    poly.push(base_inner);
    let mut inner_rev = inner_arc.clone();
    inner_rev.reverse();
    poly.extend(inner_rev);

    (poly, outer_arc, inner_arc, [base_outer, tip, base_inner])
}

/// Mirror a curved roll arrow horizontally across `center.x` (CW -> CCW).
fn mirror_arrow_data(
    center: Pos2,
    poly: &[Pos2],
    outer: &[Pos2],
    inner: &[Pos2],
    head: &[Pos2; 3],
) -> (Vec<Pos2>, Vec<Pos2>, Vec<Pos2>, [Pos2; 3]) {
    let mirror = |p: Pos2| pos2(2.0 * center.x - p.x, p.y);
    let m_poly = poly.iter().copied().map(mirror).collect();
    let m_outer = outer.iter().copied().map(mirror).collect();
    let m_inner = inner.iter().copied().map(mirror).collect();
    let m_head = [mirror(head[0]), mirror(head[1]), mirror(head[2])];
    (m_poly, m_outer, m_inner, m_head)
}

/// Build a non-convex triangle strip mesh for a curved roll arrow shaft and head.
fn curved_arrow_mesh(outer: &[Pos2], inner: &[Pos2], head: &[Pos2; 3], color: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let n = outer.len().min(inner.len());
    for &pt in outer.iter().take(n) {
        mesh.vertices.push(egui::epaint::Vertex { pos: pt, uv: egui::epaint::WHITE_UV, color });
    }
    for &pt in inner.iter().take(n) {
        mesh.vertices.push(egui::epaint::Vertex { pos: pt, uv: egui::epaint::WHITE_UV, color });
    }
    for i in 0..(n.saturating_sub(1)) as u32 {
        let o0 = i;
        let o1 = i + 1;
        let i0 = n as u32 + i;
        let i1 = n as u32 + i + 1;
        mesh.indices.extend([o0, o1, i0, i0, o1, i1]);
    }
    let base_idx = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex { pos: head[0], uv: egui::epaint::WHITE_UV, color });
    mesh.vertices.push(egui::epaint::Vertex { pos: head[1], uv: egui::epaint::WHITE_UV, color });
    mesh.vertices.push(egui::epaint::Vertex { pos: head[2], uv: egui::epaint::WHITE_UV, color });
    mesh.indices.extend([base_idx, base_idx + 1, base_idx + 2]);
    mesh
}

/// Paint a curved 90° roll arrow fill and outline.
fn paint_curved_arrow(
    p: &egui::Painter,
    poly: &[Pos2],
    outer: &[Pos2],
    inner: &[Pos2],
    head: &[Pos2; 3],
    fill: Color32,
    stroke: Stroke,
) {
    p.add(Shape::mesh(curved_arrow_mesh(outer, inner, head, fill)));
    p.add(Shape::closed_line(poly.to_vec(), stroke));
}

/// Tight bounding rectangle enclosing `pts`.
fn poly_bounding_rect(pts: &[Pos2]) -> Rect {
    let mut r = Rect::NOTHING;
    for pt in pts {
        r.extend_with(*pt);
    }
    r
}

/// Ray-casting point-in-polygon test in screen coordinates.
fn point_in_poly(pts: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let mut j = pts.len().saturating_sub(1);
    for i in 0..pts.len() {
        let (pi, pj) = (pts[i], pts[j]);
        if ((pi.y > p.y) != (pj.y > p.y))
            && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y + 1e-12) + pi.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Point-in-triangle test in screen coordinates.
fn point_in_tri(p: Pos2, t: &[Pos2; 3]) -> bool {
    let (p1, p2, p3) = (t[0], t[1], t[2]);
    let sign = |a: Pos2, b: Pos2, c: Pos2| (a.x - c.x) * (b.y - c.y) - (b.x - c.x) * (a.y - c.y);
    let d1 = sign(p, p1, p2);
    let d2 = sign(p, p2, p3);
    let d3 = sign(p, p3, p1);
    let has_neg = (d1 < -1e-4) || (d2 < -1e-4) || (d3 < -1e-4);
    let has_pos = (d1 > 1e-4) || (d2 > 1e-4) || (d3 > 1e-4);
    !(has_neg && has_pos)
}

/// Responsive scale factor for the UCS icon arms inside `target_full`.
pub(crate) fn ucs_arm_scale(target_full: Rect) -> f32 {
    let min_dim = target_full.width().min(target_full.height()).max(0.0);
    (min_dim / 260.0).clamp(0.45, 1.0)
}

/// The UCS icon's arms (as drawn by the canvas at the lower-left corner): the X arm and the Y arm.
pub fn ucs_arms(canvas: Rect) -> [Rect; 2] {
    ucs_arms_in(canvas, canvas)
}

/// The UCS icon's arms anchored in the lower-left of `viewport_clip`, scaled for `viewport_full`.
pub fn ucs_arms_in(viewport_full: Rect, viewport_clip: Rect) -> [Rect; 2] {
    let s = ucs_arm_scale(viewport_full);
    let pad_x = 36.0 * s;
    let pad_y = 34.0 * s;
    let hit_pad = 8.0 * s;
    let arm_len = 80.0 * s;
    let y_arm_len = 82.0 * s;
    let o = pos2(viewport_clip.left() + pad_x, viewport_clip.bottom() - pad_y);
    [
        Rect::from_min_max(o + vec2(-hit_pad, -hit_pad), o + vec2(arm_len, hit_pad)).intersect(viewport_clip),
        Rect::from_min_max(o + vec2(-hit_pad, -y_arm_len), o + vec2(hit_pad, hit_pad)).intersect(viewport_clip),
    ]
}

/// Projected screen-space arm segments `[origin, tip]` of the UCS icon for `cam`.
pub(crate) fn ucs_arm_segments(viewport_full: Rect, viewport_clip: Rect, cam: &Camera3D) -> Vec<(Pos2, Pos2)> {
    let s = ucs_arm_scale(viewport_full);
    let o = pos2(viewport_clip.left() + 36.0 * s, viewport_clip.bottom() - 34.0 * s);
    if cam.is_top_plan() {
        let arm = 60.0 * s;
        return vec![(o, o + vec2(arm, 0.0)), (o, o + vec2(0.0, -arm))];
    }
    let arm_3d = 56.0 * s;
    [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Z]
        .into_iter()
        .filter_map(|axis| {
            let uv = cam.project_point(axis, Vec2::ZERO);
            let dir = vec2(uv.x as f32, -(uv.y as f32));
            (dir.length() >= 0.08).then(|| (o, o + dir * arm_3d))
        })
        .collect()
}

/// Make the UCS icon clickable: hovering highlights it, a click (either button) opens the
/// coordinate-system menu.
pub fn ucs_icon(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    ucs_icon_in(app, ui, canvas, canvas);
}

/// Make the UCS icon clickable within an active viewport's screen bounds.
pub fn ucs_icon_in(app: &mut CadApp, ui: &mut egui::Ui, viewport_full: Rect, viewport_clip: Rect) {
    let cam = app.canvas.xf.map_or(Camera3D::TOP, |xf| xf.cam);
    ucs_icon_with_camera(app, ui, viewport_full, viewport_clip, &cam);
}

/// Make the UCS icon clickable using the projected arm segments of `cam`.
pub fn ucs_icon_with_camera(app: &mut CadApp, ui: &mut egui::Ui, viewport_full: Rect, viewport_clip: Rect, cam: &Camera3D) {
    let t = Tokens::get();
    let s = ucs_arm_scale(viewport_full);
    let hit_pad = 8.0 * s;
    let arms = ucs_arm_segments(viewport_full, viewport_clip, cam);
    let mut resp: Option<egui::Response> = None;
    for (i, (a, _b)) in arms.iter().enumerate() {
        let rect = if cam.is_top_plan() {
            if i == 0 {
                Rect::from_min_max(*a + vec2(-hit_pad, -hit_pad), *a + vec2(80.0 * s, hit_pad))
            } else {
                Rect::from_min_max(*a + vec2(-hit_pad, -82.0 * s), *a + vec2(hit_pad, hit_pad))
            }
        } else {
            Rect::from_two_pos(*a, arms[i].1).expand(hit_pad)
        }
        .intersect(viewport_clip);
        let r = ui.interact(rect, ui.scope_id().with(("ucs_arm", i)), Sense::click_and_drag());
        resp = Some(match resp {
            Some(prev) => prev | r,
            None => r,
        });
    }
    let Some(resp) = resp else { return };
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("CADCross / UCS icon: World Coordinate System");
    if resp.hovered() {
        let st = Stroke::new((2.0 * s).clamp(1.2, 2.0), t.hover);
        let p = ui.painter_at(viewport_clip);
        for (a, b) in &arms {
            p.line_segment([*a, *b], st);
        }
    }
    let open = (resp.clicked() || resp.secondary_clicked()).then_some(egui::SetOpenCommand::Bool(true));
    ucs_menu(app, egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed());
}

/// The coordinate-system menu: the WCS and quick 3D view presets.
fn ucs_menu(app: &mut CadApp, popup: egui::Popup<'_>) {
    let mut cmd: Option<(&'static str, serde_json::Value)> = None;
    popup.show(|ui| {
        let _ = ui.radio(true, "World (WCS)");
        ui.separator();
        if ui.button("Top (Plan)").clicked() {
            cmd = Some(("view.set", json!({"preset": "top"})));
            ui.close();
        }
        if ui.button("Front").clicked() {
            cmd = Some(("view.set", json!({"preset": "front"})));
            ui.close();
        }
        if ui.button("Right").clicked() {
            cmd = Some(("view.set", json!({"preset": "right"})));
            ui.close();
        }
        if ui.button("SE Isometric").clicked() {
            cmd = Some(("view.set", json!({"preset": "top_front_right"})));
            ui.close();
        }
        if ui.button("SW Isometric").clicked() {
            cmd = Some(("view.set", json!({"preset": "top_front_left"})));
            ui.close();
        }
        ui.separator();
        if ui.button("Hide UCS Icon").clicked() {
            cmd = Some(("ui.toggle.ucsicon", json!({"on": false})));
            ui.close();
        }
    });
    if let Some((id, params)) = cmd {
        let _ = app.run(id, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;

    fn app_and_ctx() -> (CadApp, egui::Context) {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.in_window_menu = true;
        app.start("ui.sample");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        (app, ctx)
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    pub(crate) fn click(app: &mut CadApp, ctx: &egui::Context, at: Pos2, button: egui::PointerButton) {
        let m = egui::Modifiers::default();
        frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: true, modifiers: m }]);
        frame(app, ctx, vec![egui::Event::PointerButton { pos: at, button, pressed: false, modifiers: m }]);
        frame(app, ctx, Vec::new());
    }

    fn menu_shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground))
    }

    fn canvas(app: &CadApp) -> Rect {
        app.canvas.rect.unwrap_or(Rect::NOTHING)
    }

    fn view(app: &CadApp) -> (f64, f64, f64) {
        let v = app.session.state().map(|s| s.view()).unwrap_or_default();
        (v.center.x, v.center.y, v.height)
    }

    #[test]
    fn viewcube_face_labels_render_in_orthogonal_and_3d_views() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));

        // Verify i18n translations for all 6 main faces in English and German
        crate::i18n::with_language("en", || {
            assert_eq!(crate::i18n::t("TOP"), "TOP");
            assert_eq!(crate::i18n::t("BOTTOM"), "BOTTOM");
            assert_eq!(crate::i18n::t("FRONT"), "FRONT");
            assert_eq!(crate::i18n::t("BACK"), "BACK");
            assert_eq!(crate::i18n::t("LEFT"), "LEFT");
            assert_eq!(crate::i18n::t("RIGHT"), "RIGHT");
        });

        crate::i18n::with_language("de", || {
            assert_eq!(crate::i18n::t("TOP"), "OBEN");
            assert_eq!(crate::i18n::t("BOTTOM"), "UNTEN");
            assert_eq!(crate::i18n::t("FRONT"), "VORNE");
            assert_eq!(crate::i18n::t("BACK"), "HINTEN");
            assert_eq!(crate::i18n::t("LEFT"), "LINKS");
            assert_eq!(crate::i18n::t("RIGHT"), "RECHTS");
        });

        // Verify 3D-projected face quad meshes and proportional face text meshes render inside the cube
        for preset in ["top", "bottom", "top_front_right"] {
            let _ = app.run("view.set", json!({"preset": preset}));
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            out.textures_delta.clear();
            let mut quad_meshes = 0;
            let mut text_meshes = 0;
            for cs in &out.shapes {
                if let Shape::Mesh(mesh) = &cs.shape {
                    if mesh.vertices.len() == 4
                        && mesh.vertices.iter().all(|v| (v.pos - l.center).length() < 48.0)
                    {
                        quad_meshes += 1;
                    } else if mesh.vertices.len() > 4
                        && mesh.vertices.iter().all(|v| (v.pos - l.center).length() < 35.0)
                    {
                        text_meshes += 1;
                        assert!(
                            mesh.vertices.iter().any(|v| v.uv.x > 0.0 && v.uv.y > 0.0),
                            "Text mesh should sample glyph UVs, not only origin"
                        );
                        for v in &mesh.vertices {
                            assert!(
                                v.uv.x >= 0.0 && v.uv.x <= 1.0 && v.uv.y >= 0.0 && v.uv.y <= 1.0,
                                "UV must be normalized in [0, 1]: {:?}",
                                v.uv
                            );
                        }
                    }
                }
            }
            let expected_quads = if preset == "top_front_right" { 27 } else { 9 };
            let expected_text = if preset == "top_front_right" { 3 } else { 1 };
            assert_eq!(quad_meshes, expected_quads, "Preset {preset} should paint {expected_quads} cube quad meshes");
            assert_eq!(text_meshes, expected_text, "Preset {preset} should paint {expected_text} face text meshes");
        }
    }

    #[test]
    fn parts_are_found_where_they_are_drawn() {
        let l = Layout::new(Rect::from_min_size(pos2(200.0, 80.0), vec2(1000.0, 800.0)));
        assert_eq!(l.part_at(l.center), Some(Part::Face));
        assert_eq!(l.part_at(l.home.center()), Some(Part::Home));
        assert_eq!(l.part_at(l.center + vec2(0.0, -l.ring)), Some(Part::Ring), "N");
        assert_eq!(l.part_at(l.center + vec2(l.ring + 3.0, 0.0)), Some(Part::Ring), "E");
        assert_eq!(l.part_at(l.center + vec2(30.0, 30.0)), None, "between the face and the ring");
        assert!(l.bounds.contains_rect(l.home) && l.bounds.contains_rect(l.face));

        // When rotated in an orthogonal main view (e.g. 90° CW or 45°), the 4 directional triangles
        // rotate synchronously with the cube.
        let cam_cw90 = Camera3D::TOP.roll_world_z(-90.0);
        assert_eq!(l.part_at_cam(l.center + vec2(33.0, 0.0), &cam_cw90), Some(Part::DirTop));
        assert_eq!(l.part_at_cam(l.center + vec2(-33.0, 0.0), &cam_cw90), Some(Part::DirBottom));
        assert_eq!(l.part_at_cam(l.center + vec2(0.0, -33.0), &cam_cw90), Some(Part::DirLeft));
        assert_eq!(l.part_at_cam(l.center + vec2(0.0, 33.0), &cam_cw90), Some(Part::DirRight));

        let cam_45 = Camera3D::TOP.orbit(45.0, 0.0, true);
        let (t, b, lf, rg) = l.dir_triangles(&cam_45);
        let centroid = |tri: &[Pos2; 3]| pos2((tri[0].x + tri[1].x + tri[2].x) / 3.0, (tri[0].y + tri[1].y + tri[2].y) / 3.0);
        assert_eq!(l.part_at_cam(centroid(&t), &cam_45), Some(Part::DirTop));
        assert_eq!(l.part_at_cam(centroid(&b), &cam_45), Some(Part::DirBottom));
        assert_eq!(l.part_at_cam(centroid(&lf), &cam_45), Some(Part::DirLeft));
        assert_eq!(l.part_at_cam(centroid(&rg), &cam_45), Some(Part::DirRight));
    }

    #[test]
    fn viewcube_clicks_reach_the_cube_not_the_drawing() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let fitted = view(&app);
        let _ = app.run("zoom.in", json!({}));
        assert_ne!(view(&app), fitted);
        click(&mut app, &ctx, l.center, egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Top face zooms to extents");
        assert!(app.session.pending_window.is_none(), "the click didn't start a selection window");
        assert!(app.session.selection().is_empty());
        let _ = app.run("zoom.in", json!({}));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        assert_eq!(view(&app), fitted, "Home zooms to extents");
        click(&mut app, &ctx, l.center + vec2(0.0, -l.ring), egui::PointerButton::Primary);
        assert_eq!(app.session.log.last().map(String::as_str), Some(NO_TWIST));
        assert!(app.session.pending_window.is_none());
        assert_eq!(view(&app), fitted);
        let empty = canvas(&app).center() + vec2(0.0, -300.0);
        click(&mut app, &ctx, empty, egui::PointerButton::Primary);
        assert!(app.session.pending_window.is_some(), "an empty-space click starts a selection window");
    }

    #[test]
    fn ucs_icon_and_wcs_pill_open_the_coordinate_system_menu() {
        let (mut app, ctx) = app_and_ctx();
        let [x, _] = ucs_arms(canvas(&app));
        click(&mut app, &ctx, x.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "UCS icon menu");
        assert!(app.session.pending_window.is_none(), "the click stayed on the icon");
        let l = Layout::new(canvas(&app));
        click(&mut app, &ctx, l.pill.center(), egui::PointerButton::Primary);
        assert!(menu_shown(&ctx), "WCS menu");
        assert!(app.session.pending_window.is_none());

        // In SE Isometric the X arm projects diagonally down-right; clicking near its tip opens the menu.
        let (mut iso_app, iso_ctx) = app_and_ctx();
        iso_app.run("view.set", json!({ "preset": "top_front_right" })).unwrap();
        let _ = frame(&mut iso_app, &iso_ctx, Vec::new());
        let rect = canvas(&iso_app);
        let cam = iso_app.session.state().unwrap().camera;
        let x_arm = ucs_arm_segments(rect, rect, &cam)[0];
        let near_tip = x_arm.0 + (x_arm.1 - x_arm.0) * 0.85;
        click(&mut iso_app, &iso_ctx, near_tip, egui::PointerButton::Primary);
        assert!(menu_shown(&iso_ctx), "SE Isometric projected X arm opens UCS menu");
    }

    #[test]
    fn main_face_tooltips_are_suppressed_and_control_tips_exist() {
        assert_eq!(Part::Face.tip(), None);
        for face in ["top", "bottom", "front", "back", "left", "right"] {
            assert_eq!(Part::Cube(face).tip(), None, "{face} already renders a face label");
        }
        for edge_or_corner in ["front_top", "top_front_right", "bottom_back_left"] {
            assert_eq!(Part::Cube(edge_or_corner).tip(), Some(edge_or_corner));
        }
        for control in [Part::Home, Part::Ring, Part::RollCcw, Part::RollCw, Part::DirTop, Part::DirBottom, Part::DirLeft, Part::DirRight] {
            assert!(control.tip().is_some(), "{control:?} should keep a tooltip");
        }
    }

    fn frame_at(app: &mut CadApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    fn tooltip_shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Tooltip))
    }

    #[test]
    fn viewcube_tooltips_wait_for_doubled_delay_and_reset_between_parts() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let home = l.home.center();
        let ring_n = l.center + vec2(0.0, -l.ring);
        assert_eq!(l.part_at(home), Some(Part::Home));
        assert_eq!(l.part_at(ring_n), Some(Part::Ring));

        // Hover Home: stays hidden past egui's 0.5 s default, opens at >= 1.0 s.
        frame_at(&mut app, &ctx, 10.0, vec![egui::Event::PointerMoved(home)]);
        assert!(!tooltip_shown(&ctx));
        frame_at(&mut app, &ctx, 10.6, Vec::new());
        assert!(!tooltip_shown(&ctx), "tooltip must wait past egui's 0.5 s default");
        frame_at(&mut app, &ctx, 11.05, Vec::new());
        assert!(tooltip_shown(&ctx), "tooltip should appear after 2x global tooltip_delay (1.0 s)");

        // Moving to another ViewCube part hides the open tooltip and resets the 1.0 s delay.
        frame_at(&mut app, &ctx, 11.1, vec![egui::Event::PointerMoved(ring_n)]);
        assert!(!tooltip_shown(&ctx), "tooltip must reset immediately when moving to another part");
        frame_at(&mut app, &ctx, 11.7, Vec::new());
        assert!(!tooltip_shown(&ctx));
        frame_at(&mut app, &ctx, 12.15, Vec::new());
        assert!(tooltip_shown(&ctx), "new part tooltip should appear after its own 1.0 s delay");

        // Moving onto the labeled Top face suppresses the tooltip immediately.
        frame_at(&mut app, &ctx, 12.2, vec![egui::Event::PointerMoved(l.center)]);
        assert!(!tooltip_shown(&ctx));
        frame_at(&mut app, &ctx, 13.5, Vec::new());
        assert!(!tooltip_shown(&ctx), "main cube face must never show a tooltip");
    }

    #[test]
    fn roll_arrows_rotate_around_world_z_snapping_to_cardinals() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let ccw_hit = (l.ccw_outer[l.ccw_outer.len() / 2].to_vec2() + l.ccw_inner[l.ccw_inner.len() / 2].to_vec2()) * 0.5;
        let ccw_pos = ccw_hit.to_pos2();
        let cw_hit = (l.cw_outer[l.cw_outer.len() / 2].to_vec2() + l.cw_inner[l.cw_inner.len() / 2].to_vec2()) * 0.5;
        let cw_pos = cw_hit.to_pos2();

        // 1. Orthogonal Front view: 90° arrows orbit around world Z (Front -> Left -> Front) keeping +Z upright.
        let _ = app.run("view.set", json!({"preset": "front"}));
        let front = app.session.state().map(|st| st.camera).unwrap_or_default();
        assert_eq!(l.part_at_cam(ccw_pos, &front), Some(Part::RollCcw));
        assert_eq!(l.part_at_cam(cw_pos, &front), Some(Part::RollCw));

        click(&mut app, &ctx, ccw_pos, egui::PointerButton::Primary);
        let after_ccw = app.session.state().map(|st| st.camera).unwrap_or_default();
        assert_eq!(after_ccw.view_name(), "Left", "CCW roll from Front must orbit around world Z to Left: {after_ccw:?}");
        assert!((after_ccw.y - Vec3::Z).len() < 1e-12, "world-Z roll must keep vertical axis upright: {:?}", after_ccw.y);

        click(&mut app, &ctx, cw_pos, egui::PointerButton::Primary);
        let restored = app.session.state().map(|st| st.camera).unwrap_or_default();
        assert_eq!(restored.view_name(), "Front", "CW roll from Left must orbit back to Front: {restored:?}");
        assert!((restored.y - Vec3::Z).len() < 1e-12);

        // 2. Arbitrary tilted 3D view: preserves elevation (`z.z`) and snaps compass heading to 90° cardinal steps.
        let skewed = Camera3D::TOP.orbit(27.0, -35.0, false);
        app.session.state_mut().unwrap().camera = skewed;
        click(&mut app, &ctx, ccw_pos, egui::PointerButton::Primary);
        let snapped = app.session.state().map(|st| st.camera).unwrap_or_default();
        assert!((snapped.z.z - skewed.z.z).abs() < 1e-12, "world-Z roll must preserve elevation z.z");
        let dir = snapped.compass_dir_xy();
        let heading = dir.x.atan2(dir.y).to_degrees().rem_euclid(90.0);
        assert!(heading.min(90.0 - heading) < 1e-9, "heading must snap to 90° cardinal: dir={dir:?}");
    }

    #[test]
    fn ring_drag_uses_press_origin_and_consumes_stored_twist() {
        let (mut app, ctx) = app_and_ctx();
        let l = Layout::new(canvas(&app));
        let _ = app.run("view.set", json!({"center": [0.0, 0.0], "height": 20.0, "twist": 90.0, "preset": "top"}));
        let m = egui::Modifiers::default();
        let start_n = l.center + vec2(0.0, -l.ring);
        let past_threshold_inside = start_n + vec2(0.0, 20.0);
        let end_e = l.center + vec2(l.ring, 0.0);

        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(start_n)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: start_n, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
        // Move off the thin ring past egui's click threshold; press_origin must still classify as Ring.
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(past_threshold_inside)]);
        assert!(matches!(app.canvas.viewcube_drag, Some((_, true))), "expected ring drag from press_origin");

        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(end_e)]);
        frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: end_e, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
        frame(&mut app, &ctx, Vec::new());

        let st = app.session.state().expect("session state");
        let displayed = st.camera.with_twist(st.view().twist);
        assert!(st.view().twist.abs() < 1e-12, "twist should be consumed cleanly, got {}", st.view().twist);
        assert!(displayed.is_top_plan(), "90° CW ring drag from 90° twisted Top should land on canonical Top: {displayed:?}");
    }

    #[test]
    fn home_button_uses_configured_preset_and_fits_extents() {
        let (mut app, ctx) = app_and_ctx();
        assert_eq!(app.ui.viewcube_home_preset, "top");
        let l = Layout::new(canvas(&app));
        let top_fitted = view(&app);

        // 1. Default "top": switch to SE isometric + twist + zoomed view, click Home -> restores Top & fits extents.
        let _ = app.run("view.set", json!({"preset": "se_isometric"}));
        let _ = app.run("view.set", json!({"center": [0.0, 0.0], "height": 50.0, "twist": 30.0}));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        assert!(app.session.state().is_ok());
        if let Ok(st) = app.session.state() {
            assert_eq!(st.camera, Camera3D::TOP);
            assert!(st.view().twist.abs() < 1e-12);
        }
        assert_eq!(view(&app), top_fitted);

        // 2. Configured "sw_isometric": from Front view, click Home -> switches to SW Isometric & fits extents.
        app.ui.viewcube_home_preset = "sw_isometric".into();
        let _ = app.run("view.set", json!({"preset": "front"}));
        click(&mut app, &ctx, l.home.center(), egui::PointerButton::Primary);
        assert!(app.session.state().is_ok());
        if let Ok(st) = app.session.state() {
            assert_eq!(Some(st.camera), Camera3D::preset("sw_isometric"));
            assert!(st.view().twist.abs() < 1e-12);
        }
    }

    #[test]
    fn viewport_layout_scales_proportionally_and_clamps() {
        let large = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 820.0));
        let l_large = Layout::for_viewport(large, large);
        assert!((l_large.scale - 1.0).abs() < 1e-6);

        let medium = Rect::from_min_size(pos2(200.0, 100.0), vec2(300.0, 300.0));
        let l_med = Layout::for_viewport(medium, medium);
        let expected_med_scale = (300.0 * 0.17) / 120.0;
        assert!((l_med.scale - expected_med_scale).abs() < 1e-4);
        assert!(medium.contains_rect(l_med.bounds), "scaled ViewCube bounds {:?} should fit inside medium viewport {:?}", l_med.bounds, medium);
        assert!(medium.contains_rect(l_med.pill), "scaled WCS pill {:?} should fit inside medium viewport {:?}", l_med.pill, medium);
        assert_eq!(l_med.part_at(l_med.home.center()), Some(Part::Home));
        assert_eq!(l_med.part_at(l_med.center), Some(Part::Face));
        assert_eq!(l_med.part_at(l_med.center + vec2(0.0, -l_med.ring)), Some(Part::Ring));

        let tiny = Rect::from_min_size(pos2(40.0, 40.0), vec2(80.0, 80.0));
        let l_tiny = Layout::for_viewport(tiny, tiny);
        assert!((l_tiny.scale - (35.0 / 120.0)).abs() < 1e-4, "tiny viewport must clamp to 35px minimum scale");
        assert!(tiny.contains(l_tiny.center));
        assert_eq!(l_tiny.part_at(pos2(tiny.left() - 5.0, tiny.top() - 5.0)), None, "points outside viewport clip must not hit ViewCube");
    }

    #[test]
    fn viewcube_palette_adapts_to_light_and_dark_backgrounds() {
        let dark = ViewCubePalette::for_background(Color32::from_rgb(0x1e, 0x23, 0x2a));
        assert!(!dark.is_light);
        assert_eq!(dark.ring_fill, Color32::from_rgb(0x48, 0x50, 0x5c));
        assert_eq!(dark.compass_text, Color32::from_rgb(0xd2, 0xd6, 0xdc));
        assert_eq!(dark.wcs_border, None);

        let light = ViewCubePalette::for_background(Color32::WHITE);
        assert!(light.is_light);
        assert!(crate::theme::luminance(light.compass_text) < 0.35, "compass text must stay dark on white paper");
        assert!(crate::theme::luminance(light.ring_fill) > 0.65, "compass ring must be subtle light grey on white paper");
        assert!(crate::theme::luminance(light.home_stroke) < 0.45, "home icon must stay legible on white paper");
        assert!(light.wcs_border.is_some());
        assert!(crate::theme::luminance(light.wcs_ink) < 0.35);
    }

    #[test]
    fn viewcube_bounds_contain_all_interactive_parts_near_corner() {
        let vp = Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0));
        let l = Layout::for_viewport(vp, vp);
        assert!(vp.contains_rect(l.bounds));
        assert!(l.bounds.contains_rect(l.home));
        assert!(l.bounds.contains_rect(l.face));
        assert!(l.bounds.contains_rect(l.roll_cw));
        assert!(l.bounds.contains_rect(l.roll_ccw));
        assert!(vp.right() - l.center.x < 100.0, "ViewCube should sit closer to the right border");
    }
}
