use super::{DocState, Drawing, EngineError, Result, Session, Space, Value, Vec2, View, cmd};

impl Session {
    /// Append a new untitled drawing (`DrawingN`, metric or imperial default view), end a running command, make it active, bump the document epoch and return its index.
    pub fn new_drawing(&mut self, metric: bool) -> usize {
        self.untitled_counter += 1;
        let d = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        let mut st = DocState::new(d, &format!("Drawing{}", self.untitled_counter), None);
        st.set_view_quiet(if metric { View { center: Vec2::new(210.0, 148.5), height: 297.0, twist: 0.0 } } else { View::default() });
        self.end_running();
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.bump_doc_epoch();
        self.active
    }

    /// Append a loaded drawing, fit the view to its model extents (aspect-corrected, minimum height 1e-6) when non-empty, end a running command, make it active, bump the epoch and return its index.
    pub fn open_drawing(&mut self, d: Drawing, title: &str, path: Option<String>) -> usize {
        let mut st = DocState::new(d, title, path);
        // The view the file was saved at is the one every other reader opens on. Fitting the
        // extents instead showed a drawing whose extents are dominated by far-off geometry at a
        // view hundreds of times its content, with the drawing sub-pixel and no way to zoom in
        // (#1602). The extents remain the fallback for a drawing that stores no view.
        let saved = crate::view::saved_view(&st.doc);
        let ext = st.doc.extents(&Space::Model);
        // The current tile (the first), or the lone untiled view, keeps its saved twist.
        let twist = cadcraft_geom::norm_angle(st.doc.vports.iter().find(|c| c.is_active()).and_then(|c| c.tiles.first()).map_or(0.0, |t| t.twist));
        if let Some(v) = saved {
            st.set_view_quiet(View { twist, ..v });
        } else if ext.is_empty() {
            st.set_view_quiet(View { twist, ..st.view() });
        } else {
            let (center, w, h) = View::twisted_box(ext, twist);
            st.set_view_quiet(View { center, height: (h.max(w * self.viewport_px.1 / self.viewport_px.0.max(1.0)) * 1.1).max(1e-6), twist });
        }
        self.end_running();
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.bump_doc_epoch();
        self.active
    }

    /// Record that which drawing is open or active changed (screenshot binding).
    pub fn bump_doc_epoch(&mut self) {
        self.doc_epoch = self.doc_epoch.wrapping_add(1);
    }

    /// A command still waiting for input belongs to the drawing it started in: end it there
    /// before another drawing becomes current, so its undo step lands in that drawing.
    fn end_running(&mut self) {
        if self.running.is_some() {
            self.cancel();
        }
    }

    /// Add a file read off-thread with [`cmd::file::read_file`] as the active drawing (OPEN's fast
    /// half). A panic while adding it is reported as an error, never propagated.
    pub fn finish_open(&mut self, f: cmd::file::ReadFile) -> Result<Value> {
        let n = self.docs.len();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd::file::finish_open(self, f))) {
            Ok(r) => r,
            Err(_) => {
                self.docs.truncate(n);
                self.lisp_forget_closed();
                self.active = self.active.min(self.docs.len().saturating_sub(1));
                self.bump_doc_epoch();
                Err(EngineError::Internal("open".into(), "panic".into()))
            }
        }
    }

    /// Explicitly take over an existing `.dwl`/`.dwl2` lock for an open read-only drawing and
    /// switch it to writable mode (for example after another session crashed).
    pub fn take_over_lock(&mut self, index: usize) -> Result<()> {
        let Some(st) = self.docs.get(index) else {
            return Err(EngineError::Other("no such drawing".into()));
        };
        if !st.read_only {
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let target_path = st.path.clone().filter(|p| cadcraft_doc::Format::from_path(p) == Some(cadcraft_doc::Format::Dwg));

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = target_path {
            let target = std::path::Path::new(&p);
            // If another tab in this same session currently holds the lock for this file, release
            // its guard and demote it to read-only first so closing it later cannot delete our new lock.
            for (j, other) in self.docs.iter_mut().enumerate() {
                if j != index
                    && other.lock.is_some()
                    && other.path.as_deref().is_some_and(|op| crate::drawing_lock::same_path(std::path::Path::new(op), target))
                {
                    other.lock = None;
                    other.read_only = true;
                    other.lock_info = Some("lock transferred to another open tab".into());
                    other.lock_holder = None;
                }
            }
            let result = crate::drawing_lock::force_acquire_lock(target);
            let st = &mut self.docs[index];
            match result {
                crate::drawing_lock::LockResult::Acquired(guard) => {
                    st.lock = Some(std::sync::Arc::new(guard));
                    st.read_only = false;
                    st.lock_info = None;
                    st.lock_holder = None;
                    self.bump_doc_epoch();
                    return Ok(());
                }
                crate::drawing_lock::LockResult::AlreadyLocked(info) => {
                    let desc = info.description();
                    st.lock_info = Some(desc.clone());
                    st.lock_holder = Some(info);
                    return Err(EngineError::Other(format!("file is still locked ({desc})")));
                }
                crate::drawing_lock::LockResult::Failed(err) => {
                    return Err(EngineError::Other(err));
                }
            }
        }
        let st = &mut self.docs[index];
        st.read_only = false;
        st.lock_info = None;
        st.lock_holder = None;
        self.bump_doc_epoch();
        Ok(())
    }
}
