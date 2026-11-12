//! Text styles with Linux fonts AutoCAD on Windows doesn't have: a save names them, and SAVEAS
//! with portableFonts writes Windows fonts in that file while the drawing keeps its own.

use cadcraft_doc::{Drawing, TextStyle};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn dxf(d: &Drawing, _: &str) -> Result<Vec<u8>, String> {
    Ok(cadcraft_io::write_dxf(d).into_bytes())
}

fn session() -> Session {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: dxf, plot: None });
    let mut d = Drawing::new_metric();
    for (name, font) in [("Notes", "DejaVuSans.ttf"), ("Plain", "arial.ttf"), ("Tight", "Liberation Sans Narrow")] {
        d.text_styles.push(TextStyle { name: name.into(), font: font.into(), ..TextStyle::default() });
    }
    let mut s = Session::empty();
    s.open_drawing(d, "fonts.dxf", None);
    s
}

#[test]
fn portable_fonts_writes_windows_fonts_in_that_file() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-portable-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();
    let file = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let both = json!([{ "style": "Notes", "font": "DejaVuSans.ttf" }, { "style": "Tight", "font": "Liberation Sans Narrow" }]);

    // portableFonts writes Verdana for DejaVu Sans; the narrow style has no Windows font.
    let r = s.execute("saveas", &json!({ "path": path("portable.dxf"), "portableFonts": true })).unwrap();
    assert_eq!(r["nonPortableFonts"], json!([{ "style": "Tight", "font": "Liberation Sans Narrow" }]));
    assert_eq!(s.log.last().map(String::as_str), Some("Fonts AutoCAD on Windows doesn't have: Tight (Liberation Sans Narrow)."));
    assert!(file("portable.dxf").contains("verdana.ttf"));
    assert!(s.state().unwrap().drawing().text_styles.iter().any(|t| t.font == "DejaVuSans.ttf"), "the drawing keeps its fonts");

    // A later save without it writes the drawing's fonts again, and says so.
    let r = s.execute("qsave", &json!({})).unwrap();
    assert_eq!(r["nonPortableFonts"], both);
    assert!(s.log.last().is_some_and(|l| l.ends_with("SAVEAS with portableFonts writes Windows fonts for those that have one.")), "{:?}", s.log);
    assert!(!file("portable.dxf").contains("verdana.ttf"));

    // The same list isn't said again on every save.
    let lines = s.log.len();
    let r = s.execute("qsave", &json!({})).unwrap();
    assert_eq!(r["nonPortableFonts"], both);
    assert!(s.log.iter().skip(lines).all(|l| !l.starts_with("Fonts AutoCAD")), "{:?}", s.log);

    assert!(s.execute("saveas", &json!({ "path": path("bad.dxf"), "portableFonts": "yes" })).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
