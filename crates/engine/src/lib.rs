//! The CADCraft engine.
//!
//! Every user-visible action is a command with a stable id (AutoCAD's command names in lower
//! case: `line`, `circle`, `zoom`, `layer`…, plus dotted ids for UI operations). Commands run two
//! ways: programmatically with JSON parameters ([`Session::execute`], no dialogs, used by the
//! control channel, MCP, CLI and scripts) and interactively through prompts ([`Session::start`],
//! [`Session::input`], [`Session::cmdline`]) exactly like typing at AutoCAD's command line.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod assoc;
pub mod cmd;
pub mod grips;
pub mod prompt;
pub mod sample;
pub mod select;
pub mod snap;
pub mod spatial;
pub mod sysvars;
pub mod units;

use std::sync::Arc;

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Space};
use cadcraft_geom::{Bounds2, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use cadcraft_doc as doc;
pub use cadcraft_geom as geom;
pub use cadcraft_render as render;
pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use prompt::{Accept, Input, Interactive, Prompt, Step};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no active document")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    /// A command panicked; the guard kept the drawing as it was (a bug: please report it).
    #[error("internal error in `{0}` (the drawing was kept as it was): {1}")]
    Internal(String, String),
}

impl From<cadcraft_doc::DocError> for EngineError {
    fn from(e: cadcraft_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// A 2D view of a space: world centre and visible height in drawing units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub center: Vec2,
    pub height: f64,
}

impl Default for View {
    fn default() -> Self {
        View { center: Vec2::new(6.0, 4.5), height: 10.0 }
    }
}

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
    pub selection: Vec<Handle>,
    pub previous_selection: Vec<Handle>,
    pub path: Option<String>,
    pub title: String,
    pub saved: Arc<Drawing>,
    pub revision: u64,
    pub space: Space,
    /// MSPACE: the layout viewport whose model space is being edited (None = paper space).
    pub mspace: Option<Handle>,
    pub views: Vec<(Space, View)>,
    pub view_history: Vec<View>,
    pub uid: u64,
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
            selection: Vec::new(),
            previous_selection: Vec::new(),
            path,
            title: title.into(),
            revision: 1,
            space: Space::Model,
            mspace: None,
            views: Vec::new(),
            view_history: Vec::new(),
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.doc, &self.saved)
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
    fn store_view(&mut self, v: View) {
        if let Some((h, vp)) = self.active_viewport() {
            let pv = self.paper_view();
            let k = v.height / pv.height.max(1e-12);
            if vp.locked {
                // A locked viewport keeps its scale: pan/zoom the sheet instead.
                let k0 = vp.view_height / vp.height;
                let pc = vp.center.xy() + (v.center - vp.view_center) / k0;
                self.put_paper_view(View { center: pc, height: v.height / k0 });
            } else {
                let center = v.center - (pv.center - vp.center.xy()) * k;
                let height = vp.height * k;
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
            return;
        }
        self.put_paper_view(v);
    }
    fn put_paper_view(&mut self, v: View) {
        match self.views.iter_mut().find(|(s, _)| *s == self.space) {
            Some((_, slot)) => *slot = v,
            None => self.views.push((self.space.clone(), v)),
        }
    }
    pub fn set_view(&mut self, v: View) {
        if !(v.center.is_finite() && v.height.is_finite() && v.height > 1e-12) {
            return;
        }
        let old = self.view();
        if old != v {
            self.view_history.push(old);
            if self.view_history.len() > 50 {
                self.view_history.remove(0);
            }
        }
        self.store_view(v);
    }
    /// Set without recording view history (realtime pan/zoom frames).
    pub fn set_view_quiet(&mut self, v: View) {
        if !(v.center.is_finite() && v.height.is_finite() && v.height > 1e-12) {
            return;
        }
        self.store_view(v);
    }
}

