//! AutoCAD-compatible drawing lock files (`.dwl` and `.dwl2`).
//!
//! When a drawing (`.dwg` or `.dxf`) is opened from the filesystem, CADCraft acquires a lock
//! by creating sibling `<drawing>.dwl` and `<drawing>.dwl2` files.
//!
//! If a lock file already exists:
//! - CADCraft reads the lock information (username, machine name, timestamp).
//! - If the lock is held by another user/process, the drawing is opened in **read-only** mode.
//! - Read-only drawings prevent accidental overwrites (`QSAVE` fails and prompts `SAVEAS`).
//!
//! When the drawing is closed or saved to another file via `SAVEAS`, the lock guard deletes
//! the `.dwl` and `.dwl2` files.

use std::path::{Path, PathBuf};

/// Drawing lock information parsed from an existing `.dwl` or `.dwl2` file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LockInfo {
    pub user: String,
    pub machine: String,
    pub date_time: String,
    pub drawing_path: Option<String>,
}

impl LockInfo {
    /// Human-readable description of who holds the lock.
    pub fn description(&self) -> String {
        if !self.machine.is_empty() {
            format!("in use by {} on {} ({})", self.user, self.machine, self.date_time)
        } else {
            format!("in use by {} ({})", self.user, self.date_time)
        }
    }
}

/// Sibling `.dwl` path for `drawing_path`.
pub fn dwl_path(drawing_path: &Path) -> PathBuf {
    drawing_path.with_extension("dwl")
}

