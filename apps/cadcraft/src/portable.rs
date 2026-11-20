//! Windows portable ZIP settings, selected by a marker beside the executable.

use std::path::{Path, PathBuf};

/// A marker must be a file. Prove the data folder is writable before selecting it.
pub(crate) fn directory_in(exe_dir: &Path) -> std::io::Result<Option<PathBuf>> {
    if !exe_dir.join("portable.txt").is_file() {
        return Ok(None);
    }
    let data = exe_dir.join("CADCraftData");
    std::fs::create_dir_all(&data)?;
    let probe = data.join(format!(".portable-write-test-{}", std::process::id()));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
    drop(file);
    std::fs::remove_file(probe)?;
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
