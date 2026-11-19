//! How SHX fonts are found: drawing folders, the search path, the font map, an app's font
//! provider and FONTALT. Process-wide settings, so one test in its own binary. The fonts are
//! compiled here from shape source written for the test; nothing is read from a CAD install.

use std::sync::Arc;

use cadcraft_fonts::shx::{self, FontProvider};
use cadcraft_fonts::{BUILTIN_FONT, TextFont, ttf};

const FONT: &str = "*0,4,lookup test\n6,2,0,0\n*65,4,UCA\n1,064,0\n";

struct Provider;

impl FontProvider for Provider {
    fn load(&self, file_name: &str) -> Option<Vec<u8>> {
        (file_name == "cadcraft-provided.shx").then(|| shx::compile(FONT).ok()).flatten()
    }
}

#[test]
fn fonts_are_found_where_the_drawing_and_the_app_keep_them() {
    let bytes = shx::compile(FONT).unwrap();
    let dir = std::env::temp_dir().join(format!("cadcraft-shx-lookup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cadcraft-beside.shx"), &bytes).unwrap();
    std::fs::write(dir.join("cadcraft-beside-big.shx"), &bytes).unwrap();

    // Beside the drawing: found once the drawing's folder is known; a later change of folders
    // starts a new generation (viewers regenerate).
    assert!(shx::find("cadcraft-beside.shx").is_none());
    let g = shx::generation();
    shx::add_drawing_dir(&dir);
    assert!(shx::generation() != g);
    assert_eq!(shx::drawing_dirs().first(), Some(&dir));
    assert!(shx::find("CADCRAFT-BESIDE").is_some_and(|f| f.has_shape(65)));
    assert!(TextFont::resolve("cadcraft-beside.shx").is_shx());
    // A file whose name has another case than the drawing's (found on case-sensitive systems
    // too, by looking through the folder).
    std::fs::write(dir.join("CadCraft-Mixed.SHX"), &bytes).unwrap();
    assert!(shx::find("cadcraft-mixed.shx").is_some());

    // The search path, and the font map renaming a font to one that is there.
    let other = dir.join("lib");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("cadcraft-onpath.shx"), &bytes).unwrap();
    assert!(shx::find("cadcraft-onpath.shx").is_none());
    shx::set_search_path(&other.to_string_lossy());
    assert!(shx::find("cadcraft-onpath.shx").is_some());
    shx::set_font_map("cadcraft-old.shx;cadcraft-onpath.shx\nnonsense");
    assert_eq!(shx::font_map(), "cadcraft-old.shx;cadcraft-onpath.shx");
    assert!(shx::find("cadcraft-old").is_some());
    shx::set_font_map("");

    // An app's provider (the web build's file store).
    assert!(shx::find("cadcraft-provided.shx").is_none());
    shx::set_provider(Some(Arc::new(Provider)));
    assert!(shx::find("cadcraft-provided.shx").is_some());
    shx::set_provider(None);
    assert!(shx::find("cadcraft-provided.shx").is_none());

    missing_fonts_and_what_replaces_them();

    // Registered bytes (files opened alongside a drawing on the web) and clearing them.
    shx::register("cadcraft-registered.shx", &bytes).unwrap();
    assert!(shx::find("cadcraft-registered.shx").is_some());
    shx::clear_registered();
    assert!(shx::find("cadcraft-registered.shx").is_none());

    shx::set_search_path("");
    assert_eq!(shx::search_path(), "");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Needs the fonts the test above writes beside the drawing.
fn missing_fonts_and_what_replaces_them() {
    // The SHX fonts of a text style that are missing (a found one, a TrueType name and the built-in
    // font are not), and what replaces them.
    let missing = |font: &str, big: &str| -> Vec<(String, String, String)> {
        TextFont::missing_shx(font, big).into_iter().map(|m| (m.name, m.key, m.substitute)).collect()
    };
    let entry = |name: &str, key: &str, sub: &str| (name.to_string(), key.to_string(), sub.to_string());
    assert_eq!(missing("cadcraft-missing.shx", "cadcraft-beside.shx"), vec![entry("cadcraft-missing.shx", "cadcraft-missing.shx", BUILTIN_FONT)]);
    assert_eq!(missing("cadcraft-beside", "CADCRAFT-Missing-Big"), vec![entry("CADCRAFT-Missing-Big", "cadcraft-missing-big.shx", BUILTIN_FONT)]);
    assert!(missing("arial.ttf", "").is_empty());
    assert!(missing(BUILTIN_FONT, "").is_empty());
    // A TrueType main font draws the style alone: its missing big font isn't used, so isn't listed.
    ttf::register("cadcraft-tt-main.ttf", vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(ttf::find("cadcraft-tt-main").is_some());
    assert!(missing("cadcraft-tt-main", "cadcraft-missing-big").is_empty());

    // FONTALT replaces a missing SHX font (and may name a big font after a comma): each missing font
    // names the replacement for its role.
    assert!(!TextFont::resolve("cadcraft-missing.shx").is_shx());
    ttf::set_font_alt("cadcraft-beside.shx");
    assert_eq!(missing("cadcraft-missing.shx", "")[0].2, "cadcraft-beside.shx");
    assert_eq!(missing("cadcraft-beside", "cadcraft-missing-big")[0].2, BUILTIN_FONT, "no big font in FONTALT");
    ttf::set_font_alt("cadcraft-missing.shx,cadcraft-beside-big.shx");
    assert_eq!(
        missing("cadcraft-missing.shx", "cadcraft-missing-big"),
        vec![
            entry("cadcraft-missing.shx", "cadcraft-missing.shx", BUILTIN_FONT),
            entry("cadcraft-missing-big", "cadcraft-missing-big.shx", "cadcraft-beside-big.shx")
        ]
    );
    // No SHX font for the style at all: a TrueType FONTALT font draws the whole style, big font too.
    ttf::register("cadcraft-tt-alt.ttf", vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    ttf::set_font_alt("cadcraft-tt-alt.ttf");
    assert!(TextFont::resolve_style("cadcraft-missing.shx", "cadcraft-missing-big", false).is_outline());
    assert_eq!(
        missing("cadcraft-missing.shx", "cadcraft-missing-big"),
        vec![
            entry("cadcraft-missing.shx", "cadcraft-missing.shx", "cadcraft-tt-alt.ttf"),
            entry("cadcraft-missing-big", "cadcraft-missing-big.shx", "cadcraft-tt-alt.ttf")
        ]
    );
    // A big font that is there makes it an SHX style: the TrueType font draws nothing.
    assert!(TextFont::resolve_style("cadcraft-missing.shx", "cadcraft-beside-big", false).is_shx());
    assert_eq!(missing("cadcraft-missing.shx", "cadcraft-beside-big"), vec![entry("cadcraft-missing.shx", "cadcraft-missing.shx", BUILTIN_FONT)]);
    ttf::set_font_alt("cadcraft-beside.shx");
    assert!(TextFont::resolve("cadcraft-missing.shx").is_shx());
    assert!(!TextFont::resolve("").is_shx(), "the built-in font is not replaced");
    ttf::set_font_alt("");
}
