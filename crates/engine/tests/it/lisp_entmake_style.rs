//! `entmake` of a STYLE list adds a text style, as AutoCAD does (#3130).

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};

fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    Session::new()
}

const MAKE: &str = r#"(setq r (entmake '((0 . "STYLE") (100 . "AcDbSymbolTableRecord") (100 . "AcDbTextStyleTableRecord")
    (2 . "MyStyle") (70 . 0) (40 . 2.5) (41 . 0.8) (50 . 0.2617993877991494) (71 . 2) (42 . 2.5) (3 . "arial.ttf") (4 . ""))))"#;

#[test]
fn entmake_style_creates_the_text_style() {
    let mut s = session();
    let before = s.doc().unwrap().text_styles.len();
    s.cmdline(MAKE).unwrap();
    let d = s.doc().unwrap();
    let st = d.text_style("MyStyle").expect("style was made");
    assert_eq!((st.height, st.width_factor, st.font.as_str(), st.backwards), (2.5, 0.8, "arial.ttf", true));
    assert!((st.oblique - 0.2617993877991494).abs() < 1e-12);
    assert_eq!(d.text_styles.len(), before + 1);
    // A second make of the same name (any case) or an empty name makes nothing.
    s.cmdline("(setq r2 (entmake '((0 . \"STYLE\") (2 . \"mystyle\") (70 . 0))))").unwrap();
    s.cmdline("(setq r3 (entmake '((0 . \"STYLE\") (2 . \"\") (70 . 0))))").unwrap();
    assert_eq!(s.doc().unwrap().text_styles.len(), before + 1);
    // Text where a number belongs makes nothing, rather than reading as zero and passing.
    s.cmdline("(setq r4 (entmake '((0 . \"STYLE\") (2 . \"BadHeight\") (70 . 0) (40 . \"bad\"))))").unwrap();
    s.cmdline("(setq r5 (entmake '((0 . \"STYLE\") (2 . \"BadFlags\") (70 . \"bad\"))))").unwrap();
    assert_eq!(s.doc().unwrap().text_styles.len(), before + 1);
    assert!(s.doc().unwrap().text_style("BadHeight").is_none());
    assert!(s.doc().unwrap().text_style("BadFlags").is_none());
    // A numeric zero height is a height, and an omitted one keeps the default.
    s.cmdline("(setq r6 (entmake '((0 . \"STYLE\") (2 . \"ZeroHeight\") (70 . 0) (40 . 0))))").unwrap();
    s.cmdline("(setq r7 (entmake '((0 . \"STYLE\") (2 . \"NoHeight\") (70 . 0))))").unwrap();
    assert_eq!(s.doc().unwrap().text_style("ZeroHeight").map(|st| st.height), Some(0.0));
    assert_eq!(s.doc().unwrap().text_style("NoHeight").map(|st| st.height), Some(0.0));
}
