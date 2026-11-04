//! -ATTDEF at the command line, as AutoCAD 2024 runs it (accoreconsole: these scripts with
//! TEXTSIZE 2.5 and AFLAGS 16, the drawing and AFLAGS read back afterwards).

use cadcraft_engine::doc::{Attrib, EntityKind, HAlign, VAlign};
use cadcraft_engine::{Session, sysvars};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "TEXTSIZE", "value": 2.5 })).unwrap();
    s.execute("setvar", &json!({ "name": "AFLAGS", "value": 16 })).unwrap();
    s
}

fn attdefs(s: &Session) -> Vec<Attrib> {
    s.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).collect()
}

fn aflags(s: &Session) -> i64 {
    sysvars::get(s, "AFLAGS").and_then(|v| v.as_f64()).unwrap() as i64
}

fn run(script: &str) -> Session {
    let mut s = session();
    s.cmdline(script).unwrap();
    assert!(s.running.is_none(), "{script}: {}", s.prompt_text());
    s
}

#[test]
fn modes_are_toggled_kept_in_aflags_and_stored_on_the_definition() {
    // (script, AFLAGS after, invisible, constant, verify, preset, lock position, tag, prompt, value)
    type Case = (&'static str, i64, [bool; 5], &'static str, &'static str, &'static str);
    let cases: [Case; 5] = [
        // ATTDEF-000: the modes as they are.
        (
            "(command \"_.-ATTDEF\" \"\" \"T1\" \"Nhap ten\" \"MAC\" '(5.0 10.0) 2.5 0)",
            16,
            [false, false, false, false, true],
            "T1",
            "Nhap ten",
            "MAC",
        ),
        // ATTDEF-001: Invisible and Verify.
        (
            "(command \"_.-ATTDEF\" \"_I\" \"_V\" \"\" \"T2\" \"Hoi hai\" \"D2\" '(5.0 10.0) 3.0 30)",
            21,
            [true, false, true, false, true],
            "T2",
            "Hoi hai",
            "D2",
        ),
        // ATTDEF-002: Constant asks for the value, not a prompt and a default.
        ("(command \"_.-ATTDEF\" \"_C\" \"\" \"T3\" \"GIATRI\" '(5.0 10.0) 2.5 0)", 18, [false, true, false, false, true], "T3", "", "GIATRI"),
        // ATTDEF-003: Preset.
        (
            "(command \"_.-ATTDEF\" \"_P\" \"\" \"T4\" \"Hoi bon\" \"D4\" '(612350.678 2398775.432) 2.5 0)",
            24,
            [false, false, false, true, true],
            "T4",
            "Hoi bon",
            "D4",
        ),
        // ATTDEF-004: Lock position off.
        (
            "(command \"_.-ATTDEF\" \"_L\" \"\" \"T5\" \"Hoi nam\" \"D5\" '(5.0 10.0) 2.5 0)",
            0,
            [false, false, false, false, false],
            "T5",
            "Hoi nam",
            "D5",
        ),
    ];
    for (script, flags, [inv, con, ver, pre, lock], tag, prompt, value) in cases {
        let s = run(script);
        assert_eq!(aflags(&s), flags, "{script}");
        let a = attdefs(&s).remove(0);
        assert_eq!((a.invisible, a.constant, a.modes.verify, a.modes.preset, a.modes.lock_position), (inv, con, ver, pre, lock), "{script}");
        assert_eq!((a.tag.as_str(), a.prompt.as_str(), a.text.value.as_str()), (tag, prompt, value), "{script}");
    }
    // ATTDEF-001's height 3 and 30 degrees.
    let a = attdefs(&run(cases[1].0)).remove(0);
    assert!((a.text.height - 3.0).abs() < 1e-12 && (a.text.rotation - 30f64.to_radians()).abs() < 1e-12, "{a:?}");
}

#[test]
fn tags_justification_and_style_as_autocad_takes_them() {
    // ATTDEF-021, ATTDEF-022: a tag is upper-cased, spaces and all.
    for (typed, tag) in [("maso", "MASO"), ("ten ban ve", "TEN BAN VE")] {
        let s = run(&format!("(command \"_.-ATTDEF\" \"\" \"{typed}\" \"Hoi\" \"D\" '(5.0 10.0) 2.5 0)"));
        assert_eq!(attdefs(&s)[0].tag, tag);
    }
    // ATTDEF-023: an empty prompt stays empty.
    let s = run("(command \"_.-ATTDEF\" \"\" \"T10\" \"\" \"D10\" '(612350.678 2398775.432) 2.5 0)");
    assert_eq!(attdefs(&s)[0].prompt, "");
    // ATTDEF-025: an empty tag is refused and asked again.
    let s = run("(command \"_.-ATTDEF\" \"\" \"\" \"T11\" \"Hoi muoi mot\" \"D11\" '(5.0 10.0) 2.5 0)");
    assert!(s.log.iter().any(|l| l == "Tag cannot be null"), "{:?}", s.log);
    assert_eq!(attdefs(&s)[0].tag, "T11");
    // ATTDEF-005: Justify MC at the middle point.
    let s = run("(command \"_.-ATTDEF\" \"\" \"T6\" \"Hoi sau\" \"D6\" \"_J\" \"_MC\" '(5.0 10.0) 2.5 0)");
    let a = attdefs(&s).remove(0);
    assert_eq!((a.text.halign, a.text.valign), (HAlign::Center, VAlign::Middle));
    assert!(a.text.align_pt.is_some_and(|p| p.x == 5.0 && p.y == 10.0), "{a:?}");
    // ATTDEF-006: Style, then the point.
    let s = run("(command \"_.-ATTDEF\" \"\" \"T7\" \"Hoi bay\" \"D7\" \"_S\" \"Standard\" '(0.505 0.26) 2.5 0)");
    let a = attdefs(&s).remove(0);
    assert!(a.text.style == "Standard" && a.text.insert.x == 0.505 && a.text.insert.y == 0.26, "{a:?}");
}

