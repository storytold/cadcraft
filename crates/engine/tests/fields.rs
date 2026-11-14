//! FIELD, UPDATEFIELD, DWGPROPS and SPELL (#532).

use cadcraft_doc::fields;
use cadcraft_doc::{Common, EntityKind, Handle, Space};
use cadcraft_engine::Session;
use serde_json::json;

/// 2026-10-11 08:05:09 UTC.
const NOW: f64 = 1_791_705_909.0;

fn contents(s: &Session, h: &str) -> String {
    match s.doc().unwrap().entity(Handle::parse_hex(h).unwrap()).map(|e| e.kind.clone()) {
        Some(EntityKind::MText(t)) => t.contents,
        Some(EntityKind::Text(t)) => t.value,
        k => panic!("no text: {k:?}"),
    }
}

fn shown(s: &Session, h: &str) -> String {
    fields::display(&contents(s, h)).into_owned()
}

#[test]
fn date_field_and_its_format() {
    let mut s = Session::new();
    let r = s.execute("field", &json!({"at": [0, 0], "field": "date", "format": "d.M.yyyy", "now": NOW})).unwrap();
    assert_eq!(r["value"], "11.10.2026");
    let h = r["handle"].as_str().unwrap().to_string();
    assert!(contents(&s, &h).starts_with("%<\\AcVar Date \\f \"d.M.yyyy\""));
    // UPDATEFIELD evaluates it again.
    let u = s.execute("updatefield", &json!({"handles": [h], "now": NOW + 86_400.0})).unwrap();
    assert_eq!((u["found"].as_u64(), u["updated"].as_u64()), (Some(1), Some(1)));
    assert_eq!(shown(&s, &h), "12.10.2026");
}

#[test]
fn object_property_fields_follow_the_object() {
    let mut s = Session::new();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    let ch = c["handle"].as_str().unwrap().to_string();
    let r = s.execute("field", &json!({"at": [5, 0], "field": "object", "handle": ch, "property": "diameter", "format": "%lu2%pr2"})).unwrap();
    assert_eq!(r["value"], "4.00");
    let h = r["handle"].as_str().unwrap().to_string();
    s.execute("scale", &json!({"handles": [ch], "base": [0, 0], "factor": 2})).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &h), "8.00");
    // The object is gone: the field shows ####.
    s.execute("erase", &json!({"handles": [ch]})).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &h), "####");
    // An object field needs a handle; unknown names are refused.
    assert!(s.execute("field", &json!({"at": [0, 0], "field": "object"})).is_err());
    assert!(s.execute("field", &json!({"at": [0, 0], "field": "no such thing!"})).is_err());
}

/// A hatch's Area field is its filled area, islands subtracted (odd parity), as AREA reports it.
#[test]
fn hatch_area_field_subtracts_islands() {
    let square = |x: f64, y: f64, side: f64| {
        let v: Vec<_> = [(x, y), (x + side, y), (x + side, y + side), (x, y + side)].iter().map(|(x, y)| json!({"p": {"x": x, "y": y}})).collect();
        json!({"vertices": v})
    };
    let mut s = Session::new();
    let area = |s: &mut Session, loops: Vec<serde_json::Value>| {
        let hatch: cadcraft_doc::Hatch = serde_json::from_value(json!({"pattern": "SOLID", "solid": true, "loops": loops})).unwrap();
        let h = s.doc_mut().unwrap().add(&Space::Model, Common::default(), EntityKind::Hatch(hatch)).unwrap();
        let r = s.execute("field", &json!({"at": [0, 0], "field": "object", "handle": h.hex(), "property": "area"})).unwrap();
        r["value"].as_str().unwrap().to_string()
    };
    // A 10 x 10 outer loop with a 2 x 2 island: 96, not 104.
    assert_eq!(area(&mut s, vec![square(0.0, 0.0, 10.0), square(4.0, 4.0, 2.0)]), "96.0000");
    // An island inside the island's hole is filled again: 100 - 36 + 4.
    assert_eq!(area(&mut s, vec![square(0.0, 0.0, 10.0), square(2.0, 2.0, 6.0), square(4.0, 4.0, 2.0)]), "68.0000");
}

