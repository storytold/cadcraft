//! Saving over a file that would lose content asks first, and only when a user saves. These run
//! in their own test binary because the engine's I/O hooks are set once per process, and the
//! library's unit tests register a writer that always fails.

use std::path::{Path, PathBuf};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, RawTag, Space, Unknown};
use serde_json::json;

use cadcraft_ui_egui::save_loss::{Choice, Pending};
use cadcraft_ui_egui::{CadApp, Services, menus};

const ORIGINAL: &[u8] = b"original";

fn unknown(kind: &str) -> EntityKind {
    EntityKind::Unknown(Unknown { dxf_type: kind.into(), tags: vec![RawTag { code: 8, value: "0".into() }] })
}

/// A drawing with objects of `types`, which DWG can't keep, opened from a DWG file on disk.
fn opened(tag: &str, types: &[&str]) -> (CadApp, PathBuf) {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-loss-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Plan.dwg");
    std::fs::write(&path, ORIGINAL).unwrap();
    let mut d = Drawing::new_metric();
    for t in types {
        d.add(&Space::Model, Common::default(), unknown(t)).unwrap();
    }
    let mut s = Session::empty();
    s.open_drawing(d, "Plan.dwg", Some(path.to_string_lossy().into_owned()));
    (CadApp::new(s, Services::default()), path)
}

fn cleanup(path: &Path) {
    if let Some(d) = path.parent() {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn a_users_save_asks_before_the_file_changes_and_cancel_leaves_it() {
    let (mut app, path) = opened("cancel", &["3DSOLID", "MLINE", "MLINE"]);
    menus::activate(&mut app, "qsave");
    let pending = app.save_loss.clone().expect("the save waits for an answer");
    assert_eq!(pending.summary(), "1 3DSOLID, 2 MLINE");
    assert!(app.shows_modal());
    assert_eq!(std::fs::read(&path).unwrap(), ORIGINAL, "nothing is written before the answer");

    app.answer_save_loss(Choice::Cancel);
    assert!(app.save_loss.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), ORIGINAL);
    cleanup(&path);
}

#[test]
fn save_anyway_saves_and_the_same_loss_is_not_asked_again() {
    let (mut app, path) = opened("anyway", &["MLINE"]);
    menus::activate(&mut app, "qsave");
    app.answer_save_loss(Choice::SaveAnyway);
    assert!(app.save_loss.is_none());
    assert_ne!(std::fs::read(&path).unwrap(), ORIGINAL);

    menus::activate(&mut app, "qsave");
    assert!(app.save_loss.is_none(), "already agreed to");

    // A new kind of loss is a new question.
    app.session.doc_mut().unwrap().add(&Space::Model, Common::default(), unknown("3DSOLID")).unwrap();
    menus::activate(&mut app, "qsave");
    assert_eq!(app.save_loss.as_ref().map(Pending::summary).as_deref(), Some("1 3DSOLID, 1 MLINE"));
    cleanup(&path);
}

#[test]
fn a_save_that_loses_nothing_does_not_ask() {
    let (mut app, path) = opened("clean", &[]);
    menus::activate(&mut app, "qsave");
    assert!(app.save_loss.is_none());
    assert_ne!(std::fs::read(&path).unwrap(), ORIGINAL);
    cleanup(&path);
}

/// Programmatic calls never open dialogs: the loss is in the result instead.
#[test]
fn programmatic_saves_never_ask() {
    let (mut app, path) = opened("json", &["MLINE"]);
    let r = app.run("qsave", json!({})).unwrap();
    assert!(app.save_loss.is_none());
    assert_eq!(r["notSaved"], json!({ "MLINE": 1 }));
    cleanup(&path);
}

/// Type names come from the file: however many, however long, the warning stays readable.
#[test]
fn a_hostile_list_of_types_is_capped() {
    let long = "X".repeat(5000);
    let many: Vec<String> = (0..50).map(|i| format!("{long}{i}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let (mut app, path) = opened("hostile", &refs);
    menus::activate(&mut app, "qsave");
    let s = app.save_loss.as_ref().unwrap().summary();
    assert!(s.chars().count() < 500, "{}", s.len());
    assert!(s.ends_with("42 more types"), "{s}");
    cleanup(&path);
}
