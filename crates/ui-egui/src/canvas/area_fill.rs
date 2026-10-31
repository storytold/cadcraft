//! Temporary AREA command fill. Never changes saved geometry, the reported area, or snap behavior.

use super::Xf;
use cadcraft_geom::Vec2;
use egui::Color32;

/// Check whether a closed AREA boundary is a simple polygon safe to fill.
/// Nonadjacent edges may neither cross nor touch: AREA uses shoelace area while the
/// even-odd triangulator shades individual lobes of self-intersecting outlines.
/// Fail closed on degenerate edges or non-finite intermediate calculations. This
/// runs only for previews of up to 256 vertices; the drawing and AREA result stay unchanged.
fn area_preview_boundary_is_simple(boundary: &[Vec2]) -> bool {
    let n = boundary.len();
    if !(3..=256).contains(&n) || boundary.iter().any(|p| !p.is_finite()) {
        return false;
    }
    let opposite = |a: f64, b: f64| (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0);
    let on_segment = |a: Vec2, b: Vec2, p: Vec2| {
        p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    };
    for (i, &a) in boundary.iter().enumerate() {
        let Some(&b) = boundary.get((i + 1) % n) else { return false };
        if a == b {
            return false;
        }
        // Edges sharing a corner are adjacent; only nonadjacent intersections
        // make the preview ambiguous. Include endpoint touches and collinear overlap.
        for (j, &c) in boundary.iter().enumerate().skip(i + 2) {
            if i == 0 && j == n - 1 {
                continue;
            }
            let Some(&d) = boundary.get((j + 1) % n) else { return false };
            let ab = b - a;
            let cd = d - c;
            let ac = c - a;
            let ad = d - a;
            let ca = a - c;
            let cb = b - c;
            let (o1, o2, o3, o4) = (ab.cross(ac), ab.cross(ad), cd.cross(ca), cd.cross(cb));
            if ![o1, o2, o3, o4].iter().all(|v| v.is_finite()) {
                return false;
            }
            if (opposite(o1, o2) && opposite(o3, o4))
                || (o1 == 0.0 && on_segment(a, b, c))
                || (o2 == 0.0 && on_segment(a, b, d))
                || (o3 == 0.0 && on_segment(c, d, a))
                || (o4 == 0.0 && on_segment(c, d, b))
            {
                return false;
            }
        }
    }
    true
}

