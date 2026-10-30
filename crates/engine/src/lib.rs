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
#[cfg(not(target_arch = "wasm32"))]
pub mod atomic_write;
pub mod audit;
pub mod autosave;
pub mod blockedit;
#[cfg(not(target_arch = "wasm32"))]
pub mod capped_read;
pub mod cmd;
pub mod doc_path;
mod finite;
pub mod grip_dims;
pub mod grips;
mod guard;
pub mod lisp;
pub mod mtext_edit;
pub mod pointmod;
pub mod prompt;
mod recent;
pub mod sample;
pub mod select;
pub mod settings_dir;
pub mod snap;
pub mod spatial;
pub mod sysvars;
pub mod undoctl;
pub mod units;

use std::sync::Arc;

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Space};
use cadcraft_geom::{Bounds2, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use cadcraft_doc as doc;
pub use cadcraft_fonts as fonts;
pub use cadcraft_geom as geom;
pub use cadcraft_render as render;
pub use cmd::hatch::{HatchPicks, PatternChoice, PickMode, pattern_choices};
pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use prompt::{Accept, Drag, Input, Interactive, Picker, Prompt, Step};

mod doc_state;
mod error;
mod last_used;
mod running;
mod session;
mod settings;
mod view;

pub use doc_state::{DocState, Snapshot};
pub use error::{EngineError, Result};
pub use last_used::{LastUsed, LeaderOrder, LeaderType, MLeaderOptions};
pub use running::{PendingWindow, Running};
pub use settings::Settings;
pub use view::{ModelTile, ModelTiles, View};

/// `<version> (<commit>)`, what `--version` prints after the program name in the app and the CLI.
/// The commit comes from `CADCRAFT_BUILD_SHA` at build time, `dev` without it.
pub fn build_version() -> &'static str {
    static VERSION: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!("{} ({})", env!("CARGO_PKG_VERSION"), option_env!("CADCRAFT_BUILD_SHA").unwrap_or("dev")));
    &VERSION
}

