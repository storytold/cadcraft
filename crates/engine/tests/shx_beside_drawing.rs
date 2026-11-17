//! FF8: the SHX fonts a drawing's text styles name are found beside the drawing it opens. The
//! font is compiled here from shape source written for the test.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, TextStyle};
use serde_json::json;

#[test]
fn opening_a_drawing_finds_the_shx_fonts_beside_it() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-beside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let font = cadcraft_fonts::shx::compile("*0,4,beside\n6,2,0,0\n*65,7,UCA\n1,064,040,06C,2,020,0\n").unwrap();
    std::fs::write(dir.join("cadcraft-beside-engine.shx"), font).unwrap();
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Beside".into(), font: "cadcraft-beside-engine.shx".into(), ..TextStyle::default() });
    let path = dir.join("host.dxf");
    std::fs::write(&path, cadcraft_io::write(&d, "host.dxf").unwrap()).unwrap();

    assert!(!cadcraft_fonts::TextFont::resolve("cadcraft-beside-engine.shx").is_shx());
    let mut s = Session::new();
    s.execute("open", &json!({ "path": path.to_string_lossy() })).unwrap();
    let style = s.doc().unwrap().text_style("Beside").cloned().unwrap();
    let font = cadcraft_fonts::TextFont::resolve_style(&style.font, &style.big_font, style.vertical);
    assert!(font.is_shx());
    assert!((cadcraft_fonts::text_width(&font, "AA", 3.0, 1.0) - 6.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&dir);
}
