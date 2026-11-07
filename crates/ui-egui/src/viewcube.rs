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

/// Where the ViewCube's parts are on the canvas.
#[derive(Clone, Debug)]
pub struct Layout {
    pub center: Pos2,
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
    pub fn new(canvas: Rect) -> Self {
        let center = pos2(canvas.right() - 116.0, canvas.top() + 106.0);
        let ring = RING_RADIUS;
        let ring_outer = ring + RING_THICKNESS * 0.5;
        let r_inner = ring_outer + ROLL_ARROW_GAP;
        let r_outer = r_inner + ROLL_ARROW_WIDTH;
        let (dir_top, dir_bottom, dir_left, dir_right) = directional_triangles(center);
        let (cw_poly, cw_outer, cw_inner, cw_head) = build_curved_arrow_data(center, r_inner, r_outer, ROLL_ARROW_START_DEG, ROLL_ARROW_END_DEG, true);
        let (ccw_poly, ccw_outer, ccw_inner, ccw_head) = mirror_arrow_data(center, &cw_poly, &cw_outer, &cw_inner, &cw_head);
        let roll_cw = poly_bounding_rect(&cw_poly);
        let roll_ccw = poly_bounding_rect(&ccw_poly);
        let bounds_radius = 108.0;
        Layout {
            center,
            ring,
            face: Rect::from_center_size(center, vec2(44.0, 44.0)),
            home: Rect::from_center_size(center + vec2(-86.0, -82.0), vec2(20.0, 20.0)),
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
            pill: Rect::from_center_size(center + vec2(0.0, ring + 26.0), vec2(56.0, 16.0)),
            bounds: Rect::from_center_size(center, vec2(2.0 * bounds_radius, 2.0 * bounds_radius)),
        }
    }

    /// Hit-test using default Top camera.
    pub fn part_at(&self, p: Pos2) -> Option<Part> {
        self.part_at_cam(p, &Camera3D::TOP)
    }

    /// 4 orthogonal directional navigation triangles rotated synchronously with the active cube face
    /// (ported from MATCAD `createViewCubeGraphics.m` & `synchronize_viewcube.m`).
    pub fn dir_triangles(&self, cam: &Camera3D) -> ([Pos2; 3], [Pos2; 3], [Pos2; 3], [Pos2; 3]) {
        directional_triangles_for_cam(self.center, cam)
    }

    /// Hit-test `p` against the 3D projected beveled cube, compass ring, roll buttons,
    /// directional triangles and home icon.
    pub fn part_at_cam(&self, p: Pos2, cam: &Camera3D) -> Option<Part> {
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
        if let Some(part_name) = hit_beveled_cube(self.center, cam, p) {
            if part_name == "top" && cam.is_top_plan() {
                return Some(Part::Face);
            }
            return Some(Part::Cube(part_name));
        }
        let d = p.distance(self.center);
        ((self.ring - 10.0..=self.ring + 10.0).contains(&d)).then_some(Part::Ring)
    }
}

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
    fn tip(self) -> &'static str {
        match self {
            Part::Face => "Top: plan view of the drawing (zoom extents)",
            Part::Cube(name) => name,
            Part::Home => "Home: isometric / extents view",
            Part::Ring => "Compass: drag to orbit around World Z, or click N/E/S/W",
            Part::RollCcw => "Roll view 90° counter-clockwise",
            Part::RollCw => "Roll view 90° clockwise",
            Part::DirTop => "Tilt view 90° up",
            Part::DirBottom => "Tilt view 90° down",
            Part::DirLeft => "Tilt view 90° left",
            Part::DirRight => "Tilt view 90° right",
        }
    }
}

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

fn project_cube_quads(center: Pos2, cam: &Camera3D) -> Vec<ProjectedQuad> {
    let quads = build_beveled_cube(CUBE_HALF, CUBE_BEVEL);
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

fn hit_beveled_cube(center: Pos2, cam: &Camera3D, p: Pos2) -> Option<&'static str> {
    let local = Vec2::new(f64::from(p.x - center.x), f64::from(p.y - center.y));
    let quads = project_cube_quads(center, cam);
    // Check front-to-back (reverse of draw order).
    for q in quads.iter().rev() {
        if point_in_polygon(&q.poly_world, local) {
            return Some(q.part);
        }
    }
    None
}

