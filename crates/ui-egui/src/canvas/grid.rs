use super::{Bounds2, CadApp, Shape, Stroke, Tokens, Vec2, Xf};

/// Edge-on threshold matching AutoCAD `AcGsiGrid::subViewportDraw` (`abs(viewDir_local.z) < 1e-8`):
/// below this, neither grid lines nor colored origin axes are rendered.
const EDGE_ON_ABORT_TILT: f64 = 1e-8;
/// Shallow-angle threshold matching AutoCAD `AcGsiGrid` (`sin(3°) ≈ 0.052336`):
/// below this, minor and major grid lines are hidden while origin axes remain visible.
const SHALLOW_GRID_HIDE_TILT: f64 = 0.052336;
/// Maximum number of grid lines allowed per axis (`0x1f4` in AutoCAD `AcGsiGrid`).
const MAX_GRID_LINES_PER_AXIS: i64 = 500;

/// Grid spacing adapted to zoom and surface tilt (`eff_scale = scale * |cam.z.z|`): multiply the
/// unit until lines are at least `min_px` apart and neither axis exceeds `MAX_GRID_LINES_PER_AXIS`.
fn adaptive(wb: &Bounds2, unit: f64, eff_scale: f64, min_px: f64) -> f64 {
    let mut u = unit.max(1e-12);
    let mut guard = 0;
    while (u * eff_scale < min_px || grid_index_ranges(wb, u, MAX_GRID_LINES_PER_AXIS).is_none()) && guard < 40 {
        u *= 5.0;
        guard += 1;
    }
    u
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

/// Plan grid visibility and spacing from viewport world bounds and camera Z tilt (`|cam.z.z|`).
fn plan_grid(wb: &Bounds2, cam_z_z: f64, unit: f64, scale: f64) -> GridPlan {
    let tilt = cam_z_z.abs();
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

pub(super) fn draw_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let wb = xf.world_bounds();
    let tilt = xf.cam.z.z.abs();
    if tilt < EDGE_ON_ABORT_TILT {
        return;
    }
    let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
    let plan = plan_grid(&wb, xf.cam.z.z, unit, xf.scale);
    if s.gridmode && s.isodraft && tilt >= SHALLOW_GRID_HIDE_TILT {
        draw_iso_grid(app, p, xf);
    } else if s.gridmode
        && let Some((minor, ((x0, x1), (y0, y1)))) = plan.minor
    {
        let major_n = f64::from(s.gridmajor.max(1));
        let major = minor * major_n;
        let mut lines = Vec::new();
        // World-aligned lines across the visible box (the painter clips them to the rect).
        for i in x0..=x1 {
            let x = i as f64 * minor;
            let is_major = (x / major).round() * major - x == 0.0 || ((x / major) - (x / major).round()).abs() < 1e-6;
            lines.push(Shape::line_segment(
                [xf.to_screen(Vec2::new(x, wb.min.y)), xf.to_screen(Vec2::new(x, wb.max.y))],
                Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
            ));
        }
        for j in y0..=y1 {
            let y = j as f64 * minor;
            let is_major = ((y / major) - (y / major).round()).abs() < 1e-6;
            lines.push(Shape::line_segment(
                [xf.to_screen(Vec2::new(wb.min.x, y)), xf.to_screen(Vec2::new(wb.max.x, y))],
                Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
            ));
        }
        p.extend(lines);
    }
    if plan.show_axes {
        if (wb.min.y..=wb.max.y).contains(&0.0) {
            p.line_segment([xf.to_screen(Vec2::new(wb.min.x, 0.0)), xf.to_screen(Vec2::new(wb.max.x, 0.0))], Stroke::new(1.0, t.axis_x));
        }
        if (wb.min.x..=wb.max.x).contains(&0.0) {
            p.line_segment([xf.to_screen(Vec2::new(0.0, wb.min.y)), xf.to_screen(Vec2::new(0.0, wb.max.y))], Stroke::new(1.0, t.axis_y));
        }
    }
}

/// The isometric grid (SNAPSTYL 1): lines at 30°, 90° and 150° through the snap lattice, GRIDUNIT
/// Y apart along the axes, coarsened while they would crowd closer than 8 pixels.
fn draw_iso_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    use cadcraft_engine::snap::iso;
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let mut d = s.gridunit.y.max(1e-12);
    for _ in 0..40 {
        if d * 0.866 * xf.scale >= 8.0 {
            break;
        }
        d *= 5.0;
    }
    let (origin, angle) = app.session.doc().map(cadcraft_engine::snap::grid_frame).unwrap_or((Vec2::ZERO, 0.0));
    let lines = iso::grid_lines(xf.world_bounds(), d, origin, angle, s.gridmajor);
    p.extend(
        lines.into_iter().map(|l| {
            Shape::line_segment([xf.to_screen(l.a), xf.to_screen(l.b)], Stroke::new(1.0, if l.major { t.grid_major } else { t.grid_minor }))
        }),
    );
}

#[cfg(test)]
mod grid_range_tests {
    use cadcraft_geom::{Bounds2, Vec2};

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
        // 1) Pure orthogonal side views (Front/Left): neither grid lines nor axes.
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
}