/// Session-level drafting settings (the AutoCAD system variables stored in the registry/profile).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub osmode: u32,
    pub orthomode: bool,
    pub polarmode: bool,
    /// POLARANG (radians).
    pub polarang: f64,
    pub gridmode: bool,
    pub snapmode: bool,
    pub snapunit: Vec2,
    pub gridunit: Vec2,
    /// Major grid line every N minor lines (GRIDMAJOR).
    pub gridmajor: u32,
    pub dynmode: bool,
    pub lwdisplay: bool,
    pub transparency_display: bool,
    pub selection_cycling: bool,
    pub otrack: bool,
    /// PICKBOX in pixels.
    pub pickbox: f64,
    /// APERTURE in pixels.
    pub aperture: f64,
    pub pickfirst: bool,
    pub pickadd: bool,
    pub gripsize: f64,
    pub cursorsize: f64,
    pub isodraft: bool,
    pub annoallvisible: bool,
    pub annoautoscale: bool,
    /// QPMODE: show the Quick Properties palette when objects are selected.
    pub qpmode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            osmode: 4133,
            orthomode: false,
            polarmode: true,
            polarang: std::f64::consts::FRAC_PI_2,
            gridmode: true,
            snapmode: false,
            snapunit: Vec2::new(0.5, 0.5),
            gridunit: Vec2::new(0.5, 0.5),
            gridmajor: 5,
            dynmode: true,
            lwdisplay: false,
            transparency_display: false,
            selection_cycling: false,
            otrack: true,
            pickbox: 3.0,
            aperture: 10.0,
            pickfirst: true,
            pickadd: true,
            gripsize: 5.0,
            cursorsize: 5.0,
            isodraft: false,
            annoallvisible: true,
            annoautoscale: false,
            qpmode: false,
        }
    }
}

/// An interactive command in progress.
pub struct Running {
    pub id: String,
    pub machine: Box<dyn Interactive>,
    pub before: Arc<Drawing>,
    pub selection_before: Vec<Handle>,
}

/// A pending window/crossing selection started by a click on empty space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PendingWindow {
    pub corner: Vec2,
    pub during_command: bool,
}

/// The editor session: open drawings, settings, the running command and the command log.
pub struct Session {
    pub docs: Vec<DocState>,
    pub active: usize,
    pub settings: Settings,
    pub running: Option<Running>,
    pub log: Vec<String>,
    pub last_command: Option<String>,
    /// LASTPOINT.
    pub last_point: Vec2,
    /// Cursor position in world coordinates (from the UI; used for direct distance entry).
    pub cursor: Vec2,
    /// The deferred tangent/perpendicular snap under the cursor, if any (from the UI; lets the
    /// rubber band show the line it resolves to).
    pub cursor_deferred: Option<snap::Deferred>,
    /// Viewport size in pixels (from the UI; used for zoom and pick apertures).
    pub viewport_px: (f64, f64),
    pub clipboard: Vec<Entity>,
    pub clipboard_base: Vec2,
    pub pending_window: Option<PendingWindow>,
    pub untitled_counter: u32,
    /// The last dimension created (DIMCONTINUE / DIMBASELINE).
    pub last_dim: Option<Handle>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Self {
        let mut s = Session::empty();
        s.new_drawing(false);
        s
    }
    /// A session with no open drawing (the Start tab).
    pub fn empty() -> Self {
        Session {
            docs: Vec::new(),
            active: 0,
            settings: Settings::default(),
            running: None,
            log: Vec::new(),
            last_command: None,
            last_point: Vec2::ZERO,
            cursor: Vec2::ZERO,
            cursor_deferred: None,
            viewport_px: (1200.0, 800.0),
            clipboard: Vec::new(),
            clipboard_base: Vec2::ZERO,
            pending_window: None,
            untitled_counter: 0,
            last_dim: None,
        }
    }
    pub fn new_drawing(&mut self, metric: bool) -> usize {
        self.untitled_counter += 1;
        let d = if metric { Drawing::new_metric() } else { Drawing::new_imperial() };
        let mut st = DocState::new(d, &format!("Drawing{}", self.untitled_counter), None);
        st.set_view_quiet(if metric { View { center: Vec2::new(210.0, 148.5), height: 297.0 } } else { View::default() });
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.active
    }
    pub fn open_drawing(&mut self, d: Drawing, title: &str, path: Option<String>) -> usize {
        let mut st = DocState::new(d, title, path);
        let ext = st.doc.extents(&Space::Model);
        if !ext.is_empty() {
            st.set_view_quiet(View {
                center: ext.center(),
                height: (ext.height().max(ext.width() * self.viewport_px.1 / self.viewport_px.0.max(1.0)) * 1.1).max(1e-6),
            });
        }
        self.docs.push(st);
        self.active = self.docs.len() - 1;
        self.active
    }

