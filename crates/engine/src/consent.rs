//! What the user has allowed the code running on this thread to do to their files and drawings.
//!
//! The AI Assistant runs each call with only what the user confirmed for it ([`with`]): the
//! places where the engine writes, replaces or deletes a file, or closes a drawing, ask
//! [`check_write`] or [`check_close`] first and refuse the rest. The check sits where the work is
//! done, not in the reading of the call, so a command built at run time (an encoded SCRIPT, a
//! LISP string) meets it too. With nothing installed, as for the user's own commands, everything
//! is allowed.

use std::cell::Cell;
use std::fmt;
use std::path::Path;

/// What a [`with`] scope allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Consent {
    /// Write, replace, rename or delete files on disk.
    pub write_files: bool,
    /// Close drawings (CLOSE, CLOSEALL): what wasn't saved is lost.
    pub close_drawings: bool,
}

impl Consent {
    /// Nothing that touches the user's files or closes a drawing.
    pub const NONE: Consent = Consent { write_files: false, close_drawings: false };

    fn and(self, other: Consent) -> Consent {
        Consent { write_files: self.write_files && other.write_files, close_drawings: self.close_drawings && other.close_drawings }
    }
}

thread_local! {
    static CURRENT: Cell<Option<Consent>> = const { Cell::new(None) };
}

/// Run `f` allowing at most `consent` on this thread, and restore what was there before, also when
/// `f` panics. A scope inside another only narrows it.
pub fn with<T>(consent: Consent, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Consent>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let before = CURRENT.with(Cell::get);
    let _restore = Restore(before);
    CURRENT.with(|c| c.set(Some(before.map_or(consent, |b| b.and(consent)))));
    f()
}

/// What this thread may do: `None` outside any [`with`] scope (everything).
pub fn current() -> Option<Consent> {
    CURRENT.with(Cell::get)
}

/// A file operation or a close the user hasn't allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

impl From<Refused> for std::io::Error {
    fn from(r: Refused) -> Self {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, r.0)
    }
}

/// Whether `path` may be written, replaced, renamed or deleted here.
pub fn check_write(path: impl AsRef<Path>) -> Result<(), Refused> {
    match current() {
        Some(c) if !c.write_files => {
            Err(Refused(format!("not allowed to write {}: the user hasn't approved writing files for this call", path.as_ref().display())))
        }
        _ => Ok(()),
    }
}

/// Whether a drawing may be closed here.
pub fn check_close() -> Result<(), Refused> {
    match current() {
        Some(c) if !c.close_drawings => Err(Refused("not allowed to close a drawing: the user hasn't approved it for this call".into())),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_narrow_and_restore() {
        assert_eq!(current(), None);
        assert!(check_write("a.dxf").is_ok() && check_close().is_ok());
        let writes = Consent { write_files: true, close_drawings: false };
        with(writes, || {
            assert!(check_write("a.dxf").is_ok());
            assert!(check_close().is_err());
            // An inner scope can't widen the outer one.
            with(Consent { write_files: true, close_drawings: true }, || assert!(check_close().is_err()));
            with(Consent::NONE, || assert!(check_write("a.dxf").unwrap_err().to_string().contains("a.dxf")));
            assert!(check_write("a.dxf").is_ok());
        });
        assert_eq!(current(), None);
    }

    #[test]
    fn a_panic_inside_a_scope_restores_it() {
        let r = std::panic::catch_unwind(|| with(Consent::NONE, || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(current(), None);
    }
}
