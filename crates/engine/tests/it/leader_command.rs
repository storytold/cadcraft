//! LEADER at the command line, as AutoCAD 2024 runs it (accoreconsole: these scripts, with
//! DIMTAD 1, DIMGAP 0.625, DIMASZ 2.5, DIMSCALE 1 and DIMTXT 2.5 unless a case sets another, and
//! the drawing read back afterwards).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Insert, Leader, MText};
use serde_json::json;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// A drawing as the measured scripts start from: the dimension settings, and a text "XYZ" 2.5
/// high at (100,40) that the Copy cases copy (`(entlast)` after it).
fn session(setup: &str) -> Session {
    let mut s = Session::new();
    for (name, v) in [("DIMTAD", 1.0), ("DIMGAP", 0.625), ("DIMASZ", 2.5), ("DIMSCALE", 1.0), ("DIMTXT", 2.5)] {
        s.execute("setvar", &json!({ "name": name, "value": v })).unwrap();
    }
    s.execute("text", &json!({ "at": [100, 40], "text": "XYZ", "height": 2.5 })).unwrap();
    s.cmdline("(setq hl_T (entlast))").unwrap();
    if !setup.is_empty() {
        s.cmdline(setup).unwrap();
    }
    s
}

fn run(setup: &str, script: &str) -> Session {
    let mut s = session(setup);
    s.cmdline(script).unwrap();
    assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
    s
}

fn leaders(s: &Session) -> Vec<Leader> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Leader(l) = &e.kind { Some(l.clone()) } else { None }).collect()
}

fn mtexts(s: &Session) -> Vec<MText> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::MText(m) = &e.kind { Some(m.clone()) } else { None }).collect()
}

fn inserts(s: &Session) -> Vec<Insert> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).collect()
}

fn vertices(l: &Leader) -> Vec<[f64; 2]> {
    l.vertices.iter().map(|v| [v.x, v.y]).collect()
}

fn same_points(got: &[[f64; 2]], want: &[[f64; 2]]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| near(g[0], w[0]) && near(g[1], w[1]))
}

