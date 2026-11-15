use cadcraft_doc::{Entity, EntityKind, Prim, entity_bounds};
use cadcraft_geom::{Camera3D, Vec3, intersect_ray_triangle};

use super::{Bounds2, Drawing, Result, Session, Space, Vec2, View, select};

impl Session {
    // ---------------- views ----------------

    /// The limits of the space being edited: the sheet in paper space, else LIMMIN/LIMMAX.
    pub fn zoom_limits(&self) -> Result<Bounds2> {
        let d = self.doc()?;
        if let Space::Paper(name) = self.space()
            && let Some(sheet) = cadcraft_render::sheet(d, &name)
        {
            return Ok(sheet.bounds());
        }
        let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
        let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
        Ok(Bounds2::new(lo, hi))
    }

    pub fn zoom_extents(&mut self) -> Result<()> {
        let space = self.space();
        let (ext, z_range) = {
            let d = self.doc()?;
            let ext = d.extents(&space);
            if ext.is_empty() {
                (self.zoom_limits()?, (0.0, 0.0))
            } else {
                (ext, space_z_range(d, &space))
            }
        };
        let (w, h) = self.viewport_px;
        let st = self.state_mut()?;
        fit_bounds_3d(st, (w, h), ext, z_range, 1.05);
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping the 3D world point `about` fixed on screen.
    pub fn zoom_about_3d(&mut self, factor: f64, about: Vec3) -> Result<()> {
        if !(factor.is_finite() && factor > 0.0 && about.is_finite()) {
            return Ok(());
        }
        let st = self.state_mut()?;
        let v = st.view();
        let h = (v.height / factor).clamp(1e-9, 1e12);
        let k = h / v.height;
        let target1 = about + (v.target() - about) * k;
        st.set_view_quiet(View { height: h, ..v.with_target(target1) });
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping `about` (on `Z = 0`) fixed on screen.
    pub fn zoom_about(&mut self, factor: f64, about: Vec2) -> Result<()> {
        self.zoom_about_3d(factor, about.to3(0.0))
    }

    /// Orthographic 3D cursor raycast against visible entities (ported from MATCAD
    /// `findCameraRotationCenter.m` / `custom_hittest_3d.m`), returning the closest/frontmost 3D
    /// hit point within `hit_tol` world units in the camera plane.
    pub fn ray_hit_3d(&self, cam: Camera3D, view: View, uv: Vec2, hit_tol: f64) -> Option<Vec3> {
        if !uv.is_finite() || !hit_tol.is_finite() || hit_tol <= 0.0 {
            return None;
        }
        let d = self.doc().ok()?;
        let st = self.state().ok()?;
        let edit_space = st.edit_space();
        let store = d.space(&edit_space)?;
        let target = view.target();
        let (ray_o, ray_d) = cam.ray_at_target(uv, target);
        let mut best: Option<(Vec3, f64, f64)> = None; // (hit_pt, perp_dist, depth)
        for e in store.iter() {
            if !st.shows(e) {
                continue;
            }
            if let Some((pt, perp, depth)) = entity_ray_hit_3d(d, e, cam, target, uv, ray_o, ray_d, hit_tol) {
                let better = match best {
                    None => true,
                    Some((_, best_perp, best_depth)) => {
                        if depth + 1e-6 < best_depth {
                            true
                        } else if (depth - best_depth).abs() <= 1e-6 {
                            perp < best_perp
                        } else {
                            false
                        }
                    }
                };
                if better {
                    best = Some((pt, perp, depth));
                }
            }
        }
        best.map(|(pt, _, _)| pt)
    }

    /// Resolve the 3D orbit pivot according to `ORBITAUTOTARGET` and `ORBITFREECAM`:
    /// - `ORBITAUTOTARGET == false` → always `view.target()`
    /// - `ORBITFREECAM == true` (default, MATCAD mode):
    ///   - Canvas drag (`click_uv = Some(uv)`):
    ///     1. 3D raycast hit on a visible entity (`ray_hit_3d`)
    ///     2. Projected cursor point on the target plane (`cam.ray_at_target(uv, target).0`)
    ///   - ViewCube / no cursor (`click_uv = None`):
    ///     1. Visible selection 3D extents centre (if explicitly selected)
    ///     2. `view.target()`
    /// - `ORBITFREECAM == false` (AutoCAD object/scene-centre mode):
    ///   1. 3D raycast hit (`click_uv = Some(uv)`)
    ///   2. Visible selection 3D extents centre
    ///   3. Entire scene extents fitting with 1.25× margin in viewport
    ///   4. Projected cursor point on target plane (or `view.target()` when `click_uv = None`)
    pub fn resolve_orbit_pivot(&self, click_uv: Option<Vec2>, hit_tol: f64) -> Vec3 {
        let Ok(st) = self.state() else { return Vec3::ZERO };
        let v = st.view();
        let target = v.target();
        let in_model_orbit = matches!(st.space, Space::Model) || st.active_viewport().is_some_and(|(_, vp)| !vp.locked);
        if !self.settings.orbitautotarget || !in_model_orbit {
            return target;
        }
        let cam = st.zoom_camera().with_twist(v.twist);
        if let Some(uv) = click_uv
            && let Some(hit) = self.ray_hit_3d(cam, v, uv, hit_tol)
        {
            return hit;
        }
        if self.settings.orbit_free_camera
            && let Some(uv) = click_uv
        {
            return cam.ray_at_target(uv, target).0;
        }
        let half_h = v.height * 0.5;
        let aspect = (self.viewport_px.0 / self.viewport_px.1.max(1.0)).max(1e-6);
        let half_w = half_h * aspect;
        if let Ok(d) = self.doc() {
            let shown_sel = st.shown_selection();
            if !shown_sel.is_empty() {
                let (sel_xy, sel_z) = selection_bounds_3d(d, &shown_sel);
                if !sel_xy.is_empty() && box_overlaps_viewport(sel_xy, sel_z, cam, target, half_w, half_h) {
                    return box_center_3d(sel_xy, sel_z);
                }
            }
            if !self.settings.orbit_free_camera {
                let edit_space = st.edit_space();
                let ext = d.extents(&edit_space);
                if !ext.is_empty() {
                    let scene_z = space_z_range(d, &edit_space);
                    let c2 = ext.center();
                    let exp = Bounds2::new(c2 + (ext.min - c2) * 1.25, c2 + (ext.max - c2) * 1.25);
                    let cz = (scene_z.0 + scene_z.1) * 0.5;
                    let exp_z = (cz + (scene_z.0 - cz) * 1.25, cz + (scene_z.1 - cz) * 1.25);
                    if box_fits_viewport(exp, exp_z, cam, target, half_w, half_h) {
                        return box_center_3d(ext, scene_z);
                    }
                }
            }
        }
        match click_uv {
            Some(uv) => cam.ray_at_target(uv, target).0,
            None => target,
        }
    }
}

fn box_center_3d(b: Bounds2, z: (f64, f64)) -> Vec3 {
    let c = b.center();
    Vec3::new(c.x, c.y, (z.0 + z.1) * 0.5)
}

fn projected_uv_bounds(b: Bounds2, z: (f64, f64), cam: Camera3D, target: Vec3) -> Bounds2 {
    let mut out = Bounds2::EMPTY;
    let zs = if (z.0 - z.1).abs() <= 1e-12 { [z.0, z.0] } else { [z.0, z.1] };
    for xy in b.corners() {
        out.add(cam.project_3d(xy.to3(zs[0]), target));
        if zs[0] != zs[1] {
            out.add(cam.project_3d(xy.to3(zs[1]), target));
        }
    }
    out
}

fn box_overlaps_viewport(b: Bounds2, z: (f64, f64), cam: Camera3D, target: Vec3, half_w: f64, half_h: f64) -> bool {
    let vp = Bounds2::new(Vec2::new(-half_w, -half_h), Vec2::new(half_w, half_h));
    projected_uv_bounds(b, z, cam, target).intersects(&vp)
}

fn box_fits_viewport(b: Bounds2, z: (f64, f64), cam: Camera3D, target: Vec3, half_w: f64, half_h: f64) -> bool {
    let vp = Bounds2::new(Vec2::new(-half_w, -half_h), Vec2::new(half_w, half_h));
    vp.contains_box(&projected_uv_bounds(b, z, cam, target))
}

fn z_range_points<'a>(pts: impl Iterator<Item = &'a Vec3>) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in pts {
        if p.z.is_finite() {
            lo = lo.min(p.z);
            hi = hi.max(p.z);
        }
    }
    if lo <= hi { (lo, hi) } else { (0.0, 0.0) }
}

