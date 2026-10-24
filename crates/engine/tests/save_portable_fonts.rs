//! Text styles with Linux TrueType fonts AutoCAD on Windows doesn't install: saving says which
//! ones, and SAVEAS with portableFonts writes Windows fonts in the file while the drawing keeps
//! its own.

use cadcraft_doc::{Drawing, TextStyle};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, portable_font, set_io};
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
    let mut s = Session::empty();
    s.open_drawing(d, "fonts.dxf", None);
    s
}

#[test]
fn portable_fonts_by_file_or_family_name() {
    assert_eq!(portable_font("DejaVuSans.ttf"), Some("verdana.ttf"));
    assert_eq!(portable_font("/usr/share/fonts/truetype/LiberationSerif-Bold.ttf"), Some("timesbd.ttf"));
    assert_eq!(portable_font("Liberation Mono"), Some("cour.ttf"));
    assert_eq!(portable_font("LiberationMono-BoldItalic.ttf"), Some("courbi.ttf"));
    assert_eq!(portable_font("OpenSans-SemiBold.ttf"), Some("arialbd.ttf"));
    assert_eq!(portable_font("Open Sans"), Some("arial.ttf"));
    assert_eq!(portable_font("arial.ttf"), None);
    assert_eq!(portable_font("txt"), None);
    assert_eq!(portable_font(""), None);
}

#[test]
fn saving_names_the_fonts_and_portable_fonts_replaces_them_in_the_file() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-portable-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().to_string();

    let r = s.execute("saveas", &json!({ "path": path("keep.dxf") })).unwrap();
    assert_eq!(r["nonPortableFonts"], json!([{ "style": "Notes", "font": "DejaVuSans.ttf" }, { "style": "Titles", "font": "Liberation Sans" }]));
    assert!(s.log.iter().any(|l| l.contains("Notes (DejaVuSans.ttf), Titles (Liberation Sans)")), "{:?}", s.log);
    let kept = std::fs::read_to_string(dir.join("keep.dxf")).unwrap();
    assert!(kept.contains("DejaVuSans.ttf"));

    let n = s.log.len();
    let r = s.execute("saveas", &json!({ "path": path("portable.dxf"), "portableFonts": true })).unwrap();
    assert!(r.get("nonPortableFonts").is_none() && s.log.len() == n);
    let file = std::fs::read_to_string(dir.join("portable.dxf")).unwrap();
    assert!(file.contains("verdana.ttf") && file.contains("arial.ttf") && !file.contains("DejaVuSans.ttf"));
    let fonts: Vec<&str> = s.doc().unwrap().text_styles.iter().map(|st| st.font.as_str()).collect();
    assert!(fonts.contains(&"DejaVuSans.ttf") && fonts.contains(&"Liberation Sans"), "the drawing keeps its fonts: {fonts:?}");

    assert!(s.execute("saveas", &json!({ "path": path("bad.dxf"), "portableFonts": "yes" })).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
