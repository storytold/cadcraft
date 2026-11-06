//! The folders automation may read and write (#1472).
//!
//! `cadcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>` confines the files
//! an agent's commands reach. [`AutomationRoots`] holds the two folders. While they are in force,
//! every path a command reads goes through [`check_read`] and every path it writes through
//! [`check_write`]:
//!
//! - The path is made absolute against the working directory, as the file system reads it. Its
//!   links are followed: for a file that doesn't exist yet, the links of its nearest existing
//!   folder, the rest of the path being plain names (`..` below a missing folder is refused, and so
//!   is a link that leads nowhere).
//! - The result must lie inside the root, compared by whole components (`/work2` isn't inside
//!   `/work`) and exactly: both sides have been through the file system, which gives each folder
//!   its real name, and a Windows folder can be case sensitive, with `Root` beside `root`.
//! - Reads may lie under either root; writes only under the write root, and never at the write
//!   root itself (its save-style temporary file would be outside). A root given alone grants
//!   reads under it even when it is the write root; a read root grants no writes.
//!
//! Roots are in force on a thread inside [`confine`]. The MCP headless backend runs every call in
//! it; without roots nothing is checked, so the app and the command line keep working everywhere.
//!
//! A path is checked, then opened: a process that can change the folders in between (swapping in
//! a link) can still win that race. The roots keep an agent to the files it was given; they don't
//! guard against other programs on the computer.

pub mod commands;

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Read access or write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// The folders automation may read and write; a root left out grants no access of its kind.
#[derive(Debug)]
pub struct AutomationRoots {
    read: Option<PathBuf>,
    write: Option<PathBuf>,
}

/// The folder at `path`, which must exist, made absolute with its links followed: what paths are
/// compared with.
fn open_root(path: &Path, access: Access) -> Result<PathBuf, String> {
    let kind = match access {
        Access::Read => "read",
        Access::Write => "write",
    };
    std::fs::canonicalize(path)
        .and_then(|p| if p.is_dir() { Ok(p) } else { Err(std::io::ErrorKind::NotADirectory.into()) })
        .map_err(|e| format!("cannot open the automation {kind} root `{}`: {e}", path.display()))
}

impl AutomationRoots {
    /// The roots `read` and `write`, which must be existing folders; `None` when neither is given
    /// (nothing is confined then).
    pub fn new(read: Option<&Path>, write: Option<&Path>) -> Result<Option<Arc<Self>>, String> {
        if read.is_none() && write.is_none() {
            return Ok(None);
        }
        let read = read.map(|p| open_root(p, Access::Read)).transpose()?;
        let write = write.map(|p| open_root(p, Access::Write)).transpose()?;
        Ok(Some(Arc::new(Self { read, write })))
    }

    /// The read root, absolute with its links followed.
    pub fn read_root(&self) -> Option<&Path> {
        self.read.as_deref()
    }

    /// The write root, absolute with its links followed.
    pub fn write_root(&self) -> Option<&Path> {
        self.write.as_deref()
    }

    /// May the file or folder at `path` be read (or written)? `Err` says why not.
    ///
    /// A read may lie under either root: the read root, or the write root (files the agent may
    /// write it may also read back). A write may lie only under the write root, and not at the
    /// root itself, whose save-style temporary file would sit beside it. Its name must stand there
    /// too: a link outside pointing in is refused, since what is written beside the name (a
    /// drawing's `.bak`) or done to it (a delete) happens in the folder that holds it.
    ///
    /// A path is taken as it is: turned into text on the way, a name that isn't text would name a
    /// different file.
    pub fn check(&self, given: impl AsRef<Path>, access: Access) -> Result<(), String> {
        let given = given.as_ref();
        let target = resolve(given)?;
        let path = given.display();
        if access == Access::Read {
            let roots = [self.read.as_ref(), self.write.as_ref()];
            if roots.iter().flatten().any(|r| inside(&target, r)) {
                return Ok(());
            }
            if roots.iter().all(|r| r.is_none()) {
                return Err(format!("automation filesystem access is not granted: read authority is absent: {path}"));
            }
            return Err(format!("automation path rejected: outside the read root (or the write root): {path}"));
        }
        let Some(folder) = self.write.as_ref() else {
            return Err(format!("automation filesystem access is not granted: write authority is absent: {path}"));
        };
        if !inside(&target, folder) {
            return Err(format!("automation path rejected: outside the write root {}: {path}", folder.display()));
        }
        // A write to the root itself would put its temporary file beside the root, outside it.
        if same(&target, folder) {
            return Err(format!("automation path rejected: the write root itself cannot be written: {path}"));
        }
        if !inside(&resolve(given.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))?, folder) {
            return Err(format!("automation path rejected: stands outside the write root {}: {path}", folder.display()));
        }
        Ok(())
    }
}

