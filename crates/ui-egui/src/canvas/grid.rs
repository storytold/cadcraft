use cadcraft_geom::{Camera3D, Vec3, intersect_ray_plane};
use cadcraft_render::clip::PolyClip;

use super::{Bounds2, CadApp, Color32, Rect, Shape, Stroke, Tokens, Vec2, Xf};

/// Edge-on threshold matching AutoCAD `AcGsiGrid::subViewportDraw` (`abs(viewDir_local.z) < 1e-8`):
/// below this, neither grid lines nor colored origin axes are rendered.
const EDGE_ON_ABORT_TILT: f64 = 1e-8;
/// Shallow-angle threshold matching AutoCAD `AcGsiGrid` (`sin(3°) ≈ 0.052336`):
/// below this, minor and major grid lines are hidden while origin axes remain visible.
const SHALLOW_GRID_HIDE_TILT: f64 = 0.052336;
/// Maximum number of grid lines allowed per axis (`0x1f4` in AutoCAD `AcGsiGrid`).
const MAX_GRID_LINES_PER_AXIS: i64 = 500;
/// Alignment threshold matching [`Camera3D::is_orthogonal`] and MATCAD `fillCameraVolume.m` (~2°).
const ORTHO_AXIS_DOT: f64 = 0.999;

/// World principal plane on which the coordinate grid is rendered for the current camera.
///
/// Consistent with MATCAD (`fillCameraVolume.m` / `updateAutoCPlane.m`):
/// - `Top` / `Bottom` / oblique 3D views use world `XY` (`Z = 0`, X red / Y green).
/// - `Front` / `Back` (`|cam.z.y| > 0.999`) use world `XZ` (`Y = 0`, X red / Z blue).
/// - `Left` / `Right` (`|cam.z.x| > 0.999`) use world `YZ` (`X = 0`, Y green / Z blue).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GridPlane {
    Xy,
    Xz,
    Yz,
}

impl GridPlane {
    pub(super) fn for_camera(cam: &Camera3D, ortho_planes: bool, orbiting: bool) -> Self {
        if !ortho_planes || orbiting {
            return Self::Xy;
        }
        if cam.z.y.abs() > ORTHO_AXIS_DOT {
            Self::Xz
        } else if cam.z.x.abs() > ORTHO_AXIS_DOT {
            Self::Yz
        } else {
            Self::Xy
        }
    }

    pub(super) fn basis(self) -> (Vec3, Vec3, Vec3) {
        let ux = Vec3::new(1.0, 0.0, 0.0);
        let uy = Vec3::new(0.0, 1.0, 0.0);
        match self {
            Self::Xy => (ux, uy, Vec3::Z),
            Self::Xz => (ux, Vec3::Z, uy),
            Self::Yz => (uy, Vec3::Z, ux),
        }
    }

    fn axis_colors(self, t: &Tokens) -> (Color32, Color32) {
        match self {
            Self::Xy => (t.axis_x, t.axis_y),
            Self::Xz => (t.axis_x, t.axis_z),
            Self::Yz => (t.axis_y, t.axis_z),
        }
    }
}

/// Target surface descriptor for [`draw_grid_surface`], shared between Model Space, Paper Space,
/// and active/inactive layout viewports.
pub(super) struct GridSurface<'a> {
    pub painter: &'a egui::Painter,
    pub xf: &'a Xf,
    pub clip_rect: Rect,
    pub tokens: &'a Tokens,
    pub grid_on: bool,
    pub show_axes_always: bool,
    pub poly_clip: Option<(&'a Xf, &'a PolyClip)>,
}

/// Grid spacing adapted to zoom and surface tilt (`eff_scale = scale * |cam.z · normal|`): multiply
/// the unit until lines are at least `min_px` apart and neither axis exceeds `MAX_GRID_LINES_PER_AXIS`.
fn adaptive(wb: &Bounds2, unit: f64, eff_scale: f64, min_px: f64) -> f64 {
    let mut u = unit.max(1e-12);
    let mut guard = 0;
    while (u * eff_scale < min_px || grid_index_ranges(wb, u, MAX_GRID_LINES_PER_AXIS).is_none()) && guard < 40 {
        u *= 5.0;
        guard += 1;
    }
    u
}

