//! OSNAP (F3) with no running object snap set (OSMODE 0, or only the F3 bit): asking to turn
//! object snap on reports `noModes` instead of silently turning it "off" again, as AutoCAD shows
//! its Object Snap settings then. The on state is the status bar's: the F3 bit clear and a mode set.

use cadcraft_engine::Session;
use cadcraft_engine::snap::mode;
use serde_json::{Value, json};

/// AutoCAD's default running snaps.
const DEFAULT: u32 = 4133;

fn osnap(s: &mut Session, p: Value) -> Value {
    s.execute("osnap", &p).unwrap()
}

#[test]
fn f3_with_no_running_snaps_reports_it() {
    let mut s = Session::new();
    // Off and on with modes set.
    let r = osnap(&mut s, json!({}));
    assert_eq!((r["on"].clone(), r["osmode"].clone()), (json!(false), json!(DEFAULT | mode::OFF)));
    assert!(r.get("noModes").is_none());
    let r = osnap(&mut s, json!({}));
    assert_eq!((r["on"].clone(), r["osmode"].clone(), r["message"].clone()), (json!(true), json!(DEFAULT), json!("<Osnap on>")));
    // OSMODE 0 (the status bar shows it off): F3 asks to turn it on, and there is nothing to snap.
    s.execute("setvar", &json!({ "name": "OSMODE", "value": 0 })).unwrap();
    let r = osnap(&mut s, json!({}));
    assert_eq!(r["noModes"], json!(true), "{r}");
    assert_eq!(r["on"], json!(false));
    assert_eq!(s.settings.osmode, 0, "not turned off instead");
    assert!(r["message"].as_str().unwrap().contains("No running object snaps"), "{r}");
    // The message is echoed on the command line: plain words, the JSON form only in `hint`.
    assert!(!r["message"].as_str().unwrap().contains(['{', '[', '"']), "{r}");
    assert!(r["hint"].as_str().unwrap().contains("\"modes\""), "{r}");
    // Only the F3 bit set: the same.
    s.execute("setvar", &json!({ "name": "OSMODE", "value": mode::OFF })).unwrap();
    let r = osnap(&mut s, json!({ "on": true }));
    assert_eq!(r["noModes"], json!(true));
    assert_eq!(s.settings.osmode, 0);
    // Turning it off is not reported.
    assert!(osnap(&mut s, json!({ "on": false })).get("noModes").is_none());
    // Choosing modes turns it on.
    let r = osnap(&mut s, json!({ "modes": ["end", "mid"] }));
    assert_eq!((r["on"].clone(), r["osmode"].clone()), (json!(true), json!(mode::END | mode::MID)));
}

#[test]
fn hostile_osnap_params_never_panic() {
    let mut s = Session::new();
    for p in [
        json!({ "on": "yes" }),
        json!({ "osmode": -1 }),
        json!({ "osmode": 1e300 }),
        json!({ "osmode": u64::MAX }),
        json!({ "modes": [1, null, "zzz", ""] }),
        json!({ "modes": "end" }),
        json!(null),
    ] {
        let _ = s.execute("osnap", &p);
    }
    assert_eq!(s.settings.osmode & !(0x7fff), 0, "only OSMODE bits");
}
