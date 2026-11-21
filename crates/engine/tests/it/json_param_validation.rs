//! The JSON forms of the draw and annotation commands refuse sizes and points that would make a
//! degenerate or invalid object (a negative radius made positive, a zero hatch scale, a
//! zero-length line …) instead of adding it (#343).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::{Value, json};

fn count(s: &Session) -> usize {
    s.doc().map(|d| d.model.iter().count()).unwrap_or(0)
}

/// A session with a 10×10 square (for HATCH's internal point) and a circle (for DIMRADIUS).
fn session() -> Session {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    s
}

#[test]
fn invalid_sizes_and_points_are_refused() {
    let cases: &[(&str, Value, &str)] = &[
        ("circle", json!({"center": [0, 0], "radius": -5}), "`radius` must be positive"),
        ("circle", json!({"center": [0, 0], "radius": 1e-300}), "radius must be positive"),
        ("circle", json!({"center": [0, 0], "diameter": -2}), "`diameter` must be positive"),
        ("arc", json!({"center": [0, 0], "radius": -5, "start": 0, "end": 90}), "`radius` must be positive"),
        ("polygon", json!({"sides": -3, "center": [0, 0], "radius": 5}), "sides must be 3..1024"),
        ("polygon", json!({"sides": 6, "center": [0, 0], "radius": -5}), "`radius` must be positive"),
        ("polygon", json!({"sides": 6, "center": [0, 0], "radius": 0}), "`radius` must be positive"),
        ("polygon", json!({"sides": 6, "edge": [[1, 1], [1, 1]]}), "edge endpoints must differ"),
        ("donut", json!({"center": [0, 0], "inside": -1, "outside": 2}), "`inside` must be zero or positive"),
        ("donut", json!({"center": [0, 0], "inside": 0, "outside": 0}), "`outside` must be positive"),
        ("donut", json!({"center": [0, 0], "inside": 5, "outside": 2}), "must not be larger"),
        ("rectang", json!({"p1": [0, 0], "p2": [10, 5], "fillet": -1}), "`fillet` must be zero or positive"),
        ("rectang", json!({"p1": [0, 0], "p2": [10, 5], "chamfer": [-1, 1]}), "`chamfer` distances"),
        ("rectang", json!({"p1": [0, 0], "p2": [10, 5], "width": -1}), "`width` must be zero or positive"),
        ("pline", json!({"vertices": [[0, 0], [1, 0]], "width": -1}), "`width` must be zero or positive"),
        ("pline", json!({"vertices": [{"p": [0, 0], "startWidth": -1}, [1, 0]]}), "`startWidth` must be zero or positive"),
        ("line", json!({"points": [[2, 2], [2, 2]]}), "2 different points"),
        ("spline", json!({"fit": [[0, 0], [0, 0], [0, 0]]}), "different fit points"),
        ("hatch", json!({"points": [[1, 1]], "scale": 0}), "`scale` must be positive"),
        ("hatch", json!({"points": [[1, 1]], "scale": -1}), "`scale` must be positive"),
        ("table", json!({"at": [0, 0], "rows": 2, "cols": 2, "rowHeight": -1}), "`rowHeight` must be positive"),
        ("table", json!({"at": [0, 0], "rows": 2, "cols": 2, "colWidth": 0}), "`colWidth` must be positive"),
        ("dimradius", json!({"center": [3, 3], "point": [3, 3]}), "must differ from `center`"),
        ("dimdiameter", json!({"center": [3, 3], "point": [3, 3]}), "must differ from `center`"),
        ("dimangular", json!({"vertex": [0, 0], "p1": [0, 0], "p2": [5, 0], "at": [1, 1]}), "must differ from `vertex`"),
        ("ddptype", json!({"pdmode": 999}), "`pdmode` must be"),
        ("ddptype", json!({"pdmode": 5}), "`pdmode` must be"),
    ];
    for (cmd, p, want) in cases {
        let mut s = session();
        let before = count(&s);
        let err = s.execute(cmd, p).expect_err(&format!("{cmd} {p} should be refused")).to_string();
        assert!(err.contains(want), "{cmd} {p}: {err}");
        assert_eq!(count(&s), before, "{cmd} {p} added an object");
    }
}

#[test]
fn valid_values_still_work() {
    let cases: &[(&str, Value)] = &[
        ("circle", json!({"center": [0, 0], "radius": 5})),
        ("circle", json!({"center": [0, 0], "diameter": 4})),
        ("arc", json!({"center": [0, 0], "radius": 5, "start": 0, "end": 90})),
        ("polygon", json!({"center": [0, 0], "radius": 5})),
        ("polygon", json!({"sides": 6, "edge": [[0, 0], [1, 0]]})),
        ("donut", json!({"center": [0, 0], "inside": 0, "outside": 2})),
        ("donut", json!({"center": [0, 0], "inside": 2, "outside": 2})),
        ("rectang", json!({"p1": [0, 0], "p2": [10, 5], "fillet": 0, "width": 0, "chamfer": [0, 0]})),
        ("pline", json!({"vertices": [{"p": [0, 0], "startWidth": 0, "endWidth": 1}, [1, 0]], "width": 0.5})),
        ("hatch", json!({"points": [[1, 1]], "scale": 2})),
        ("table", json!({"at": [20, 0], "rows": 2, "cols": 2, "rowHeight": 1, "colWidth": 3})),
        ("dimradius", json!({"center": [3, 3], "point": [5, 3]})),
    ];
    for (cmd, p) in cases {
        let mut s = session();
        let before = count(&s);
        s.execute(cmd, p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"));
        assert_eq!(count(&s), before + 1, "{cmd} {p}");
    }
    let mut s = Session::new();
    for m in [0, 3, 35, 66, 100] {
        s.execute("ddptype", &json!({"pdmode": m, "pdsize": -5})).unwrap();
    }
}

#[test]
fn repeated_line_points_make_no_zero_length_line() {
    let mut s = Session::new();
    let r = s.execute("line", &json!({"points": [[0, 0], [0, 0], [5, 0], [5, 0], [5, 5]], "closed": true})).unwrap();
    assert_eq!(r["handles"].as_array().map(Vec::len), Some(3), "{r}");
    let d = s.doc().unwrap();
    assert!(d.model.iter().all(|e| matches!(&e.kind, EntityKind::Line(l) if l.a != l.b)));
}

#[test]
fn dimension_style_sizes_out_of_range_are_ignored() {
    let mut s = Session::new();
    let r = s.execute("dimstyle", &json!({"name": "D", "DIMTXT": -1, "DIMASZ": -1, "DIMSCALE": -2, "DIMGAP": 0.1})).unwrap();
    let ignored: Vec<&str> = r["ignored"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    for k in ["DIMTXT", "DIMASZ", "DIMSCALE"] {
        assert!(ignored.contains(&k), "{r}");
    }
    let st = s.doc().unwrap().dim_style("D").unwrap();
    assert!(st.text_height > 0.0 && st.arrow_size >= 0.0 && st.scale >= 0.0);
    assert!((st.text_gap - 0.1).abs() < 1e-12);
    // Zero arrows and DIMSCALE 0 stay valid.
    let r = s.execute("dimstyle", &json!({"name": "D", "DIMASZ": 0, "DIMSCALE": 0})).unwrap();
    assert!(r.get("ignored").is_none(), "{r}");
}
