//! What a shared session accepts: roles, the command allowlist and the size limits.
//!
//! A collaboration session is not a shell. A command that opens or writes a file, drives the UI,
//! runs a script or reads another drawing would either escape the session or make every participant
//! depend on one client's files, so it is refused on the server (ADR-0001, ADR-0008). The role
//! check is here too: the engine has no read-only mode, so a viewer is stopped at this boundary.

use cadcraft_collab::Op;

/// The most entities, table records and layout changes one operation may change.
pub const MAX_CHANGES: usize = 5_000;
/// The most bytes one operation's parameters may take.
pub const MAX_OP_BYTES: usize = 1 << 20;
/// The most participants one room keeps presence for.
pub const MAX_PARTICIPANTS: usize = 8;

/// Why `command` is not a shared-drawing operation, or `None` when it is.
///
/// The deny list is explicit rather than an allowlist of 400 command ids: every command CADCraft
/// has is a drawing operation unless it touches a file, the UI, a script or another drawing, and a
/// new command should not need a protocol change to work. The hard exclusions are the ones that can
/// leave the session or bypass its order (ADR-0001 §allowlist).
pub fn deny_reason(command: &str) -> Option<&'static str> {
    let command = command.trim().to_ascii_lowercase();
    if command.is_empty() {
        return Some("a command is required");
    }
    const DENIED: &[&str] = &[
        "open",
        "save",
        "qsave",
        "saveas",
        "new",
        "quit",
        "exit",
        "close",
        "recover",
        "audit",
        "purge",
        "plot",
        "preview",
        "publish",
        "export",
        "script",
        "deliver",
        "undo",
        "redo",
        "compare",
        "compareexport",
        "xref",
        "xattach",
        "xbind",
        "xopen",
        "options",
        "mcp",
        "control",
        "document.bytes",
    ];
    if DENIED.contains(&command.as_str()) {
        return Some("the command changes the file or the session, not the shared drawing");
    }
    const DENIED_PREFIXES: &[&str] = &["ui.", "app.", "file.", "lisp", "plot", "recover", "script"];
    for prefix in DENIED_PREFIXES {
        if command.starts_with(prefix) {
            return Some("the command is not a shared-drawing operation");
        }
    }
    None
}

/// Why `op` is too large for one transaction, or `None` when it fits.
pub fn size_reason(op: &Op) -> Option<&'static str> {
    match serde_json::to_vec(&op.params) {
        Ok(bytes) if bytes.len() > MAX_OP_BYTES => Some("the operation's parameters are larger than 1 MiB"),
        Ok(_) => None,
        Err(_) => Some("the operation's parameters are not valid JSON"),
    }
}

/// Why `op` may not run in a shared session, or `None` when it may.
///
/// `imageattach` names an image file in the drawing. Without `data` the engine would open that file
/// on the **server's** disk at a path the client chose, and the other participants would see a
/// different image or a crossed-out frame. A shared session therefore takes the raster bytes in the
/// operation and rejects the file-reading form before the drawing changes (ADR 0021).
pub fn params_reason(op: &Op) -> Option<&'static str> {
    if op.command.trim().eq_ignore_ascii_case("imageattach") {
        let has_data = op.params.get("data").and_then(|value| value.as_str()).is_some_and(|data| !data.trim().is_empty());
        if !has_data {
            return Some("imageattach needs the image bytes in `data`; a shared session never reads a file by the client's path");
        }
    }
    None
}

/// Whether a change set exceeds [`MAX_CHANGES`].
pub fn changes_too_many(changes: usize) -> bool {
    changes > MAX_CHANGES
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_collab::Role;
    use cadcraft_collab::{OpContext, OpKind};
    use cadcraft_doc::Space;
    use serde_json::json;

    fn op(command: &str, params: serde_json::Value) -> Op {
        Op {
            op_id: "op".into(),
            actor: "u1".into(),
            base_version: 0,
            kind: OpKind::Command,
            command: command.into(),
            params,
            context: OpContext::in_space(Space::Model),
        }
    }

    #[test]
    fn file_session_and_ui_commands_are_denied() {
        for command in [
            "open",
            "save",
            "saveas",
            "qsave",
            "new",
            "recover",
            "purge",
            "audit",
            "undo",
            "redo",
            "lisp",
            "ui.theme",
            "file.new",
            "app.quit",
            "document.bytes",
        ] {
            assert!(deny_reason(command).is_some(), "`{command}` must not be a shared operation");
        }
    }

    #[test]
    fn drawing_commands_are_allowed() {
        for command in [
            "line",
            "pline",
            "move",
            "copy",
            "erase",
            "rotate",
            "layer.set",
            "layout.rename",
            "layout.new",
            "rename",
            "hatch",
            "dimlinear",
            "draworder",
            "insert",
        ] {
            assert!(deny_reason(command).is_none(), "`{command}` is a drawing operation");
        }
    }

    #[test]
    fn a_viewer_may_not_write_but_may_close_only_as_owner() {
        assert!(!Role::Viewer.may_write());
        assert!(Role::Editor.may_write());
        assert!(Role::Owner.may_write());
        assert!(!Role::Editor.may_close());
        assert!(Role::Owner.may_close());
    }

    #[test]
    fn an_oversized_parameter_object_is_refused() {
        assert!(size_reason(&op("line", json!({ "points": [[0, 0], [1, 1]] }))).is_none());
        let huge = "x".repeat(MAX_OP_BYTES + 1);
        assert!(size_reason(&op("mtext", json!({ "text": huge }))).is_some());
    }

    #[test]
    fn the_change_limit_is_inclusive() {
        assert!(!changes_too_many(MAX_CHANGES));
        assert!(changes_too_many(MAX_CHANGES + 1));
    }

    #[test]
    fn imageattach_needs_its_bytes_in_the_operation() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
        // The client's path is only a label: the bytes must travel with the operation.
        assert!(params_reason(&op("imageattach", json!({ "path": "map.png" }))).is_some());
        assert!(params_reason(&op("imageattach", json!({ "path": "map.png", "data": "  " }))).is_some());
        assert!(params_reason(&op("imageattach", json!({ "path": "map.png", "data": 7 }))).is_some());
        assert!(params_reason(&op("imageattach", json!({ "path": "map.png", "data": png }))).is_none());
        // Every other drawing command is untouched by the rule.
        assert!(params_reason(&op("line", json!({ "points": [[0, 0], [1, 1]] }))).is_none());
        // The command name is matched the way the engine spells it, ignoring case.
        assert!(params_reason(&op("ImageAttach", json!({ "path": "map.png" }))).is_some());
    }
}