/// Sibling `.dwl2` path for `drawing_path`.
pub fn dwl2_path(drawing_path: &Path) -> PathBuf {
    drawing_path.with_extension("dwl2")
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{LockInfo, Path, PathBuf, dwl_path, dwl2_path};

    /// RAII guard that holds a drawing lock and deletes `.dwl` and `.dwl2` on drop.
    #[derive(Debug)]
    pub struct DrawingLockGuard {
        dwl: PathBuf,
        dwl2: PathBuf,
        active: bool,
    }

    impl DrawingLockGuard {
        pub fn new(dwl: PathBuf, dwl2: PathBuf) -> Self {
            Self { dwl, dwl2, active: true }
        }

        /// Explicitly release and remove the lock files.
        pub fn release(&mut self) {
            if self.active {
                self.active = false;
                let _ = std::fs::remove_file(&self.dwl);
                let _ = std::fs::remove_file(&self.dwl2);
            }
        }

        pub fn dwl(&self) -> &Path {
            &self.dwl
        }

        pub fn dwl2(&self) -> &Path {
            &self.dwl2
        }
    }

    impl Drop for DrawingLockGuard {
        fn drop(&mut self) {
            self.release();
        }
    }

    /// Result of attempting to acquire a drawing lock.
    pub enum LockResult {
        /// Lock acquired successfully.
        Acquired(DrawingLockGuard),
        /// Already locked by another session or user.
        AlreadyLocked(LockInfo),
        /// Failed to write lock files (e.g. read-only filesystem or insufficient permissions).
        Failed(String),
    }

    fn current_user() -> String {
        std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "User".to_string())
    }

    fn current_machine() -> String {
        std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "localhost".to_string())
    }

    fn xml_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
        out
    }

    fn extract_tag(content: &str, tag: &str) -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start_pos = content.find(&open)?;
        let val_start = start_pos + open.len();
        let end_rel = content[val_start..].find(&close)?;
        Some(content[val_start..val_start + end_rel].trim().to_string())
    }

    pub fn parse_dwl2(content: &str) -> Option<LockInfo> {
        let user = extract_tag(content, "UserName")?;
        let machine = extract_tag(content, "MachineName").unwrap_or_default();
        let date_time = extract_tag(content, "DateTime").unwrap_or_default();
        let drawing_path = extract_tag(content, "DrawingPath");
        Some(LockInfo { user, machine, date_time, drawing_path })
    }

    pub fn parse_dwl(content: &str) -> Option<LockInfo> {
        let clean = content.trim_start_matches('\u{feff}');
        let mut lines = clean.lines().map(str::trim);
        let user = lines.next()?.to_string();
        let machine = lines.next().unwrap_or("").to_string();
        let date_time = lines.next().unwrap_or("").to_string();
        if user.is_empty() && machine.is_empty() {
            return None;
        }
        Some(LockInfo { user, machine, date_time, drawing_path: None })
    }

    /// Read lock information from `<drawing>.dwl2` or `<drawing>.dwl` if either exists.
    pub fn read_lock_info(drawing_path: &Path) -> Option<LockInfo> {
        let dwl2 = dwl2_path(drawing_path);
        if dwl2.is_file()
            && let Ok(content) = std::fs::read_to_string(&dwl2)
            && let Some(info) = parse_dwl2(&content)
        {
            return Some(info);
        }
        let dwl = dwl_path(drawing_path);
        if dwl.is_file()
            && let Ok(content) = std::fs::read_to_string(&dwl)
            && let Some(info) = parse_dwl(&content)
        {
            return Some(info);
        }
        None
    }

    /// Attempt to acquire lock files for `drawing_path`.
    pub fn try_acquire_lock(drawing_path: &Path) -> LockResult {
        let dwl = dwl_path(drawing_path);
        let dwl2 = dwl2_path(drawing_path);

        let user = current_user();
        let machine = current_machine();
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let dt = chrono::DateTime::from_timestamp(secs, 0);
        let date_text = dt.map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string()).unwrap_or_default();
        let iso_date = dt.map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
        let path_str = drawing_path.to_string_lossy();

        let dwl_content = format!("{user}\r\n{machine}\r\n{date_text}\r\n");
        let esc_user = xml_escape(&user);
        let esc_machine = xml_escape(&machine);
        let esc_path = xml_escape(&path_str);
        let dwl2_content = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<DrawingLock xmlns=\"dwl.xsd\">\r\n  <UserName>{esc_user}</UserName>\r\n  <MachineName>{esc_machine}</MachineName>\r\n  <DateTime>{iso_date}</DateTime>\r\n  <DrawingPath>{esc_path}</DrawingPath>\r\n</DrawingLock>\r\n"
        );

        // Atomic create_new to prevent TOCTOU race conditions
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&dwl) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Some(info) = read_lock_info(drawing_path) {
                    return LockResult::AlreadyLocked(info);
                }
                return LockResult::AlreadyLocked(LockInfo {
                    user: "Unknown".to_string(),
                    machine: String::new(),
                    date_time: "unknown time".to_string(),
                    drawing_path: None,
                });
            }
            Err(e) => return LockResult::Failed(format!("Cannot create {}: {e}", dwl.display())),
        };

        use std::io::Write;
        if let Err(e) = file.write_all(dwl_content.as_bytes()) {
            let _ = std::fs::remove_file(&dwl);
            return LockResult::Failed(format!("Cannot write {}: {e}", dwl.display()));
        }
        drop(file);

        if let Err(e) = std::fs::write(&dwl2, dwl2_content) {
            let _ = std::fs::remove_file(&dwl);
            return LockResult::Failed(format!("Cannot write {}: {e}", dwl2.display()));
        }

        LockResult::Acquired(DrawingLockGuard::new(dwl, dwl2))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;

    #[derive(Clone, Debug)]
    pub struct DrawingLockGuard;

    pub enum LockResult {
        Acquired(DrawingLockGuard),
        AlreadyLocked(LockInfo),
        Failed(String),
    }

    pub fn read_lock_info(_drawing_path: &Path) -> Option<LockInfo> {
        None
    }

    pub fn try_acquire_lock(_drawing_path: &Path) -> LockResult {
        LockResult::Acquired(DrawingLockGuard)
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dwl_paths_derive_correctly() {
        let p = Path::new("tests/floorplan.dwg");
        assert_eq!(dwl_path(p), PathBuf::from("tests/floorplan.dwl"));
        assert_eq!(dwl2_path(p), PathBuf::from("tests/floorplan.dwl2"));

        let dxf = Path::new("tests/sample.dxf");
        assert_eq!(dwl_path(dxf), PathBuf::from("tests/sample.dwl"));
        assert_eq!(dwl2_path(dxf), PathBuf::from("tests/sample.dwl2"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parse_dwl_text() {
        let text = "johndoe\nWORKSTATION-1\nFriday, October 30, 2026  08:15:30 PM\n";
        let info = parse_dwl(text).expect("parsed dwl");
        assert_eq!(info.user, "johndoe");
        assert_eq!(info.machine, "WORKSTATION-1");
        assert_eq!(info.date_time, "Friday, October 30, 2026  08:15:30 PM");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parse_dwl2_xml() {
        let xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<DrawingLock xmlns=\"dwl.xsd\">\n  <UserName>alice</UserName>\n  <MachineName>CAD-PC</MachineName>\n  <DateTime>2026-10-30T20:15:00</DateTime>\n  <DrawingPath>C:\\Projects\\site.dwg</DrawingPath>\n</DrawingLock>\n";
        let info = parse_dwl2(xml).expect("parsed dwl2");
        assert_eq!(info.user, "alice");
        assert_eq!(info.machine, "CAD-PC");
        assert_eq!(info.date_time, "2026-10-30T20:15:00");
        assert_eq!(info.drawing_path.as_deref(), Some("C:\\Projects\\site.dwg"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn acquire_and_release_lock_lifecycle() {
        let temp_dir = std::env::temp_dir();
        let fake_drawing = temp_dir.join("cadcraft_test_lock_dummy.dwg");

        // Clean up any leftovers
        let _ = std::fs::remove_file(dwl_path(&fake_drawing));
        let _ = std::fs::remove_file(dwl2_path(&fake_drawing));

        match try_acquire_lock(&fake_drawing) {
            LockResult::Acquired(guard) => {
                assert!(guard.dwl().is_file(), "dwl should exist");
                assert!(guard.dwl2().is_file(), "dwl2 should exist");

                // Attempting second lock while guard is alive must report AlreadyLocked
                match try_acquire_lock(&fake_drawing) {
                    LockResult::AlreadyLocked(info) => {
                        assert!(!info.user.is_empty());
                    }
                    _ => panic!("Expected AlreadyLocked"),
                }

                // Drop guard explicitly
                drop(guard);
            }
            _ => panic!("Expected Acquired"),
        }

        // After dropping guard, lock files must be gone
        assert!(!dwl_path(&fake_drawing).exists());
        assert!(!dwl2_path(&fake_drawing).exists());
    }
}