thread_local! {
    /// The roots in force on this thread.
    static ROOTS: RefCell<Option<Arc<AutomationRoots>>> = const { RefCell::new(None) };
}

/// Puts the roots it replaced back when dropped (also when `f` panics).
struct Restore(Option<Arc<AutomationRoots>>);

impl Drop for Restore {
    fn drop(&mut self) {
        ROOTS.with(|r| *r.borrow_mut() = self.0.take());
    }
}

/// Run `f` with `roots` in force on this thread (`None`: as things stand).
pub fn confine<T>(roots: Option<&Arc<AutomationRoots>>, f: impl FnOnce() -> T) -> T {
    let previous = ROOTS.with(|r| r.replace(roots.cloned()));
    let _restore = Restore(previous);
    f()
}

/// This thread's roots, if any.
pub fn current() -> Option<Arc<AutomationRoots>> {
    ROOTS.with(|r| r.borrow().clone())
}

/// May `path` be read here? Always, without roots in force.
pub fn check_read(path: impl AsRef<Path>) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Read))
}

/// May `path` be written here? Always, without roots in force.
pub fn check_write(path: impl AsRef<Path>) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Write))
}

/// `path` made absolute with its links followed; for a path that doesn't exist (yet), its nearest
/// existing folder's links followed and the rest appended.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = path.display();
    if path.as_os_str().is_empty() {
        return Err("automation path rejected: path is empty".into());
    }
    // Checked as given: making `…\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && path.components().any(|c| matches!(c, Component::Normal(n) if n.to_str().is_some_and(is_device_name))) {
        return Err(format!("automation path rejected: reserved device names are not allowed: {shown}"));
    }
    let absolute = std::path::absolute(path).map_err(|e| format!("automation path rejected: cannot resolve {shown}: {e}"))?;
    // The names below the nearest existing folder, last first.
    let mut missing: Vec<OsString> = vec![];
    let mut at = absolute.as_path();
    loop {
        match std::fs::canonicalize(at) {
            Ok(mut found) => {
                found.extend(missing.iter().rev());
                return Ok(found);
            }
            Err(e) => match std::fs::symlink_metadata(at) {
                Ok(m) if m.file_type().is_symlink() => return Err(format!("automation path rejected: a link that leads nowhere: {shown}")),
                // There, yet its real path can't be had (no permission): nothing to compare.
                Ok(_) => return Err(format!("automation path rejected: cannot resolve {shown}: {e}")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("automation path rejected: cannot resolve {shown}: {e}")),
            },
        }
        let parent = at.parent();
        match (at.components().next_back(), parent) {
            (Some(Component::Normal(name)), Some(parent)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            (Some(Component::ParentDir), _) => {
                return Err(format!("automation path rejected: parent traversal below a folder that doesn't exist: {shown}"));
            }
            // Down to the root (or drive) and it doesn't exist either.
            _ => {
                let e = std::io::Error::from(std::io::ErrorKind::NotFound);
                return Err(format!("automation path rejected: cannot resolve {shown}: {e}"));
            }
        }
    }
}

