use super::{CadApp, Handle, Input, Pos2, Rect, Xf, vec2};

/// A click on grip `g` (idle): Shift+click makes it hot or cold again; a plain click starts
/// editing it, with the other hot grips when it is one of them.
pub(super) fn click_grip(app: &mut CadApp, g: (Handle, usize), shift: bool) {
    let hot = &mut app.canvas.hot_grips;
    if shift {
        match hot.iter().position(|x| *x == g) {
            Some(i) => {
                hot.remove(i);
            }
            None => hot.push(g),
        }
        return;
    }
    if !hot.contains(&g) {
        hot.clear();
    }
    hot.retain(|x| *x != g);
    hot.insert(0, g);
    let hs = hot.clone();
    if let Err(e) = app.session.start_grip_edit(&hs) {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// The multifunctional options of grip `g` (Add Vertex, Lengthen…; empty: Stretch only).
pub(super) fn grip_options(app: &CadApp, (h, i): (Handle, usize)) -> Vec<cadcraft_engine::grips::GripAction> {
    let Ok(st) = app.session.state() else { return Vec::new() };
    st.doc.entity(h).map(|e| cadcraft_engine::grips::grip_actions(&e.kind, i)).unwrap_or_default()
}

/// The menu of a multifunctional grip: Stretch and its options. A choice makes the grip hot and
/// runs the option at the grip prompt, as typing its keyword would.
pub(super) fn grip_menu(app: &mut CadApp, resp: &egui::Response) {
    let Some(g) = app.canvas.grip_menu else { return };
    let options = grip_options(app, g);
    let mut chosen: Option<Option<cadcraft_engine::grips::GripAction>> = None;
    let open = resp
        .context_menu(|ui| {
            let vertex = options.contains(&cadcraft_engine::grips::GripAction::RemoveVertex);
            if ui.button(crate::i18n::t(if vertex { "Stretch Vertex" } else { "Stretch" })).clicked() {
                chosen = Some(None);
                ui.close();
            }
            for a in &options {
                if ui.button(crate::i18n::t(a.label())).clicked() {
                    chosen = Some(Some(*a));
                    ui.close();
                }
            }
        })
        .is_some();
    if !open || chosen.is_some() {
        app.canvas.grip_menu = None;
    }
    let Some(choice) = chosen else { return };
    app.canvas.hot_grips = vec![g];
    let r = app.session.start_grip_edit(&[g]).and_then(|()| match choice {
        Some(a) => app.session.input(Input::Keyword(a.keyword().to_string())),
        None => Ok(()),
    });
    if let Err(e) = r {
        app.canvas.hot_grips.clear();
        app.session.echo(e.to_string());
    }
}

/// Polyline segment midpoint grips are rectangular; vertex grips remain square.
pub(super) fn grip_rect(kind: &cadcraft_doc::EntityKind, count: usize, index: usize, center: Pos2, size: f32) -> Rect {
    // The grip list interleaves vertices and available segment midpoints, then any remaining vertices.
    let midpoint = matches!(kind, cadcraft_doc::EntityKind::LwPolyline(p) if index % 2 == 1 && index / 2 < count.saturating_sub(p.vertices.len()));
    Rect::from_center_size(center, vec2(size * 2.0, if midpoint { size } else { size * 2.0 }))
}

/// The grip of a selected object under the cursor, if any: (object, grip index).
pub(super) fn grip_at(app: &CadApp, xf: &Xf, hover: Option<Pos2>) -> Option<(Handle, usize)> {
    let hp = hover?;
    let st = app.session.state().ok()?;
    let gs = app.session.settings.gripsize as f32;
    let sel = app.session.selection();
    if !app.session.shows_grips(sel.len()) {
        return None;
    }
    for h in sel.iter().take(200) {
        let e = st.doc.entity(*h)?;
        if st.doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
            continue;
        }
        let grips = e.kind.grips();
        let count = grips.len();
        for (i, g) in grips.into_iter().enumerate() {
            let sp = xf.to_screen(g);
            if grip_rect(&e.kind, count, i, sp, gs).expand(3.0).contains(hp) {
                return Some((*h, i));
            }
        }
    }
    None
}

// Cache grip rectangles: pointer movement changes colours, not their geometry.

use std::ops::Range;
use std::sync::Arc;

use cadcraft_doc::Drawing;
use egui::epaint::{RectShape, TessellationOptions, Tessellator};
use egui::{Color32, Mesh, Painter, Shape, Stroke, StrokeKind};

use crate::theme::Tokens;

const MAX_BYTES: usize = 128 << 20;
type Key = (u64, u64, Xf, f32, f32, TessellationOptions);

struct Mark {
    grip: (Handle, usize),
    rect: Rect,
    fill: Range<usize>,
    color: Color32,
}

#[derive(Default)]
pub(super) struct Cache {
    key: Option<Key>,
    selection: Vec<Handle>,
    mesh: Option<Arc<Mesh>>,
    marks: Vec<Mark>,
}

impl Cache {
    pub(super) fn paint(
        &mut self,
        d: &Drawing,
        uid: u64,
        revision: u64,
        sel: &[Handle],
        xf: Xf,
        gs: f32,
        painter: &Painter,
        hot: &[(Handle, usize)],
        hover: Option<Pos2>,
    ) {
        if let Some(mesh) = self.mesh(d, uid, revision, sel, xf, gs, painter.ctx(), hot, hover) {
            painter.add(Shape::Mesh(mesh));
        } else {
            let t = Tokens::get();
            for (grip, rect) in rectangles(d, sel, xf, gs) {
                painter.rect(rect, 0.0, fill(grip, rect, &t, hot, hover), border(), StrokeKind::Middle);
            }
        }
    }

    /// None above the memory budget: use the existing uncached drawing path, keeping all grips.
    fn mesh(
        &mut self,
        d: &Drawing,
        uid: u64,
        revision: u64,
        sel: &[Handle],
        xf: Xf,
        gs: f32,
        ctx: &egui::Context,
        hot: &[(Handle, usize)],
        hover: Option<Pos2>,
    ) -> Option<Arc<Mesh>> {
        let sel = sel.get(..sel.len().min(200)).unwrap_or_default();
        let key = (uid, revision, xf, gs, ctx.pixels_per_point(), ctx.tessellation_options(|o| *o));
        let t = Tokens::get();
        if self.key.as_ref() != Some(&key) || self.selection != sel {
            *self = Self { key: Some(key), selection: sel.to_vec(), ..Self::default() };
            let mut mesh = Mesh::default();
            let mut tess = Tessellator::new(key.4, key.5, [1, 1], Vec::new());
            tess.set_clip_rect(xf.rect);
            for (grip, rect) in rectangles(d, sel, xf, gs) {
                let start = mesh.vertices.len();
                tess.tessellate_rect(&RectShape::new(rect, 0.0, t.grip, border(), StrokeKind::Middle), &mut mesh);
                self.marks.push(Mark { grip, rect, fill: start..mesh.vertices.len(), color: t.grip });
                let bytes = mesh
                    .vertices
                    .capacity()
                    .saturating_mul(std::mem::size_of::<egui::epaint::Vertex>())
                    .saturating_add(mesh.indices.capacity().saturating_mul(std::mem::size_of::<u32>()))
                    .saturating_add(self.marks.capacity().saturating_mul(std::mem::size_of::<Mark>()));
                if bytes > MAX_BYTES {
                    self.marks = Vec::new();
                    return None;
                }
            }
            self.mesh = Some(Arc::new(mesh));
        }
        let mesh = self.mesh.as_mut()?;
        for mark in &mut self.marks {
            let color = fill(mark.grip, mark.rect, &t, hot, hover);
            if color != mark.color {
                for v in Arc::make_mut(mesh).vertices.get_mut(mark.fill.clone()).unwrap_or(&mut []) {
                    if v.color == mark.color {
                        v.color = color;
                    }
                }
                mark.color = color;
            }
        }
        Some(mesh.clone())
    }
}

fn rectangles<'a>(d: &'a Drawing, sel: &'a [Handle], xf: Xf, gs: f32) -> impl Iterator<Item = ((Handle, usize), Rect)> + 'a {
    sel.iter().take(200).filter_map(move |h| Some((*h, d.entity(*h)?))).flat_map(move |(h, e)| {
        let grips = e.kind.grips();
        let count = grips.len();
        grips.into_iter().enumerate().filter_map(move |(i, g)| {
            let p = xf.to_screen(g);
            xf.rect.contains(p).then(|| ((h, i), grip_rect(&e.kind, count, i, p, gs)))
        })
    })
}

fn border() -> Stroke {
    Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80))
}

