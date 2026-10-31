//! Text styles with Linux fonts AutoCAD on Windows doesn't have: a save names them, and SAVEAS
//! with portableFonts writes Windows fonts in the file while the drawing keeps its own.

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
    d.text_styles.push(TextStyle { name: "Notes".into(), font: "DejaVuSans.ttf".into(), ..Default::default() });
    d.text_styles.push(TextStyle { name: "Titles".into(), font: "Liberation Sans".into(), ..Default::default() });
    d.text_styles.push(TextStyle { name: "Plain".into(), font: "arial.ttf".into(), ..Default::default() });
    // A narrow family: named, but Windows has no stand-in for it.
    d.text_styles.push(TextStyle { name: "Tight".into(), font: "Liberation Sans Narrow".into(), ..Default::default() });
    let mut s = Session::empty();
    s.open_drawing(d, "fonts.dxf", None);
    s
}

#[test]
fn saving_names_the_fonts_and_portable_fonts_replaces_them_in_the_file() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-portable-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();

    let r = s.execute("saveas", &json!({ "path": path("keep.dxf") })).unwrap();
    let named = json!([
        { "style": "Notes", "font": "DejaVuSans.ttf" },
        { "style": "Titles", "font": "Liberation Sans" },
        { "style": "Tight", "font": "Liberation Sans Narrow" }
    ]);
    assert_eq!(r["nonPortableFonts"], named);
    assert!(s.log.iter().any(|l| l.contains("Notes (DejaVuSans.ttf), Titles (Liberation Sans), Tight (Liberation Sans Narrow)")), "{:?}", s.log);
    let kept = std::fs::read_to_string(dir.join("keep.dxf")).unwrap();
    assert!(kept.contains("DejaVuSans.ttf"));

    // With portableFonts the others are replaced in the file; the narrow one is kept and named.
    let r = s.execute("saveas", &json!({ "path": path("portable.dxf"), "portableFonts": true })).unwrap();
    assert_eq!(r["nonPortableFonts"], json!([{ "style": "Tight", "font": "Liberation Sans Narrow" }]));
    assert!(s.log.last().is_some_and(|l| l.contains("No Windows font for: Tight (Liberation Sans Narrow)")), "{:?}", s.log);
    let file = std::fs::read_to_string(dir.join("portable.dxf")).unwrap();
    assert!(file.contains("verdana.ttf") && file.contains("arial.ttf") && !file.contains("DejaVuSans.ttf"));
    assert!(file.contains("Liberation Sans Narrow") && !file.contains("arialn"), "kept, not replaced by an Office font");
    // QSAVE takes no SAVEAS options: it names every such font and replaces none.
    let r = s.execute("qsave", &json!({ "portableFonts": true })).unwrap();
    assert_eq!(r["nonPortableFonts"], named);
    assert!(std::fs::read_to_string(dir.join("portable.dxf")).unwrap().contains("DejaVuSans.ttf"));
    let fonts: Vec<&str> = s.doc().unwrap().text_styles.iter().map(|st| st.font.as_str()).collect();
    assert!(fonts.contains(&"DejaVuSans.ttf") && fonts.contains(&"Liberation Sans"), "the drawing keeps its fonts: {fonts:?}");

    assert!(s.execute("saveas", &json!({ "path": path("bad.dxf"), "portableFonts": "yes" })).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