/// Visible `(u, v)` bounds of `clip_rect` intersected with the active `plane` through the origin.
fn plane_bounds(xf: &Xf, clip_rect: Rect, plane: GridPlane) -> Bounds2 {
    let (u_axis, v_axis, normal) = plane.basis();
    let corners = [clip_rect.left_bottom(), clip_rect.right_top(), clip_rect.left_top(), clip_rect.right_bottom()];
    if plane == GridPlane::Xy && xf.cam.is_top_plan() {
        return Bounds2::from_points(corners.map(|p| xf.to_world(p)));
    }
    let target = xf.target();
    let center_ray = xf.cam.ray_at_target(xf.screen_uv_at(clip_rect.center()), target);
    let center_uv = intersect_ray_plane(center_ray.0, center_ray.1, Vec3::ZERO, normal)
        .map_or_else(|| Vec2::new(target.dot(u_axis), target.dot(v_axis)), |p| Vec2::new(p.dot(u_axis), p.dot(v_axis)));
    let diag = f64::from(clip_rect.width().hypot(clip_rect.height())) / xf.scale.max(1e-12);
    let cap = diag * 3.0;
    let capped = Bounds2::new(center_uv - Vec2::new(cap, cap), center_uv + Vec2::new(cap, cap));
    let tilt = xf.cam.z.dot(normal).abs();
    if tilt <= 0.15 {
        return capped;
    }
    let mut pts = [Vec2::ZERO; 4];
    for (i, p) in corners.into_iter().enumerate() {
        let uv = xf.screen_uv_at(p);
        let (ray_o, ray_d) = xf.cam.ray_at_target(uv, target);
        let Some(hit) = intersect_ray_plane(ray_o, ray_d, Vec3::ZERO, normal) else {
            return capped;
        };
        pts[i] = Vec2::new(hit.dot(u_axis), hit.dot(v_axis));
    }
    let b = Bounds2::from_points(pts);
    let lo = Vec2::new(b.min.x.clamp(capped.min.x, capped.max.x), b.min.y.clamp(capped.min.y, capped.max.y));
    let hi = Vec2::new(b.max.x.clamp(capped.min.x, capped.max.x), b.max.y.clamp(capped.min.y, capped.max.y));
    Bounds2::new(lo.min(center_uv), hi.max(center_uv))
}

/// Index ranges of the grid lines inside `wb` for line spacing `minor`, or `None` when either axis
/// would reach `max_lines` or overflow `i64`.
fn grid_index_ranges(wb: &Bounds2, minor: f64, max_lines: i64) -> Option<((i64, i64), (i64, i64))> {
    let range = |lo: f64, hi: f64| {
        let a = (lo / minor).floor() as i64;
        let b = (hi / minor).ceil() as i64;
        b.checked_sub(a).filter(|n| *n < max_lines).map(|_| (a, b))
    };
    Some((range(wb.min.x, wb.max.x)?, range(wb.min.y, wb.max.y)?))
}

#[derive(Debug, PartialEq)]
struct GridPlan {
    show_axes: bool,
    minor: Option<(f64, ((i64, i64), (i64, i64)))>,
}

/// Plan grid visibility and spacing from plane bounds and camera tilt (`|cam.z · plane_normal|`).
fn plan_grid(wb: &Bounds2, tilt: f64, unit: f64, scale: f64) -> GridPlan {
    let tilt = tilt.abs();
    if tilt < EDGE_ON_ABORT_TILT {
        return GridPlan { show_axes: false, minor: None };
    }
    if tilt < SHALLOW_GRID_HIDE_TILT {
        return GridPlan { show_axes: true, minor: None };
    }
    let minor = adaptive(wb, unit, scale * tilt, 8.0);
    let ranges = grid_index_ranges(wb, minor, MAX_GRID_LINES_PER_AXIS).map(|r| (minor, r));
    GridPlan { show_axes: true, minor: ranges }
}

fn emit_segment(
    out: &mut Vec<Shape>,
    xf: &Xf,
    a: Vec3,
    b: Vec3,
    stroke: Stroke,
    poly_clip: Option<(&Xf, &PolyClip)>,
) {
    let sa = xf.to_screen_3d(a);
    let sb = xf.to_screen_3d(b);
    if let Some((xf_paper, clip)) = poly_clip {
        for run in clip.clip_polyline(&[xf_paper.to_world(sa), xf_paper.to_world(sb)]) {
            for w in run.windows(2) {
                out.push(Shape::line_segment([xf_paper.to_screen(w[0]), xf_paper.to_screen(w[1])], stroke));
            }
        }
    } else {
        out.push(Shape::line_segment([sa, sb], stroke));
    }
}

pub(super) fn draw_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    draw_grid_surface(
        app,
        &GridSurface {
            painter: p,
            xf,
            clip_rect: xf.rect,
            tokens: &t,
            grid_on: s.gridmode,
            show_axes_always: true,
            poly_clip: None,
        },
    );
}