/// Screen-only AREA rubber-band shading. This is not a HATCH and never enters the drawing.
pub(super) fn area_preview_fill(command: Option<&str>, ents: &[cadcraft_doc::Entity], xf: &Xf) -> Option<egui::Mesh> {
    if command != Some("area") {
        return None;
    }
    let cadcraft_doc::EntityKind::LwPolyline(poly) = &ents.first()?.kind else {
        return None;
    };
    // This runs on pointer movement; keep triangulation and mesh allocation bounded.
    if !poly.closed || poly.vertices.len() < 3 || poly.vertices.len() > 256 {
        return None;
    }
    let mut boundary: Vec<Vec2> = poly.vertices.iter().map(|v| v.p).collect();
    // AREA appends the cursor after the selected vertices. It can revisit any
    // selected point (including the first when closing the shape), so discard
    // that final *preview* vertex only. Actual selected duplicates remain and
    // are rejected by the simple-boundary check below.
    if boundary.split_last().is_some_and(|(cursor, selected)| selected.contains(cursor)) {
        boundary.truncate(boundary.len() - 1);
    }
    // A self-crossing polygon may shade multiple lobes even when AREA's shoelace
    // measurement is zero. Keep its outline, but do not show a misleading fill.
    if !area_preview_boundary_is_simple(&boundary) {
        return None;
    }
    // Use the existing even-odd renderer for simple concave polygons (not a triangle fan).
    let triangles = cadcraft_render::triangulate_evenodd(&[boundary]);
    if triangles.len() > 24_000 || triangles.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let mut mesh = egui::Mesh::default();
    let fill = Color32::from_rgba_unmultiplied(48, 195, 112, 76); // ~30% opacity
    for triangle in triangles.as_chunks::<3>().0 {
        let start = mesh.vertices.len() as u32;
        for point in triangle {
            mesh.colored_vertex(xf.to_screen(*point), fill);
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    (!mesh.indices.is_empty()).then_some(mesh)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use cadcraft_engine::Input;
    use egui::{Rect, pos2};

    /// A regular AREA rectangle shades only the transient mesh, not the saved drawing.
    #[test]
    fn area_preview_fills_rectangle_without_changing_drawing() {
        let mut s = cadcraft_engine::Session::new();
        let initial = s.doc().unwrap().model.len();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let ents = s.preview(Vec2::new(0.0, 10.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(5.0, 5.0), scale: 10.0 };
        let mesh = area_preview_fill(s.active_command_id(), &ents, &xf).unwrap();
        assert!(!mesh.indices.is_empty());
        assert!(mesh.vertices.iter().all(|v| v.color.a() == 76));
        assert_eq!(s.doc().unwrap().model.len(), initial);
        assert!(area_preview_fill(Some("line"), &ents, &xf).is_none());
        s.input(Input::Enter).unwrap();
        assert_eq!(s.active_command_id(), None);
        assert_eq!(s.doc().unwrap().model.len(), initial);
    }
    
    /// A simple concave boundary shades its notch correctly without filling its bounding box.
    #[test]
    fn area_preview_concave_polygon_respects_notch() {
        let mut s = cadcraft_engine::Session::new();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(6.0, 2.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 6.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let ents = s.preview(Vec2::new(0.0, 6.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(3.0, 3.0), scale: 10.0 };
        let mesh = area_preview_fill(s.active_command_id(), &ents, &xf).unwrap();
        let drawn_area: f32 = mesh.indices.as_chunks::<3>().0.iter().map(|tri| {
            let a = mesh.vertices[tri[0] as usize].pos;
            let b = mesh.vertices[tri[1] as usize].pos;
            let c = mesh.vertices[tri[2] as usize].pos;
            ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
        }).sum();
        // L-shaped polygon = (6 x 2) + (2 x 4) = 20 square drawing units.
        assert!((drawn_area / 100.0 - 20.0).abs() < 0.01);
        s.cancel();
        assert_eq!(s.active_command_id(), None);
    }
    
    /// Collinear AREA points create no triangles, even though the outline remains.
    #[test]
    fn area_preview_collinear_points_have_no_fill() {
        let mut s = cadcraft_engine::Session::new();
        s.start("area").unwrap();
        s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
        s.input(Input::Point(Vec2::new(2.0, 0.0))).unwrap();
        let ents = s.preview(Vec2::new(4.0, 0.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(2.0, 0.0), scale: 10.0 };
        assert!(area_preview_fill(s.active_command_id(), &ents, &xf).is_none());
        s.cancel();
    }
    
    /// A stationary cursor repeats only the temporary preview vertex; the valid triangle stays shaded.
    #[test]
    fn area_preview_keeps_fill_when_cursor_equals_last_selected_point() {
        let mut s = cadcraft_engine::Session::new();
        let initial = s.doc().unwrap().model.len();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let cursor = Vec2::new(0.0, 3.0);
        let ents = s.preview(cursor);
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(2.0, 1.5), scale: 10.0 };
        let mesh = area_preview_fill(s.active_command_id(), &ents, &xf).unwrap();
        assert!(!mesh.indices.is_empty());
        assert_eq!(s.doc().unwrap().model.len(), initial);
        s.cancel();
        assert_eq!(s.active_command_id(), None);
    }
    
    /// Revisit the first or an interior selected point without hiding a valid AREA fill.
    #[test]
    fn area_preview_keeps_fill_when_cursor_returns_to_earlier_selected_points() {
        let mut s = cadcraft_engine::Session::new();
        let initial = s.doc().unwrap().model.len();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(2.0, 1.5), scale: 10.0 };
        for cursor in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0)] {
            let ents = s.preview(cursor);
            let mesh = area_preview_fill(s.active_command_id(), &ents, &xf).unwrap();
            assert!(!mesh.indices.is_empty(), "valid selected boundary should remain shaded at {cursor:?}");
        }
        assert_eq!(s.doc().unwrap().model.len(), initial);
        s.cancel();
    }

    /// A repeated earlier *selected* point remains invalid even if the temporary cursor repeats it.
    #[test]
    fn area_preview_rejects_real_duplicate_earlier_selected_point() {
        let mut s = cadcraft_engine::Session::new();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0), Vec2::new(4.0, 0.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let ents = s.preview(Vec2::new(4.0, 0.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(2.0, 1.5), scale: 10.0 };
        assert!(!ents.is_empty(), "preview outline still exists");
        assert!(area_preview_fill(s.active_command_id(), &ents, &xf).is_none());
        s.cancel();
    }

    /// Real duplicate clicks remain invalid even when the cursor repeats the final selected point.
    #[test]
    fn area_preview_still_rejects_duplicate_selected_points() {
        let mut s = cadcraft_engine::Session::new();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(0.0, 3.0), Vec2::new(0.0, 3.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let ents = s.preview(Vec2::new(0.0, 3.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(2.0, 1.5), scale: 10.0 };
        assert!(!ents.is_empty(), "the outline remains available");
        assert!(area_preview_fill(s.active_command_id(), &ents, &xf).is_none());
        s.cancel();
    }
    
    /// A self-crossing bowtie has zero signed shoelace area; its two visual lobes must not be shaded.
    #[test]
    fn area_preview_bowtie_does_not_show_misleading_fill() {
        let mut s = cadcraft_engine::Session::new();
        let initial = s.doc().unwrap().model.len();
        s.start("area").unwrap();
        for p in [Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 0.0)] {
            s.input(Input::Point(p)).unwrap();
        }
        let ents = s.preview(Vec2::new(0.0, 2.0));
        let xf = Xf { rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0)), center: Vec2::new(1.0, 1.0), scale: 10.0 };
        assert!(!ents.is_empty(), "the outline is still available");
        assert!(area_preview_fill(s.active_command_id(), &ents, &xf).is_none());
        assert_eq!(s.doc().unwrap().model.len(), initial);
        s.cancel();
        assert_eq!(s.active_command_id(), None);
    }
    
    /// Touching nonadjacent edges, duplicate points and invalid coordinates are not safe to shade.
    #[test]
    fn area_preview_rejects_touching_and_degenerate_boundaries() {
        let v = |x, y| Vec2::new(x, y);
        assert!(!area_preview_boundary_is_simple(&[
            v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(2.0, 2.0), v(0.0, 4.0), v(2.0, 2.0)
        ]));
        assert!(!area_preview_boundary_is_simple(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 0.0), v(0.0, 4.0)]));
        assert!(!area_preview_boundary_is_simple(&[v(0.0, 0.0), v(f64::NAN, 2.0), v(0.0, 4.0)]));
        assert!(area_preview_boundary_is_simple(&[
            v(0.0, 0.0), v(6.0, 0.0), v(6.0, 2.0), v(2.0, 2.0), v(2.0, 6.0), v(0.0, 6.0)
        ]));
    }
}
