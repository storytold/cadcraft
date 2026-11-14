//! #3009: opening a drawing whose text styles name SHX fonts that can't be found says so, naming the
//! fonts, their styles and the font that replaces them; SETVAR SUPPORTPATH adds the fonts' folder.
//! The fonts are compiled here from shape source written for the test. The search path is
//! process-wide, so this is one test in its own binary.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, TextStyle};
use serde_json::{Value, json};

const FONT: &str = "*0,4,note test\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n";

fn open(s: &mut Session, path: &str) -> (Vec<String>, Vec<String>) {
    let from = s.log.len();
    let r = s.execute("open", &json!({ "path": path })).unwrap();
    let warnings = r["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect();
    (warnings, s.log[from..].to_vec())
}

#[test]
fn a_missing_shx_font_is_reported_when_the_drawing_is_opened() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-note-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fonts = dir.join("fonts");
    std::fs::create_dir_all(&fonts).unwrap();
    let bytes = cadcraft_fonts::shx::compile(FONT).unwrap();
    // One font is beside the drawing; the other two are in a folder that is not searched yet.
    std::fs::write(dir.join("cadcraft-note-beside.shx"), &bytes).unwrap();
    std::fs::write(fonts.join("cadcraft-note-lost.shx"), &bytes).unwrap();
    std::fs::write(fonts.join("cadcraft-note-lostbig.shx"), &bytes).unwrap();
    let mut d = Drawing::new_imperial();
    let style = |name: &str, font: &str, big: &str| TextStyle { name: name.into(), font: font.into(), big_font: big.into(), ..TextStyle::default() };
    d.text_styles.push(style("Beside", "cadcraft-note-beside.shx", ""));
    d.text_styles.push(style("Notes", r"C:\Fonts\Cadcraft-Note-Lost.SHX", "cadcraft-note-lostbig.shx"));
    d.text_styles.push(style("Titles", "cadcraft-note-lost", ""));
    d.text_styles.push(style("Plain", "CADCraft Stroke", ""));
    let path = dir.join("host.dxf").to_string_lossy().into_owned();
    std::fs::write(&path, cadcraft_io::write(&d, "host.dxf").unwrap()).unwrap();

    let mut s = Session::new();
    let (warnings, log) = open(&mut s, &path);
    let note = warnings.iter().find(|w| w.contains("not found")).unwrap_or_else(|| panic!("no note in {warnings:?}"));
    assert!(note.contains("cadcraft-note-lost") && note.contains("cadcraft-note-lostbig.shx"), "{note}");
    assert!(note.contains("Notes") && note.contains("Titles"), "the styles that use the font: {note}");
    assert!(note.contains("CADCraft Stroke"), "what replaces it: {note}");
    assert!(note.contains("SUPPORTPATH"), "how to fix it: {note}");
    assert!(!note.contains("beside") && !note.contains("Plain"), "a font that is found is not reported: {note}");
    assert!(log.contains(note), "the command line shows it too: {log:?}");

    // FONTALT names the replacement.
    cadcraft_fonts::ttf::set_font_alt("cadcraft-note-beside.shx");
    let (warnings, _) = open(&mut s, &path);
    assert!(warnings.iter().any(|w| w.contains("drawn with cadcraft-note-beside.shx")), "{warnings:?}");
    cadcraft_fonts::ttf::set_font_alt("");

    // The fonts' folder on the support search path: nothing is missing.
    s.execute("setvar", &json!({ "name": "SUPPORTPATH", "value": fonts.to_string_lossy() })).unwrap();
    assert_eq!(cadcraft_engine::sysvars::get(&s, "ACADPREFIX"), Some(json!(fonts.to_string_lossy())), "ACADPREFIX reports the path");
    assert!(s.execute("setvar", &json!({ "name": "ACADPREFIX", "value": "x" })).is_err(), "ACADPREFIX is read-only");
    let (warnings, log) = open(&mut s, &path);
    assert!(!warnings.iter().any(|w| w.contains("not found")), "{warnings:?}");
    assert!(!log.iter().any(|l| l.contains("not found")), "{log:?}");
    let font = cadcraft_fonts::TextFont::resolve_style("cadcraft-note-lost.shx", "cadcraft-note-lostbig.shx", false);
    assert!(font.is_shx());

    cadcraft_fonts::shx::set_search_path("");
    let _ = std::fs::remove_dir_all(&dir);
}
