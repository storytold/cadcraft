//! What the user did recently, read from the command log: the shortcut menus' Recent Commands.

use crate::{Session, find_command};

/// How a started command appears in the log ([`Session::start`]).
const STARTED: &str = "Command: ";

impl Session {
    /// The commands started most recently (typed, from a menu or repeated), newest first, each
    /// once, at most `max`. Ids are registered command ids, or the lower-case names of commands
    /// the active drawing's LISP defines (`c:` functions) that it still defines.
    pub fn recent_commands(&self, max: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in self.log.iter().rev() {
            if out.len() >= max {
                break;
            }
            let Some(name) = line.strip_prefix(STARTED) else { continue };
            let id = match find_command(name.trim()) {
                Some(spec) => spec.id.to_string(),
                None if self.is_lisp_command(name.trim()) => name.trim().to_ascii_lowercase(),
                None => continue,
            };
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }
}
