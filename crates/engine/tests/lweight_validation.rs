//! LWEIGHT rejects values that are not standard lineweights instead of clamping them into CELWEIGHT.

use cadcraft_engine::Session;
use serde_json::json;

fn celweight(s: &Session) -> i64 {
    s.doc().unwrap().header.i64("CELWEIGHT", -1)
}

#[test]
fn lweight_rejects_invalid_values() {
    let mut s = Session::new();
    for bad in [-5.0, 0.37, 7.0, 1e9] {
        let r = s.execute("lweight", &json!({"lineweight": bad}));
        assert!(r.is_err(), "lweight {bad} accepted and stored CELWEIGHT={}", celweight(&s));
        assert_eq!(celweight(&s), -1, "lweight {bad} changed CELWEIGHT");
    }
}

#[test]
fn lweight_accepts_standard_values() {
    let mut s = Session::new();
    for (mm, code) in [(0.25, 25), (2.11, 211), (0.0, 0), (1.0, 100)] {
        s.execute("lweight", &json!({"lineweight": mm})).unwrap();
        assert_eq!(celweight(&s), code);
    }
    for (name, code) in [("ByLayer", -1), ("ByBlock", -2), ("Default", -3)] {
        s.execute("lweight", &json!({"lineweight": name})).unwrap();
        assert_eq!(celweight(&s), code);
    }
}