/// Paint the 3D Beveled ViewCube, rotating compass ring, roll buttons, directional triangles
/// and Home button, and handle clicks and orbit drags.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, l: &Layout) {
    let cam = app.session.state().map(|st| st.camera).unwrap_or_default();
    let resp = ui.interact(l.bounds, ui.scope_id().with("viewcube"), Sense::click_and_drag());
    let part = resp.hover_pos().and_then(|p| l.part_at_cam(p, &cam));
    let p = ui.painter_at(l.bounds.union(l.pill));

    // 1. Rotating Compass Ring (always robust at all camera angles, ported from MATCAD synchronize_viewcube.m).
    let ring_col = if part == Some(Part::Ring) { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x48, 0x50, 0x5c) };
    p.circle_stroke(l.center, l.ring, Stroke::new(9.0, ring_col));
    p.circle_stroke(l.center, l.ring + 4.5, Stroke::new(1.0, Color32::from_rgb(0x5c, 0x65, 0x72)));
    p.circle_stroke(l.center, l.ring - 4.5, Stroke::new(1.0, Color32::from_rgb(0x38, 0x3d, 0x46)));

    let dir_xy = cam.compass_dir_xy();
    let f = egui::FontId::proportional(13.5);
    let lc = Color32::from_rgb(0xd2, 0xd6, 0xdc);
    for (label, wx, wy) in [("N", 0.0_f64, 1.0_f64), ("E", 1.0, 0.0), ("S", 0.0, -1.0), ("W", -1.0, 0.0)] {
        let u = wx * dir_xy.y - wy * dir_xy.x;
        let v = wx * dir_xy.x + wy * dir_xy.y;
        let pos = l.center + vec2(u as f32, -v as f32) * l.ring;
        p.text(pos, egui::Align2::CENTER_CENTER, crate::i18n::t(label), f.clone(), lc);
    }

    // 2. 4 Directional Navigation Triangles (visible only in orthogonal main views, rotating with the cube, ported from MATCAD).
    if cam.is_orthogonal() {
        let (dir_top, dir_bottom, dir_left, dir_right) = l.dir_triangles(&cam);
        let arr_col = Color32::from_rgb(0x4a, 0x52, 0x60);
        let arr_hover = Color32::from_rgb(0x6e, 0xa8, 0xe5);
        let arr_edge = Color32::from_rgb(0x6e, 0x76, 0x86);
        for (tri, is_hot) in [
            (&dir_top, part == Some(Part::DirTop)),
            (&dir_bottom, part == Some(Part::DirBottom)),
            (&dir_left, part == Some(Part::DirLeft)),
            (&dir_right, part == Some(Part::DirRight)),
        ] {
            let fill = if is_hot { arr_hover } else { arr_col };
            p.add(Shape::convex_polygon(tri.to_vec(), fill, Stroke::new(1.0, arr_edge)));
        }
    }

    // 3. Curved 90° Roll Arrows (CCW top-left and CW top-right outside the ring, ported from MATCAD).
    let arr_col = Color32::from_rgb(0x4a, 0x52, 0x60);
    let arr_hover = Color32::from_rgb(0x6e, 0xa8, 0xe5);
    let arr_edge = Color32::from_rgb(0x6e, 0x76, 0x86);

    let ccw_fill = if part == Some(Part::RollCcw) { arr_hover } else { arr_col };
    paint_curved_arrow(&p, &l.ccw_poly, &l.ccw_outer, &l.ccw_inner, &l.ccw_head, ccw_fill, Stroke::new(1.0, arr_edge));

    let cw_fill = if part == Some(Part::RollCw) { arr_hover } else { arr_col };
    paint_curved_arrow(&p, &l.cw_poly, &l.cw_outer, &l.cw_inner, &l.cw_head, cw_fill, Stroke::new(1.0, arr_edge));

    // 4. 3D Beveled Cube Quads (26 interactive zones).
    let hot_part = match part {
        Some(Part::Face) => Some("top"),
        Some(Part::Cube(name)) => Some(name),
        _ => None,
    };
    let quads = project_cube_quads(l.center, &cam);
    let edge_stroke = Stroke::new(0.9, Color32::from_rgb(0x52, 0x56, 0x5e));
    for q in &quads {
        let is_hot = hot_part == Some(q.part);
        let base: u8 = if q.is_main_face {
            (130.0 + 45.0 * q.shade) as u8
        } else {
            (105.0 + 35.0 * q.shade) as u8
        };
        let fill = if is_hot {
            Color32::from_rgb(0x6e, 0xa8, 0xe5)
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
            paint_face_text(&p, l.center, &cam, q);
        }
    }

    // 5. Home button at the top-left.
    paint_home(&p, l.home, part == Some(Part::Home));

    let resp = match part {
        Some(part) => resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(part.tip()),
        None => resp,
    };

    // 6. ViewCube and Compass Ring Dragging:
    // Storing (start_pos, cam0) at drag start and calculating orbit from cam0 prevents horizon drift
    // and eliminates the gimbal / gumball effect entirely.
    if resp.drag_started_by(egui::PointerButton::Primary) {
        if let Some(press_pos) = resp.interact_pointer_pos() {
            let is_ring = l.part_at_cam(press_pos, &cam) == Some(Part::Ring);
            app.canvas.viewcube_drag = Some((press_pos, cam, is_ring));
        }
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        if let Some((start_pos, cam0, is_ring)) = app.canvas.viewcube_drag {
            if let Some(curr_pos) = resp.interact_pointer_pos() {
                if is_ring {
                    // Angular delta around ring center (MATCAD move_viewcube.m lines 191-198)
                    let a0 = -(start_pos.y - l.center.y).atan2(start_pos.x - l.center.x);
                    let a1 = -(curr_pos.y - l.center.y).atan2(curr_pos.x - l.center.x);
                    let daz_deg = f64::from((a1 - a0).to_degrees());
                    if let Ok(st) = app.session.state_mut() {
                        st.camera = cam0.orbit(daz_deg, 0.0, true);
                    }
                } else {
                    // Cube drag: total pixel offset from drag start
                    let delta = curr_pos - start_pos;
                    let daz_deg = f64::from(delta.x) * 0.5;
                    let del_deg = -f64::from(delta.y) * 0.5; // Negative because egui Y increases downwards
                    if let Ok(st) = app.session.state_mut() {
                        st.camera = cam0.orbit(daz_deg, del_deg, true);
                    }
                }
            }
        }
    }
    if resp.drag_stopped() {
        app.canvas.viewcube_drag = None;
    }

    if resp.clicked() {
        let pressed_on = resp.interact_pointer_pos().and_then(|o| l.part_at_cam(o, &cam));
        match pressed_on {
            Some(Part::Face) => {
                let _ = app.run("view.set", json!({"preset": "top"}));
                let _ = app.run("zoom.extents", json!({}));
            }
            Some(Part::Cube(preset)) => {
                let _ = app.run("view.set", json!({"preset": preset}));
            }
            Some(Part::Home) => {
                let _ = app.run("zoom.extents", json!({}));
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
    let pill = ui.interact(l.pill, ui.scope_id().with("viewcube_wcs"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
    let pill = pill.on_hover_text("Coordinate system");
    ucs_menu(app, egui::Popup::menu(&pill));
}

/// A small house, drawn in code.
fn paint_home(p: &egui::Painter, r: Rect, hot: bool) {
    let t = Tokens::get();
    if hot {
        p.rect_filled(r, 3.0, t.control_hover);
    }
    let st = Stroke::new(1.3, if hot { t.text } else { t.text_dim });
    let c = r.center();
    let roof = [c + vec2(-6.5, 0.5), c + vec2(0.0, -6.0), c + vec2(6.5, 0.5)];
    p.line(roof.to_vec(), st);
    p.line(vec![c + vec2(-4.5, -1.0), c + vec2(-4.5, 6.0), c + vec2(4.5, 6.0), c + vec2(4.5, -1.0)], st);
    p.line_segment([c + vec2(-1.2, 6.0), c + vec2(-1.2, 2.5)], st);
    p.line_segment([c + vec2(1.2, 6.0), c + vec2(1.2, 2.5)], st);
    p.line_segment([c + vec2(-1.2, 2.5), c + vec2(1.2, 2.5)], st);
}

/// Project the localized main-face label directly onto the 3D cube face plane (`q.centroid`, `u_3d`, `v_3d`)
/// with a view-invariant 3D scale using the clean, thin proportional UI font.
fn paint_face_text(
    p: &egui::Painter,
    center: Pos2,
    cam: &Camera3D,
    q: &ProjectedQuad,
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
    // Base font size of 13.0 pt matches the on-screen size closely, avoiding heavy downsampling blur.
    let base_font_size = 13.0_f32;
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
    // 9.8 3D units nominal font size, clamped so long labels (e.g. BOTTOM, HINTEN, RECHTS)
    // never exceed 86% of the 3D inner face width (`inner_span`).
    let inner_span = (2.0 * (1.0 - CUBE_BEVEL) * CUBE_HALF) as f32;
    let base_scale = 9.8 / base_font_size;
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

/// The 4 directional navigation triangles pointing towards the 4 cube face edges (default Top orientation).
fn directional_triangles(center: Pos2) -> ([Pos2; 3], [Pos2; 3], [Pos2; 3], [Pos2; 3]) {
    directional_triangles_for_cam(center, &Camera3D::TOP)
}

/// The 4 directional navigation triangles rotated with the active orthogonal cube face
/// (ported from MATCAD `createViewCubeGraphics.m` & `synchronize_viewcube.m`).
fn directional_triangles_for_cam(center: Pos2, cam: &Camera3D) -> ([Pos2; 3], [Pos2; 3], [Pos2; 3], [Pos2; 3]) {
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
    let tip = CUBE_HALF as f32 + 3.5;
    let base = tip + 11.0;
    let hw = 7.5;
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

fn poly_bounding_rect(pts: &[Pos2]) -> Rect {
    let mut r = Rect::NOTHING;
    for pt in pts {
        r.extend_with(*pt);
    }
    r
}

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

/// The UCS icon's arms (as drawn by the canvas at the lower-left corner): the X arm and the Y arm.
pub fn ucs_arms(canvas: Rect) -> [Rect; 2] {
    let o = pos2(canvas.left() + 36.0, canvas.bottom() - 34.0);
    [Rect::from_min_max(o + vec2(-8.0, -8.0), o + vec2(80.0, 8.0)), Rect::from_min_max(o + vec2(-8.0, -82.0), o + vec2(8.0, 8.0))]
}

/// Make the UCS icon clickable: hovering highlights it, a click (either button) opens the
/// coordinate-system menu.
pub fn ucs_icon(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    let [x, y] = ucs_arms(canvas);
    let rx = ui.interact(x, ui.scope_id().with("ucs_x"), Sense::click_and_drag());
    let ry = ui.interact(y, ui.scope_id().with("ucs_y"), Sense::click_and_drag());
    let resp = (rx | ry).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("CADCross / UCS icon: World Coordinate System");
    if resp.hovered() {
        let o = pos2(x.left() + 8.0, x.center().y);
        let st = Stroke::new(2.0, t.hover);
        let p = ui.painter_at(canvas);
        p.line_segment([o, o + vec2(60.0, 0.0)], st);
        p.line_segment([o, o + vec2(0.0, -60.0)], st);
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
                        for v in &mesh.vertices {
                            assert!(
                                v.uv.x > 0.0 && v.uv.x <= 1.0 && v.uv.y >= 0.0 && v.uv.y <= 1.0,
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
    }
}
