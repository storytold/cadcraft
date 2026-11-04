//! Paths stored in a drawing: xref paths and table data links.
//!
//! Such a path reaches the file system only through [`resolve`]: a relative path is taken from
//! the drawing's folder, and only files on this computer, or on the network share the drawing
//! itself is on, are opened (see `craft_docpath`). A refused path is handled like a missing file,
//! with the reason in its status. Paths the user gives a command (XATTACH, -XREF Path, DATALINK)
//! are their own choice and are used as given.

use std::path::{Path, PathBuf};

pub use craft_docpath::Refused;

/// The file to open for `stored`, a path or `file:` URL stored in the drawing saved at
/// `document` (`None` for a drawing without a file).
///
/// A result that isn't absolute is refused as [`Refused::Invalid`]: without a document a
/// relative path would depend on the working directory, and a Windows drive path names no file
/// on other platforms.
pub fn resolve(stored: &str, document: Option<&Path>) -> Result<PathBuf, Refused> {
    let document = document.filter(|d| !d.as_os_str().is_empty()).map(|d| std::path::absolute(d).unwrap_or_else(|_| d.to_path_buf()));
    craft_docpath::resolve_absolute(stored.trim(), document.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_resolve_beside_the_drawing_and_other_computers_are_refused() {
        let doc = std::env::temp_dir().join("plans").join("host.dxf");
        assert_eq!(resolve("parts/a.dxf", Some(&doc)), Ok(std::env::temp_dir().join("plans").join("parts").join("a.dxf")));
        assert_eq!(resolve("a.dxf", None), Err(Refused::Invalid));
        assert_eq!(resolve(r"\\192.0.2.1\share\a.dxf", Some(&doc)), Err(Refused::Network));
        assert_eq!(resolve("file://192.0.2.1/s/a.dxf", Some(&doc)), Err(Refused::Network));
        assert_eq!(resolve("//192.0.2.1/s/a.dxf", None), Err(Refused::Network));
    }
}
