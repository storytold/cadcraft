//! The error type of the container layer.

use std::fmt;

/// What kind of failure it was, so a caller can word it for the person using the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The bytes are not a DWG file, or the version code is unknown.
    NotDwg,
    /// A DWG version that is recognised but not read (AutoCAD 2007-2009).
    UnsupportedVersion,
    /// A feature inside a supported file that is not implemented (encrypted sections).
    Unsupported,
    /// Ran past the end of a buffer or bit stream.
    Truncated,
    /// The data does not have the structure the specification describes.
    Format,
}

/// A failure to read or write a DWG container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error { kind, message: message.into() }
    }
    pub fn format(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Format, message)
    }
    pub fn truncated(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Truncated, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Unsupported, message)
    }
    /// Prefix the message with context, keeping the kind.
    pub fn context(mut self, ctx: impl fmt::Display) -> Self {
        self.message = format!("{ctx}: {}", self.message);
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ErrorKind::NotDwg => "not a DWG file",
            ErrorKind::UnsupportedVersion => "unsupported DWG version",
            ErrorKind::Unsupported => "unsupported feature",
            ErrorKind::Truncated => "truncated data",
            ErrorKind::Format => "format error",
        };
        write!(f, "{kind}: {}", self.message)
    }
}

impl std::error::Error for Error {}