fn entity_z_range(d: &Drawing, e: &Entity, depth: usize) -> (f64, f64) {
    match &e.kind {
        EntityKind::Line(l) => z_range_points([l.a, l.b].iter()),
        EntityKind::Point(p) => (p.p.z, p.p.z),
        EntityKind::Circle(c) => (c.center.z, c.center.z),
        EntityKind::Arc(a) => (a.center.z, a.center.z),
        EntityKind::Ellipse(el) => (el.center.z, el.center.z),
        EntityKind::LwPolyline(p) => (p.elevation, p.elevation),
        EntityKind::Polyline3d(p) => z_range_points(p.points.iter()),
        EntityKind::Solid(s) | EntityKind::Trace(s) => z_range_points(s.corners.iter()),
        EntityKind::Face3d(f) => z_range_points(f.corners.iter()),
        EntityKind::PolyfaceMesh(m) => z_range_points(m.vertices.iter()),
        EntityKind::Insert(ins) => {
            let insert_z = if ins.insert.z.is_finite() { ins.insert.z } else { 0.0 };
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            if depth < cadcraft_doc::MAX_BLOCK_DEPTH
                && let Some(blk) = d.block(&ins.block)
            {
                let base_z = if blk.base.z.is_finite() { blk.base.z } else { 0.0 };
                let sz = if ins.scale.z.is_finite() { ins.scale.z } else { 1.0 };
                for be in blk.entities.iter().filter(|be| !matches!(be.kind, EntityKind::AttDef(_)) && d.is_visible(be)) {
                    let (blo, bhi) = entity_z_range(d, be, depth + 1);
                    let z0 = ins.insert.z + (blo - base_z) * sz;
                    let z1 = ins.insert.z + (bhi - base_z) * sz;
                    if z0.is_finite() && z1.is_finite() {
                        lo = lo.min(z0.min(z1));
                        hi = hi.max(z0.max(z1));
                    }
                }
            }
            for a in &ins.attribs {
                if !a.invisible && a.text.insert.z.is_finite() {
                    lo = lo.min(a.text.insert.z);
                    hi = hi.max(a.text.insert.z);
                }
            }
            if lo > hi {
                (insert_z, insert_z)
            } else {
                (lo, hi)
            }
        }
        _ => (0.0, 0.0),
    }
}

