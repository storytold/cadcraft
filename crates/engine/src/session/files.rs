use super::{DocState, Drawing, EngineError, Result, Session, Space, Value, Vec2, View, cmd};

impl Session {
    /// Append a new untitled drawing (`DrawingN`, metric or imperial default view), end a running command, make it active, bump the document epoch and return its index.
    pub fn new_drawing(&mut self, metric: bool) -> usize {
        self.untitled_counter += 1;
        let d = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        let mut st = DocState::new(d, &format!("Drawing{}", self.untitled_counter), None);
        st.set_view_quiet(if metric { View { center: Vec2::new(210.0, 148.5), height: 297.0 } } else { View::default() });
        self.end_running();
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.bump_doc_epoch();
        self.active
    }

    /// Append a loaded drawing, fit the view to its model extents (aspect-corrected, minimum height 1e-6) when non-empty, end a running command, make it active, bump the epoch and return its index.
    pub fn open_drawing(&mut self, d: Drawing, title: &str, path: Option<String>) -> usize {
        let mut st = DocState::new(d, title, path);
        let ext = st.doc.extents(&Space::Model);
        if !ext.is_empty() {
            st.set_view_quiet(View {
                center: ext.center(),
                height: (ext.height().max(ext.width() * self.viewport_px.1 / self.viewport_px.0.max(1.0)) * 1.1).max(1e-6),
            });
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
                self.active = self.active.min(self.docs.len().saturating_sub(1));
                self.bump_doc_epoch();
                Err(EngineError::Internal("open".into(), "panic".into()))
            }
        }
    }
}
