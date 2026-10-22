//! [`EngineError`] as data for programmatic callers (MCP, control channel): a stable `code` per
//! variant, the command it concerns and, when the message names one, the parameter. The text
//! (`message`) stays as it was; the codes are documented in `docs/mcp.md` and
//! `docs/control-protocol.md` and never change meaning.

use serde::Serialize;

use crate::EngineError;

/// Every error code with what it means, in the order of the docs' tables.
pub const ERROR_CODES: &[(&str, &str)] = &[
    ("unknown_command", "No command has this name or alias."),
    ("not_available", "The command exists but can't run now (its `enabled` check refused it)."),
    ("bad_params", "The parameters were refused: missing, of the wrong type, out of range, or naming something that doesn't exist."),
    ("no_document", "No drawing is open."),
    ("failed", "The command ran and failed (a file that can't be read, nothing to act on…)."),
    ("internal", "A bug in the command: it panicked and the drawing was kept as it was."),
];

/// An engine error as data: `{code, command, param?, message}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErrorInfo {
    /// One of [`ERROR_CODES`].
    pub code: &'static str,
    /// The command the error concerns: the one named by the error, else the one that was called.
    pub command: String,
    /// The parameter the message names first, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// The error text, as `to_string` gives it.
    pub message: String,
}

impl EngineError {
    /// The stable code of this error (one of [`ERROR_CODES`]).
    pub fn code(&self) -> &'static str {
        match self {
            EngineError::UnknownCommand(_) => "unknown_command",
            EngineError::Disabled(..) => "not_available",
            EngineError::BadParams { .. } => "bad_params",
            EngineError::NoDocument => "no_document",
            EngineError::Other(_) => "failed",
            EngineError::Internal(..) => "internal",
        }
    }

    /// The command this error names, if it names one.
    pub fn command(&self) -> Option<&str> {
        match self {
            EngineError::UnknownCommand(c) | EngineError::Disabled(c, _) | EngineError::BadParams { cmd: c, .. } | EngineError::Internal(c, _) => {
                Some(c)
            }
            EngineError::NoDocument | EngineError::Other(_) => None,
        }
    }

    /// The parameter a [`EngineError::BadParams`] message names, best effort: a message that starts with a
    /// quoted key (`` `radius` must be positive``) or serde's ``missing field `radius` ``.
    pub fn param(&self) -> Option<String> {
        let EngineError::BadParams { msg, .. } = self else { return None };
        let quoted = |s: &str| {
            let (name, _) = s.strip_prefix('`')?.split_once('`')?;
            // Shaped like a JSON key (`radius`, `leaderType`, `layer.name`), not a value such as
            // `Door`, `A4` or `ISO-25`.
            let ok = name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            ok.then(|| name.to_string())
        };
        quoted(msg).or_else(|| {
            ["missing field ", "unknown field ", "duplicate field "]
                .iter()
                .find_map(|p| msg.find(p).and_then(|i| msg.get(i + p.len()..)).and_then(quoted))
        })
    }

    /// This error as data. `command`: the command that was called, for errors that don't name one.
    pub fn info(&self, command: &str) -> ErrorInfo {
        ErrorInfo { code: self.code(), command: self.command().unwrap_or(command).to_string(), param: self.param(), message: self.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_variant_has_a_documented_code() {
        let all = [
            EngineError::UnknownCommand("frob".into()),
            EngineError::Disabled("bedit".into(), "no blocks".into()),
            EngineError::BadParams { cmd: "circle".into(), msg: "`radius` must be positive".into() },
            EngineError::NoDocument,
            EngineError::Other("nothing selected".into()),
            EngineError::Internal("trim".into(), "index out of bounds".into()),
        ];
        let codes: Vec<&str> = all.iter().map(EngineError::code).collect();
        assert_eq!(codes, ERROR_CODES.iter().map(|(c, _)| *c).collect::<Vec<_>>());
        for e in &all {
            let i = e.info("line");
            assert_eq!(i.message, e.to_string());
            assert!(i.code.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", i.code);
        }
    }

    /// `docs/mcp.md` and `docs/control-protocol.md` list every code.
    #[test]
    fn the_docs_list_every_code() {
        for doc in [include_str!("../../../docs/mcp.md"), include_str!("../../../docs/control-protocol.md")] {
            for (code, _) in ERROR_CODES {
                assert!(doc.contains(&format!("| `{code}` |")), "{code} missing from the docs");
            }
        }
    }

    #[test]
    fn command_comes_from_the_error_else_from_the_call() {
        assert_eq!(EngineError::UnknownCommand("frob".into()).info("x").command, "frob");
        assert_eq!(EngineError::Disabled("bedit".into(), "-".into()).info("x").command, "bedit");
        assert_eq!(EngineError::Internal("trim".into(), "-".into()).info("x").command, "trim");
        assert_eq!(EngineError::NoDocument.info("line").command, "line");
        assert_eq!(EngineError::Other("-".into()).info("open").command, "open");
    }

    #[test]
    fn param_is_taken_from_the_message_when_it_names_one() {
        let p = |msg: &str| EngineError::BadParams { cmd: "c".into(), msg: msg.into() }.param();
        assert_eq!(p("`radius` must be positive").as_deref(), Some("radius"));
        assert_eq!(p("`layer.name` is required").as_deref(), Some("layer.name"));
        assert_eq!(p("missing field `center` at line 1 column 2").as_deref(), Some("center"));
        assert_eq!(p("unknown field `centre`, expected one of `center`, `radius`").as_deref(), Some("centre"));
        assert_eq!(p("`leaderType` must be \"none\", \"straight\" or \"spline\"").as_deref(), Some("leaderType"));
        assert_eq!(p("no block `Door`"), None);
        assert_eq!(p("`Door` is not in the scale list"), None);
        assert_eq!(p("`A4` is not a paper size"), None);
        assert_eq!(p("`ISO-25` is not a dimension style"), None);
        assert_eq!(p("factor must be positive"), None);
        assert_eq!(p("``"), None);
        assert_eq!(p("`unclosed"), None);
        assert_eq!(EngineError::Other("`radius` must be positive".into()).param(), None);
    }

    #[test]
    fn info_serializes_without_an_absent_param() {
        let e = EngineError::NoDocument.info("line");
        assert_eq!(serde_json::to_value(&e).ok(), Some(json!({"code": "no_document", "command": "line", "message": "no active document"})));
        let e = EngineError::BadParams { cmd: "circle".into(), msg: "`radius` must be positive".into() }.info("x");
        assert_eq!(serde_json::to_value(&e).ok().and_then(|v| v.get("param").cloned()), Some(json!("radius")));
    }
}