#[test]
fn drawing_properties_filename_and_system_variables() {
    let mut s = Session::new();
    s.execute("dwgprops", &json!({"title": "Bracket", "author": "A. Drafter"})).unwrap();
    let t = s.execute("field", &json!({"at": [0, 0], "field": "title"})).unwrap();
    assert_eq!(t["value"], "Bracket");
    let a = s.execute("field", &json!({"at": [0, 1], "field": "Author"})).unwrap();
    assert_eq!(a["value"], "A. Drafter");
    let f = s.execute("field", &json!({"at": [0, 2], "field": "filename"})).unwrap();
    assert!(!f["value"].as_str().unwrap().is_empty());
    let v = s.execute("field", &json!({"at": [0, 3], "field": "ltscale"})).unwrap();
    assert_eq!(v["value"], "1.000000");
    // DWGPROPS at the command line, then UPDATEFIELD over everything.
    s.cmdline("dwgprops t").unwrap();
    s.cmdline("Plate").unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, t["handle"].as_str().unwrap()), "Plate");
}

#[test]
fn field_from_the_command_line() {
    let mut s = Session::new();
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.cmdline("field object").unwrap();
    s.cmdline("0,1").unwrap();
    s.cmdline("radius").unwrap();
    s.cmdline("10,10").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = s.doc().unwrap().model.iter().last().map(|e| e.kind.clone());
    let Some(EntityKind::MText(m)) = t else { panic!("no field text") };
    assert_eq!(cadcraft_fonts::plain_mtext(&fields::display_mtext(&m.contents)), "1.0000");
}

#[test]
fn text_fields_and_hostile_codes() {
    let mut s = Session::new();
    let t = s.execute("text", &json!({"at": [0, 0], "text": "Rev %<\\AcVar Date \\f \"yyyy\">% %<\\AcVar nosuchvar>% %<unclosed"})).unwrap();
    let h = t["handle"].as_str().unwrap().to_string();
    s.execute("updatefield", &json!({"handles": [h], "now": NOW})).unwrap();
    assert_eq!(shown(&s, &h), "Rev 2026 #### %<unclosed");
    for bad in ["%<\\AcObjProp Object(%<\\_ObjId FFFFFFFFFFFFFFFFFFFF>%).Area>%", &"%<".repeat(10_000), "%<\\AcVar \\f \"\\\">%"] {
        s.execute("text", &json!({"at": [0, 0], "text": bad})).unwrap();
    }
    s.execute("updatefield", &json!({})).unwrap();
}

#[test]
fn spell_reports_and_replaces_unknown_words() {
    let mut s = Session::new();
    let t = s.execute("mtext", &json!({"at": [0, 0], "text": "Break teh edges {\\H2x;of} the plate"})).unwrap();
    let h = t["handle"].as_str().unwrap().to_string();
    let words = json!(["break", "the", "edges", "of", "plate", "ten"]);
    let r = s.execute("spell", &json!({"words": words})).unwrap();
    let unknown = r["unknown"].as_array().unwrap();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0]["word"], "teh");
    assert_eq!(unknown[0]["suggestions"], json!(["ten", "the"]));
    s.execute("spell", &json!({"words": words, "replace": {"teh": "the"}})).unwrap();
    assert_eq!(contents(&s, &h), "Break the edges {\\H2x;of} the plate");
    // No word list at all is an error, not an empty pass.
    assert!(s.execute("spell", &json!({})).is_err());
}

#[test]
fn spell_replaces_shown_text_only_not_formatting_payloads() {
    let mut s = Session::new();
    let t = s.execute("mtext", &json!({"at": [0, 0], "text": "\\fArial|b0;Arial teh"})).unwrap();
    let h = t["handle"].as_str().unwrap().to_string();
    s.execute("spell", &json!({"words": ["teh"], "replace": {"Arial": "Font", "teh": "the"}})).unwrap();
    assert_eq!(contents(&s, &h), "\\fArial|b0;Font the");
}

#[test]
fn spell_check_without_replace_does_not_dirty_the_drawing() {
    let mut s = Session::new();
    s.execute("mtext", &json!({"at": [0, 0], "text": "teh"})).unwrap();
    let before = s.state().unwrap().revision;
    s.execute("spell", &json!({"words": ["the"]})).unwrap();
    assert_eq!(s.state().unwrap().revision, before);
}

