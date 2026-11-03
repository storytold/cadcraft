use super::{Bounds2, CadApp, Shape, Stroke, Tokens, Vec2, Xf, pos2};
use egui::Color32;

/// Grid spacing adapted to the zoom: multiply the unit until lines are at least `min_px` apart.
fn adaptive(unit: f64, scale: f64, min_px: f64) -> f64 {
    let mut u = unit.max(1e-12);
    let mut guard = 0;
    while u * scale < min_px && guard < 40 {
        u *= 5.0;
        guard += 1;
    }
    u
}

/// Index ranges of the grid lines inside `wb` for line spacing `minor`, or `None` when there would be
/// 2000 lines or more along an axis. The span is computed with checked arithmetic: a huge view
/// saturates the float-to-integer casts at the i64 limits, and subtracting those would overflow.
fn grid_index_ranges(wb: &Bounds2, minor: f64) -> Option<((i64, i64), (i64, i64))> {
    let range = |lo: f64, hi: f64| {
        let a = (lo / minor).floor() as i64;
        let b = (hi / minor).ceil() as i64;
        b.checked_sub(a).filter(|n| *n < 2000).map(|_| (a, b))
    };
    Some((range(wb.min.x, wb.max.x)?, range(wb.min.y, wb.max.y)?))
}

pub(super) fn draw_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    if xf.projection.is_some() {
        draw_plane_grid(app, p, xf);
        return;
    }
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let wb = xf.world_bounds();
    if s.gridmode && s.isodraft {
        draw_iso_grid(app, p, xf);
    } else if s.gridmode {
        let major_n = f64::from(s.gridmajor.max(1));
        let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
        let minor = adaptive(unit, xf.scale, 8.0);
        let major = minor * major_n;
        let mut lines = Vec::new();
        if let Some(((x0, x1), (y0, y1))) = grid_index_ranges(&wb, minor) {
            for i in x0..=x1 {
                let x = i as f64 * minor;
                let is_major = (x / major).round() * major - x == 0.0 || ((x / major) - (x / major).round()).abs() < 1e-6;
                let sx = xf.to_screen(Vec2::new(x, 0.0)).x;
                lines.push(Shape::line_segment(
                    [pos2(sx, xf.rect.top()), pos2(sx, xf.rect.bottom())],
                    Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
                ));
            }
            for j in y0..=y1 {
                let y = j as f64 * minor;
                let is_major = ((y / major) - (y / major).round()).abs() < 1e-6;
                let sy = xf.to_screen(Vec2::new(0.0, y)).y;
                lines.push(Shape::line_segment(
                    [pos2(xf.rect.left(), sy), pos2(xf.rect.right(), sy)],
                    Stroke::new(1.0, if is_major { t.grid_major } else { t.grid_minor }),
                ));
            }
        }
        p.extend(lines);
    }
    // Axes through the origin.
    let o = xf.to_screen(Vec2::ZERO);
    if xf.rect.y_range().contains(o.y) {
        p.line_segment([pos2(xf.rect.left(), o.y), pos2(xf.rect.right(), o.y)], Stroke::new(1.0, t.axis_x));
    }
    if xf.rect.x_range().contains(o.x) {
        p.line_segment([pos2(o.x, xf.rect.top()), pos2(o.x, xf.rect.bottom())], Stroke::new(1.0, t.axis_y));
    }
}

/// The grid and axes of the WCS XY plane in a 3D view, as segments across the area of the plane on
/// screen; none when the plane is seen edge-on or so nearly that its area on screen is huge.
fn draw_plane_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    let wb = xf.world_bounds();
    let edge_on = xf.projection.and_then(|m| m.plane(0.0).inverse()).is_none();
    let size = wb.width().max(wb.height());
    if edge_on || !size.is_finite() || size >= 1e12 {
        return;
    }
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    let seg = |a: Vec2, b: Vec2, c: Color32| Shape::line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, c));
    if s.gridmode {
        let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
        let minor = adaptive(unit, xf.scale, 8.0);
        let major = minor * f64::from(s.gridmajor.max(1));
        let color = |v: f64| if ((v / major) - (v / major).round()).abs() < 1e-6 { t.grid_major } else { t.grid_minor };
        if let Some(((x0, x1), (y0, y1))) = grid_index_ranges(&wb, minor) {
            let mut lines = Vec::new();
            for i in x0..=x1 {
                let x = i as f64 * minor;
                lines.push(seg(Vec2::new(x, wb.min.y), Vec2::new(x, wb.max.y), color(x)));
            }
            for j in y0..=y1 {
                let y = j as f64 * minor;
                lines.push(seg(Vec2::new(wb.min.x, y), Vec2::new(wb.max.x, y), color(y)));
            }
            p.extend(lines);
        }
    }
    p.add(seg(Vec2::new(wb.min.x, 0.0), Vec2::new(wb.max.x, 0.0), t.axis_x));
    p.add(seg(Vec2::new(0.0, wb.min.y), Vec2::new(0.0, wb.max.y), t.axis_y));
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
        assert_eq!(super::grid_index_ranges(&huge, 1.0), None);
        let normal = Bounds2::new(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 10.0));
        assert_eq!(super::grid_index_ranges(&normal, 1.0), Some(((-5, 5), (0, 10))));
    }

    /// A 3D view whose XY plane is all but edge-on maps the screen to a huge area of the plane:
    /// the grid is skipped instead of overflowing or drawing without end.
    #[test]
    fn a_nearly_edge_on_plane_draws_no_endless_grid() {
        use cadcraft_geom::{Mat4, Vec3};
        let mut app = crate::CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("setvar", &serde_json::json!({"name": "GRIDMODE", "value": 1})).unwrap();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let projection = Some(Mat4::view(Vec3::new(1.0, 0.0, 1e-200), Vec3::ZERO, 0.0));
        let xf = super::Xf { projection, ..super::Xf::new(rect, Vec2::ZERO, 10.0) };
        let painter = egui::Painter::new(egui::Context::default(), egui::LayerId::background(), rect);
        super::draw_grid(&app, &painter, &xf);
    }
}
