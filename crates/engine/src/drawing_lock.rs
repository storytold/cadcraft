//! AutoCAD-compatible drawing lock files (`.dwl` and `.dwl2`).
//!
//! When a `.dwg` drawing is opened from the filesystem, CADCraft acquires a lock
//! by creating sibling `<drawing>.dwl` (3-line plain text) and `<drawing>.dwl2`
//! (`<whohas>` XML) files atomically (`create_new`).
//!
//! If a lock file already exists:
//! - CADCraft reads the lock metadata (username, machine name, timestamp, and optional PID).
//! - If the lock belongs to the same user and machine and its recorded process is no longer
//!   running (for example after a crash), CADCraft reclaims the stale lock automatically.
//! - Otherwise, the drawing opens in **read-only** mode to prevent accidental overwrites
//!   (`QSAVE` fails and suggests `SAVEAS`), and the user can still explicitly take over the
//!   lock via [`crate::Session::take_over_lock`] after seeing who holds it.
//!
//! When the drawing is closed or saved to a different file via `SAVEAS`, the lock guard
//! deletes the `.dwl` and `.dwl2` files.

use std::path::{Path, PathBuf};

/// Maximum size in bytes of a `.dwl` or `.dwl2` lock file read from disk.
#[cfg(not(target_arch = "wasm32"))]
const MAX_LOCK_FILE_BYTES: usize = 64 * 1024;

