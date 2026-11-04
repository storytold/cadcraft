//! The saved two-click preference disables drag windows without changing typed selection.
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, cmd::options};
use serde_json::json;

#[test]
fn pickdrag_is_saved_reset_and_validated() {
    let mut s = Session::new();
    assert_eq!(options::profile(&s)["PICKDRAG"], 2);
    s.execute("options", &json!({"PICKDRAG":0})).unwrap();
    let saved = options::profile(&s);
    let mut restored = Session::new();
    options::restore(&mut restored, &saved);
    assert!(!restored.settings.pickdrag);
    for value in [json!(1), json!(-1), json!(3), json!("bad"), json!(null), json!([0]), json!(1e300)] {
        assert!(s.execute("options", &json!({"PICKDRAG":value})).is_err());
        assert_eq!(options::profile(&s), saved);
    }
    s.execute("options", &json!({"reset":true})).unwrap();
    assert!(s.settings.pickdrag);
    options::restore(&mut restored, &json!({"PICKDRAG":1}));
    assert!(!restored.settings.pickdrag, "unsupported saved modes are ignored");
}

#[test]
fn two_click_selection_works_with_drag_disabled() {
    let mut s = Session::new();
    s.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    s.cmdline("PICKDRAG 0").unwrap();
    assert!(!s.begin_window(Vec2::new(-5.0, -5.0)));
    assert!(s.pending_window.is_none());
    s.idle_click(Vec2::new(-5.0, -5.0), false).unwrap();
    assert!(s.pending_window.is_some());
    s.idle_click(Vec2::new(15.0, 5.0), false).unwrap();
    assert!(s.pending_window.is_none());
    assert_eq!(s.selection().len(), 1);
    s.set_selection(Vec::new());
    s.cmdline("ERASE W -5,-5 15,5").unwrap();
    s.cmdline("").unwrap();
    assert!(s.doc().unwrap().model.is_empty());
    s.execute("setvar", &json!({"name":"PICKDRAG","value":2})).unwrap();
    assert!(s.begin_window(Vec2::new(-5.0, -5.0)));
}
