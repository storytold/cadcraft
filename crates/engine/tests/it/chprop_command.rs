//! CHPROP as AutoCAD 2024 runs it from AutoLISP's `command` (accoreconsole, every object read back
//! with `entget`).

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::{Color, Rgb};
use cadcraft_engine::doc::{Handle, Lineweight, Transparency};
use serde_json::json;

/// A session with line A (0,0)-(50,0) on layer 0 and line B (0,20)-(50,30) on layer L1, as `a` and
/// `b` in LISP, after `code`; the properties of A then B.
fn run(code: &str) -> (Session, [cadcraft_engine::doc::Common; 2]) {
    let mut s = Session::new();
    s.doc_mut().unwrap().ensure_layer("L1");
    s.execute("line", &json!({ "points": [[0, 0], [50, 0]] })).unwrap();
    s.cmdline("(setq a (entlast))").unwrap();
    s.execute("line", &json!({ "points": [[0, 20], [50, 30]] })).unwrap();
    s.cmdline("(setq b (entlast))").unwrap();
    s.cmdline("(command \"_.CHPROP\" b \"\" \"_LA\" \"L1\" \"\")").unwrap();
    s.cmdline(&format!("(progn {code})")).unwrap();
    if s.running.is_some() {
        s.cancel();
    }
    let common = {
        let d = s.doc().unwrap();
        let mut it = d.model.iter().map(|e| e.common.clone());
        [it.next().unwrap(), it.next().unwrap()]
    };
    (s, common)
}

#[test]
fn chprop_sets_each_property_as_autocad() {
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" 1 \"\")");
    assert_eq!(a.color, Color::Index(1));
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" \"blue\" \"\")");
    assert_eq!(a.color, Color::Index(5));
    let (_, [_, b]) = run("(command \"_.CHPROP\" b \"\" \"_C\" \"_BYBLOCK\" \"\")");
    assert_eq!(b.color, Color::ByBlock);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" 3 \"\") (command \"_.CHPROP\" a \"\" \"_C\" \"_BYLAYER\" \"\")");
    assert_eq!(a.color, Color::ByLayer);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" \"_T\" \"255,128,0\" \"\")");
    assert_eq!(a.color, Color::True(Rgb(255, 128, 0)));
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LA\" \"L1\" \"\")");
    assert_eq!(a.layer, "L1");
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LT\" \"DASHED\" \"\")");
    assert_eq!(a.linetype, "DASHED");
    // Not loaded yet: loaded and given, as AutoCAD loads it from acadiso.lin.
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LT\" \"PHANTOM2\" \"\")");
    assert_eq!(a.linetype, "PHANTOM2");
    assert!(s.doc().unwrap().linetype("PHANTOM2").is_some());
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_S\" 2.5 \"\")");
    assert_eq!(a.ltscale, 2.5);
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LW\" 0.33 \"\")");
    assert_eq!(a.lineweight, Lineweight::Mm100(35));
    assert!(s.log.iter().any(|l| l == "Lineweight rounded to nearest valid value of 0.35 mm"), "{:?}", s.log);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_T\" 5.0 \"\")");
    assert_eq!(a.thickness, 5.0);
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_TR\" 50 \"\")");
    assert_eq!(a.transparency, Transparency::Percent(50));
    let (_, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_TR\" \"_BYBLOCK\" \"\")");
    assert_eq!(a.transparency, Transparency::ByBlock);
    // Several objects and several properties in one go.
    let (_, [a, b]) = run("(command \"_.CHPROP\" a b \"\" \"_C\" 3 \"_LA\" \"L1\" \"_LW\" 0.5 \"\")");
    for c in [a, b] {
        assert_eq!((c.color, c.layer.as_str(), c.lineweight), (Color::Index(3), "L1", Lineweight::Mm100(50)));
    }
    let (_, [a, _]) = run(
        "(command \"_.CHPROP\" a \"\" \"_C\" 3 \"\") (command \"_.CHPROP\" a \"\" \"_C\" \"_BYLAYER\" \"_LT\" \"_BYBLOCK\" \"_LW\" \"_BYLAYER\" \"\")",
    );
    assert_eq!((a.color, a.linetype.as_str(), a.lineweight), (Color::ByLayer, "ByBlock", Lineweight::ByLayer));
}

