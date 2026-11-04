//! Keywords typed at "Select objects" with a leading underscore (`_L`, `_ALL`, `_W`…) select the
//! same objects as without it: scripts and routines meant for every language write them that way
//! (#1172).

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::json;

/// Three lines on the X axis: A 0–10, B 12–20, C 30–40.
fn drawing() -> Session {
    let mut s = Session::new();
    for (a, b) in [(0.0, 10.0), (12.0, 20.0), (30.0, 40.0)] {
        s.execute("line", &json!({ "points": [[a, 0.0], [b, 0.0]] })).unwrap();
    }
    s
}

/// The lines left in the drawing, as sorted (start, end) points.
fn lines(s: &Session) -> Vec<[f64; 4]> {
    let mut v: Vec<[f64; 4]> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some([l.a.x, l.a.y, l.b.x, l.b.y]) } else { None })
        .collect();
    v.sort_by(|a, b| a.iter().zip(b).map(|(x, y)| x.total_cmp(y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
    v
}

/// The lines after the script, or what went wrong.
fn run(script: &str) -> Result<Vec<[f64; 4]>, String> {
    let mut s = drawing();
    s.script(script).map_err(|e| e.to_string())?;
    if s.running.is_some() {
        return Err(format!("still at {}", s.prompt_text()));
    }
    Ok(lines(&s))
}

#[test]
fn underscore_selection_keywords_select_like_the_plain_ones() {
    let untouched = run("").unwrap();
    let mut wrong = Vec::new();
    // `{}` is the keyword, typed plain and with the underscore.
    for (case, kw) in [
        ("MOVE\n{}\n\n0,0\n0,5\n", "L"),
        ("MOVE\n{}\n\n0,0\n0,5\n", "Last"),
        ("ERASE\n{}\n\n", "ALL"),
        ("MOVE\n15,0\n\n0,0\n0,0\nMOVE\n{}\n\n0,0\n0,5\n", "P"),
        ("ERASE\n{}\n25,1\n5,-1\n\n", "W"),
        ("ERASE\n{}\n5,-1\n25,1\n\n", "C"),
        ("ERASE\n{}\n25,1\n5,-1\n\n", "BOX"),
        ("ERASE\n{}\n6,-1\n6,1\n35,1\n35,-1\n\n\n", "F"),
        ("ERASE\n{}\n5,-1\n25,-1\n25,1\n5,1\n\n\n", "WP"),
        ("ERASE\n{}\n5,-1\n25,-1\n25,1\n5,1\n\n\n", "CP"),
        ("ERASE\nALL\n{}\n15,0\n\n", "R"),
        ("ERASE\nALL\nR\n15,0\n{}\n15,0\n35,0\n\n", "A"),
        ("ERASE\n15,0\n35,0\n{}\n\n", "U"),
        ("ERASE\n{}\n15,0\n", "SI"),
        // STRETCH reads its own "Select objects".
        ("STRETCH\n{}\n5,-1\n25,1\n\n0,0\n0,5\n", "C"),
        ("STRETCH\n{}\n15,0\n\n0,0\n0,5\n", "L"),
    ] {
        let plain = run(&case.replace("{}", kw)).unwrap_or_else(|e| panic!("{case:?} with {kw}: {e}"));
        assert_ne!(plain, untouched, "{case:?} with {kw} changes the drawing");
        let underscore = run(&case.replace("{}", &format!("_{kw}")));
        if underscore.as_ref() != Ok(&plain) {
            wrong.push(format!("_{kw} in {case:?}: {underscore:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// At HATCH's "Select objects", `_P` is Previous like `P`, not the "picK internal point" keyword.
#[test]
fn underscore_previous_beats_a_command_keyword_sharing_the_letter() {
    let mut s = Session::new();
    s.execute("rectang", &json!({"p1": [0, 0], "p2": [10, 10]})).unwrap();
    s.cmdline("hatch s").unwrap();
    s.cmdline("_P").unwrap();
    assert!(s.prompt_text().contains("HATCH Select objects"), "{}", s.prompt_text());
}
