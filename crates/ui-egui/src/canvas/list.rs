use super::viewport::{split_viewport, stencil_clip};
use super::{Bounds2, CadApp, DisplayList, Handle, ListKey, Rgb, Tokens, Xf, rebuild, same_content};

/// Primitives of `list` sorted by entity handle, built on the first highlight after each list
/// rebuild. Highlighting the hovered entity used to scan every primitive on every frame: ~2.5 ms
/// per frame on a 270k-entity drawing, whether or not anything changed.
#[derive(Default)]
pub(super) struct PrimIndex {
    list_gen: Option<u64>,
    by_handle: Vec<(Handle, usize)>,
}

impl PrimIndex {
    /// Indices of the primitives drawn for `handles` (in list order).
    pub(super) fn prims(&mut self, list_gen: u64, list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
        if handles.is_empty() {
            return Vec::new();
        }
        if self.list_gen != Some(list_gen) {
            self.by_handle = list.prims.iter().enumerate().map(|(i, p)| (p.handle, i)).collect();
            self.by_handle.sort_unstable();
            self.list_gen = Some(list_gen);
        }
        let mut out = Vec::new();
        for h in handles {
            let from = self.by_handle.partition_point(|(k, _)| k < h);
            out.extend(self.by_handle.get(from..).unwrap_or(&[]).iter().take_while(|(k, _)| k == h).map(|(_, i)| *i));
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Indices of the primitives in `list` drawn for `handles` (a scan: the reference [`PrimIndex`]
/// is tested against).
#[cfg(test)]
pub(super) fn prims_of(list: &DisplayList, handles: &[Handle]) -> Vec<usize> {
    if handles.is_empty() {
        return Vec::new();
    }
    let set: std::collections::HashSet<Handle> = handles.iter().copied().collect();
    list.prims.iter().enumerate().filter(|(_, pr)| set.contains(&pr.handle)).map(|(i, _)| i).collect()
}

/// Build options for previews at `scale` pixels per drawing unit (opaque).
pub(super) fn preview_options(scale: f64) -> cadcraft_render::Options {
    cadcraft_render::Options { tolerance: 0.5 / scale, min_dash: 2.0 / scale, ..Default::default() }
}

/// The display list of preview entities the session builds with the current properties
/// ([`cadcraft_engine::Session::preview`], the extras of [`cadcraft_engine::Session::drag`]): the
/// command's rubber band and rebuilt copies of existing objects. They carry CLAYER's properties,
/// not the objects', so they are drawn opaque (the GPU drag of the objects themselves keeps their
/// transparency, see [`draw_drag_gpu`]).
pub(super) fn rebuilt_preview<'a>(doc: &cadcraft_doc::Drawing, ents: impl IntoIterator<Item = &'a cadcraft_doc::Entity>, scale: f64) -> DisplayList {
    cadcraft_render::build_entities(doc, ents, &preview_options(scale))
}

/// Rebuild the display list when needed. `xf`: the current view; `views`: every area on screen
/// that shows the list (the current view, other model tiles) with its drawing units per pixel.
///
/// A rebuild only the view asks for (a new zoom band, a pan out of the built area) runs in the
/// background while the current list is drawn (see [`rebuild`]); a changed drawing, or no list
/// yet, is built at once.
pub(super) fn ensure_list(app: &mut CadApp, xf: &Xf, views: &[(Bounds2, f64)], ctx: &egui::Context) {
    let px = 1.0 / xf.scale;
    let Ok(st) = app.session.state() else { return };
    // Rebuild when the drawing changes, the zoom moves by more than 2x (tessellation band) or a
    // view leaves the area the list was built for.
    let band = |px: f64| px.max(1e-300).log2().floor() as i32;
    // Hatch level of detail follows the finest view showing the list.
    let lod_band = views.iter().map(|v| band(v.1)).fold(band(px), i32::min);
    // The active MSPACE viewport's contents are drawn apart (see `VpContent`), so panning and
    // zooming inside it leave this list alone. Without a stencil buffer a clipped viewport's
    // contents stay in the list, which then follows its view: realtime navigation changes it
    // without a new revision.
    let split = split_viewport(st, stencil_clip(app)).map(|(h, ..)| h);
    let vp_view = {
        use std::hash::{Hash, Hasher};
        let mut s = std::collections::hash_map::DefaultHasher::new();
        split.hash(&mut s);
        if split.is_none()
            && let Some((h, vp)) = st.active_viewport()
        {
            (h, vp.view_center.x.to_bits(), vp.view_center.y.to_bits(), vp.view_height.to_bits()).hash(&mut s);
        }
        s.finish()
    };
    let (lwdisplay, transparency_display) = (app.session.settings.lwdisplay, app.session.settings.transparency_display);
    let key: ListKey = (st.uid, st.revision, band(px), lod_band, lwdisplay, transparency_display, std::sync::Arc::as_ptr(&st.doc) as usize, vp_view);
    // A background build that finished: use it unless the drawing has changed since it started
    // (or the list was dropped to force a rebuild).
    if let Some(job) = &app.canvas.job {
        match job.poll() {
            rebuild::Poll::Pending => {}
            rebuild::Poll::Done(built) => {
                let (job_key, window) = (job.key, job.window);
                app.canvas.job = None;
                if app.canvas.list.is_some() && same_content(&job_key, &key) {
                    app.canvas.install(job_key, window, *built);
                }
            }
            rebuild::Poll::Lost => {
                log::warn!("canvas: a background rebuild failed; rebuilding on the UI thread");
                app.canvas.job = None;
                app.canvas.list = None;
            }
        }
    }
    let visible = views.iter().fold(Bounds2::EMPTY, |acc, v| acc.union(&v.0));
    let inside = app.canvas.window.is_none_or(|w| visible.is_empty() || w.contains_box(&visible));
    if app.canvas.list.is_some() && app.canvas.key == key && inside {
        return;
    }
    // Only the view moved: keep drawing the current list and build the new one in the background.
    let view_only = app.canvas.list.is_some() && same_content(&app.canvas.key, &key);
    if view_only && app.canvas.job.as_ref().is_some_and(|j| j.serves(&key, &visible)) {
        return;
    }
    // A build for another view, or for a drawing that has since changed, is no longer wanted:
    // dropping it cancels it.
    app.canvas.job = None;
    let t0 = crate::now_ms();
    let opts = cadcraft_render::Options {
        tolerance: 2f64.powi(band(px)) * 0.5,
        // Two pixels at the band's finest zoom: the list serves every zoom in the band, so its
        // level of detail must hold at the finest one.
        min_dash: 2f64.powi(lod_band) * 2.0,
        text: true,
        fill: true,
        lineweights: lwdisplay,
        view_height: st.paper_view().height,
        plot_style_table: None,
        view: build_window(views),
        transparency: app.session.settings.transparency_display,
        skip_viewport: split,
        cancel: None,
    };
    if view_only {
        let t = Tokens::get();
        let mesh = app.canvas.gpu.is_some().then(|| rebuild::MeshRequest {
            view_center: xf.center,
            view_height: f64::from(xf.rect.height()) / xf.scale.max(1e-300),
            model_bg: Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b()),
            paper_bg: Rgb(t.paper.r(), t.paper.g(), t.paper.b()),
            lwdisplay,
            device: app.canvas.gpu_device.clone(),
        });
        let window = opts.view;
        let req = rebuild::Request { doc: st.doc.clone(), space: st.space.clone(), opts: opts.clone(), mesh };
        if let Some(job) = rebuild::spawn(req, key, window, ctx) {
            app.canvas.job = Some(job);
            return;
        }
    }
    let space = st.space.clone();
    app.canvas.list = Some(cadcraft_render::build(&st.doc, &space, &opts));
    app.canvas.key = key;
    app.canvas.window = opts.view;
    app.canvas.list_gen = app.canvas.list_gen.wrapping_add(1);
    app.canvas.build_ms = crate::now_ms() - t0;
}

/// The area a display list is built for: each view with half its larger side of margin all
/// round (panning by up to that much doesn't rebuild), joined. `None` (everything) when a view
/// isn't usable.
fn build_window(views: &[(Bounds2, f64)]) -> Option<Bounds2> {
    let mut w = Bounds2::EMPTY;
    for (v, _) in views {
        let side = v.width().max(v.height());
        if v.is_empty() || !side.is_finite() {
            return None;
        }
        w = w.union(&v.expand(side * 0.5));
    }
    (!w.is_empty()).then_some(w)
}
