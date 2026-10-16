//! Display-list rebuilds that only follow the view run on a worker thread, as AutoCAD redraws
//! instead of regenerating while you zoom and pan.
//!
//! Zooming past a 2x tessellation band or panning out of the area the list was built for used to
//! rebuild the list (and convert it to GPU vertex data) on the UI thread: on a hatched 12k-entity
//! drawing each step froze the window for 2-3 s. The current list stays valid at any view (only
//! hatch patterns are limited to its area, and curves and dashes are a band coarser or finer than
//! ideal), so the canvas keeps drawing it, scaled and moved by the view uniform, while the worker
//! builds the new one. A view that needs a different list cancels the build in flight (dropping
//! its [`Job`] sets the build's cancel flag, see `cadcraft_render::Options::cancel`) and starts
//! one for itself, so a long scroll through many zoom bands builds only for where it stops.
//! Edits still rebuild at once, so a change is never drawn late. The web build has no threads and
//! rebuilds synchronously.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::{DisplayList, Kind, Options};

/// What a build needs: the drawing (shared, never copied), the space and the options.
pub(super) struct Request {
    pub doc: Arc<Drawing>,
    pub cache: cadcraft_render::screen::Cache,
    pub space: Space,
    pub opts: Options,
    /// Also convert the list to GPU vertex data (GPU canvas only).
    pub mesh: Option<MeshRequest>,
}

/// How to convert the list for the GPU: the view the vertex origin is chosen for and the
/// background the colours resolve against (paper when the list is a layout's sheet).
pub(super) struct MeshRequest {
    pub view_center: Vec2,
    pub view_height: f64,
    pub model_bg: Rgb,
    pub paper_bg: Rgb,
    pub lwdisplay: bool,
    /// Create the GPU buffers on the worker too, so the frame that swaps the list in only swaps
    /// buffers (`None`: the paint callback uploads them).
    pub device: Option<egui_wgpu::wgpu::Device>,
}

/// A finished build.
pub(super) struct Built {
    pub list: DisplayList,
    pub cache: cadcraft_render::screen::Cache,
    pub parts: Vec<cadcraft_render::screen::Part>,
    pub rebuilt: usize,
    pub reused: usize,
    pub build_ms: f64,
    pub mesh: Option<BuiltMesh>,
}

pub(super) struct BuiltMesh {
    pub data: crate::gpu::MeshData,
    pub origin: Vec2,
    pub bg: Rgb,
    pub lwdisplay: bool,
    /// Indices of the infinite lines (drawn on the CPU in GPU mode).
    pub infinite: Vec<usize>,
    pub ms: f64,
}

/// A build in flight: the list key and area it is for, and where its result arrives. Dropping
/// it cancels the build.
pub(super) struct Job {
    pub key: super::ListKey,
    pub window: Option<Bounds2>,
    rx: Receiver<Built>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// What [`Job::poll`] found.
pub(super) enum Poll {
    Pending,
    Done(Box<Built>),
    /// The worker died without a result (it panicked).
    Lost,
}

impl Job {
    pub fn poll(&self) -> Poll {
        match self.rx.try_recv() {
            Ok(b) => Poll::Done(Box::new(b)),
            Err(TryRecvError::Empty) => Poll::Pending,
            Err(TryRecvError::Disconnected) => Poll::Lost,
        }
    }