/// (case, setup, script, leader vertices, text insertion point, attachment, height, contents)
type Case = (&'static str, &'static str, &'static str, &'static [[f64; 2]], [f64; 2], u8, f64, &'static str);

#[test]
fn leaders_and_their_text_are_where_autocad_puts_them() {
    let cases: [Case; 17] = [
        // A horizontal last segment: no hook line; the text DIMGAP right of and above the end.
        (
            "000",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 15.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 15.0]],
            [40.625, 15.625],
            7,
            2.5,
            "ABC",
        ),
        // More than 15° off horizontal: a hook line DIMASZ long into the end.
        (
            "001",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(35.0 30.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [32.5, 30.0], [35.0, 30.0]],
            [35.625, 30.625],
            7,
            2.5,
            "ABC",
        ),
        (
            "002",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"AB\" \"CD\" \"\")",
            &[[0.0, 0.0], [17.5, 15.0], [20.0, 15.0]],
            [20.625, 15.625],
            7,
            2.5,
            "AB\\PCD",
        ),
        // To the left: the text right-attached, DIMGAP left of the end.
        (
            "004",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(-20.0 15.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [-17.5, 15.0], [-20.0, 15.0]],
            [-20.625, 15.625],
            9,
            2.5,
            "ABC",
        ),
        // A vertical last segment hooks to the right.
        (
            "007-VN",
            "",
            "(command \"_.LEADER\" '(612345.678 2398765.432) '(612345.678 2398795.432) \"\" \"ABC\" \"\")",
            &[[612345.678, 2398765.432], [612343.178, 2398795.432], [612345.678, 2398795.432]],
            [612346.303, 2398796.057],
            7,
            2.5,
            "ABC",
        ),
        // 14.9° and 15° keep the end; 15.1° hooks.
        (
            "100",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 20.32158853384029) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 20.32158853384029]],
            [40.625, 20.94658853384029],
            7,
            2.5,
            "ABC",
        ),
        (
            "101",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 20.358983848622454) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 20.358983848622454]],
            [40.625, 20.983983848622454],
            7,
            2.5,
            "ABC",
        ),
        (
            "102",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 20.39641415635812) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [37.5, 20.39641415635812], [40.0, 20.39641415635812]],
            [40.625, 21.02141415635812],
            7,
            2.5,
            "ABC",
        ),
        // DIMTAD 0: centred on the end, left- or right-attached.
        (
            "020",
            "(setvar \"DIMTAD\" 0)",
            "(command \"_.LEADER\" '(0.5 0.25) '(0.52 0.265) '(0.54 0.265) \"\" \"ABC\" \"\")",
            &[[0.5, 0.25], [0.52, 0.265], [0.54, 0.265]],
            [1.165, 0.265],
            4,
            2.5,
            "ABC",
        ),
        (
            "103-VN",
            "(setvar \"DIMTAD\" 0)",
            "(command \"_.LEADER\" '(612345.678 2398765.432) '(612325.678 2398780.432) '(612305.678 2398780.432) \"\" \"ABC\" \"\")",
            &[[612345.678, 2398765.432], [612325.678, 2398780.432], [612305.678, 2398780.432]],
            [612305.053, 2398780.432],
            6,
            2.5,
            "ABC",
        ),
        // DIMGAP, DIMSCALE (hook, gap and height) and DIMTXT.
        (
            "021",
            "(setvar \"DIMGAP\" 2.0)",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 15.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 15.0]],
            [42.0, 17.0],
            7,
            2.5,
            "ABC",
        ),
        (
            "022",
            "(setvar \"DIMSCALE\" 2.0)",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(35.0 30.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [30.0, 30.0], [35.0, 30.0]],
            [36.25, 31.25],
            7,
            5.0,
            "ABC",
        ),
        (
            "024",
            "(setvar \"DIMTXT\" 5.0)",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(40.0 15.0) \"\" \"AB\" \"CD\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 15.0]],
            [40.625, 15.625],
            7,
            5.0,
            "AB\\PCD",
        ),
        // A spline takes no hook line.
        (
            "026",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"_F\" \"_S\" '(35.0 30.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [35.0, 30.0]],
            [35.625, 30.625],
            7,
            2.5,
            "ABC",
        ),
        // Undo takes the last point back.
        (
            "014",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) '(35.0 30.0) \"_U\" '(40.0 15.0) \"\" \"ABC\" \"\")",
            &[[0.0, 0.0], [20.0, 15.0], [40.0, 15.0]],
            [40.625, 15.625],
            7,
            2.5,
            "ABC",
        ),
        // Annotation options: Mtext, at the command line, comes back to the first line.
        (
            "106",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"\" \"_M\" \"ABC\" \"\")",
            &[[0.0, 0.0], [17.5, 15.0], [20.0, 15.0]],
            [20.625, 15.625],
            7,
            2.5,
            "ABC",
        ),
        // Copy: centred on its line; with DIMTAD on the leader ends half the text height lower.
        (
            "016",
            "",
            "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"\" \"_C\" hl_T)",
            &[[0.0, 0.0], [17.5, 13.75], [20.0, 13.75]],
            [20.625, 15.625],
            4,
            2.5,
            "XYZ",
        ),
    ];
    for (case, setup, script, want_vertices, at, attach, height, contents) in cases {
        let s = run(setup, script);
        let ls = leaders(&s);
        assert_eq!(ls.len(), 1, "{case}");
        assert!(same_points(&vertices(&ls[0]), want_vertices), "{case}: {:?}", vertices(&ls[0]));
        let ms = mtexts(&s);
        assert_eq!(ms.len(), 1, "{case}");
        let m = &ms[0];
        assert!(near(m.insert.x, at[0]) && near(m.insert.y, at[1]), "{case}: {:?}", m.insert);
        assert_eq!((m.attach, m.contents.as_str()), (attach, contents), "{case}");
        assert!(near(m.height, height) && m.width == 0.0, "{case}: {} {}", m.height, m.width);
    }
}

#[test]
fn format_sets_the_path_and_the_arrow() {
    // LEADER-011 Format None; LEADER-012 None then Arrow; LEADER-013 Spline then STraight.
    for (script, spline, arrow) in [
        ("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"_F\" \"_N\" '(40.0 15.0) \"\" \"ABC\" \"\")", false, false),
        ("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"_F\" \"_N\" \"_F\" \"_A\" '(40.0 15.0) \"\" \"ABC\" \"\")", false, true),
        ("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"_F\" \"_S\" \"_F\" \"_ST\" '(40.0 15.0) \"\" \"ABC\" \"\")", false, true),
        ("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"_F\" \"\" '(40.0 15.0) \"\" \"ABC\" \"\")", false, true),
    ] {
        let l = leaders(&run("", script)).remove(0);
        assert_eq!((l.spline, l.arrow), (spline, arrow), "{script}");
    }
}

