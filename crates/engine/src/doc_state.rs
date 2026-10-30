use super::view::{ModelTiles, View, usable_view};
use super::{Arc, Drawing, Handle, Space, blockedit, undoctl};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub label: String,
    pub doc: Arc<Drawing>,
    pub selection: Vec<Handle>,
}

/// One open drawing.
#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Drawing>,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    /// UNDO marks, BEgin/End group and Control (see [`undoctl`]).
    pub undo_ctl: undoctl::UndoCtl,
    pub selection: Vec<Handle>,
    pub previous_selection: Vec<Handle>,
    pub path: Option<String>,
    pub title: String,
    pub saved: Arc<Drawing>,
    pub revision: u64,
    pub space: Space,
    /// Tiled model viewports (VPORTS).
    pub tiles: ModelTiles,
    /// MSPACE: the layout viewport whose model space is being edited (None = paper space).
    pub mspace: Option<Handle>,
    pub views: Vec<(Space, View)>,
    pub view_history: Vec<View>,
    pub uid: u64,
    /// The drawing is saved as binary DXF (it was opened from one, or last saved as one).
    pub binary_dxf: bool,
    /// The Block Editor (BEDIT), when it is open: `doc` is then the editor's drawing, and the
    /// drawing itself waits in it ([`DocState::drawing`]).
    pub block_edit: Option<blockedit::BlockEdit>,
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl DocState {
    pub fn new(doc: Drawing, title: &str, path: Option<String>) -> Self {
        let doc = Arc::new(doc);
        DocState {
            saved: doc.clone(),
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_ctl: undoctl::UndoCtl::default(),
            selection: Vec::new(),
            previous_selection: Vec::new(),
            path,
            title: title.into(),
            revision: 1,
            space: Space::Model,
            tiles: ModelTiles::default(),
            mspace: None,
            views: Vec::new(),
            view_history: Vec::new(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            binary_dxf: false,
            block_edit: None,
        }
    }
    /// The drawing has changes not saved to its file, or the Block Editor has changes not yet
    /// saved into its block.
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(self.drawing(), &self.saved) || self.block_unsaved()
    }
    /// After undo/redo replaced the drawing (`before` is the one shown until then): if the current
    /// layout no longer exists, follow it to its other name when it was renamed (same tab order,
    /// a name `before` did not have), else go to the Model tab.
    pub(crate) fn follow_layout(&mut self, before: &Drawing) {
        let Space::Paper(name) = &self.space else { return };
        if self.doc.layouts.iter().any(|l| l.name == *name) {
            return;
        }
        let renamed = before
            .layouts
            .iter()
            .find(|l| l.name == *name)
            .and_then(|old| self.doc.layouts.iter().find(|l| l.tab_order == old.tab_order && !before.layouts.iter().any(|b| b.name == l.name)));
        match renamed.map(|l| Space::Paper(l.name.clone())) {
            Some(to) => {
                let from = self.space.clone();
                for (sp, _) in self.views.iter_mut() {
                    if *sp == from {
                        *sp = to.clone();
                    }
                }
                self.space = to;
            }
            None => {
                self.space = Space::Model;
                self.mspace = None;
            }
        }
    }
    /// The space edits, picks and snaps act on: model space inside an active viewport.
    pub fn edit_space(&self) -> Space {
        if self.active_viewport().is_some() { Space::Model } else { self.space.clone() }
    }
    /// The active MSPACE viewport, if it still exists in the current layout.
    pub fn active_viewport(&self) -> Option<(Handle, cadcraft_doc::Viewport)> {
        let h = self.mspace?;
        let e = self.doc.space(&self.space)?.get(h)?;
        match &e.kind {
            cadcraft_doc::EntityKind::Viewport(v) if v.height > 1e-12 && v.view_height > 1e-12 => Some((h, v.clone())),
            _ => None,
        }
    }
    pub fn paper_view(&self) -> View {
        self.views.iter().find(|(s, _)| *s == self.space).map(|(_, v)| *v).unwrap_or_default()
    }
    /// The screen view. Inside an MSPACE viewport this is the model-space view that maps the
    /// screen consistently with the paper view, so picks, snaps and previews work in model units.
    pub fn view(&self) -> View {
        let pv = self.paper_view();
        match self.active_viewport() {
            Some((_, vp)) => {
                let k = vp.view_height / vp.height;
                View { center: vp.view_center + (pv.center - vp.center.xy()) * k, height: pv.height * k }
            }
            None => pv,
        }
    }
    /// Map a requested screen view to the paper view, or (in MSPACE) to the viewport's model view.
    /// A finite request can still map to a view too large to represent: then nothing changes and
    /// this returns false.
    fn store_view(&mut self, v: View) -> bool {
        if let Some((h, vp)) = self.active_viewport() {
            let pv = self.paper_view();
            let k = v.height / pv.height.max(1e-12);
            if vp.locked {
                // A locked viewport keeps its scale: pan/zoom the sheet instead.
                let k0 = vp.view_height / vp.height;
                let pc = vp.center.xy() + (v.center - vp.view_center) / k0;
                let paper = View { center: pc, height: v.height / k0 };
                if !usable_view(&paper) {
                    return false;
                }
                self.put_paper_view(paper);
            } else {
                let center = v.center - (pv.center - vp.center.xy()) * k;
                let height = vp.height * k;
                if !usable_view(&View { center, height }) {
                    return false;
                }
                let doc = Arc::make_mut(&mut self.doc);
                if let Some(store) = doc.space_mut(&self.space.clone()) {
                    store.modify(h, |e| {
                        if let cadcraft_doc::EntityKind::Viewport(x) = &mut e.kind {
                            x.view_center = center;
                            x.view_height = height;
                        }
                    });
                }
            }
            return true;
        }
        self.put_paper_view(v);
        true
    }
    fn put_paper_view(&mut self, v: View) {
        match self.views.iter_mut().find(|(s, _)| *s == self.space) {
            Some((_, slot)) => *slot = v,
            None => self.views.push((self.space.clone(), v)),
        }
    }
    /// Set the view, recording view history. Returns false, changing nothing, when the view (or,
    /// in MSPACE, the viewport view it maps to) isn't finite with a positive height.
    pub fn set_view(&mut self, v: View) -> bool {
        if !usable_view(&v) {
            return false;
        }
        let old = self.view();
        if !self.store_view(v) {
            return false;
        }
        if old != v {
            self.view_history.push(old);
            if self.view_history.len() > 50 {
                self.view_history.remove(0);
            }
        }
        true
    }
    /// The active MSPACE viewport when ZOOM works on its own view, as in AutoCAD: extents and
    /// windows fit the viewport, and zooms keep its centre. A locked viewport keeps its scale, so
    /// zooming it moves the sheet instead (see `store_view`).
    pub fn zoom_viewport(&self) -> Option<cadcraft_doc::Viewport> {
        self.active_viewport().map(|(_, v)| v).filter(|v| !v.locked && v.width > 1e-12)
    }
    /// The view ZOOM starts from: the model view of the active unlocked viewport, else [`Self::view`].
    pub fn zoom_frame(&self) -> View {
        match self.zoom_viewport() {
            Some(v) => View { center: v.view_center, height: v.view_height },
            None => self.view(),
        }
    }
    /// Width / height of the area ZOOM fits into: the active unlocked viewport, else `screen`.
    pub fn zoom_aspect(&self, screen: f64) -> f64 {
        self.zoom_viewport().map_or(screen, |v| v.width / v.height)
    }
    /// Set the view ZOOM works on (see [`Self::zoom_frame`]), recording view history.
    pub fn set_zoom_frame(&mut self, f: View) {
        let v = match self.zoom_viewport() {
            // The screen view that `store_view` maps to this model view of the viewport.
            Some(vp) => {
                let pv = self.paper_view();
                let k = f.height / vp.height;
                View { center: f.center + (pv.center - vp.center.xy()) * k, height: pv.height * k }
            }
            None => f,
        };
        self.set_view(v);
    }
    /// Set without recording view history (realtime pan/zoom frames).
    pub fn set_view_quiet(&mut self, v: View) {
        if !(v.center.is_finite() && v.height.is_finite() && v.height > 1e-12) {
            return;
        }
        self.store_view(v);
    }
}
