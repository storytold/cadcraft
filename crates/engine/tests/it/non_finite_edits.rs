//! Edits whose result would have infinite or undefined coordinates fail and leave the drawing as it
//! was, whether they come as JSON (MCP, control channel) or are typed at the command line.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn circle(s: &mut Session) -> String {
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap()["handle"].as_str().unwrap().to_string()
}

fn snapshot(s: &mut Session) -> (Value, usize) {
    let i = s.execute("drawing.inspect", &json!({})).unwrap();
    let undo = i["undo"].as_array().map_or(0, Vec::len);
    (i["entities"].clone(), undo)
}

#[test]
fn json_transforms_that_overflow_are_refused() {
    let huge = [1e308, 1e308];
    let cases: Vec<(&str, Value)> = vec![
        ("move", json!({"delta": huge})),
        ("copy", json!({"delta": huge})),
        ("scale", json!({"base": [0, 0], "factor": 1e308})),
        ("rotate", json!({"base": [-1e308, -1e308], "angle": 180})),
        ("mirror", json!({"p1": [-1e308, -1e308], "p2": [-1e308, 0]})),
        ("arrayrect", json!({"rows": 3, "cols": 1, "rowSpacing": 1e308, "colSpacing": 1})),
        ("stretch", json!({"window": [[-2, -2], [1.7e308, 1.7e308]], "delta": huge})),
        ("offset", json!({"distance": 1e308, "side": [1.7e308, 1.7e308]})),
    ];
    for (cmd, p) in cases {
        let mut s = Session::new();
        let h = circle(&mut s);
        // Put it where the edit overflows: far out, then once more.
        s.execute("move", &json!({"handles": [h], "delta": huge})).unwrap();
        let before = snapshot(&mut s);
        let mut p = p;
        p["handles"] = json!([h]);
        p["handle"] = json!(h);
        let r = s.execute(cmd, &p);
        let err = r.expect_err(&format!("{cmd} {p} must fail"));
        assert!(err.to_string().contains("too large"), "{cmd}: {err}");
        assert_eq!(snapshot(&mut s), before, "{cmd} must leave the drawing and undo history unchanged");
    }
}

#[test]
fn typed_move_that_overflows_is_refused() {
    let mut s = Session::new();
    circle(&mut s);
    s.script("MOVE ALL\n\n0,0 1e308,1e308\n").unwrap();
    let before = snapshot(&mut s);
    s.script("MOVE ALL\n\n0,0 1e308,1e308\n").unwrap();
    assert_eq!(snapshot(&mut s), before);
    assert!(s.log.iter().any(|l| l.contains("too large")), "{:?}", s.log);
    assert!(s.running.is_none());
}