    /// Whether this build serves list `key` for the visible area `visible`.
    pub fn serves(&self, key: &super::ListKey, visible: &Bounds2) -> bool {
        self.key == *key && self.window.is_none_or(|w| visible.is_empty() || w.contains_box(visible))
    }
}

/// Build `req` on the calling thread; `None` once it has been cancelled (the partial list is
/// thrown away, and no vertex data is made for it).
fn run(mut req: Request) -> Option<Built> {
    let t0 = crate::now_ms();
    let built = req.cache.build(&req.doc, &req.space, &req.opts);
    let list = built.list;
    if req.opts.cancelled() {
        return None;
    }
    let build_ms = crate::now_ms() - t0;
    let mesh = match req.mesh {
        None => None,
        Some(m) => {
            let t1 = crate::now_ms();
            // Match the drawing callback's stable origin, so installing a worker result does
            // not immediately convert and upload the same scene again on the UI thread.
            let bounds = req.opts.view.as_ref().filter(|_| !built.parts.is_empty()).unwrap_or(&list.bounds);
            let origin = crate::gpu::choose_origin(bounds, m.view_center, m.view_height);
            let bg = if list.sheet.is_some() { m.paper_bg } else { m.model_bg };
            let mut data = crate::gpu::build_mesh(&list, origin, bg, m.lwdisplay);
            if req.opts.cancelled() {
                return None;
            }
            if let Some(device) = &m.device {
                data.upload_now(device);
            }
            let infinite = list.prims.iter().enumerate().filter(|(_, p)| matches!(p.kind, Kind::Infinite { .. })).map(|(i, _)| i).collect();
            Some(BuiltMesh { data, origin, bg, lwdisplay: m.lwdisplay, infinite, ms: crate::now_ms() - t1 })
        }
    };
    Some(Built { list, build_ms, mesh, cache: req.cache, parts: built.parts, rebuilt: built.rebuilt, reused: built.reused })
}

/// Start building `req` on a worker thread for `key` / `window`; the worker asks `ctx` for a
/// frame when it is done. `None` when there are no threads (the web build) or none could be
/// started: the caller builds synchronously.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn spawn(mut req: Request, key: super::ListKey, window: Option<Bounds2>, ctx: &egui::Context) -> Option<Job> {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    req.opts.cancel = Some(cancel.clone());
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("cadcraft-regen".into()).spawn(move || {
        // A panic drops `tx` unsent: the canvas sees the job as lost and builds on its own. A
        // cancelled build sends nothing either, but its job is gone and nobody is listening.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(req))) {
            Ok(None) => {}
            Ok(Some(built)) => {
                let _ = tx.send(built);
                ctx.request_repaint();
            }
            Err(_) => {
                drop(tx);
                ctx.request_repaint();
            }
        }
    });
    match spawned {
        Ok(_) => Some(Job { key, window, rx, cancel }),
        Err(e) => {
            log::warn!("canvas: could not start a regen thread ({e}); rebuilding on the UI thread");
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn spawn(_req: Request, _key: super::ListKey, _window: Option<Bounds2>, _ctx: &egui::Context) -> Option<Job> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_build_matches_a_synchronous_one() {
        use cadcraft_geom::Vec3;
        let mut d = Drawing::new_imperial();
        let line = cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) };
        let Ok(_) = d.add(&Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Line(line)) else { panic!("add") };
        let doc = Arc::new(d);
        let opts = Options::default();
        let sync = cadcraft_render::build(&doc, &Space::Model, &opts);
        let req = Request {
            cache: Default::default(),
            doc: doc.clone(),
            space: Space::Model,
            opts: opts.clone(),
            mesh: Some(MeshRequest {
                view_center: Vec2::ZERO,
                view_height: 20.0,
                model_bg: Rgb(0, 0, 0),
                paper_bg: Rgb(255, 255, 255),
                lwdisplay: false,
                device: None,
            }),
        };
        let ctx = egui::Context::default();
        let Some(job) = spawn(req, Default::default(), None, &ctx) else { panic!("no worker") };
        let built = loop {
            match job.poll() {
                Poll::Done(b) => break b,
                Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(2)),
                Poll::Lost => panic!("worker lost"),
            }
        };
        assert_eq!(built.list.prims.len(), sync.prims.len());
        assert_eq!(built.list.verts, sync.verts);
        let Some(mesh) = built.mesh else { panic!("no mesh") };
        assert_eq!(mesh.bg, Rgb(0, 0, 0));
        assert!(mesh.data.line_vertices() >= 2);
    }

    #[test]
    fn cancelled_run_makes_nothing() {
        let opts = Options { cancel: Some(Arc::new(AtomicBool::new(true))), ..Default::default() };
        let req = Request { cache: Default::default(), doc: Arc::new(Drawing::new_imperial()), space: Space::Model, opts, mesh: None };
        assert!(run(req).is_none());
    }

    #[test]
    fn dropping_a_job_cancels_its_build() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        drop(Job { key: Default::default(), window: None, rx, cancel: cancel.clone() });
        assert!(cancel.load(Ordering::Relaxed));
    }
}
