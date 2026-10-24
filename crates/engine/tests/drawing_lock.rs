//! Drawing locks (#540): an app with a drawing open keeps a lock beside it; another app opening
//! the same drawing is told who has it, without taking the lock; the lock goes when the drawing
//! closes, and a stale one is taken over.

use std::time::{Duration, SystemTime};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::drawlock::{Locks, holder, lock_path};
use serde_json::json;

fn session_with(path: &str) -> Session {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let mut s = Session::new();
    s.execute("open", &json!({ "path": path })).unwrap();
    s
}

#[test]
fn a_second_app_is_told_who_has_the_drawing_and_the_lock_follows_the_first() {
    let dir = std::env::temp_dir().join(format!("cadcraft-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Shared Plan.dxf");
    std::fs::write(&file, cadcraft_io::write(&cadcraft_engine::doc::Drawing::new_metric(), "x.dxf").unwrap()).unwrap();
    let path = file.to_string_lossy().to_string();
    let now = Duration::ZERO;

    let mut a = session_with(&path);
    let mut la = Locks::with_heartbeat(now);
    la.sync(&a, "1-a");
    assert!(lock_path(&file).file_name().unwrap().to_string_lossy().starts_with(".Shared Plan.dxf"));
    assert_eq!(holder(&file).map(|h| h.instance), Some("1-a".to_string()));
    assert!(la.take_notes().is_empty());

    // A second app opening it is told, and the lock stays the first app's.
    let b = session_with(&path);
    let mut lb = Locks::with_heartbeat(now);
    lb.sync(&b, "2-b");
    let notes = lb.take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("Shared Plan.dxf is also open by ") && notes[0].contains("UTC"), "{}", notes[0]);
    lb.sync(&b, "2-b");
    assert!(lb.take_notes().is_empty(), "said once");
    assert_eq!(holder(&file).map(|h| h.instance), Some("1-a".to_string()));
    // WHOHAS names the holder.
    let mut b = b;
    let r = b.execute("whohas", &json!({})).unwrap();
    assert_eq!(r["holder"]["instance"], "1-a");

    // Releasing what isn't ours leaves it alone.
    lb.release_all("2-b");
    assert_eq!(holder(&file).map(|h| h.instance), Some("1-a".to_string()));

    // The first app closes the drawing: its lock goes, and the second takes it.
    a.execute("close", &json!({})).unwrap();
    la.sync(&a, "1-a");
    assert!(holder(&file).is_none());
    lb.sync(&b, "2-b");
    assert_eq!(holder(&file).map(|h| h.instance), Some("2-b".to_string()));

    // A lock left by a crashed app (not touched for a while) is taken over without a note.
    let stale = SystemTime::now() - Duration::from_secs(cadcraft_engine::drawlock::STALE_SECS + 60);
    std::fs::OpenOptions::new().write(true).open(lock_path(&file)).unwrap().set_modified(stale).unwrap();
    assert!(holder(&file).is_none(), "stale");
    let c = session_with(&path);
    let mut lc = Locks::with_heartbeat(now);
    lc.sync(&c, "3-c");
    assert!(lc.take_notes().is_empty());
    assert_eq!(holder(&file).map(|h| h.instance), Some("3-c".to_string()));
    // The app whose lock was taken over notices on its next heartbeat and doesn't touch it.
    lb.sync(&b, "2-b");
    assert_eq!(lb.take_notes().len(), 1, "told who has it now");
    assert_eq!(holder(&file).map(|h| h.instance), Some("3-c".to_string()));
    lc.release_all("3-c");
    assert!(!lock_path(&file).exists());
    let _ = std::fs::remove_dir_all(&dir);
}
