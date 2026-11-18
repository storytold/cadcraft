//! The sandbox's view of the session.

use std::sync::Arc;

use craft_plugin::{Access, Manifest};
use serde_json::Value;

use super::{Before, MAX_NOTE_CHARS, MAX_NOTES, refusal, same_settings, selection_json, set_setting};
use crate::Session;

/// The sandbox's view of the session: every call is checked by [`refusal`] and run in its JSON
/// form on the drawing the run started in; one that, from a read-only plug-in, changes the drawing
/// (or the drafting settings) is put back and refused.
pub(super) struct EngineHost<'s> {
    s: &'s mut Session,
    manifest: &'s Manifest,
    /// The drawing the run started in.
    uid: u64,
    notes: Vec<String>,
    /// A call left that drawing (it isn't the active one any more): the run fails.
    strayed: bool,
}

impl<'s> EngineHost<'s> {
    pub(super) fn new(s: &'s mut Session, manifest: &'s Manifest, uid: u64) -> Self {
        EngineHost { s, manifest, uid, notes: Vec::new(), strayed: false }
    }

    /// What the run asked to show the user, and whether a call left the drawing.
    pub(super) fn finish(self) -> (Vec<String>, bool) {
        (self.notes, self.strayed)
    }

    /// Whether the drawing the run started in is still the active one.
    fn on_drawing(&self) -> bool {
        self.s.state().is_ok_and(|st| st.uid == self.uid)
    }

    /// Run `command` for a read-only plug-in: a call that changed the drawing (or recorded an undo
    /// step, or changed the drafting settings) is put back and refused.
    fn call_reading(&mut self, command: &str, params: &Value) -> Result<String, String> {
        let before = Before::take(self.s).map_err(|e| e.to_string())?;
        let r = self.s.execute(command, params);
        let changed = match self.s.state() {
            Ok(st) => st.uid != before.uid || !Arc::ptr_eq(&st.doc, &before.doc.doc) || st.undo.len() != before.doc.undo.len(),
            Err(_) => true,
        } || !same_settings(&self.s.settings, &before.settings);
        if changed {
            before.restore(self.s);
            return Err(craft_plugin::check_read_only(self.manifest, command, true)
                .err()
                .unwrap_or_else(|| format!("`{command}` changes the drawing")));
        }
        r.map(|v| v.to_string()).map_err(|e| e.to_string())
    }
}

impl craft_plugin::Host for EngineHost<'_> {
    fn call(&mut self, command: &str, params: &str) -> Result<String, String> {
        if self.strayed || !self.on_drawing() {
            self.strayed = true;
            return Err("plug-ins work on the drawing they started in, which isn't the active one any more".into());
        }
        // Command ids are matched in lower case (`find_command`): the rules are checked on that id
        let command = command.trim().to_ascii_lowercase();
        let params: Value = serde_json::from_str(params).map_err(|e| format!("the params for `{command}` aren't JSON: {e}"))?;
        if let Some(why) = refusal(self.manifest, &command, &params) {
            return Err(why);
        }
        let r = if self.manifest.permissions.access == Access::Write {
            self.s.execute(&command, &params).map(|v| v.to_string()).map_err(|e| e.to_string())
        } else {
            self.call_reading(&command, &params)
        };
        if !self.on_drawing() {
            self.strayed = true;
        }
        r
    }

    fn selection(&mut self) -> String {
        selection_json(self.s).to_string()
    }

    fn notify(&mut self, message: &str) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(message.chars().take(MAX_NOTE_CHARS).collect());
        }
    }

    fn log(&mut self, line: &str) {
        // Also in the run's result (`logs`); the command history stays the plug-in's notes
        log::info!("plug-in {}: {line}", self.manifest.id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        Some(self.s.plugins.prefs.settings.get(&self.manifest.id)?.get(key)?.to_string())
    }

    fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        set_setting(&mut self.s.plugins.prefs.settings, &self.manifest.id, key, value)
    }
}