/// Drawing lock information parsed from an existing `.dwl` or `.dwl2` file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LockInfo {
    pub user: String,
    pub machine: String,
    pub date_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
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
    use super::{LockInfo, MAX_LOCK_FILE_BYTES, Path, PathBuf, dwl_path, dwl2_path};

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
                remove_regular_lock_file(&self.dwl);
                remove_regular_lock_file(&self.dwl2);
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
        /// Already locked by another active session or user.
        AlreadyLocked(LockInfo),
        /// Failed to write lock files (e.g. read-only filesystem or insufficient permissions).
        Failed(String),
    }

    /// Current OS user name used in lock files and stale-lock ownership checks.
    pub fn current_user() -> String {
        std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "User".to_string())
    }

    /// Current machine name used in lock files and stale-lock ownership checks.
    pub fn current_machine() -> String {
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

    fn xml_unescape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            rest = &rest[amp..];
            if let Some(after) = rest.strip_prefix("&lt;") {
                out.push('<');
                rest = after;
            } else if let Some(after) = rest.strip_prefix("&gt;") {
                out.push('>');
                rest = after;
            } else if let Some(after) = rest.strip_prefix("&quot;") {
                out.push('"');
                rest = after;
            } else if let Some(after) = rest.strip_prefix("&apos;") {
                out.push('\'');
                rest = after;
            } else if let Some(after) = rest.strip_prefix("&amp;") {
                out.push('&');
                rest = after;
            } else {
                out.push('&');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);
        out
    }

    /// Extract the trimmed, unescaped text of `<tag>...</tag>` case-insensitively (ASCII).
    fn extract_tag(content: &str, tag: &str) -> Option<String> {
        // `to_ascii_lowercase` only folds 1-byte ASCII A-Z -> a-z, so UTF-8 byte offsets stay identical.
        let lower = content.to_ascii_lowercase();
        let tag_lower = tag.to_ascii_lowercase();
        let open = format!("<{tag_lower}>");
        let close = format!("</{tag_lower}>");
        let start_pos = lower.find(&open)?;
        let val_start = start_pos + open.len();
        let end_rel = lower[val_start..].find(&close)?;
        Some(xml_unescape(content[val_start..val_start + end_rel].trim()))
    }

    /// Parse AutoCAD's `<whohas>` XML `.dwl2` format (tags matched case-insensitively).
    pub fn parse_dwl2(content: &str) -> Option<LockInfo> {
        let user = extract_tag(content, "username")?;
        let machine = extract_tag(content, "machinename").unwrap_or_default();
        let date_time = extract_tag(content, "datetime").unwrap_or_default();
        if user.is_empty() && machine.is_empty() {
            return None;
        }
        let pid = extract_tag(content, "pid").and_then(|s| s.parse::<u32>().ok());
        Some(LockInfo { user, machine, date_time, pid })
    }

    /// Parse AutoCAD's 3-line plain-text `.dwl` format (stripping any UTF-8 BOM).
    pub fn parse_dwl(content: &str) -> Option<LockInfo> {
        let clean = content.trim_start_matches('\u{feff}');
        let mut lines = clean.lines().map(str::trim);
        let user = lines.next()?.to_string();
        let machine = lines.next().unwrap_or("").to_string();
        let date_time = lines.next().unwrap_or("").to_string();
        if user.is_empty() && machine.is_empty() {
            return None;
        }
        Some(LockInfo { user, machine, date_time, pid: None })
    }

    /// True if `p` is a regular file (rejects symlinks, FIFOs, and directories).
    fn is_regular_file(p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
    }

    /// Remove `p` only if automation roots allow writing and `p` is a regular file (never follows symlinks).
    fn remove_regular_lock_file(p: &Path) {
        if crate::file_access::check_write(p).is_ok() && is_regular_file(p) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Read a regular lock file up to [`MAX_LOCK_FILE_BYTES`] while honoring automation read roots.
    /// On Unix, opens with `O_NOFOLLOW` and checks `fstat` on the opened handle to eliminate
    /// symlink-swap TOCTOU races (CWE-59).
    fn read_lock_file_capped(p: &Path) -> Option<String> {
        if crate::file_access::check_read(p).is_err() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::io::Read;
            use std::os::unix::fs::OpenOptionsExt;

            let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(p).ok()?;
            let meta = file.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let cap = MAX_LOCK_FILE_BYTES as u64;
            let mut bytes = Vec::with_capacity(usize::try_from(meta.len().min(cap)).unwrap_or(0));
            file.take(cap + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() > MAX_LOCK_FILE_BYTES {
                return None;
            }
            String::from_utf8(bytes).ok()
        }
        #[cfg(not(unix))]
        {
            if !is_regular_file(p) {
                return None;
            }
            let bytes = crate::capped_read::read_capped(p, MAX_LOCK_FILE_BYTES).ok()?;
            String::from_utf8(bytes).ok()
        }
    }

    /// Check whether `pid` currently refers to a live OS process without using `unsafe`.
    /// Fails safe (`true`) if the OS refuses inspection so live locks are never falsely reclaimed.
    pub fn is_process_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        if pid == std::process::id() {
            return true;
        }
        #[cfg(target_os = "linux")]
        {
            let stat_path = format!("/proc/{pid}/stat");
            match std::fs::read_to_string(&stat_path) {
                Ok(stat) => {
                    // Format: "<pid> (<comm>) <state> ..."; state 'Z' is a zombie process.
                    if let Some(after_comm) = stat.rfind(')').and_then(|i| stat.get(i + 1..)) {
                        !after_comm.trim_start().starts_with('Z')
                    } else {
                        true
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(_) => true,
            }
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            let kill_bin = if Path::new("/bin/kill").is_file() { "/bin/kill" } else { "kill" };
            match std::process::Command::new(kill_bin).env("LC_ALL", "C").args(["-0", &pid.to_string()]).output() {
                Ok(out) if out.status.success() => true,
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
                    if stderr.contains("operation not permitted") || stderr.contains("eperm") {
                        true
                    } else if stderr.contains("no such process") || stderr.contains("esrch") {
                        false
                    } else {
                        true
                    }
                }
                Err(_) => true,
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let tasklist_bin = std::env::var_os("SystemRoot")
                .map(|root| PathBuf::from(root).join("System32").join("tasklist.exe"))
                .filter(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from("tasklist.exe"));
            match std::process::Command::new(tasklist_bin)
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
            {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    stdout.contains(&format!(",\"{pid}\","))
                }
                _ => true,
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            true
        }
    }

    /// True if `info` belongs to the current user on this machine and its recorded process has exited.
    pub fn is_stale_lock(info: &LockInfo) -> bool {
        let Some(pid) = info.pid else {
            return false;
        };
        if info.user.is_empty() || info.machine.is_empty() {
            return false;
        }
        info.user.eq_ignore_ascii_case(&current_user()) && info.machine.eq_ignore_ascii_case(&current_machine()) && !is_process_alive(pid)
    }

    /// Whether two filesystem paths refer to the same drawing (resolving relative/case differences).
    pub fn same_path(a: &Path, b: &Path) -> bool {
        if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            return ca == cb;
        }
        #[cfg(windows)]
        {
            a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
        }
        #[cfg(not(windows))]
        {
            a == b
        }
    }

    /// Whether a sibling `.dwl` or `.dwl2` entry exists for `drawing_path` (even if unparseable).
    pub fn has_lock_files(drawing_path: &Path) -> bool {
        let dwl = dwl_path(drawing_path);
        let dwl2 = dwl2_path(drawing_path);
        (crate::file_access::check_read(&dwl).is_ok() && std::fs::symlink_metadata(&dwl).is_ok())
            || (crate::file_access::check_read(&dwl2).is_ok() && std::fs::symlink_metadata(&dwl2).is_ok())
    }

    /// Read lock information from `<drawing>.dwl2` or `<drawing>.dwl` if either exists.
    pub fn read_lock_info(drawing_path: &Path) -> Option<LockInfo> {
        let dwl2 = dwl2_path(drawing_path);
        if let Some(content) = read_lock_file_capped(&dwl2)
            && let Some(info) = parse_dwl2(&content)
        {
            return Some(info);
        }
        let dwl = dwl_path(drawing_path);
        if let Some(content) = read_lock_file_capped(&dwl)
            && let Some(info) = parse_dwl(&content)
        {
            return Some(info);
        }
        None
    }

    fn unknown_lock_info() -> LockInfo {
        LockInfo { user: "Unknown".to_string(), machine: String::new(), date_time: "unknown time".to_string(), pid: None }
    }

    /// Attempt to acquire lock files for `drawing_path` atomically (`create_new`), respecting
    /// automation roots and automatically reclaiming stale locks from dead local processes.
    pub fn try_acquire_lock(drawing_path: &Path) -> LockResult {
        try_acquire_lock_inner(drawing_path, true)
    }

    /// Explicitly take over an existing lock by removing regular `.dwl`/`.dwl2` files (after
    /// verifying automation write permissions) and acquiring a fresh atomic lock.
    pub fn force_acquire_lock(drawing_path: &Path) -> LockResult {
        let dwl = dwl_path(drawing_path);
        let dwl2 = dwl2_path(drawing_path);

        if let Err(e) = crate::file_access::check_write(&dwl) {
            return LockResult::Failed(format!("Lock write refused for {}: {e}", dwl.display()));
        }
        if let Err(e) = crate::file_access::check_write(&dwl2) {
            return LockResult::Failed(format!("Lock write refused for {}: {e}", dwl2.display()));
        }

        remove_regular_lock_file(&dwl);
        remove_regular_lock_file(&dwl2);
        try_acquire_lock_inner(drawing_path, false)
    }

    fn try_acquire_lock_inner(drawing_path: &Path, allow_stale_reclaim: bool) -> LockResult {
        let dwl = dwl_path(drawing_path);
        let dwl2 = dwl2_path(drawing_path);

        if let Err(e) = crate::file_access::check_write(&dwl) {
            return LockResult::Failed(format!("Lock write refused for {}: {e}", dwl.display()));
        }
        if let Err(e) = crate::file_access::check_write(&dwl2) {
            return LockResult::Failed(format!("Lock write refused for {}: {e}", dwl2.display()));
        }

        let user = current_user();
        let machine = current_machine();
        let pid = std::process::id();
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let dt = chrono::DateTime::from_timestamp(secs, 0);
        let date_text = dt.map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string()).unwrap_or_default();

        let dwl_content = format!("{user}\r\n{machine}\r\n{date_text}\r\n");
        let esc_user = xml_escape(&user);
        let esc_machine = xml_escape(&machine);
        let dwl2_content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<whohas>\r\n  <username>{esc_user}</username>\r\n  <machinename>{esc_machine}</machinename>\r\n  <datetime>{date_text}</datetime>\r\n  <pid>{pid}</pid>\r\n</whohas>\r\n"
        );

        use std::io::Write;
        // Atomic create_new on .dwl prevents TOCTOU races and refuses pre-existing symlinks (CWE-59).
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&dwl) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = read_lock_info(drawing_path);
                if allow_stale_reclaim && existing.as_ref().is_some_and(is_stale_lock) {
                    remove_regular_lock_file(&dwl);
                    remove_regular_lock_file(&dwl2);
                    return try_acquire_lock_inner(drawing_path, false);
                }
                return LockResult::AlreadyLocked(existing.unwrap_or_else(unknown_lock_info));
            }
            Err(e) => return LockResult::Failed(format!("Cannot create {}: {e}", dwl.display())),
        };

        if let Err(e) = file.write_all(dwl_content.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&dwl);
            return LockResult::Failed(format!("Cannot write {}: {e}", dwl.display()));
        }
        drop(file);

        // Atomic create_new on .dwl2 prevents symlink overwrite (CWE-59) and clobbering existing .dwl2 locks.
        let mut file2 = match std::fs::OpenOptions::new().write(true).create_new(true).open(&dwl2) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = std::fs::remove_file(&dwl);
                let existing = read_lock_info(drawing_path);
                if allow_stale_reclaim && existing.as_ref().is_some_and(is_stale_lock) {
                    remove_regular_lock_file(&dwl2);
                    return try_acquire_lock_inner(drawing_path, false);
                }
                return LockResult::AlreadyLocked(existing.unwrap_or_else(unknown_lock_info));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dwl);
                return LockResult::Failed(format!("Cannot create {}: {e}", dwl2.display()));
            }
        };

        if let Err(e) = file2.write_all(dwl2_content.as_bytes()) {
            drop(file2);
            let _ = std::fs::remove_file(&dwl);
            let _ = std::fs::remove_file(&dwl2);
            return LockResult::Failed(format!("Cannot write {}: {e}", dwl2.display()));
        }
        drop(file2);

        LockResult::Acquired(DrawingLockGuard::new(dwl, dwl2))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{LockInfo, Path};

    #[derive(Clone, Debug)]
    pub struct DrawingLockGuard;

    pub enum LockResult {
        Acquired(DrawingLockGuard),
        AlreadyLocked(LockInfo),
        Failed(String),
    }

    pub fn same_path(a: &Path, b: &Path) -> bool {
        a == b
    }

    pub fn has_lock_files(_drawing_path: &Path) -> bool {
        false
    }

    pub fn is_stale_lock(_info: &LockInfo) -> bool {
        false
    }

    pub fn read_lock_info(_drawing_path: &Path) -> Option<LockInfo> {
        None
    }

    pub fn try_acquire_lock(_drawing_path: &Path) -> LockResult {
        LockResult::Acquired(DrawingLockGuard)
    }

    pub fn force_acquire_lock(_drawing_path: &Path) -> LockResult {
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
    fn parse_dwl_text_with_bom() {
        let text = "\u{feff}johndoe\r\nWORKSTATION-1\r\nFriday, October 30, 2026  08:15:30 PM\r\n";
        let info = parse_dwl(text).expect("parsed dwl");
        assert_eq!(info.user, "johndoe");
        assert_eq!(info.machine, "WORKSTATION-1");
        assert_eq!(info.date_time, "Friday, October 30, 2026  08:15:30 PM");
        assert_eq!(info.pid, None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parse_dwl2_whohas_xml_unescapes_entities_once() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<whohas>\n  <username>alice &amp; bob &amp;lt;cad&amp;gt;</username>\n  <machinename>CAD-PC</machinename>\n  <datetime>2026-10-30 20:15:00 UTC</datetime>\n  <pid>4242</pid>\n</whohas>\n";
        let info = parse_dwl2(xml).expect("parsed dwl2");
        assert_eq!(info.user, "alice & bob &lt;cad&gt;");
        assert_eq!(info.machine, "CAD-PC");
        assert_eq!(info.date_time, "2026-10-30 20:15:00 UTC");
        assert_eq!(info.pid, Some(4242));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parse_dwl2_handles_mixed_case_tags() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<whohas>\n  <UserName>Jane</UserName>\n  <MachineName>DESK-2</MachineName>\n  <DateTime>2026-11-25 12:00:00 UTC</DateTime>\n</whohas>\n";
        let info = parse_dwl2(xml).expect("parsed dwl2");
        assert_eq!(info.user, "Jane");
        assert_eq!(info.machine, "DESK-2");
        assert_eq!(info.date_time, "2026-11-25 12:00:00 UTC");
        assert_eq!(info.pid, None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn acquire_and_release_lock_lifecycle() {
        let unique_dir = std::env::temp_dir().join(format!("cadcraft-dwl-unit-{}", std::process::id()));
        std::fs::create_dir_all(&unique_dir).expect("create test dir");
        let fake_drawing = unique_dir.join("dummy.dwg");

        let _ = std::fs::remove_file(dwl_path(&fake_drawing));
        let _ = std::fs::remove_file(dwl2_path(&fake_drawing));

        match try_acquire_lock(&fake_drawing) {
            LockResult::Acquired(guard) => {
                assert!(guard.dwl().is_file(), "dwl should exist");
                assert!(guard.dwl2().is_file(), "dwl2 should exist");

                let xml = std::fs::read_to_string(guard.dwl2()).expect("read dwl2");
                assert!(xml.contains("<whohas>"), "dwl2 should use <whohas> root element");
                assert!(xml.contains("<username>"), "dwl2 should use <username>");
                assert!(xml.contains("<machinename>"), "dwl2 should use <machinename>");
                assert!(xml.contains("<datetime>"), "dwl2 should use <datetime>");
                assert!(xml.contains(&format!("<pid>{}</pid>", std::process::id())));

                match try_acquire_lock(&fake_drawing) {
                    LockResult::AlreadyLocked(info) => {
                        assert!(!info.user.is_empty());
                        assert_eq!(info.pid, Some(std::process::id()));
                    }
                    _ => panic!("Expected AlreadyLocked"),
                }

                drop(guard);
            }
            _ => panic!("Expected Acquired"),
        }

        assert!(!dwl_path(&fake_drawing).exists(), "dwl should be deleted on drop");
        assert!(!dwl2_path(&fake_drawing).exists(), "dwl2 should be deleted on drop");
        let _ = std::fs::remove_dir_all(&unique_dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stale_lock_detection_checks_user_machine_and_pid() {
        assert!(is_process_alive(std::process::id()));
        assert!(!is_process_alive(0));

        let live =
            LockInfo { user: current_user(), machine: current_machine(), date_time: "2026-11-25 12:00:00 UTC".into(), pid: Some(std::process::id()) };
        assert!(!is_stale_lock(&live));

        let dead_same_host =
            LockInfo { user: current_user(), machine: current_machine(), date_time: "2026-11-25 12:00:00 UTC".into(), pid: Some(999_999_999) };
        assert!(is_stale_lock(&dead_same_host));

        let dead_other_user = LockInfo {
            user: "definitely_another_user".into(),
            machine: current_machine(),
            date_time: "2026-11-25 12:00:00 UTC".into(),
            pid: Some(999_999_999),
        };
        assert!(!is_stale_lock(&dead_other_user));

        let no_pid = LockInfo { user: current_user(), machine: current_machine(), date_time: "2026-11-25 12:00:00 UTC".into(), pid: None };
        assert!(!is_stale_lock(&no_pid));
    }
}
