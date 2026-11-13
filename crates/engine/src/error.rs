#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    /// Reading, writing, or converting a drawing file failed (missing file,
    /// disk full, unreadable content): the storage or the content is at fault,
    /// not the command's parameters — embedders surface these as I/O errors
    /// distinct from bad input.
    #[error("io error in `{cmd}`: {msg}")]
    Io { cmd: String, msg: String },
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