pub(super) fn draw_grid_surface(app: &CadApp, surf: &GridSurface<'_>) {
    let s = app.session.drafting_settings();
    let xf = surf.xf;
    let t = surf.tokens;
    let orbiting = app.canvas.active_orbit_pivot().is_some();
    let plane = GridPlane::for_camera(&xf.cam, s.ortho_plane_grid, orbiting);
    let (u_axis, v_axis, normal) = plane.basis();
    let (u_col, v_col) = plane.axis_colors(t);
    let tilt = xf.cam.z.dot(normal).abs();
    if tilt < EDGE_ON_ABORT_TILT {
        return;
    }
    let wb = plane_bounds(xf, surf.clip_rect, plane);
    let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
    let plan = plan_grid(&wb, tilt, unit, xf.scale);
    let mut lines = Vec::new();
    if surf.grid_on && s.isodraft && plane == GridPlane::Xy && tilt >= SHALLOW_GRID_HIDE_TILT {
        draw_iso_grid(app, surf, &wb);
    } else if surf.grid_on
        && let Some((minor, ((u0, u1), (v0, v1)))) = plan.minor
    {
        let major_n = f64::from(s.gridmajor.max(1));
        let major = minor * major_n;
        for i in u0..=u1 {
            let u = i as f64 * minor;
            let is_major = (u / major).round() * major - u == 0.0 || ((u / major) - (u / major).round()).abs() < 1e-6;
            let stroke = Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor });
            emit_segment(&mut lines, xf, u_axis * u + v_axis * wb.min.y, u_axis * u + v_axis * wb.max.y, stroke, surf.poly_clip);
        }
        for j in v0..=v1 {
            let v = j as f64 * minor;
            let is_major = ((v / major) - (v / major).round()).abs() < 1e-6;
            let stroke = Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor });
            emit_segment(&mut lines, xf, u_axis * wb.min.x + v_axis * v, u_axis * wb.max.x + v_axis * v, stroke, surf.poly_clip);
        }
    }
    if plan.show_axes && (surf.show_axes_always || surf.grid_on) {
        if (wb.min.y..=wb.max.y).contains(&0.0) {
            emit_segment(&mut lines, xf, u_axis * wb.min.x, u_axis * wb.max.x, Stroke::new(1.0, u_col), surf.poly_clip);
        }
        if (wb.min.x..=wb.max.x).contains(&0.0) {
            emit_segment(&mut lines, xf, v_axis * wb.min.y, v_axis * wb.max.y, Stroke::new(1.0, v_col), surf.poly_clip);
        }
    }
    surf.painter.extend(lines);
}

/// The isometric grid (SNAPSTYL 1): lines at 30°, 90° and 150° through the snap lattice, GRIDUNIT
/// Y apart along the axes, coarsened while they would crowd closer than 8 pixels.
fn draw_iso_grid(app: &CadApp, surf: &GridSurface<'_>, wb: &Bounds2) {
    use cadcraft_engine::snap::iso;
    let t = surf.tokens;
    let xf = surf.xf;
    let s = app.session.drafting_settings();
    let mut d = s.gridunit.y.max(1e-12);
    for _ in 0..40 {
        if d * 0.866 * xf.scale >= 8.0 {
            break;
        }
        d *= 5.0;
    }
    let (origin, angle) = app.session.doc().map(cadcraft_engine::snap::grid_frame).unwrap_or((Vec2::ZERO, 0.0));
    let iso_lines = iso::grid_lines(*wb, d, origin, angle, s.gridmajor);
    let mut shapes = Vec::with_capacity(iso_lines.len());
    for l in iso_lines {
        let stroke = Stroke::new(1.0, if l.major { t.grid_major } else { t.grid_minor });
        emit_segment(&mut shapes, xf, l.a.to3(0.0), l.b.to3(0.0), stroke, surf.poly_clip);
    }
    surf.painter.extend(shapes);
}

#[cfg(test)]
mod grid_range_tests {
    use cadcraft_geom::{Bounds2, Camera3D, Vec2};
    use egui::{Pos2, Rect, vec2};

    use crate::canvas::Xf;