#[test]
fn the_prompts_are_autocads() {
    let mut s = session();
    s.cmdline("-attdef").unwrap();
    assert!(s.log.iter().any(|l| l == "Current attribute modes:"), "{:?}", s.log);
    assert_eq!(s.log.last().map(String::as_str), Some("Invisible=N  Constant=N  Verify=N  Preset=N  Lock position=Y  Annotative=N  Multiple line=N"));
    assert!(s.prompt_text().contains("[Invisible/Constant/Verify/Preset/Lock position/Annotative/Multiple lines] <done>:"), "{}", s.prompt_text());
    s.cmdline("X").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Invalid option keyword."));
    s.cmdline("_C").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Invisible=N  Constant=Y  Verify=N  Preset=N  Lock position=Y  Annotative=N  Multiple line=N"));
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Enter attribute tag name:"), "{}", s.prompt_text());
    s.cmdline("T3").unwrap();
    assert!(s.prompt_text().ends_with("Enter attribute value:"), "{}", s.prompt_text());
    s.cmdline("GIATRI").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Current text style:  \"Standard\"  Text height:  2.5000  Justify:  Left"));
    assert!(s.prompt_text().ends_with("Specify start point of text or [Justify/Style]:"), "{}", s.prompt_text());
    s.cmdline("_J").unwrap();
    // A wrong answer, or a justification not available here, is asked again.
    for typed in ["X", "_A"] {
        s.cmdline(typed).unwrap();
        assert!(s.prompt_text().contains("[Left/Center/Right/Align/Middle/Fit/TL/"), "{typed}: {}", s.prompt_text());
    }
    s.cmdline("_MC").unwrap();
    assert!(s.prompt_text().ends_with("Specify middle point of text:"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().ends_with("Specify height <2.5000>:"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Specify rotation angle of text <0>:"), "{}", s.prompt_text());
    s.cancel();
}

#[test]
fn a_constant_attribute_stays_in_the_block() {
    // INSERTATT-011: the constant TC is no attribute of the reference; the other one, TN, is.
    let mut s = session();
    s.execute("line", &json!({ "points": [[612345.678, 2398765.432], [612355.678, 2398765.432]] })).unwrap();
    s.cmdline("(setq hl_A (entlast))").unwrap();
    s.cmdline("(progn (command \"_.-ATTDEF\" \"_C\" \"\" \"TC\" \"HANG\" '(612350.678 2398775.432) 2.5 0) (setq hl_a1 (entlast)))").unwrap();
    s.cmdline("(setvar \"AFLAGS\" 16)").unwrap();
    s.cmdline("(progn (command \"_.-ATTDEF\" \"\" \"TN\" \"Hoi N\" \"MN\" '(612350.678 2398785.432) 2.5 0) (setq hl_a2 (entlast)))").unwrap();
    s.cmdline("(command \"_.-BLOCK\" \"B11\" '(612345.678 2398765.432) hl_a1 hl_a2 hl_A \"\")").unwrap();
    s.cmdline("(command \"_.-INSERT\" \"B11\" '(612445.678 2398815.432) 1 1 0 \"XN\")").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let ins = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.clone()) } else { None }).unwrap();
    let tags: Vec<(String, String)> = ins.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect();
    assert_eq!(tags, [("TN".to_string(), "XN".to_string())]);
}

#[test]
fn a_typed_height_is_a_distance() {
    let mut s = session();
    for typed in ["-attdef", "", "T1", "P", "V", "5,10", "x"] {
        s.cmdline(typed).unwrap();
    }
    assert_eq!(s.log.last().map(String::as_str), Some("Requires a positive height."));
    // Feet and inches, as TEXT takes them.
    for typed in ["1'6\"", "0"] {
        s.cmdline(typed).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(attdefs(&s)[0].text.height, 18.0);
}

#[test]
fn the_modes_start_from_aflags() {
    // AFLAGS 0, not the default 16: Invisible toggled on gives 1, and nothing else.
    let mut s = session();
    s.execute("setvar", &json!({ "name": "AFLAGS", "value": 0 })).unwrap();
    s.cmdline("(command \"_.-ATTDEF\" \"_I\" \"\" \"T1\" \"Hoi\" \"D\" '(5.0 10.0) 2.5 0)").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(aflags(&s), 1);
    let a = attdefs(&s).remove(0);
    assert_eq!((a.invisible, a.modes.lock_position), (true, false));
}