    pub fn state(&self) -> Result<&DocState> {
        self.docs.get(self.active).ok_or(EngineError::NoDocument)
    }
    pub fn state_mut(&mut self) -> Result<&mut DocState> {
        self.docs.get_mut(self.active).ok_or(EngineError::NoDocument)
    }
    pub fn doc(&self) -> Result<&Drawing> {
        Ok(&self.state()?.doc)
    }
    /// Copy-on-write access to the drawing.
    pub fn doc_mut(&mut self) -> Result<&mut Drawing> {
        Ok(Arc::make_mut(&mut self.state_mut()?.doc))
    }
    /// The space commands act on (model space while working inside a layout viewport).
    pub fn space(&self) -> Space {
        self.state().map(|s| s.edit_space()).unwrap_or_default()
    }
    /// The tab being displayed: Model or a layout, regardless of MSPACE.
    pub fn layout_space(&self) -> Space {
        self.state().map(|s| s.space.clone()).unwrap_or_default()
    }
    pub fn selection(&self) -> Vec<Handle> {
        self.state().map(|s| s.selection.clone()).unwrap_or_default()
    }
    pub fn set_selection(&mut self, sel: Vec<Handle>) {
        if let Ok(st) = self.state_mut() {
            let mut seen = std::collections::HashSet::new();
            st.selection = sel.into_iter().filter(|h| seen.insert(*h)).collect();
        }
    }

    /// Message on the command line history.
    pub fn echo(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        log::debug!("{m}");
        self.log.push(m);
        if self.log.len() > 5000 {
            self.log.drain(..1000);
        }
    }