#[test]
fn spell_at_the_command_line_with_a_user_dictionary() {
    let dir = std::env::temp_dir().join(format!("cadcraft-spell-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dict = dir.join("user.dic");
    std::fs::write(&dict, "the\nedges\nplate\n").unwrap();
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "SPELLDICTIONARY", "value": dict.to_string_lossy()})).unwrap();
    let t = s.execute("text", &json!({"at": [0, 0], "text": "teh edges of plaet"})).unwrap();
    let h = t["handle"].as_str().unwrap().to_string();
    s.cmdline("spell all").unwrap();
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("\"teh\""), "{}", s.prompt_text());
    s.cmdline("the").unwrap();
    assert!(s.prompt_text().contains("\"of\""), "{}", s.prompt_text());
    s.cmdline("add").unwrap();
    assert!(s.prompt_text().contains("\"plaet\""), "{}", s.prompt_text());
    s.cmdline("ignore").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(contents(&s, &h), "the edges of plaet");
    assert!(std::fs::read_to_string(&dict).unwrap().lines().any(|l| l == "of"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spell_never_uses_a_dictionary_path_from_the_drawing() {
    let dir = std::env::temp_dir().join(format!("cadcraft-spell-evil-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let evil = dir.join("victim.txt");
    std::fs::write(&evil, "the\nedges\n").unwrap();
    let good = dir.join("good.dic");
    std::fs::write(&good, "the\nedges\n").unwrap();
    let mut s = Session::new();
    // The drawing header names a file (as a downloaded drawing could).
    s.execute("setvar", &json!({"name": "DCTCUST", "value": evil.to_string_lossy()})).unwrap();
    s.execute("text", &json!({"at": [0, 0], "text": "teh edges"})).unwrap();
    // Command line with the setting pointing elsewhere: Add writes there, not to the header's file.
    s.execute("setvar", &json!({"name": "SPELLDICTIONARY", "value": good.to_string_lossy()})).unwrap();
    s.cmdline("spell all").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("add").unwrap();
    assert_eq!(std::fs::read_to_string(&evil).unwrap(), "the\nedges\n");
    assert!(std::fs::read_to_string(&good).unwrap().lines().any(|l| l == "teh"));
    // The explicit `dictionary` parameter reads the named file.
    let r = s.execute("spell", &json!({"dictionary": good.to_string_lossy()})).unwrap();
    assert_eq!(r["unknown"].as_array().unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A formula field with a field nested in it (the LTSCALE case of #551) is computed: the nested
/// field gets its value, the formula the result in the format asked for.
#[test]
fn formula_field_with_a_nested_field_is_evaluated() {
    let mut s = Session::new();
    let code = "Double: %<\\AcExpr (%<\\AcVar LTSCALE>%*2) \\f \"%lu2\">%";
    let m = s.execute("mtext", &json!({"at": [0, 0], "text": code, "height": 2.5, "width": 80})).unwrap();
    let h = m["handle"].as_str().unwrap().to_string();
    assert_eq!(shown(&s, &h), "Double: ####");
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &h), "Double: 2.0000");
    let c = contents(&s, &h);
    assert!(c.contains("\\AcVar LTSCALE \\CcValue \"1.000000\""), "{c}");
    // The formula follows the variable.
    s.execute("setvar", &json!({"name": "LTSCALE", "value": 2.5})).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &h), "Double: 5.0000");
    // The precision comes from the format.
    s.execute("mtext", &json!({"at": [0, 5], "text": "%<\\AcExpr (1+2)/8 \\f \"%lu2%pr3\">%", "height": 2.5, "width": 80})).unwrap();
    s.execute("updatefield", &json!({})).unwrap();
    let texts: Vec<String> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| if let EntityKind::MText(t) = &e.kind { Some(fields::display(&t.contents).into_owned()) } else { None })
        .collect();
    assert!(texts.contains(&"0.375".to_string()), "{texts:?}");
}

/// A formula that cannot be computed keeps the value it had, and never divides by zero.
#[test]
fn formula_that_cannot_be_computed_keeps_its_value() {
    let mut s = Session::new();
    let m =
        s.execute("mtext", &json!({"at": [0, 0], "text": "%<\\AcExpr (1/0) \\f \"%lu2\" \\CcValue \"7.00\">%", "height": 2.5, "width": 80})).unwrap();
    let h = m["handle"].as_str().unwrap().to_string();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &h), "7.00");
}

/// Formula edge cases: a sign binds looser than `^`, and a long `^` chain is refused, not a stack overflow.
#[test]
fn formula_precedence_and_hostile_chains() {
    let mut s = Session::new();
    let chain = "2^".repeat(5000);
    let texts = ["%<\\AcExpr -2^2 \\f \"%lu2%pr1\">%".to_string(), format!("%<\\AcExpr {chain}2 \\f \"%lu2\" \\CcValue \"9\">%")];
    let hs: Vec<String> = texts
        .iter()
        .map(|t| s.execute("mtext", &json!({"at": [0, 0], "text": t, "height": 2.5, "width": 80})).unwrap()["handle"].as_str().unwrap().to_string())
        .collect();
    s.execute("updatefield", &json!({})).unwrap();
    assert_eq!(shown(&s, &hs[0]), "-4.0");
    assert_eq!(shown(&s, &hs[1]), "9");
}