#[test]
fn chprop_refuses_what_autocad_refuses() {
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_LA\" \"KHONGCO\" \"\")");
    assert_eq!(a.layer, "0");
    assert!(s.log.iter().any(|l| l == "Cannot find layer \"KHONGCO\"."), "{:?}", s.log);
    for n in [0, 256, 300] {
        let (s, [a, _]) = run(&format!("(command \"_.CHPROP\" a \"\" \"_C\" {n} \"\")"));
        assert_eq!(a.color, Color::ByLayer, "{n}");
        assert!(s.log.iter().any(|l| l == "Invalid color number."), "{n}: {:?}", s.log);
    }
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_C\" \"_T\" \"300,0,0\" \"\")");
    assert_eq!(a.color, Color::ByLayer);
    assert!(s.log.iter().any(|l| l == "Requires three integers from 0 to 255, separated by commas."), "{:?}", s.log);
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_TR\" 95 \"\")");
    assert_eq!(a.transparency, Transparency::ByLayer);
    assert!(s.log.iter().any(|l| l == "Requires ByLayer, ByBlock or an integer from 0 to 90."), "{:?}", s.log);
    // `L` is LAyer, LType, ltScale or LWeight: none of them.
    let (s, [a, _]) = run("(command \"_.CHPROP\" a \"\" \"_L\")");
    assert_eq!(a.layer, "0");
    assert!(s.log.iter().any(|l| l == "Ambiguous response, please clarify..."), "{:?}", s.log);
}

/// Line a on layer 0 and lines b and c on the locked layer L1 (`a`, `b` and `c` in LISP); their
/// handles.
fn locked_drawing() -> (Session, Vec<Handle>) {
    let mut s = Session::new();
    s.doc_mut().unwrap().ensure_layer("L1");
    for (v, y) in [("a", 0), ("b", 20), ("c", 40)] {
        s.execute("line", &json!({ "points": [[0, y], [50, y]] })).unwrap();
        s.cmdline(&format!("(setq {v} (entlast))")).unwrap();
    }
    s.cmdline("(command \"_.CHPROP\" b c \"\" \"_LA\" \"L1\" \"\")").unwrap();
    s.execute("layer.set", &json!({ "name": "L1", "locked": true })).unwrap();
    s.log.clear();
    let hs = s.doc().unwrap().model.iter().map(|e| e.handle).collect();
    (s, hs)
}

fn colors(s: &Session) -> Vec<Color> {
    s.doc().unwrap().model.iter().map(|e| e.common.color).collect()
}

/// AutoCAD 2024 leaves objects on locked layers out at "Select objects" and says how many after
/// what was found; with none left, Enter ends the command.
#[test]
fn chprop_leaves_objects_on_locked_layers_alone() {
    // The colours of a, b and c after `code`, run with `pickfirst` selected.
    let after = |code: &str, pickfirst: &[usize]| {
        let (mut s, hs) = locked_drawing();
        s.set_selection(pickfirst.iter().map(|i| hs[*i]).collect());
        s.cmdline(code).unwrap();
        (colors(&s), s.log.clone(), s.running.is_some())
    };
    let (c, log, _) = after("(command \"_.CHPROP\" (ssadd b (ssadd a)) \"\" \"_C\" 3 \"\")", &[]);
    assert_eq!(c, [Color::Index(3), Color::ByLayer, Color::ByLayer]);
    assert!(log.iter().any(|l| l == "1 was on a locked layer."), "{log:?}");
    let (c, log, _) = after("(command \"_.CHPROP\" (ssadd c (ssadd b (ssadd a))) \"\" \"_C\" 4 \"\")", &[]);
    assert_eq!(c, [Color::Index(4), Color::ByLayer, Color::ByLayer]);
    assert!(log.iter().any(|l| l == "2 were on a locked layer."), "{log:?}");
    // Nothing left: Enter ends the command.
    let (_, log, running) = after("(command \"_.CHPROP\" b \"\")", &[]);
    assert!(log.iter().any(|l| l == "1 was on a locked layer."), "{log:?}");
    assert!(!running);
    // The pickfirst selection loses them too; with none of it left, CHPROP asks for objects.
    let (c, log, _) = after("(command \"_.CHPROP\" \"_C\" 5 \"\")", &[0, 1]);
    assert_eq!(c, [Color::Index(5), Color::ByLayer, Color::ByLayer]);
    assert!(log.iter().any(|l| l == "1 was on a locked layer."), "{log:?}");
    let (c, _, _) = after("(command \"_.CHPROP\" a \"\" \"_C\" 6 \"\")", &[1]);
    assert_eq!(c, [Color::Index(6), Color::ByLayer, Color::ByLayer]);
    // The JSON form leaves them out as well, and says how many.
    let (mut s, hs) = locked_drawing();
    let r = s.execute("chprop", &json!({ "handles": [hs[0].hex(), hs[1].hex()], "color": 3 })).unwrap();
    assert_eq!((r["changed"].as_u64(), r["locked"].as_u64()), (Some(1), Some(1)), "{r}");
    let r = s.execute("chprop", &json!({ "handles": [hs[2].hex()], "color": 4 })).unwrap();
    assert_eq!((r["changed"].as_u64(), r["locked"].as_u64()), (Some(0), Some(1)), "{r}");
    assert_eq!(colors(&s), [Color::Index(3), Color::ByLayer, Color::ByLayer]);
}
