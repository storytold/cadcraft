//! Windows portable ZIP settings, selected by a marker beside the executable.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The marker file a portable copy ships beside `cadcraft.exe`.
pub(crate) const PORTABLE_MARKER: &str = "portable.txt";

/// The settings folder a portable copy keeps beside the executable.
pub(crate) const PORTABLE_DATA_DIR: &str = "CADCraftData";

/// The marker path, when `dir` holds a marker file (not a directory of that name).
pub(crate) fn portable_marker(dir: &Path) -> Option<PathBuf> {
    let marker = dir.join(PORTABLE_MARKER);
    marker.is_file().then_some(marker)
}

/// Prove `dir` is writable with the one exclusive probe this app uses: a fresh file holding a byte
/// (a full drive can still create an empty file and refuse its data) and its flush, removed again.
/// The nightly profile check and the portable check share it, so both test the same thing.
pub(crate) fn probe(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".portable-write-test-{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
    let written = file.write_all(&[0]).and_then(|()| file.sync_all());
    drop(file);
    let removed = std::fs::remove_file(&probe);
    written?;
    removed
}

/// A marker must be a file. Prove the data folder is writable before selecting it.
pub(crate) fn directory_in(exe_dir: &Path) -> std::io::Result<Option<PathBuf>> {
    if portable_marker(exe_dir).is_none() {
        return Ok(None);
    }
    let data = exe_dir.join(PORTABLE_DATA_DIR);
    probe(&data)?;
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("cadcraft-portable-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn absent_marker_and_directory_named_marker_do_not_enable_portable_mode() {
        let temp = Temp::new("no-marker");
        assert_eq!(directory_in(&temp.0).unwrap(), None);
        std::fs::create_dir(temp.0.join("portable.txt")).unwrap();
        assert_eq!(directory_in(&temp.0).unwrap(), None);
        assert!(!temp.0.join("CADCraftData").exists());
    }

    #[test]
    fn marker_selects_adjacent_data_and_preserves_existing_settings() {
        let temp = Temp::new("settings-é");
        std::fs::write(temp.0.join("portable.txt"), "").unwrap();
        let data = directory_in(&temp.0).unwrap().unwrap();
        assert_eq!(data, temp.0.join("CADCraftData"));
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0);
        std::fs::write(data.join("workspace.json"), "existing settings").unwrap();
        assert_eq!(directory_in(&temp.0).unwrap(), Some(data.clone()));
        assert_eq!(std::fs::read_to_string(data.join("workspace.json")).unwrap(), "existing settings");
    }

    #[test]
    fn blocked_data_folder_is_an_error() {
        let temp = Temp::new("blocked");
        std::fs::write(temp.0.join("portable.txt"), "").unwrap();
        std::fs::write(temp.0.join("CADCraftData"), "a file blocks the directory").unwrap();
        assert!(directory_in(&temp.0).is_err());
    }

    #[test]
    fn independent_copies_have_independent_settings() {
        let a = Temp::new("copy-a");
        let b = Temp::new("copy-b");
        for temp in [&a, &b] {
            std::fs::write(temp.0.join("portable.txt"), "").unwrap();
        }
        assert_ne!(directory_in(&a.0).unwrap(), directory_in(&b.0).unwrap());
    }
}