/// The editor session: open drawings, settings, the running command and the command log.
pub struct Session {
    pub docs: Vec<DocState>,
    pub active: usize,
    /// Monotonic count of changes to the open drawings and the active one. A screenshot queued
    /// across a switch is rejected even when a later switch returns to the same drawing.
    pub doc_epoch: u64,
    pub settings: Settings,
    pub running: Option<Running>,
    /// A script is running ([`Session::script`]); a SCRIPT command inside it is refused.
    in_script: bool,
    /// The command suspended while a transparent command (`'ZOOM`, `'PAN`…) prompts inside it;
    /// it resumes when that command ends. One level, as in AutoCAD.
    pub suspended: Option<Running>,
    pub log: Vec<String>,
    pub last_command: Option<String>,
    /// LASTPOINT.
    pub last_point: Vec2,
    /// The last segment given at a point prompt (its base point and the point): polar angles
    /// relative to the last segment (POLARMODE bit 1) are measured from it.
    pub last_segment: Option<(Vec2, Vec2)>,
    /// The last typed `@distance<angle` input, as `(distance, angle in radians)`.
    /// Set by [`Session::typed`] and consumed by commands that resolve a deferred
    /// tangent first point (e.g. [`cmd::draw::LineM`]) when the next point is typed
    /// rather than clicked. Cleared at [`Session::start`].
    pub last_polar: Option<(f64, f64)>,
    /// Point modifiers pending at the current point prompt (`FROM`, `M2P`, `.x`, `<a`…).
    pub point_mods: Vec<pointmod::Frame>,
    /// Cursor position in world coordinates (from the UI; used for direct distance entry).
    pub cursor: Vec2,
    /// The deferred tangent/perpendicular snap under the cursor, if any (from the UI; lets the
    /// rubber band show the line it resolves to).
    pub cursor_deferred: Option<snap::Deferred>,
    /// Whether Ctrl is held right now (from the UI, overwritten every frame). Commands that
    /// change behavior on Ctrl — currently only ARC's "hold Ctrl to switch direction" — read
    /// this instead of receiving modifiers through `Input`, which stays free of UI state.
    pub ctrl_down: bool,
    /// Origin of the point currently being fed by `input_snapped`; never reused by typed points.
    picked_viewport: Option<snap::ViewportPoint>,
    /// The last drawing comparison (COMPARE), drawn over the drawing it was made for.
    pub compare: Option<cmd::file::compare::Comparison>,
    /// Object snap tracking: acquired points and the path under the cursor (from the UI's
    /// [`Session::snap_cursor`] calls; direct distance entry follows the path).
    pub tracking: snap::tracking::Tracker,
    /// Viewport size in pixels (from the UI; used for zoom and pick apertures).
    pub viewport_px: (f64, f64),
    pub clipboard: Vec<Entity>,
    pub clipboard_base: Vec2,
    pub pending_window: Option<PendingWindow>,
    /// A selection mode typed at "Select objects" (W, C, F, WP, CP) collecting its points.
    pub select_mode: Option<select::SelectMode>,
    /// SIngle typed at "Select objects": the next selection ends the prompt.
    pub select_single: bool,
    pub untitled_counter: u32,
    /// The last dimension created (DIMCONTINUE / DIMBASELINE).
    pub last_dim: Option<Handle>,
    /// Options remembered between invocations of interactive commands.
    pub last_used: LastUsed,
    /// Where `clipboard` came from and the layers, styles and blocks its objects need.
    pub clipboard_source: Option<cmd::clipboard::ClipSource>,
    /// The window last picked with PLOTWINDOW (the Plot dialog's "Window <").
    pub plot_window: Option<cadcraft_geom::Bounds2>,
    /// AutoLISP: each drawing's symbols and the routine running ([`lisp`]).
    lisp: lisp::LispState,
    /// How the last LISP run ended (its value or error message).
    lisp_last: Option<std::result::Result<lisp::value::Value, String>>,
    /// What the last HATCHPICK kept (the Hatch and Gradient dialog's picks).
    pub hatch_picks: Option<cmd::hatch::HatchPicks>,
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
            doc_epoch: 0,
            settings: Settings::default(),
            running: None,
            in_script: false,
            suspended: None,
            log: Vec::new(),
            last_command: None,
            last_point: Vec2::ZERO,
            last_segment: None,
            last_polar: None,
            point_mods: Vec::new(),
            cursor: Vec2::ZERO,
            cursor_deferred: None,
            ctrl_down: false,
            picked_viewport: None,
            compare: None,
            tracking: snap::tracking::Tracker::default(),
            viewport_px: (1200.0, 800.0),
            clipboard: Vec::new(),
            clipboard_base: Vec2::ZERO,
            pending_window: None,
            select_mode: None,
            select_single: false,
            untitled_counter: 0,
            last_dim: None,
            last_used: LastUsed::default(),
            clipboard_source: None,
            plot_window: None,
            lisp: lisp::LispState::default(),
            lisp_last: None,
            hatch_picks: None,
        }
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
    /// Settings for the active drawing, without caching drawing variables in the profile.
    pub fn drafting_settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        if let Ok(d) = self.doc() {
            settings.snapunit = d.header.snap_unit();
            settings.gridunit = d.header.grid_unit();
        }
        settings
    }
    /// The space commands act on (model space while working inside a layout viewport).
    pub fn space(&self) -> Space {
        self.state().map(DocState::edit_space).unwrap_or_default()
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
            plot_style: h.str("CPLOTSTYLE", "ByLayer"),
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
        // Text, dimensions, multileaders, hatches and block references of annotative styles are
        // annotative, supporting the current annotation scale.
        let d = self.doc()?;
        if d.annotative_by_style(&kind) {
            common.anno_scales = vec![d.current_anno_scale()];
        }
        // Lines, arcs, circles, points, polylines and 2D solids take the current 3D thickness
        // (THICKNESS). RECTANG sets its own, and text never takes it.
        if matches!(
            kind,
            EntityKind::Line(_)
                | EntityKind::Arc(_)
                | EntityKind::Circle(_)
                | EntityKind::Point(_)
                | EntityKind::LwPolyline(_)
                | EntityKind::Solid(_)
        ) {
            common.thickness = Some(d.header.f64("THICKNESS", 0.0)).filter(|t| t.is_finite()).unwrap_or(0.0);
        }
        let h = self.doc_mut()?.add(&space, common, kind)?;
        Ok(h)
    }

    /// World units per pixel at the current view.
    pub fn pixel_size(&self) -> f64 {
        let v = self.state().map(DocState::view).unwrap_or_default();
        v.height / self.viewport_px.1.max(1.0)
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
                    "blockEditor": d.block_editor(),
                })
            })
            .collect();
        serde_json::json!({
            "documents": docs,
            "prompt": self.prompt_text(),
            "running": self.running.as_ref().map(|r| r.id.clone()),
            "selection": self.selection().iter().map(|h| h.hex()).collect::<Vec<_>>(),
            "settings": self.drafting_settings(),
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../tests/support/temp_dir.rs"]
mod temp_dir;