fn union_z(acc: (f64, f64), next: (f64, f64), has_any: bool) -> (f64, f64) {
    if !has_any { next } else { (acc.0.min(next.0), acc.1.max(next.1)) }
}

/// Fit 3D bounds `(b, z_range)` cleanly inside the viewport with `margin` multiplier.
pub(crate) fn fit_bounds_3d(
    st: &mut super::DocState,
    viewport_px: (f64, f64),
    b: Bounds2,
    z_range: (f64, f64),
    margin: f64,
) {
    let aspect = st.zoom_aspect(viewport_px.0 / viewport_px.1.max(1.0));
    let cam = st.zoom_camera();
    let twist = st.zoom_frame().twist;
    let (target, bw, bh) = View::projected_box_3d(b, z_range, cam, twist);
    let height = bh.max(bw / aspect.max(1e-6)).max(1e-6) * margin;
    st.set_zoom_frame(View { center: target.xy(), height, twist, target_z: target.z });
}

pub(crate) fn selection_bounds_3d(d: &Drawing, handles: &[super::Handle]) -> (Bounds2, (f64, f64)) {
    let mut xy = Bounds2::EMPTY;
    let mut zr = (0.0, 0.0);
    let mut any = false;
    for h in handles {
        let Some(e) = d.entity(*h) else { continue };
        let eb = entity_bounds(d, e, 0);
        if eb.is_empty() {
            continue;
        }
        xy = xy.union(&eb);
        zr = union_z(zr, entity_z_range(d, e, 0), any);
        any = true;
    }
    (xy, zr)
}

pub(crate) fn space_z_range(d: &Drawing, space: &Space) -> (f64, f64) {
    let Some(store) = d.space(space) else { return (0.0, 0.0) };
    let mut zr = (0.0, 0.0);
    let mut any = false;
    for e in store.iter().filter(|e| d.is_visible(e)) {
        zr = union_z(zr, entity_z_range(d, e, 0), any);
        any = true;
    }
    zr
}

