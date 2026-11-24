//! Reopening a path selects the existing drawing, preserving edits, even across Finder events.
#![cfg(not(target_arch = "wasm32"))]

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Files(PathBuf);

impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("cadcraft-reopen-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(Vec::new()), plot: None });
        Self(root)
    }

    fn drawing(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        // Synthetic input: the test reader returns an empty drawing for any bytes.
        std::fs::write(&path, b"0\nEOF\n").unwrap();
        path
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app() -> CadApp {
    CadApp::new(Session::empty(), Services::default())
}

fn open(app: &mut CadApp, path: &Path) {
    app.open_path(path);
}

fn settle(app: &mut CadApp) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.opening.busy() {
        app.finish_opening();
        assert!(Instant::now() < deadline, "background OPEN did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn reopening_dwg_and_dxf_preserves_edits_view_and_recent_files() {
    let files = Files::new();
    for name in ["检查井 成果.dwg", "管网 检测.dxf"] {
        let path = files.drawing(name);
        let mut app = app();
        open(&mut app, &path);
        settle(&mut app);
        app.run("circle", json!({"center": [1, 2], "radius": 3})).unwrap();
        let before = app.session.state().unwrap().clone();
        app.run("new", json!({})).unwrap();
        let count = app.session.docs.len();
        app.ui.start_tab = true;
        open(&mut app, &path);
        assert!(!app.opening.busy(), "an existing drawing must not be re-read");
        let after = app.session.state().unwrap();
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.doc, before.doc);
        assert_eq!(after.view(), before.view());
        assert_eq!(after.undo.len(), before.undo.len());
        assert!(after.is_dirty());
        assert_eq!(app.session.docs.len(), count);
        assert!(!app.ui.start_tab);
        assert_eq!(app.ui.recent_files.first().map(String::as_str), path.to_str());
    }
}

#[test]
fn reopening_a_deleted_file_keeps_the_unsaved_drawing() {
    let files = Files::new();
    let path = files.drawing("deleted.dxf");
    let mut app = app();
    open(&mut app, &path);
    settle(&mut app);
    app.run("circle", json!({"center": [1, 2], "radius": 3})).unwrap();
    let uid = app.session.state().unwrap().uid;
    std::fs::remove_file(&path).unwrap();
    app.run("new", json!({})).unwrap();
    open(&mut app, &path);
    assert!(!app.opening.busy());
    assert_eq!(app.session.state().unwrap().uid, uid);
    assert!(app.session.state().unwrap().is_dirty());
}

#[test]
fn repeated_requests_while_loading_add_one_tab_per_file() {
    let files = Files::new();
    let first = files.drawing("first.dxf");
    let second = files.drawing("second.dwg");
    let mut app = app();
    open(&mut app, &first);
    open(&mut app, &first);
    open(&mut app, &second);
    open(&mut app, &first);
    settle(&mut app);
    assert_eq!(app.session.docs.len(), 2);
    assert!(app.session.docs.iter().any(|d| d.path.as_deref() == first.to_str()));
    assert!(app.session.docs.iter().any(|d| d.path.as_deref() == second.to_str()));
}

#[cfg(unix)]
#[test]
fn symlinks_share_both_pending_reads_and_existing_dirty_tabs() {
    let files = Files::new();
    let path = files.drawing("drawing.dxf");
    let alias = files.0.join("alias.dxf");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let mut app = app();
    open(&mut app, &path);
    open(&mut app, &alias);
    settle(&mut app);
    assert_eq!(app.session.docs.len(), 1);
    app.run("circle", json!({"center": [1, 2], "radius": 3})).unwrap();
    let uid = app.session.state().unwrap().uid;
    app.run("new", json!({})).unwrap();
    open(&mut app, &alias);
    assert!(!app.opening.busy());
    assert_eq!(app.session.docs.len(), 2);
    assert_eq!(app.session.state().unwrap().uid, uid);
    assert!(app.session.state().unwrap().is_dirty());
}

#[test]
fn a_read_finishing_after_the_path_is_opened_keeps_existing_edits() {
    let files = Files::new();
    let path = files.drawing("pending.dxf");
    let mut app = app();
    open(&mut app, &path);
    // Simulate another OPEN/Save As assigning this path before the UI polls the background read.
    app.run("new", json!({})).unwrap();
    app.session.state_mut().unwrap().path = path.to_str().map(str::to_string);
    app.run("circle", json!({"center": [1, 2], "radius": 3})).unwrap();
    let uid = app.session.state().unwrap().uid;
    settle(&mut app);
    assert_eq!(app.session.docs.len(), 1);
    assert_eq!(app.session.state().unwrap().uid, uid);
    assert!(app.session.state().unwrap().is_dirty());
    assert_eq!(app.session.doc().unwrap().entity_count(), 1);
}