#[test]
fn no_annotation_a_block_or_a_copied_multiline_text() {
    // LEADER-015: None leaves the leader alone, without a hook line.
    let s = run("", "(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"\" \"_N\")");
    assert!(same_points(&vertices(&leaders(&s)[0]), &[[0.0, 0.0], [20.0, 15.0]]));
    assert!(mtexts(&s).is_empty());
    // LEADER-107: a block, scale 2 (Y as X), 30°, where it is placed; no hook line.
    let mut s = session("");
    s.execute("line", &json!({ "points": [[100, 0], [110, 0]] })).unwrap();
    s.cmdline("(command \"_.-BLOCK\" \"LB107\" '(100.0 0.0) (entlast) \"\")").unwrap();
    s.cmdline("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"\" \"_B\" \"LB107\" '(50.0 20.0) 2 \"\" 30)").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(same_points(&vertices(&leaders(&s)[0]), &[[0.0, 0.0], [20.0, 15.0]]));
    let i = inserts(&s).remove(0);
    assert!(i.block == "LB107" && near(i.insert.x, 50.0) && near(i.insert.y, 20.0), "{i:?}");
    assert!(near(i.scale.x, 2.0) && near(i.scale.y, 2.0) && near(i.rotation, 30f64.to_radians()), "{i:?}");
    // LEADER-105: a copied multiline text keeps its width; LEADER-137 to the left.
    let mut s = session("");
    s.cmdline("(command \"_.MTEXT\" '(60.0 40.0) '(90.0 30.0) \"MT\" \"\")").unwrap();
    s.cmdline("(command \"_.LEADER\" '(0.0 0.0) '(20.0 15.0) \"\" \"\" \"_C\" (entlast))").unwrap();
    let copy = mtexts(&s).into_iter().find(|m| near(m.insert.x, 20.625)).unwrap();
    assert!(near(copy.width, 30.0) && copy.attach == 4 && copy.contents == "MT", "{copy:?}");
    let s = run("", "(command \"_.LEADER\" '(0.0 0.0) '(-20.0 15.0) \"\" \"\" \"_C\" hl_T)");
    assert!(same_points(&vertices(&leaders(&s)[0]), &[[0.0, 0.0], [-17.5, 13.75], [-20.0, 13.75]]));
    let m = mtexts(&s).into_iter().next().unwrap();
    assert!(near(m.insert.x, -20.625) && near(m.insert.y, 15.625) && m.attach == 6, "{m:?}");
}

#[test]
fn the_json_form_places_its_text_as_the_command_line_does() {
    let mut s = session("");
    let v = s.execute("leader", &json!({ "points": [[0, 0], [20, 15], [35, 30]], "text": "ABC" })).unwrap();
    assert_eq!(v["handles"].as_array().map(Vec::len), Some(2));
    assert!(same_points(&vertices(&leaders(&s)[0]), &[[0.0, 0.0], [20.0, 15.0], [32.5, 30.0], [35.0, 30.0]]));
    let m = mtexts(&s).remove(0);
    assert!(near(m.insert.x, 35.625) && near(m.insert.y, 30.625) && m.attach == 7 && near(m.height, 2.5), "{m:?}");
}

#[test]
fn the_prompts_are_autocads_and_one_point_draws_nothing() {
    let mut s = session("");
    s.cmdline("leader").unwrap();
    assert!(s.prompt_text().ends_with("Specify leader start point:"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().ends_with("Specify next point:"), "{}", s.prompt_text());
    s.cmdline("20,15").unwrap();
    assert!(s.prompt_text().ends_with("Specify next point or [Annotation/Format/Undo] <Annotation>:"), "{}", s.prompt_text());
    s.cmdline("_F").unwrap();
    assert!(s.prompt_text().ends_with("[Spline/STraight/Arrow/None] <Exit>:"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Enter first line of annotation text or <options>:"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("[Tolerance/Copy/Block/None/Mtext] <Mtext>:"), "{}", s.prompt_text());
    // LEADER-019: Tolerance, at the command line, comes back to the first line.
    s.cmdline("_T").unwrap();
    assert!(s.prompt_text().ends_with("Enter first line of annotation text or <options>:"), "{}", s.prompt_text());
    // An answer that isn't an option is refused; `?` at the block name lists the blocks.
    s.cmdline("").unwrap();
    s.cmdline("X").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Invalid option keyword."));
    s.cmdline("_B").unwrap();
    assert!(s.prompt_text().ends_with("Enter block name or [?]:"), "{}", s.prompt_text());
    s.cmdline("?").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Defined blocks: (none)"));
    s.cancel();
    // LEADER-139: one point and Enter.
    let s = run("", "(command \"_.LEADER\" '(0.0 0.0) \"\")");
    assert!(leaders(&s).is_empty() && mtexts(&s).is_empty());
}