fn update_hit(best: &mut Option<(Vec3, f64, f64)>, pt: Vec3, perp: f64, depth: f64, hit_tol: f64) {
    if perp > hit_tol || !pt.is_finite() || !depth.is_finite() {
        return;
    }
    let better = match *best {
        None => true,
        Some((_, b_perp, b_depth)) => {
            depth + 1e-6 < b_depth || ((depth - b_depth).abs() <= 1e-6 && perp < b_perp)
        }
    };
    if better {
        *best = Some((pt, perp, depth));
    }
}

fn test_segment_3d(
    best: &mut Option<(Vec3, f64, f64)>,
    cam: Camera3D,
    target: Vec3,
    uv: Vec2,
    a: Vec3,
    b: Vec3,
    hit_tol: f64,
) {
    let (pt, perp, depth) = cam.closest_segment_hit(uv, target, a, b);
    update_hit(best, pt, perp, depth, hit_tol);
}

fn test_quad_triangles(best: &mut Option<(Vec3, f64, f64)>, target: Vec3, ray_o: Vec3, ray_d: Vec3, q: [Vec3; 4]) {
    for [i0, i1, i2] in [[0, 1, 2], [0, 2, 3]] {
        if let Some((pt, _)) = intersect_ray_triangle(ray_o, ray_d, q[i0], q[i1], q[i2]) {
            update_hit(best, pt, 0.0, (pt - target).dot(ray_d), 1.0);
        }
    }
}

fn prim_hit_3d(
    best: &mut Option<(Vec3, f64, f64)>,
    p: &Prim,
    z: f64,
    cam: Camera3D,
    target: Vec3,
    uv: Vec2,
    hit_tol: f64,
) {
    let seg_tol = (hit_tol * 0.25).max(1e-6);
    let mut pts = Vec::new();
    match p {
        Prim::Seg(s) => s.tessellate(seg_tol, &mut pts),
        Prim::Circle(c) => c.tessellate(seg_tol, &mut pts),
        Prim::Ellipse(el) => el.tessellate(seg_tol, &mut pts),
        Prim::Spline(sp) => pts = sp.tessellate(seg_tol),
        Prim::Point(pt) => {
            let p3 = pt.to3(z);
            let perp = cam.project_3d(p3, target).dist(uv);
            update_hit(best, p3, perp, (p3 - target).dot(cam.z), hit_tol);
            return;
        }
        Prim::Infinite { base, dir, ray } => {
            let far = 1e6;
            let a = if *ray { *base } else { *base - *dir * far };
            let b = *base + *dir * far;
            test_segment_3d(best, cam, target, uv, a.to3(z), b.to3(z), hit_tol);
            return;
        }
        Prim::Fill(f) => pts = f.clone(),
    }
    for w in pts.windows(2) {
        if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
            test_segment_3d(best, cam, target, uv, a.to3(z), b.to3(z), hit_tol);
        }
    }
}