/// Is `path` (resolved) `root` or inside it, component by component?
fn inside(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    root.components().all(|r| path.next() == Some(r))
}

/// Is `path` (resolved) exactly `root`, compared like [`inside`] does?
fn same(path: &Path, root: &Path) -> bool {
    path.components().eq(root.components())
}

/// A Windows device name (`CON`, `NUL`, `COM1`, `LPT¹`… also with an extension or a `:`), which
/// names no file.
fn is_device_name(component: &str) -> bool {
    let stem = component.split(['.', ':']).next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
    };
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of a test's own, removed when the test ends, also when it fails.
    struct TestDir(PathBuf);

    impl std::ops::Deref for TestDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A new folder of this test's own: made here, so nothing else put it there first.
    fn fresh_dir(tag: &str) -> TestDir {
        let mut n = 0u32;
        loop {
            let dir = std::env::temp_dir().join(format!("cc-file-access-{tag}-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return TestDir(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => panic!("{}: {e}", dir.display()),
            }
        }
    }

    /// A fresh folder holding `in` (read root), `out` (write root) and `outside`.
    fn folders(name: &str) -> (TestDir, Arc<AutomationRoots>) {
        let base = fresh_dir(name);
        for d in ["in/sub", "out", "outside", "in2"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("in/a.dxf"), "0\nEOF\n").unwrap();
        std::fs::write(base.join("outside/secret.dxf"), "0\nEOF\n").unwrap();
        let roots = AutomationRoots::new(Some(&base.join("in")), Some(&base.join("out"))).unwrap().unwrap();
        (base, roots)
    }

    /// `p` as a string a command would pass.
    fn s(p: PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Paths under either root may be read; writes stay inside the write root.
    #[test]
    fn paths_inside_the_roots_pass_and_others_dont() {
        let (base, roots) = folders("inside");
        assert_eq!(roots.check(s(base.join("in/a.dxf")), Access::Read), Ok(()));
        assert_eq!(roots.check(s(base.join("in")), Access::Read), Ok(()), "the root itself");
        assert_eq!(roots.check(s(base.join("in/sub/new.dxf")), Access::Read), Ok(()), "missing files are checked by their folder");
        assert_eq!(roots.check(s(base.join("out/new/deeper/x.png")), Access::Write), Ok(()), "and by their nearest folder");
        let e = roots.check(s(base.join("outside/secret.dxf")), Access::Read).unwrap_err();
        assert!(e.starts_with("automation path rejected: outside the read root"), "{e}");
        // Reads are allowed under the write root too; writes are not allowed under the read root.
        assert_eq!(roots.check(s(base.join("out/x.png")), Access::Read), Ok(()), "the write root also grants reads");
        let e = roots.check(s(base.join("in/a.dxf")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
    }

    /// A write-only root still permits reads inside it; reads outside both roots are refused.
    #[test]
    fn reads_are_allowed_under_either_root() {
        let (base, _) = folders("either");
        let write_only = AutomationRoots::new(None, Some(&base.join("out"))).unwrap().unwrap();
        assert_eq!(write_only.check(s(base.join("out/new.dxf")), Access::Read), Ok(()), "a file to read back");
        let e = write_only.check(s(base.join("in/a.dxf")), Access::Read).unwrap_err();
        assert!(e.contains("outside the read root (or the write root)"), "{e}");
        let read_only = AutomationRoots::new(Some(&base.join("in")), None).unwrap().unwrap();
        assert_eq!(read_only.check(s(base.join("in/a.dxf")), Access::Read), Ok(()));
        assert!(read_only.check(s(base.join("out/new.dxf")), Access::Read).is_err(), "a read root alone grants no read of another folder");
    }

    /// A write to the write root itself would put its `.cadcraft-tmp` file beside the root: refused.
    #[test]
    fn writing_to_the_write_root_itself_is_refused() {
        let (base, roots) = folders("write-root");
        let out = s(base.join("out"));
        let sep = std::path::MAIN_SEPARATOR;
        for p in [out.clone(), format!("{out}{sep}."), format!("{out}{sep}..{sep}out")] {
            let e = roots.check(&p, Access::Write).unwrap_err();
            assert!(e.contains("the write root itself cannot be written"), "{p}: {e}");
        }
        assert_eq!(roots.check(&out, Access::Read), Ok(()), "reading the root stays allowed");
        assert_eq!(roots.check(s(base.join("out/new/x.png")), Access::Write), Ok(()), "a normal save inside still works");
    }

    /// `in2` starts like `in` and isn't inside it.
    #[test]
    fn a_sibling_with_the_same_prefix_is_outside() {
        let (base, roots) = folders("prefix");
        std::fs::write(base.join("in2/b.dxf"), "0\nEOF\n").unwrap();
        assert!(roots.check(s(base.join("in2/b.dxf")), Access::Read).is_err(), "in2 is not inside in");
        assert!(roots.check(format!("{}2/c.dxf", s(base.join("in"))), Access::Read).is_err());
    }

    /// `..` is followed where it can be, and refused below a missing folder.
    #[test]
    fn parent_traversal_is_resolved_or_refused() {
        let (base, roots) = folders("dotdot");
        let sep = std::path::MAIN_SEPARATOR;
        let up = format!("{}{sep}..{sep}outside{sep}secret.dxf", s(base.join("in")));
        assert!(roots.check(&up, Access::Read).unwrap_err().contains("outside the read root"), "`..` out of the root");
        let back = format!("{}{sep}..{sep}a.dxf", s(base.join("in/sub")));
        assert_eq!(roots.check(&back, Access::Read), Ok(()), "`..` that stays inside");
        let missing = format!("{}{sep}nope{sep}..{sep}..{sep}outside{sep}x.dxf", s(base.join("out")));
        let e = roots.check(&missing, Access::Write).unwrap_err();
        // Windows resolves `..` by name first (so does opening the file there): then it's outside.
        assert!(e.contains("parent traversal below a folder that doesn't exist") || e.contains("outside the write root"), "{e}");
    }

    /// A relative path means the working directory, and an empty one is refused.
    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        let (_base, roots) = folders("relative");
        // The tests' working directory is the crate's folder: not inside the roots.
        assert!(roots.check("a.dxf", Access::Read).unwrap_err().contains("outside the read root"));
        assert!(roots.check("", Access::Read).unwrap_err().contains("path is empty"));
    }

    /// A root must be a folder that exists; no roots at all confines nothing.
    #[test]
    fn a_missing_root_is_an_error_and_none_is_no_confinement() {
        let (base, _) = folders("missing");
        let e = AutomationRoots::new(Some(&base.join("nope")), None).unwrap_err();
        assert!(e.starts_with("cannot open the automation read root"), "{e}");
        let e = AutomationRoots::new(None, Some(&base.join("in/a.dxf"))).unwrap_err();
        assert!(
            e.starts_with("cannot open the automation write root")
                && e.ends_with(&std::io::Error::from(std::io::ErrorKind::NotADirectory).to_string()),
            "{e}"
        );
        assert!(AutomationRoots::new(Some(Path::new("")), None).unwrap_err().starts_with("cannot open the automation read root ``"));
        assert!(AutomationRoots::new(None, None).unwrap().is_none());
        // One root alone grants only its own access.
        let read_only = AutomationRoots::new(Some(&base.join("in")), None).unwrap().unwrap();
        let e = read_only.check(s(base.join("in/x.dxf")), Access::Write).unwrap_err();
        assert!(e.contains("write authority is absent"), "{e}");
    }

    /// Links are judged by where they lead, so a link out is refused and one inside is fine.
    #[test]
    fn links_are_followed_before_comparing() {
        let (base, roots) = folders("links");
        #[cfg(unix)]
        let made = |target: &Path, link: &Path| std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        let made = |target: &Path, link: &Path| {
            let made =
                if target.is_dir() { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) };
            made.is_ok()
        };
        #[cfg(not(any(unix, windows)))]
        let made = |_: &Path, _: &Path| false;
        // Windows needs Developer Mode or elevation to make links: nothing to test without them.
        if !made(&base.join("outside/secret.dxf"), &base.join("in/leak.dxf")) {
            eprintln!("skipped: symbolic links can't be made here");
            return;
        }
        assert!(roots.check(s(base.join("in/leak.dxf")), Access::Read).unwrap_err().contains("outside the read root"), "a file link out");
        assert!(made(&base.join("outside"), &base.join("out/away")));
        let e = roots.check(s(base.join("out/away/new.png")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "a folder link out: {e}");
        assert!(made(&base.join("outside/gone.dxf"), &base.join("out/dangling.png")));
        let e = roots.check(s(base.join("out/dangling.png")), Access::Write).unwrap_err();
        assert!(e.contains("a link that leads nowhere"), "writing would create the file outside: {e}");
        // A link to a folder inside the root stays inside.
        assert!(made(&base.join("in/sub"), &base.join("in/alias")));
        assert_eq!(roots.check(s(base.join("in/alias/x.dxf")), Access::Read), Ok(()));
        // A root given through a link is the folder it leads to.
        assert!(made(&base.join("in"), &base.join("in-link")));
        let linked = AutomationRoots::new(Some(&base.join("in-link")), None).unwrap().unwrap();
        assert_eq!(linked.check(s(base.join("in/a.dxf")), Access::Read), Ok(()));
    }

    /// Names are compared exactly: `Root` isn't `root`, which a case-sensitive folder can hold beside it.
    #[test]
    fn a_name_differing_only_in_case_is_not_the_root() {
        assert!(!inside(Path::new("/w/Root/x"), Path::new("/w/root")));
        assert!(!same(Path::new("/w/Root"), Path::new("/w/root")));
        assert!(inside(Path::new("/w/root/x"), Path::new("/w/root")));
        assert!(same(Path::new("/w/root"), Path::new("/w/root")));
    }

    /// The superscript ports, `COM0`, `LPT0` and the `:` form are device names.
    #[test]
    fn device_names_include_the_superscript_ports() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.dxf", "NUL ", "COM¹", "lpt³.txt", "com²", "NUL:", "con:stream", "COM0", "lpt0.txt"] {
            assert!(is_device_name(d), "{d}");
        }
        for d in ["console.txt", "COM", "COM10", "LPT⁴", "a.nul", "nullable"] {
            assert!(!is_device_name(d), "{d}");
        }
    }

    /// A name that isn't text is checked as itself: the text of it names a file that isn't there,
    /// which would pass, when the name itself is a link leading out.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_judged_as_itself() {
        use std::os::unix::ffi::OsStrExt;
        let (base, roots) = folders("not-text");
        let name = std::ffi::OsStr::from_bytes(b"a\xff.dxf");
        let link = base.join("in").join(name);
        // APFS and others refuse such a name: nothing to test there.
        if std::os::unix::fs::symlink(base.join("outside/secret.dxf"), &link).is_err() {
            eprintln!("skipped: this file system refuses a name that isn't text");
            return;
        }
        assert!(roots.check(&link, Access::Read).is_err());
        assert_eq!(roots.check(&*link.to_string_lossy(), Access::Read), Ok(()), "as text it is another file, which isn't there");
    }

    /// A name is judged by the folder it stands in as well as by where it leads.
    #[cfg(unix)]
    #[test]
    fn a_link_standing_outside_the_write_root_is_refused() {
        let (base, roots) = folders("entry");
        std::fs::write(base.join("out/t.dxf"), "x").unwrap();
        let link = base.join("outside/link.dxf");
        std::os::unix::fs::symlink(base.join("out/t.dxf"), &link).unwrap();
        assert!(roots.check(&link, Access::Write).unwrap_err().contains("stands outside the write root"));
        assert_eq!(roots.check(&link, Access::Read), Ok(()), "reading it reads what is inside");
        let inner = base.join("out/link.dxf");
        std::os::unix::fs::symlink(base.join("out/t.dxf"), &inner).unwrap();
        assert_eq!(roots.check(&inner, Access::Write), Ok(()));
        assert_eq!(roots.check(base.join("out/new/deep/x.dxf"), Access::Write), Ok(()));
    }

    /// On a real case-sensitive folder, `Root` beside the root `root` is outside it.
    #[cfg(windows)]
    #[test]
    fn a_case_sensitive_windows_folder_can_hold_a_sibling_of_the_root() {
        let base = fresh_dir("sensitive");
        let sensitive = std::process::Command::new("fsutil").args(["file", "setCaseSensitiveInfo"]).arg(&*base).arg("enable").output();
        if !sensitive.is_ok_and(|o| o.status.success()) {
            eprintln!("this runner can't make a case-sensitive folder: skipped");
            return;
        }
        std::fs::create_dir(base.join("root")).unwrap();
        std::fs::create_dir(base.join("Root")).unwrap();
        std::fs::write(base.join("Root/secret.dxf"), "x").unwrap();
        let roots = AutomationRoots::new(Some(&base.join("root")), Some(&base.join("root"))).unwrap().unwrap();
        for p in ["Root/secret.dxf", "Root/new.dxf"] {
            assert!(roots.check(&s(base.join(p)), Access::Read).is_err(), "{p}");
            assert!(roots.check(&s(base.join(p)), Access::Write).is_err(), "{p}");
        }
        assert_eq!(roots.check(&s(base.join("root/new.dxf")), Access::Write), Ok(()));
    }

    /// On an ordinary Windows folder any case finds the root, and device names are refused.
    #[cfg(windows)]
    #[test]
    fn windows_gives_a_root_its_real_spelling_and_refuses_devices() {
        let (base, roots) = folders("case");
        let upper = s(base.join("IN/A.DXF"));
        assert_eq!(roots.check(&upper, Access::Read), Ok(()), "{upper}");
        assert_eq!(roots.check(&s(base.join("In/Sub/New.dxf")), Access::Read), Ok(()));
        let e = roots.check(&s(base.join("out/NUL")), Access::Write).unwrap_err();
        assert!(e.contains("reserved device names"), "{e}");
        assert!(roots.check(&s(base.join("out/com1.txt")), Access::Write).is_err());
        assert!(roots.check(&s(base.join("out/COM¹")), Access::Write).is_err());
        assert_eq!(roots.check(&s(base.join("out/console.txt")), Access::Write), Ok(()));
    }

    /// Roots end with their scope, also when a nested scope or a panic ends it.
    #[test]
    fn scopes_nest_and_restore_after_a_panic() {
        let (base, roots) = folders("scopes");
        let outside = s(base.join("outside/secret.dxf"));
        assert_eq!(check_read(&outside), Ok(()), "nothing is confined by default");
        confine(Some(&roots), || {
            assert!(check_read(&outside).is_err());
            // A nested scope replaces the outer one and gives it back on the way out.
            confine(None, || assert_eq!(check_read(&outside), Ok(())));
            assert!(check_read(&outside).is_err(), "restored after the inner scope");
        });
        assert_eq!(check_read(&outside), Ok(()), "restored after confine");
        let r = std::panic::catch_unwind(|| confine(Some(&roots), || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(check_read(&outside), Ok(()), "restored after a panic");
        assert_eq!(confine(None, || check_read(&outside)), Ok(()));
    }

    /// The plain device names, and names that only look like them.
    #[test]
    fn device_names() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.svg", "NUL "] {
            assert!(is_device_name(d), "{d}");
        }
        for f in ["console", "com10", "lpt", "a.svg", "nul-x"] {
            assert!(!is_device_name(f), "{f}");
        }
    }
}