fn fill(grip: (Handle, usize), rect: Rect, t: &Tokens, hot: &[(Handle, usize)], hover: Option<Pos2>) -> Color32 {
    if hot.contains(&grip) {
        t.grip_hot
    } else if hover.is_some_and(|p| rect.expand(2.0).contains(p)) {
        Color32::from_rgb(0xff, 0x7f, 0x9f)
    } else {
        t.grip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, EntityKind, LwPolyline, Space};
    use cadcraft_geom::{PolyVertex, Vec2};

    fn contour(n: u32) -> (Drawing, Handle) {
        let mut d = Drawing::new_metric();
        let vertices = (0..n).map(|i| PolyVertex::new(Vec2::new(f64::from(i % 100), f64::from(i / 100)))).collect();
        let h = d
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false }),
            )
            .unwrap();
        (d, h)
    }

    fn xf() -> Xf {
        Xf { rect: Rect::from_min_size(Pos2::ZERO, vec2(500.0, 500.0)), center: Vec2::new(50.0, 100.0), scale: 1.0 }
    }

    #[test]
    fn cached_grips_keep_indices_and_match_uncached_hot_and_warm_shapes() {
        let (d, h) = contour(3);
        let mut cache = Cache::default();
        let ctx = egui::Context::default();
        for dpi in [0.75, 1.0, 2.0, 3.0] {
            ctx.set_pixels_per_point(dpi);
            // egui applies a DPI change at the next pass, not when it is requested.
            ctx.begin_pass(Default::default());
            assert_eq!(ctx.pixels_per_point(), dpi);
            for hot in [vec![], vec![(h, 0)], vec![(h, 2), (h, 4)]] {
                let hover = xf().to_screen(Vec2::new(1.0, 0.0));
                let actual = cache.mesh(&d, 1, 1, &[h], xf(), 5.0, &ctx, &hot, Some(hover)).unwrap();
                let mut expected = Mesh::default();
                let mut tess = Tessellator::new(ctx.pixels_per_point(), ctx.tessellation_options(|o| *o), [1, 1], Vec::new());
                tess.set_clip_rect(xf().rect);
                let t = Tokens::get();
                for (i, g) in d.entity(h).unwrap().kind.grips().into_iter().enumerate() {
                    let r = grip_rect(&d.entity(h).unwrap().kind, 5, i, xf().to_screen(g), 5.0);
                    let color = if hot.contains(&(h, i)) {
                        t.grip_hot
                    } else if r.expand(2.0).contains(hover) {
                        Color32::from_rgb(0xff, 0x7f, 0x9f)
                    } else {
                        t.grip
                    };
                    tess.tessellate_shape(
                        Shape::Rect(RectShape::new(r, 0.0, color, Stroke::new(1.0, Color32::from_rgb(0x10, 0x30, 0x80)), StrokeKind::Middle)),
                        &mut expected,
                    );
                }
                assert_eq!(*actual, expected, "dpi {dpi}, hot {hot:?}");
                assert_eq!(cache.marks.iter().map(|m| m.grip).collect::<Vec<_>>(), (0..5).map(|i| (h, i)).collect::<Vec<_>>());
            }
            ctx.end_pass().textures_delta.clear();
        }
    }

    #[test]
    fn dense_grips_reuse_the_mesh_and_invalidate_after_edits_or_view_changes() {
        let (mut d, h) = contour(20_000);
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let first = cache.mesh(&d, 1, 1, &[h], xf(), 5.0, &ctx, &[], None).unwrap();
        assert_eq!(cache.marks.len(), 39_999);
        let same = cache.mesh(&d, 1, 1, &[h], xf(), 5.0, &ctx, &[], None).unwrap();
        assert!(Arc::ptr_eq(&first, &same));
        d.modify_entity(h, |e| {
            if let EntityKind::LwPolyline(p) = &mut e.kind {
                p.vertices[0].p.x += 10.0;
            }
        })
        .unwrap();
        let edited = cache.mesh(&d, 1, 2, &[h], xf(), 5.0, &ctx, &[], None).unwrap();
        assert_ne!(first.vertices, edited.vertices);
        let zoomed = cache.mesh(&d, 1, 2, &[h], Xf { scale: 2.0, ..xf() }, 5.0, &ctx, &[], None).unwrap();
        assert_ne!(edited.vertices, zoomed.vertices);
        let new_document = cache.mesh(&d, 2, 2, &[h], Xf { scale: 2.0, ..xf() }, 5.0, &ctx, &[], None).unwrap();
        assert!(!Arc::ptr_eq(&zoomed, &new_document));
        let empty = cache.mesh(&d, 2, 2, &[], xf(), 5.0, &ctx, &[], None).unwrap();
        assert!(empty.is_empty());
        assert!(cache.marks.is_empty());
    }

    #[test]
    fn oversized_grip_meshes_fall_back_without_retaining_or_rebuilding_them() {
        let (d, h) = contour(200_000);
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let view = Xf { scale: 0.1, center: Vec2::new(50.0, 1000.0), ..xf() };
        assert!(cache.mesh(&d, 1, 1, &[h], view, 5.0, &ctx, &[], None).is_none());
        assert!(cache.marks.is_empty());
        assert!(cache.key.is_some());
        assert!(cache.mesh(&d, 1, 1, &[h], view, 5.0, &ctx, &[], None).is_none());
        assert!(cache.mesh(&d, 1, 1, &[], view, 5.0, &ctx, &[], None).unwrap().is_empty());
    }
}
