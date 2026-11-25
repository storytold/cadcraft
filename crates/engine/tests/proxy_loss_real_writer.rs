//! What a DXF before 2007 leaves out for its proxies' string streams, counted by the real DXF
//! writer (#3229): only what 2007 would keep, from the drawing the command writes. In its own
//! test binary because it installs the real io hooks for the whole process.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, SaveAs, set_io, set_proxies_lost, set_write_as};
use cadcraft_engine::doc::Drawing;
use serde_json::{Value, json};
#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// The real reader and writer, and the real proxy-loss count.
fn install() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_write_as(|d, name, o: &SaveAs| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    set_proxies_lost(cadcraft_io::proxies_lost);
    cadcraft_engine::cmd::file::set_dwg_compat_lost(cadcraft_io::dwg_compat_lost);
}

/// A proxy entity of class 500 (WIDGETX) in `owner`, with a string stream of `strings` bytes.
fn proxy(handle: &str, owner: &str, strings: &str, class: &str) -> Vec<(String, String)> {
    let p = |c: &str, v: &str| (c.to_string(), v.to_string());
    vec![
        p("0", "ACAD_PROXY_ENTITY"),
        p("5", handle),
        p("330", owner),
        p("100", "AcDbEntity"),
        p("8", "0"),
        p("100", "AcDbProxyEntity"),
        p("90", "498"),
        p("91", class),
        p("95", "393245"),
        p("70", "0"),
        p("162", strings),
        p("161", "16"),
        p("310", "AABB"),
    ]
}

fn pairs(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines.chunks(2).map(|c| (c[0].trim().to_string(), c.get(1).map_or(String::new(), |v| v.trim().to_string()))).collect()
}

/// A drawing with `$ACADVER` `acadver` read from a DXF holding the class WIDGETX and, in model
/// space, the proxies `entities` gives (each from the model space block record's handle).
fn drawing(acadver: &str, entities: impl Fn(&str) -> Vec<Vec<(String, String)>>) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.header.set_str("ACADVER", "AC1024");
    let text = String::from_utf8(cadcraft_io::write(&d, "p.dxf").unwrap()).unwrap();
    let mut p = pairs(&text);
    let g = |c: &str, v: &str| (c.to_string(), v.to_string());
    let classes = p.iter().position(|q| q.0 == "2" && q.1 == "CLASSES").unwrap() + 1;
    let class =
        [g("0", "CLASS"), g("1", "WIDGETX"), g("2", "AcDbWidgetX"), g("3", "ObjectDBX Classes"), g("90", "4095"), g("280", "0"), g("281", "1")];
    p.splice(classes..classes, class);
    // The model space block record: the owner of model space entities.
    let record_end = |i: usize| i + 1 + p[i + 1..].iter().position(|q| q.0 == "0").unwrap_or(p.len() - i - 1);
    let ms = (0..p.len())
        .filter(|&i| p[i].0 == "0" && p[i].1 == "BLOCK_RECORD")
        .find(|&i| p[i..record_end(i)].iter().any(|q| q.0 == "2" && q.1.eq_ignore_ascii_case("*Model_Space")))
        .unwrap();
    let owner = p[ms..].iter().find(|q| q.0 == "5").unwrap().1.clone();
    let ents = p.iter().position(|q| q.0 == "2" && q.1 == "ENTITIES").unwrap();
    let end = ents + p[ents..].iter().position(|q| q.0 == "0" && q.1 == "ENDSEC").unwrap();
    p.splice(end..end, entities(&owner).into_iter().flatten());
    let bytes: String = p.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect();
    let mut d = cadcraft_io::read(bytes.as_bytes(), "p.dxf").unwrap();
    d.header.set_str("ACADVER", acadver);
    d
}

/// Two proxies with string streams, one whose class number names no class (no version writes
/// it), and one without: before 2007 only the first one is lost to the string stream.
fn three_proxies(owner: &str) -> Vec<Vec<(String, String)>> {
    vec![proxy("AAA1", owner, "8", "500"), proxy("AAA2", owner, "8", "777"), proxy("AAA3", owner, "0", "500")]
}

fn not_saved(r: &Value) -> Value {
    r.get("notSaved").cloned().unwrap_or(Value::Null)
}

