//! MBUTTONPAN (#2720): whether the middle button pans (1) or opens the object snap menu (0). A
//! profile variable: read and set like any other, saved, restored and reset with the profile.

use cadcraft_engine::{Session, cmd::options};
use serde_json::json;

#[test]
fn mbuttonpan_is_set_saved_reset_and_validated() {
    let mut s = Session::new();
    assert!(s.settings.mbuttonpan, "the middle button pans by default");
    assert_eq!(s.execute("getvar", &json!({"name":"MBUTTONPAN"})).unwrap()["value"], 1);

    // Typed as a bare name, through SETVAR and through LISP alike.
    s.cmdline("MBUTTONPAN 0").unwrap();
    assert!(!s.settings.mbuttonpan);
    s.execute("setvar", &json!({"name":"MBUTTONPAN","value":1})).unwrap();
    assert!(s.settings.mbuttonpan);
    s.cmdline("(setvar \"MBUTTONPAN\" 0)").unwrap();
    assert!(!s.settings.mbuttonpan);

    for bad in [json!(2), json!(-1), json!("on"), json!(null), json!([0]), json!(1e300)] {
        assert!(s.execute("setvar", &json!({"name":"MBUTTONPAN","value":bad})).is_err(), "{bad} refused");
        assert!(!s.settings.mbuttonpan, "{bad} left it as it was");
    }

    let saved = options::profile(&s);
    assert_eq!(saved["MBUTTONPAN"], 0);
    let mut restored = Session::new();
    options::restore(&mut restored, &saved);
    assert!(!restored.settings.mbuttonpan, "kept across restarts with the profile");
    s.execute("options", &json!({"reset":true})).unwrap();
    assert!(s.settings.mbuttonpan, "OPTIONS reset brings panning back");
}