    #[test]
    fn grid_index_ranges_survive_extreme_views() {
        let huge = Bounds2::new(Vec2::new(-1e300, -1e300), Vec2::new(1e300, 1e300));
        assert_eq!(super::grid_index_ranges(&huge, 1.0, super::MAX_GRID_LINES_PER_AXIS), None);
        let normal = Bounds2::new(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 10.0));
        assert_eq!(super::grid_index_ranges(&normal, 1.0, super::MAX_GRID_LINES_PER_AXIS), Some(((-5, 5), (0, 10))));
    }

    #[test]
    fn grid_edge_on_and_tilt_thresholds_match_autocad() {
        let wb = Bounds2::new(Vec2::new(-50.0, -50.0), Vec2::new(50.0, 50.0));
        // 1) Oblique XY edge-on (|z · normal| == 0): neither grid lines nor axes.
        let flat = super::plan_grid(&wb, 0.0, 1.0, 10.0);
        assert!(!flat.show_axes);
        assert!(flat.minor.is_none());

        // 2) Shallow angle (~2°, below sin 3° ≈ 0.052336): axes remain, grid lines hidden.
        let shallow = super::plan_grid(&wb, 0.035, 1.0, 10.0);
        assert!(shallow.show_axes);
        assert!(shallow.minor.is_none());

        // 3) Isometric/steep angle: coarsens when tilt shrinks pixel spacing and keeps <= 500 lines.
        let tilted = super::plan_grid(&wb, 0.5, 1.0, 10.0);
        assert!(tilted.show_axes);
        let (spacing, ((x0, x1), (y0, y1))) = tilted.minor.expect("steep view must produce grid lines");
        assert!(spacing > 1.0, "1.0 * 10.0 * 0.5 = 5px < 8px should coarsen to 5.0: got {spacing}");
        assert!(x1 - x0 <= super::MAX_GRID_LINES_PER_AXIS && y1 - y0 <= super::MAX_GRID_LINES_PER_AXIS);
    }

    #[test]
    fn principal_plane_selection_and_bounds_cover_front_and_left() {
        // AutoCAD default (GRIDORTHO off) or active orbit stays on XY ground plane.
        for view_name in ["top", "bottom", "front", "back", "left", "right", "sw_isometric"] {
            let cam = Camera3D::preset(view_name).unwrap();
            assert_eq!(super::GridPlane::for_camera(&cam, false, false), super::GridPlane::Xy);
            assert_eq!(super::GridPlane::for_camera(&cam, true, true), super::GridPlane::Xy);
        }

        // With GRIDORTHO enabled and no active drag, Front/Back select XZ and Left/Right select YZ.
        assert_eq!(super::GridPlane::for_camera(&Camera3D::TOP, true, false), super::GridPlane::Xy);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("bottom").unwrap(), true, false), super::GridPlane::Xy);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("front").unwrap(), true, false), super::GridPlane::Xz);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("back").unwrap(), true, false), super::GridPlane::Xz);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("left").unwrap(), true, false), super::GridPlane::Yz);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("right").unwrap(), true, false), super::GridPlane::Yz);
        assert_eq!(super::GridPlane::for_camera(&Camera3D::preset("sw_isometric").unwrap(), true, false), super::GridPlane::Xy);

        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        for view_name in ["front", "back", "left", "right"] {
            let cam = Camera3D::preset(view_name).unwrap();
            let plane = super::GridPlane::for_camera(&cam, true, false);
            let (_, _, normal) = plane.basis();
            let xf = Xf { rect, center: Vec2::new(10.0, 5.0), scale: 10.0, twist: 0.0, target_z: 12.0, cam };
            let wb = super::plane_bounds(&xf, rect, plane);
            assert!(wb.width() > 70.0 && wb.height() > 50.0, "{view_name}: unexpected plane bounds {wb:?}");
            assert!(wb.min.y < 12.0 && wb.max.y > 12.0, "{view_name}: vertical Z range should span target_z=12: {wb:?}");
            let plan = super::plan_grid(&wb, xf.cam.z.dot(normal).abs(), 1.0, xf.scale);
            assert!(plan.show_axes, "{view_name}: axes must remain visible");
            assert!(plan.minor.is_some(), "{view_name}: orthogonal grid lines must be planned");
        }
    }

    #[test]
    fn off_center_viewport_retains_visible_grid_bounds() {
        let full = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0));
        // Small Front viewport sitting 450px right of full canvas center.
        let off_center = Rect::from_min_size(Pos2::new(900.0, 350.0), vec2(100.0, 100.0));
        let cam = Camera3D::preset("front").unwrap();
        let xf = Xf { rect: full, center: Vec2::ZERO, scale: 1.0, twist: 0.0, target_z: 0.0, cam };
        let wb = super::plane_bounds(&xf, off_center, super::GridPlane::Xz);
        let center_x = f64::from(off_center.center().x - full.center().x);
        assert!(wb.min.x <= center_x && wb.max.x >= center_x, "off-center X bounds {wb:?} must cover {center_x}");
    }
}