#[test]
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
fn legacy_visualstyles_warn_only_for_2007_dwg() {
    install();
    cadcraft_io::dwg_worker::allow_in_process();
    let source = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n\
                  0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n30\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n0\n20\n0\n30\n0\n11\n10\n21\n5\n31\n0\n0\nENDSEC\n\
                  0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nACAD_VISUALSTYLE\n350\n20\n\
                  0\nDICTIONARY\n5\n20\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nSynthetic\n350\n21\n\
                  0\nVISUALSTYLE\n5\n21\n330\n20\n100\nAcDbVisualStyle\n2\nSynthetic\n70\n0\n71\n1\n72\n1\n73\n1\n90\n0\n40\n1.0\n41\n0.5\n63\n7\n\
                  74\n2\n91\n0\n64\n7\n65\n7\n75\n1\n175\n1\n42\n30.0\n92\n0\n66\n7\n43\n1.0\n76\n1\n77\n0\n78\n0\n67\n7\n79\n1\n\
                  170\n0\n171\n0\n290\n0\n174\n0\n93\n0\n44\n0.0\n173\n0\n291\n0\n0\nENDSEC\n0\nEOF\n";
    let drawing = cadcraft_io::read_dxf(source.as_bytes()).unwrap();
    let mut session = Session::empty();
    session.open_drawing(drawing, "styles.dxf", None);
    assert_eq!(cadcraft_engine::cmd::file::loss_on_save(&session, "styles.dwg").get("VISUALSTYLE"), Some(&1));
    for (format, version) in [("dwg", 2007), ("dwg", 2013), ("dxf", 2007)] {
        let start = session.log.len();
        let result = session.execute("document.bytes", &json!({"format": format, "version": version})).unwrap();
        if format == "dwg" && version == 2007 {
            assert_eq!(not_saved(&result), json!({"VISUALSTYLE": 1}));
            assert!(session.log[start..].iter().any(|message| message.contains("VISUALSTYLE")));
        } else {
            assert_eq!(not_saved(&result), Value::Null);
            assert!(!session.log[start..].iter().any(|message| message.contains("VISUALSTYLE")));
        }
    }
}

#[test]
fn a_proxy_no_version_writes_is_not_reported() {
    install();
    let dir = TempDir::new("proxy-loss");
    let mut s = Session::empty();
    s.open_drawing(drawing("AC1015", three_proxies), "proxy.dxf", None);
    let path = dir.path_str("out.dxf");
    let r = s.execute("saveas", &json!({ "path": path, "version": 2000 })).unwrap();
    assert_eq!(not_saved(&r), json!({ "ACAD_PROXY_ENTITY": 1 }), "{:?}", s.log);
    let r = s.execute("saveas", &json!({ "path": path, "version": 2007 })).unwrap();
    assert_eq!(not_saved(&r), Value::Null);
}

#[test]
fn export_reports_the_drawing_it_writes() {
    // In the Block Editor EXPORT writes the block, which holds no proxy: nothing is lost.
    install();
    let mut s = Session::empty();
    s.open_drawing(drawing("AC1015", three_proxies), "proxy.dxf", None);
    let r = s.execute("export", &json!({ "format": "dxf" })).unwrap();
    assert_eq!(not_saved(&r), json!({ "ACAD_PROXY_ENTITY": 1 }));
    s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let line = s.execute("entities", &json!({ "type": "LINE" })).unwrap()["entities"][0]["handle"].as_str().unwrap().to_string();
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [line], "keep": "convert" })).unwrap();
    s.execute("bedit", &json!({ "name": "B" })).unwrap();
    let n = s.log.len();
    let r = s.execute("export", &json!({ "format": "dxf" })).unwrap();
    assert_eq!(not_saved(&r), Value::Null, "{:?}", &s.log[n..]);
}

#[test]
fn a_reference_edit_working_copy_is_not_counted_twice() {
    install();
    let dir = TempDir::new("proxy-loss");
    let mut s = Session::empty();
    s.open_drawing(drawing("AC1015", |o| vec![proxy("AAA1", o, "8", "500")]), "proxy.dxf", None);
    s.execute("block", &json!({ "name": "W", "base": [0, 0], "handles": ["AAA1"], "keep": "convert" })).unwrap();
    let ins = s.execute("entities", &json!({ "type": "INSERT" })).unwrap()["entities"][0]["handle"].as_str().unwrap().to_string();
    let path = dir.path_str("before.dxf");
    let r = s.execute("saveas", &json!({ "path": path, "version": 2000 })).unwrap();
    assert_eq!(not_saved(&r), json!({ "ACAD_PROXY_ENTITY": 1 }), "in the block");
    s.execute("refedit", &json!({ "handle": ins })).unwrap();
    // Saves wait for REFCLOSE; the QSAVE question and `document.bytes` count the committed drawing.
    let r = s.execute("document.bytes", &json!({ "format": "dxf", "version": 2000 })).unwrap();
    assert_eq!(not_saved(&r), json!({ "ACAD_PROXY_ENTITY": 1 }), "the working copy isn't written");
    assert_eq!(cadcraft_engine::cmd::file::loss_on_save(&s, "during.dxf"), [("ACAD_PROXY_ENTITY".to_string(), 1)].into_iter().collect());
}

#[test]
fn dash_wblock_of_the_whole_drawing_reports_a_pre_2007_dxf() {
    install();
    let dir = TempDir::new("proxy-loss");
    let mut s = Session::empty();
    s.open_drawing(drawing("AC1015", three_proxies), "proxy.dxf", None);
    let path = dir.path_str("whole.dxf").replace('\\', "/");
    s.cmdline(&format!("(command \"_.-WBLOCK\" \"{path}\" \"\" \"*\")")).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(std::path::Path::new(&path).exists());
    assert!(s.log.iter().any(|l| l.contains("DXF before 2007") && l.ends_with("1 ACAD_PROXY_ENTITY")), "{:?}", s.log);
}