    /// Common properties for new objects (CLAYER, CECOLOR, CELTYPE, CELWEIGHT, CELTSCALE).
    pub fn current_common(&self) -> Common {
        let Ok(d) = self.doc() else { return Common::default() };
        let h = &d.header;
        Common {
            layer: h.str("CLAYER", "0"),
            color: cadcraft_color::Color::from_aci(h.i64("CECOLOR", 256) as i16),
            linetype: h.str("CELTYPE", "ByLayer"),
            lineweight: cadcraft_doc::Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16),
            ltscale: h.f64("CELTSCALE", 1.0),
            ..Common::default()
        }
    }

    /// Add an entity to the current space with current properties.
    pub fn add_entity(&mut self, kind: EntityKind) -> Result<Handle> {
        let common = self.current_common();
        let space = self.space();
        let true_color = self.doc()?.header.get("CECOLOR_RGB").and_then(|v| v.as_i64());
        let mut common = common;
        if let Some(rgb) = true_color {
            common.color = cadcraft_color::Color::True(cadcraft_color::Rgb::from_u32(rgb as u32));
        }
        let h = self.doc_mut()?.add(&space, common, kind)?;
        Ok(h)
    }

    /// World units per pixel at the current view.
    pub fn pixel_size(&self) -> f64 {
        let v = self.state().map(|s| s.view()).unwrap_or_default();
        v.height / self.viewport_px.1.max(1.0)
    }

    // ---------------- programmatic execution ----------------

    /// Run a command with JSON parameters (no dialogs). Records undo when the drawing changes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        let before = self.state().ok().map(|s| (s.doc.clone(), s.selection.clone(), s.uid));
        let run = spec.run;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self, params)));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg =
                    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
                // Restore the drawing as it was.
                if let Some((doc, sel, uid)) = &before
                    && let Some(st) = self.docs.iter_mut().find(|d| d.uid == *uid)
                {
                    st.doc = doc.clone();
                    st.selection = sel.clone();
                }
                return Err(EngineError::Internal(id.into(), msg));
            }
        };
        cmd::constraints::after_command(self, before.as_ref().map(|b| &b.0), spec.undoable && result.is_ok());
        assoc::after_command(self, before.as_ref().map(|b| &b.0), before.as_ref().map(|b| b.2));
        if spec.undoable
            && let Some((doc, sel, uid)) = before
            && let Some(st) = self.docs.iter_mut().find(|d| d.uid == uid)
            && !Arc::ptr_eq(&doc, &st.doc)
        {
            st.undo.push(Snapshot { label: spec.label.to_string(), doc, selection: sel });
            st.redo.clear();
            st.revision += 1;
            if st.undo.len() > 2000 {
                st.undo.remove(0);
            }
        }
        result
    }

    /// Bump the revision without an undo step (view-only or settings changes that redraw).
    pub fn touch(&mut self) {
        if let Ok(st) = self.state_mut() {
            st.revision += 1;
        }
    }

    // ---------------- interactive commands ----------------

    /// Start a command by name or alias as if typed at the command line.
    pub fn start(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        let (transparent, name) = match name.strip_prefix('\'') {
            Some(n) => (true, n),
            None => (false, name),
        };
        let lower = name.trim_start_matches(['_', '.', '-']).to_ascii_lowercase();
        let id = cmd::resolve_alias(&lower);
        let spec = find_command(&id).ok_or_else(|| EngineError::UnknownCommand(name.to_string()))?;
        if transparent && spec.transparent && self.running.is_some() {
            // Transparent commands (zoom, pan…) run without cancelling the active one.
            if spec.interactive.is_none() {
                self.echo(format!(">>{}", spec.label));
                return self.execute(spec.id, &Value::Null).map(|_| ());
            }
        }
        if self.running.is_some() {
            self.cancel();
        }
        (spec.enabled)(self).map_err(|m| EngineError::Disabled(spec.id.into(), m))?;
        self.last_command = Some(spec.id.to_string());
        self.echo(format!("Command: {}", spec.id.to_ascii_uppercase()));
        match spec.interactive {
            Some(factory) => {
                let machine = factory(self)?;
                let st = self.state()?;
                let before = st.doc.clone();
                let selection_before = st.selection.clone();
                self.running = Some(Running { id: spec.id.to_string(), machine, before, selection_before });
                self.pending_window = None;
                // Some commands complete immediately (e.g. ERASE with a pickfirst selection).
                self.feed(None)?;
                Ok(())
            }
            None => {
                let r = self.execute(spec.id, &Value::Null)?;
                if let Some(s) = r.as_str() {
                    self.echo(s.to_string());
                } else if !r.is_null()
                    && let Some(msg) = r.get("message").and_then(Value::as_str)
                {
                    for l in msg.lines() {
                        self.echo(l.to_string());
                    }
                }
                Ok(())
            }
        }
    }

    /// Give an input to the running command. With no command running, `Enter` repeats the last
    /// command (as in AutoCAD) and points do nothing.
    pub fn input(&mut self, input: Input) -> Result<()> {
        if self.running.is_none() {
            if input == Input::Enter
                && let Some(last) = self.last_command.clone()
            {
                return self.start(&last);
            }
            return Ok(());
        }
        // A deferred snap is only meaningful where the prompt resolves it; elsewhere it is the
        // point it was picked at.
        let input = match input {
            Input::Deferred(d) if !self.current_prompt().is_some_and(|p| p.deferred) => Input::Point(d.at),
            other => other,
        };
        // Generic selection handling during "Select objects:" prompts.
        let input = self.preprocess_selection(input)?;
        match input {
            Some(i) => self.feed(Some(i)),
            None => Ok(()),
        }
    }

    fn preprocess_selection(&mut self, input: Input) -> Result<Option<Input>> {
        let Some(prompt) = self.current_prompt() else { return Ok(Some(input)) };
        if !prompt.accept.select {
            return Ok(Some(input));
        }
        let space = self.space();
        match input {
            Input::Point(p) => {
                let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
                if let Some(pw) = self.pending_window.take() {
                    let crossing = p.x < pw.corner.x;
                    let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
                    return Ok(Some(Input::Pick(hs)));
                }
                match select::pick(self.doc()?, &space, p, ap) {
                    Some(h) => Ok(Some(Input::Pick(vec![h]))),
                    None => {
                        self.pending_window = Some(PendingWindow { corner: p, during_command: true });
                        Ok(None)
                    }
                }
            }
            Input::Text(t) | Input::Keyword(t) => {
                let tl = t.trim().to_ascii_lowercase();
                let d = self.doc()?;
                let store = d.space(&space);
                let picked = match tl.as_str() {
                    "all" => store.map(|s| s.iter().filter(|e| d.is_visible(e)).map(|e| e.handle).collect()),
                    "l" | "last" => store.and_then(|s| s.last()).map(|e| vec![e.handle]),
                    "p" | "previous" => Some(self.state()?.previous_selection.clone()),
                    _ => None,
                };
                match picked {
                    Some(hs) => Ok(Some(Input::Pick(hs))),
                    None => Ok(Some(Input::Text(t))),
                }
            }
            Input::Cancel => {
                self.pending_window = None;
                Ok(Some(Input::Cancel))
            }
            other => Ok(Some(other)),
        }
    }

    fn feed(&mut self, input: Option<Input>) -> Result<()> {
        let Some(mut run) = self.running.take() else { return Ok(()) };
        let step = match input {
            Some(Input::Cancel) => Ok(Step::Cancel),
            Some(i) => {
                if let Input::Point(p) = &i {
                    self.last_point = *p;
                }
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run.machine.input(self, i)));
                match r {
                    Ok(r) => r,
                    Err(_) => Err(EngineError::Internal(run.id.clone(), "panic".into())),
                }
            }
            None => run.machine.begin(self),
        };
        // Redraw what the command has added so far (LINE adds a segment per point), not only when it ends.
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.revision += 1;
        }
        match step {
            Ok(Step::Continue) => {
                self.running = Some(run);
                Ok(())
            }
            Ok(Step::Done) => {
                self.finish(run, false);
                Ok(())
            }
            Ok(Step::Cancel) => {
                self.echo("*Cancel*");
                self.finish(run, true);
                Ok(())
            }
            Err(EngineError::Internal(id, m)) => {
                // Restore and end the command.
                if let Ok(st) = self.state_mut() {
                    st.doc = run.before.clone();
                }
                self.echo(format!("Internal error in {id}: {m}"));
                Err(EngineError::Internal(id, m))
            }
            Err(e) => {
                // Invalid input: report and keep prompting (AutoCAD re-prompts).
                self.echo(e.to_string());
                self.running = Some(run);
                Ok(())
            }
        }
    }

    fn finish(&mut self, run: Running, cancelled: bool) {
        self.pending_window = None;
        let label = find_command(&run.id).map(|c| c.label).unwrap_or("Command");
        let _ = cancelled;
        cmd::constraints::after_command(self, Some(&run.before), true);
        assoc::after_command(self, Some(&run.before), None);
        if let Ok(st) = self.state_mut()
            && !Arc::ptr_eq(&run.before, &st.doc)
        {
            st.undo.push(Snapshot { label: label.to_string(), doc: run.before, selection: run.selection_before });
            st.redo.clear();
            st.revision += 1;
        }
    }

    /// Cancel the running command (Esc). With no command, clears the selection.
    pub fn cancel(&mut self) {
        if self.running.is_some() {
            let _ = self.feed(Some(Input::Cancel));
        } else {
            self.pending_window = None;
            self.set_selection(Vec::new());
        }
    }

    pub fn current_prompt(&self) -> Option<Prompt> {
        self.running.as_ref().map(|r| r.machine.prompt(self))
    }

    /// The text shown on the command line: the active prompt, or "Command:".
    pub fn prompt_text(&self) -> String {
        match &self.running {
            Some(r) => format!("{} {}", r.id.to_ascii_uppercase(), r.machine.prompt(self).display()),
            None => "Command:".into(),
        }
    }

    /// Rubber-band preview entities for the cursor.
    pub fn preview(&self, cursor: Vec2) -> Vec<Entity> {
        let Some(r) = &self.running else { return Vec::new() };
        let common = self.current_common();
        r.machine.preview(self, cursor).into_iter().map(|k| Entity { handle: Handle(0), common: common.clone(), kind: k }).collect()
    }

    /// Process one line typed at the command line (Enter pressed).
    pub fn cmdline(&mut self, text: &str) -> Result<()> {
        let t = text.trim_end_matches(['\r', '\n']);
        match &self.running {
            None => {
                let t = t.trim();
                if t.is_empty() {
                    return self.input(Input::Enter);
                }
                // `cmd {json}`: programmatic call with parameters.
                if let Some((name, json)) = t.split_once(' ')
                    && json.trim_start().starts_with('{')
                {
                    let params: Value =
                        serde_json::from_str(json.trim()).map_err(|e| EngineError::BadParams { cmd: name.into(), msg: e.to_string() })?;
                    let id = cmd::resolve_alias(&name.to_ascii_lowercase());
                    let r = self.execute(&id, &params)?;
                    if !r.is_null() {
                        self.echo(r.to_string());
                    }
                    return Ok(());
                }
                // Expressions like "LINE 0,0 5,5" in one line: first token is the command.
                let mut parts = t.split_whitespace();
                let name = parts.next().unwrap_or("");
                self.start(name)?;
                for rest in parts {
                    if self.running.is_none() {
                        break;
                    }
                    self.typed(rest)?;
                }
                Ok(())
            }
            Some(_) => {
                // Space acts as Enter except where the prompt wants free text.
                let text_prompt = self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
                if text_prompt || !t.contains(' ') {
                    return self.typed(t);
                }
                for tok in t.split_whitespace() {
                    if self.running.is_none() {
                        break;
                    }
                    self.typed(tok)?;
                }
                Ok(())
            }
        }
    }

    /// One token typed while a command runs.
    fn typed(&mut self, t: &str) -> Result<()> {
        let Some(prompt) = self.current_prompt() else { return Ok(()) };
        let tt = t.trim();
        if tt.is_empty() {
            return self.input(Input::Enter);
        }
        // Transparent command.
        if tt.starts_with('\'') {
            return self.start(tt);
        }
        if let Some(k) = prompt.match_keyword(tt)
            && !(prompt.accept.number && tt.parse::<f64>().is_ok())
        {
            return self.input(Input::Keyword(k));
        }
        if prompt.accept.point {
            if let Some(p) = prompt::parse_point(tt, self.last_point) {
                return self.input(Input::Point(p));
            }
            // Direct distance entry along the rubber band.
            if let (Some(base), Some(dist)) = (prompt.base, units::parse_distance(tt))
                && !prompt.accept.number
            {
                let mut c = self.cursor;
                if self.settings.orthomode {
                    c = snap::ortho(base, c);
                }
                let dir = (c - base).normalized();
                let dir = if dir == Vec2::ZERO { Vec2::X } else { dir };
                return self.input(Input::Point(base + dir * dist));
            }
        }
        self.input(Input::Text(tt.to_string()))
    }

    /// Run a script: one input per line; within a line, spaces separate inputs except at text
    /// prompts (as AutoCAD scripts do).
    pub fn script(&mut self, text: &str) -> Result<()> {
        for line in text.lines() {
            let line = line.trim_end();
            if line.trim_start().starts_with(';') {
                continue;
            }
            if line.is_empty() {
                self.cmdline("")?;
                continue;
            }
            let mut rest = line;
            loop {
                // Free text only when the prompt has no keywords; otherwise split so `I 15` reaches the keyword and the value.
                let text_prompt = self.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && p.keywords.is_empty());
                if text_prompt || self.running.is_none() && rest.contains('{') {
                    self.cmdline(rest)?;
                    break;
                }
                match rest.split_once(' ') {
                    Some((tok, r)) => {
                        if self.running.is_none() {
                            self.start(tok)?;
                        } else {
                            self.typed(tok)?;
                        }
                        rest = r;
                    }
                    None => {
                        if self.running.is_none() {
                            self.start(rest)?;
                        } else {
                            self.typed(rest)?;
                        }
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    // ---------------- undo ----------------

    pub fn undo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.undo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.selection = Vec::new();
        st.redo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }
    pub fn redo(&mut self) -> Result<Option<String>> {
        let st = self.state_mut()?;
        let Some(snap) = st.redo.pop() else { return Ok(None) };
        let cur = Snapshot { label: snap.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() };
        st.doc = snap.doc;
        st.selection = Vec::new();
        st.undo.push(cur);
        st.revision += 1;
        Ok(Some(snap.label))
    }

    // ---------------- picking without a command ----------------

    /// A click with no command running: pick/toggle objects or start a selection window.
    ///
    /// PICKADD on (the default): picks add to the selection and Shift removes. PICKADD off: each pick
    /// replaces the selection and Shift adds (Shift+pick on a selected object removes it).
    pub fn idle_click(&mut self, p: Vec2, shift: bool) -> Result<()> {
        let space = self.space();
        let pickadd = self.settings.pickadd;
        if let Some(pw) = self.pending_window.take() {
            let crossing = p.x < pw.corner.x;
            let hs = select::select_window(self.doc()?, &space, Bounds2::new(pw.corner, p), crossing);
            let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
            if shift && pickadd {
                sel.retain(|h| !hs.contains(h));
            } else {
                sel.extend(hs);
            }
            self.set_selection(sel);
            return Ok(());
        }
        let ap = self.pixel_size() * self.settings.pickbox.max(1.0) * 1.5;
        match select::pick(self.doc()?, &space, p, ap) {
            Some(h) => {
                let mut sel = if pickadd || shift { self.selection() } else { Vec::new() };
                let had = sel.contains(&h);
                if shift && (pickadd || had) {
                    sel.retain(|x| *x != h);
                } else if !had {
                    sel.push(h);
                }
                self.set_selection(sel);
            }
            None => self.pending_window = Some(PendingWindow { corner: p, during_command: false }),
        }
        Ok(())
    }

    /// Remember the selection used by a command as "Previous".
    pub fn remember_selection(&mut self, sel: &[Handle]) {
        if let Ok(st) = self.state_mut() {
            st.previous_selection = sel.to_vec();
        }
    }

    // ---------------- views ----------------

    pub fn zoom_extents(&mut self) -> Result<()> {
        let space = self.space();
        let ext = self.doc()?.extents(&space);
        let (w, h) = self.viewport_px;
        let ext = if ext.is_empty() {
            let d = self.doc()?;
            let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
            let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
            Bounds2::new(lo, hi)
        } else {
            ext
        };
        let aspect = w / h.max(1.0);
        let height = ext.height().max(ext.width() / aspect.max(1e-6)).max(1e-6) * 1.05;
        self.state_mut()?.set_view(View { center: ext.center(), height });
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping `about` fixed on screen.
    pub fn zoom_about(&mut self, factor: f64, about: Vec2) -> Result<()> {
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(());
        }
        let st = self.state_mut()?;
        let v = st.view();
        let h = (v.height / factor).clamp(1e-9, 1e12);
        let k = h / v.height;
        let c = about + (v.center - about) * k;
        st.set_view_quiet(View { center: c, height: h });
        Ok(())
    }

    /// Serializable summary for inspect/automation.
    pub fn summary(&self) -> Value {
        let docs: Vec<Value> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                serde_json::json!({
                    "index": i, "title": d.title, "path": d.path, "dirty": d.is_dirty(),
                    "entities": d.doc.entity_count(), "active": i == self.active, "revision": d.revision,
                })
            })
            .collect();
        serde_json::json!({
            "documents": docs,
            "prompt": self.prompt_text(),
            "running": self.running.as_ref().map(|r| r.id.clone()),
            "selection": self.selection().iter().map(|h| h.hex()).collect::<Vec<_>>(),
            "settings": self.settings,
        })
    }
}

#[cfg(test)]
mod tests;
