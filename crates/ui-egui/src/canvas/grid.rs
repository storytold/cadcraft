use super::draw::clip_infinite;
use super::{CadApp, Shape, Stroke, Tokens, Vec2, Xf};

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

pub(super) fn draw_grid(app: &CadApp, p: &egui::Painter, xf: &Xf) {
    let t = Tokens::get();
    let s = app.session.drafting_settings();
    if s.gridmode && s.isodraft {
        draw_iso_grid(app, p, xf);
    } else if s.gridmode {
        let unit = if s.gridunit.x == 0.0 { s.snapunit.x } else { s.gridunit.x };
        let minor = adaptive(unit, xf.scale, 8.0);
        // The grid lies in the snap frame: SNAPBASE and SNAPANG in the current UCS.
        let space = app.session.space();
        let (origin, angle) = app.session.doc().map(|d| cadcraft_engine::snap::grid_frame_in(d, &space)).unwrap_or((Vec2::ZERO, 0.0));
        let lines = cadcraft_engine::snap::iso::rect_grid_lines(xf.world_bounds(), minor, origin, angle, s.gridmajor);
        p.extend(lines.into_iter().map(|l| {
            Shape::line_segment([xf.to_screen(l.a), xf.to_screen(l.b)], Stroke::new(1.0, if l.major { t.grid_major } else { t.grid_minor }))
        }));
    }
    // The current UCS's X and Y axes through its origin.
    let u = app.session.input_ucs();
    let o = u.origin.xy();
    for (dir, color) in [(u.x.xy(), t.axis_x), (u.y.xy(), t.axis_y)] {
        if let Some(seg) = clip_infinite(xf, o, dir, false) {
            p.line_segment(seg, Stroke::new(1.0, color));
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
    // In the current UCS (SNAPBASE from its origin, SNAPANG from its X axis).
    let space = app.session.space();
    let (origin, angle) = app.session.doc().map(|d| cadcraft_engine::snap::grid_frame_in(d, &space)).unwrap_or((Vec2::ZERO, 0.0));
    let lines = iso::grid_lines(xf.world_bounds(), d, origin, angle, s.gridmajor);
    p.extend(
        lines.into_iter().map(|l| {
            Shape::line_segment([xf.to_screen(l.a), xf.to_screen(l.b)], Stroke::new(1.0, if l.major { t.grid_major } else { t.grid_minor }))
        }),
    );
}