fn entity_ray_hit_3d(
    d: &Drawing,
    e: &Entity,
    cam: Camera3D,
    target: Vec3,
    uv: Vec2,
    ray_o: Vec3,
    ray_d: Vec3,
    hit_tol: f64,
) -> Option<(Vec3, f64, f64)> {
    let mut best: Option<(Vec3, f64, f64)> = None;
    match &e.kind {
        EntityKind::Line(l) => test_segment_3d(&mut best, cam, target, uv, l.a, l.b, hit_tol),
        EntityKind::Point(p) => {
            let perp = cam.project_3d(p.p, target).dist(uv);
            update_hit(&mut best, p.p, perp, (p.p - target).dot(cam.z), hit_tol);
        }
        EntityKind::Polyline3d(p) => {
            for w in p.points.windows(2) {
                if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                    test_segment_3d(&mut best, cam, target, uv, *a, *b, hit_tol);
                }
            }
            if p.closed
                && let (Some(a), Some(b)) = (p.points.last(), p.points.first())
            {
                test_segment_3d(&mut best, cam, target, uv, *a, *b, hit_tol);
            }
        }
        EntityKind::Face3d(f) => {
            test_quad_triangles(&mut best, target, ray_o, ray_d, f.corners);
            for (a, b) in f.visible_edges() {
                test_segment_3d(&mut best, cam, target, uv, a, b, hit_tol);
            }
        }
        EntityKind::PolyfaceMesh(m) => {
            for f in m.faces3d() {
                test_quad_triangles(&mut best, target, ray_o, ray_d, f.corners);
                for (a, b) in f.visible_edges() {
                    test_segment_3d(&mut best, cam, target, uv, a, b, hit_tol);
                }
            }
        }
        EntityKind::Solid(s) | EntityKind::Trace(s) => {
            let c = s.corners;
            let q = [c[0], c[1], c[3], c[2]];
            test_quad_triangles(&mut best, target, ray_o, ray_d, q);
            for i in 0..4 {
                test_segment_3d(&mut best, cam, target, uv, q[i], q[(i + 1) % 4], hit_tol);
            }
        }
        EntityKind::Circle(_)
        | EntityKind::Arc(_)
        | EntityKind::Ellipse(_)
        | EntityKind::LwPolyline(_)
        | EntityKind::Spline(_)
        | EntityKind::Helix(_)
        | EntityKind::Ray(_)
        | EntityKind::XLine(_)
        | EntityKind::Region(_)
        | EntityKind::MLine(_)
        | EntityKind::Wipeout(_) => {
            let z = entity_z_range(d, e, 0).0;
            for prim in e.kind.prims() {
                prim_hit_3d(&mut best, &prim, z, cam, target, uv, hit_tol);
            }
        }
        _ => {
            // Complex annotations/inserts/hatches live on Z = 0 and reuse 2D picking logic along the cursor ray through `target`.
            if !cam.is_edge_on_xy()
                && let Some(w) = cadcraft_geom::intersect_ray_plane(ray_o, ray_d, Vec3::ZERO, Vec3::Z)
                && select::entity_distance(d, e, w.xy(), (hit_tol * 0.25).max(1e-6)) <= hit_tol
            {
                let p = Vec3::new(w.x, w.y, 0.0);
                update_hit(&mut best, p, 0.0, (p - target).dot(cam.z), hit_tol);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Line as DocLine};
    use serde_json::json;

    #[test]
    fn ray_hit_3d_picks_frontmost_3d_line_under_cursor() {
        let mut s = Session::new();
        s.viewport_px = (1000.0, 1000.0);
        // Back line at Z = 0, front line crossing on top at Z = 25 in Top view.
        {
            let d = s.doc_mut().expect("doc");
            d.add(
                &Space::Model,
                Common::default(),
                EntityKind::Line(DocLine { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(20.0, 0.0, 0.0) }),
            )
            .expect("add back line");
            d.add(
                &Space::Model,
                Common::default(),
                EntityKind::Line(DocLine { a: Vec3::new(10.0, -5.0, 25.0), b: Vec3::new(10.0, 5.0, 25.0) }),
            )
            .expect("add front line");
        }
        let cam = Camera3D::TOP;
        let view = View::plan(Vec2::new(5.0, 5.0), 100.0, 0.0);
        // Click offset from target (5, 5, 0) to (10, 0) in camera plane is (+5, -5).
        let uv = cam.project_3d(Vec3::new(10.0, 0.0, 0.0), view.target());
        let hit = s.ray_hit_3d(cam, view, uv, 0.5).expect("front line hit");
        assert!((hit.x - 10.0).abs() < 1e-9 && hit.y.abs() < 1e-9 && (hit.z - 25.0).abs() < 1e-9, "got {hit:?}");
        // Clicking far in empty space returns None.
        assert!(s.ray_hit_3d(cam, view, Vec2::new(80.0, 80.0), 0.5).is_none());
    }

    #[test]
    fn resolve_orbit_pivot_follows_hybrid_priority_and_sysvar() {
        let mut s = Session::new();
        s.viewport_px = (1000.0, 1000.0);
        let (h1, _h2) = {
            let d = s.doc_mut().expect("doc");
            let h1 = d
                .add(
                    &Space::Model,
                    Common::default(),
                    EntityKind::Line(DocLine { a: Vec3::new(10.0, 10.0, 12.0), b: Vec3::new(20.0, 10.0, 12.0) }),
                )
                .expect("add line 1");
            let h2 = d
                .add(
                    &Space::Model,
                    Common::default(),
                    EntityKind::Line(DocLine { a: Vec3::new(50.0, 50.0, 0.0), b: Vec3::new(60.0, 50.0, 0.0) }),
                )
                .expect("add line 2");
            (h1, h2)
        };
        // View centered at (0, 0, 0) with height 200 so the entire scene fits cleanly with 1.25x buffer.
        s.state_mut().expect("state").set_view_quiet(View::plan(Vec2::ZERO, 200.0, 0.0));
        assert!(s.settings.orbit_free_camera, "ORBITFREECAM must default to true (MATCAD mode)");

        // 1) ORBITFREECAM = 1 (default): Cursor on line 1 (15, 10, 12) → hits 3D object directly.
        let uv_on_line1 = Vec2::new(15.0, 10.0);
        let p_ray = s.resolve_orbit_pivot(Some(uv_on_line1), 1.0);
        assert!((p_ray - Vec3::new(15.0, 10.0, 12.0)).len() < 1e-9, "got {p_ray:?}");

        // 2) ORBITFREECAM = 1: Cursor in empty space stays free on target plane, even with selection or fitting scene.
        s.set_selection(vec![h1]);
        let p_free_sel = s.resolve_orbit_pivot(Some(Vec2::new(-40.0, -40.0)), 1.0);
        assert!((p_free_sel - Vec3::new(-40.0, -40.0, 0.0)).len() < 1e-9, "got {p_free_sel:?}");
        // ViewCube (click_uv = None) with explicit selection uses selection centre; without selection uses view.target().
        let p_vc_sel = s.resolve_orbit_pivot(None, 0.0);
        assert!((p_vc_sel - Vec3::new(15.0, 10.0, 12.0)).len() < 1e-9, "got {p_vc_sel:?}");
        s.set_selection(Vec::new());
        let p_vc_free = s.resolve_orbit_pivot(None, 0.0);
        assert_eq!(p_vc_free, Vec3::ZERO);

        // 3) ORBITFREECAM = 0 (AutoCAD object/scene centre): falls back to selection & scene extents.
        s.execute("setvar", &json!({ "name": "ORBITFREECAM", "value": 0 })).expect("setvar ORBITFREECAM");
        s.set_selection(vec![h1]);
        let p_sel = s.resolve_orbit_pivot(Some(Vec2::new(-40.0, -40.0)), 1.0);
        assert!((p_sel - Vec3::new(15.0, 10.0, 12.0)).len() < 1e-9, "got {p_sel:?}");

        // Clear selection, whole scene fits with 1.25x → overall scene extents centre ((10..60, 10..50, 0..12) → (35, 30, 6)).
        s.set_selection(Vec::new());
        let p_scene = s.resolve_orbit_pivot(Some(Vec2::new(-40.0, -40.0)), 1.0);
        assert!((p_scene - Vec3::new(35.0, 30.0, 6.0)).len() < 1e-9, "got {p_scene:?}");

        // Zoomed tightly in (1.25x scene outside viewport) + empty space click → target plane ray intersection.
        s.state_mut().expect("state").set_view_quiet(View::plan(Vec2::ZERO, 10.0, 0.0));
        let p_plane = s.resolve_orbit_pivot(Some(Vec2::new(2.0, -3.0)), 0.5);
        assert!((p_plane - Vec3::new(2.0, -3.0, 0.0)).len() < 1e-9, "got {p_plane:?}");

        // 4) ORBITAUTOTARGET = 0 → always returns view.target().
        s.execute("setvar", &json!({ "name": "ORBITAUTOTARGET", "value": 0 })).expect("setvar ORBITAUTOTARGET");
        let p_off = s.resolve_orbit_pivot(Some(uv_on_line1), 1.0);
        assert_eq!(p_off, Vec3::ZERO);
    }

    #[test]
    fn zoom_extents_3d_fits_all_8_aabb_corners_and_updates_target_z() {
        let mut s = Session::new();
        s.viewport_px = (800.0, 600.0);
        {
            let d = s.doc_mut().expect("doc");
            let a = DocLine { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(20.0, 10.0, 0.0) };
            let b = DocLine { a: Vec3::new(0.0, 0.0, 60.0), b: Vec3::new(20.0, 10.0, 60.0) };
            d.add(&Space::Model, Common::default(), EntityKind::Line(a)).expect("add base");
            d.add(&Space::Model, Common::default(), EntityKind::Line(b)).expect("add top");
        }
        let corners = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(20.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 60.0),
            Vec3::new(20.0, 0.0, 60.0),
            Vec3::new(20.0, 10.0, 60.0),
            Vec3::new(0.0, 10.0, 60.0),
        ];
        for preset in ["se_isometric", "front"] {
            s.execute("view.set", &json!({ "preset": preset })).expect("preset");
            s.zoom_extents().expect("zoom extents");
            let st = s.state().expect("state");
            let v = st.view();
            let cam = st.camera;
            let half_h = v.height * 0.5;
            let half_w = half_h * (800.0 / 600.0);
            assert!((v.target_z - 30.0).abs() < 1e-6, "{preset}: expected target_z=30, got {v:?}");
            for pt in corners {
                let uv = cam.project_3d(pt, v.target());
                assert!(
                    uv.x.abs() <= half_w + 1e-9 && uv.y.abs() <= half_h + 1e-9,
                    "{preset}: corner {pt:?} outside viewport ({uv:?} vs +/-({half_w}, {half_h}))"
                );
            }
        }
    }

    #[test]
    fn zoom_about_3d_keeps_target_plane_anchor_pinned_in_3d_and_front_view() {
        let mut s = Session::new();
        for preset in ["se_isometric", "front"] {
            s.execute("view.set", &json!({ "preset": preset })).expect("preset");
            let cam = s.state().expect("state").camera;
            let v0 = View { center: Vec2::new(10.0, 20.0), height: 100.0, twist: 0.0, target_z: 35.0 };
            s.state_mut().expect("state").set_view_quiet(v0);
            let uv0 = Vec2::new(18.0, -12.0);
            let anchor = cam.ray_at_target(uv0, v0.target()).0;
            s.zoom_about_3d(1.6, anchor).expect("zoom_about_3d");
            let v1 = s.state().expect("state").view();
            let uv1 = cam.project_3d(anchor, v1.target());
            let scale_ratio = v0.height / v1.height;
            assert!((uv1.x * scale_ratio - uv0.x).abs() < 1e-9 && (uv1.y * scale_ratio - uv0.y).abs() < 1e-9);
            assert!(
                ((v1.target() - v0.target()).dot(cam.z)).abs() < 1e-9,
                "{preset}: zoom in target plane must preserve depth coordinate"
            );
        }
    }

    #[test]
    fn insert_z_range_excludes_insert_z_when_block_has_geometry() {
        use cadcraft_doc::{Block, Handle, Insert};
        let mut s = Session::new();
        {
            let d = s.doc_mut().expect("doc");
            let mut blk = Block::new("ELEV");
            blk.entities.push(Entity::new(
                Handle(1),
                EntityKind::Line(DocLine { a: Vec3::new(0.0, 0.0, 1000.0), b: Vec3::new(20.0, 10.0, 1000.0) }),
            ));
            d.blocks.insert("ELEV".into(), std::sync::Arc::new(blk));
            d.blocks.insert("EMPTY".into(), std::sync::Arc::new(Block::new("EMPTY")));
            let elev_h = d
                .add(
                    &Space::Model,
                    Common::default(),
                    EntityKind::Insert(Insert { block: "ELEV".into(), insert: Vec3::ZERO, ..Default::default() }),
                )
                .expect("add elev insert");
            let elev_e = d.entity(elev_h).expect("elev entity");
            assert_eq!(entity_z_range(d, elev_e, 0), (1000.0, 1000.0));
        }
        assert_eq!(space_z_range(s.doc().expect("doc"), &Space::Model), (1000.0, 1000.0));
        s.execute("view.set", &json!({ "preset": "front" })).expect("front");
        s.zoom_extents().expect("zoom extents");
        let target_z = s.state().expect("state").view().target_z;
        assert!((target_z - 1000.0).abs() < 1e-6, "expected target_z=1000, got {target_z}");

        // When a block has neither visible geometry nor attributes, fallback uses insert.z.
        let d = s.doc_mut().expect("doc");
        let empty_h = d
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::Insert(Insert { block: "EMPTY".into(), insert: Vec3::new(0.0, 0.0, 42.0), ..Default::default() }),
            )
            .expect("add empty insert");
        let empty_e = d.entity(empty_h).expect("empty entity");
        assert_eq!(entity_z_range(d, empty_e, 0), (42.0, 42.0));
    }
}

