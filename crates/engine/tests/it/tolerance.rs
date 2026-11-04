//! TOLERANCE (#420): feature control frames from the JSON form and the command line, edited by
//! MOVE / ROTATE / MIRROR, exploded into plain geometry and text, hostile input refused.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Tolerance};
use cadcraft_engine::geom::Vec3;
use serde_json::json;

fn tolerance(s: &Session) -> (String, Tolerance) {
    let d = s.doc().unwrap();
    d.model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Tolerance(t) => Some((e.handle.hex(), t.clone())),
            _ => None,
        })
        .expect("a tolerance")
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

#[test]
fn json_form_composes_the_frame() {
    let mut s = Session::new();
    let r = s
        .execute(
            "tolerance",
            &json!({
                "at": [10, 20],
                "symbol": "position",
                "tol1": "%%c0.1(M)",
                "datums": ["A", "B(L)", { "letter": "C", "material": "S" }],
                "line2": { "symbol": "flatness", "tol1": { "value": 0.02 } },
                "height": "2.5",
                "projected": true,
                "datumId": "D",
                "rotation": 90
            }),
        )
        .unwrap();
    let want = "{\\Fgdt;j}%%v{\\Fgdt;n}0.1{\\Fgdt;m}%%v%%vA%%vB{\\Fgdt;l}%%vC{\\Fgdt;s}^J{\\Fgdt;c}%%v0.02^J2.5{\\Fgdt;p}^JD";
    assert_eq!(r["text"], want);
    let (h, t) = tolerance(&s);
    assert_eq!(r["handle"], h);
    assert_eq!(t.text, want);
    assert_eq!(t.style, "Standard");
    assert!(near(t.insert, Vec3::new(10.0, 20.0, 0.0)) && near(t.dir, Vec3::new(0.0, 1.0, 0.0)), "{t:?}");
}

#[test]
fn command_line_prompts() {
    let mut s = Session::new();
    // Characteristic, two tolerance values (the second skipped), datums until an empty input,
    // then the location.
    s.script("TOLERANCE\nposition\n%%c0.1(M)\n\nA\nB(M)\n\n5,5\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (_, t) = tolerance(&s);
    assert_eq!(t.text, "{\\Fgdt;j}%%v{\\Fgdt;n}0.1{\\Fgdt;m}%%v%%vA%%vB{\\Fgdt;m}");
    assert!(near(t.insert, Vec3::new(5.0, 5.0, 0.0)));

    // One line with keyword shortcuts; "none" skips a value, the third datum ends the prompts.
    let mut s = Session::new();
    s.cmdline("tol F 0.05 none A B C 1,2").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let (_, t) = tolerance(&s);
    assert_eq!(t.text, "{\\Fgdt;c}%%v0.05%%v%%vA%%vB%%vC");
    assert!(near(t.insert, Vec3::new(1.0, 2.0, 0.0)));
}

#[test]
fn command_line_rejects_bad_symbols_and_empty_frames() {
    let mut s = Session::new();
    s.cmdline("tolerance").unwrap();
    s.cmdline("mmc").unwrap();
    // A material condition is no characteristic: the prompt repeats.
    assert!(s.prompt_text().contains("geometric characteristic"), "{}", s.prompt_text());
    s.script("None\n\n\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(s.doc().unwrap().model.iter().all(|e| !matches!(e.kind, EntityKind::Tolerance(_))));
}

#[test]
fn transforms_and_grips() {
    let mut s = Session::new();
    s.execute("tolerance", &json!({ "at": [1, 1], "symbol": "flatness", "tol1": "0.1" })).unwrap();
    let (h, t0) = tolerance(&s);
    assert_eq!(EntityKind::Tolerance(t0.clone()).grips().len(), 1);
    s.execute("move", &json!({ "handles": [h], "delta": [2, 3] })).unwrap();
    assert!(near(tolerance(&s).1.insert, Vec3::new(3.0, 4.0, 0.0)));
    s.execute("rotate", &json!({ "handles": [h], "base": [0, 0], "angle": 90 })).unwrap();
    let t = tolerance(&s).1;
    assert!(near(t.insert, Vec3::new(-4.0, 3.0, 0.0)) && near(t.dir, Vec3::new(0.0, 1.0, 0.0)), "{t:?}");
    // Mirrored frames keep reading the same way (like text with MIRRTEXT 0).
    s.execute("mirror", &json!({ "handles": [h], "p1": [0, 0], "p2": [0, 1], "erase": true })).unwrap();
    let t = tolerance(&s).1;
    assert!(near(t.insert, Vec3::new(4.0, 3.0, 0.0)) && near(t.dir, Vec3::new(0.0, 1.0, 0.0)), "{t:?}");
    assert_eq!(t.text, t0.text);
}

#[test]
fn explode_gives_lines_symbols_and_text() {
    let mut s = Session::new();
    s.execute("tolerance", &json!({ "at": [0, 0], "symbol": "position", "tol1": "%%c0.1(M)", "datums": ["A"] })).unwrap();
    let (h, _) = tolerance(&s);
    let r = s.execute("explode", &json!({ "handles": [h] })).unwrap();
    assert!(r["handles"].as_array().unwrap().len() > 8, "{r}");
    let d = s.doc().unwrap();
    let kinds: Vec<&EntityKind> = d.model.iter().map(|e| &e.kind).collect();
    assert!(!kinds.iter().any(|k| matches!(k, EntityKind::Tolerance(_))));
    let texts: Vec<&str> = kinds
        .iter()
        .filter_map(|k| match k {
            EntityKind::Text(t) => Some(t.value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["0.1", "A"]);
    // Position and diameter symbols and the circled M: circles; the boxes: lines.
    assert_eq!(kinds.iter().filter(|k| matches!(k, EntityKind::Circle(_))).count(), 3);
    assert!(kinds.iter().filter(|k| matches!(k, EntityKind::Line(_))).count() >= 6);
}

#[test]
fn hostile_params_are_refused_without_panicking() {
    let mut s = Session::new();
    for p in [
        json!({ "at": [0, 0] }),
        json!({ "at": [0, 0], "symbol": "nonsense" }),
        json!({ "at": [0, 0], "symbol": "mmc" }),
        json!({ "symbol": "position" }),
        json!({ "at": [0, 0], "symbol": "position", "style": "NoSuchStyle" }),
        json!({ "at": [0, 0], "text": "" }),
    ] {
        assert!(s.execute("tolerance", &p).is_err(), "{p}");
    }
    let huge = "{\\Fgdt;j}%%v".repeat(100_000);
    for p in [
        json!({ "at": [0, 0], "text": "{\\Fgdt;" }),
        json!({ "at": [0, 0], "text": "x{\\Fgdt;j" }),
        json!({ "at": [0, 0], "text": huge }),
        json!({ "at": [0, 0], "symbol": "position", "tol1": "x".repeat(100_000), "datums": [1, null, {}, "A"], "rotation": 1e308 }),
        json!({ "at": [1e308, -1e308], "tol1": { "value": "%%v^J{}\\" }, "height": 1e300 }),
    ] {
        if s.execute("tolerance", &p).is_ok() {
            let t = tolerance(&s).1;
            assert!(t.text.chars().count() <= cadcraft_engine::doc::MAX_FRAME_CHARS);
            let _ = s.execute("explode", &json!({ "handles": [tolerance(&s).0] }));
        }
    }
}
