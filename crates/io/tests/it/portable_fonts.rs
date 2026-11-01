//! Portable fonts: a drawing saved with them gets the Windows font for each Linux font in its
//! STYLE table, keeps its own in CADCraft xdata, and reopens with it, from DXF and DWG.

use cadcraft_doc::{Drawing, TextStyle};
use cadcraft_io::{read, read_dxf, write, write_dxf};

fn drawing(portable_fonts: bool) -> Drawing {
    let mut d = Drawing { portable_fonts, ..Drawing::new_metric() };
    // A Linux font, and one with no Windows font.
    for (name, font) in [("Notes", "DejaVuSans-Bold.ttf"), ("Tight", "Liberation Sans Narrow")] {
        d.text_styles.push(TextStyle { name: name.into(), font: font.into(), ..TextStyle::default() });
    }
    d
}

/// (style, font) of the drawing's text styles.
fn fonts(d: &Drawing) -> Vec<(String, String)> {
    d.text_styles.iter().map(|t| (t.name.clone(), t.font.clone())).collect()
}

/// (style, font file) of the STYLE records of DXF text.
fn style_files(dxf: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut style = false;
    for t in cadcraft_dxf::parse(dxf.as_bytes()).unwrap() {
        match t.code {
            0 => style = t.str() == "STYLE",
            2 if style => out.push((t.str(), String::new())),
            3 if style => out.last_mut().unwrap().1 = t.str(),
            _ => {}
        }
    }
    out
}

#[test]
fn styles_get_windows_fonts_and_reopen_with_their_own() {
    let d = drawing(true);
    let dxf = write_dxf(&d);
    let files = style_files(&dxf);
    for (style, file) in [("Notes", "verdanab.ttf"), ("Tight", "Liberation Sans Narrow")] {
        assert!(files.contains(&(style.into(), file.into())), "{style}: {files:?}");
    }
    for back in [read_dxf(dxf.as_bytes()).unwrap(), read(&write(&d, "t.dwg").unwrap(), "t.dwg").unwrap()] {
        assert_eq!(fonts(&back), fonts(&drawing(false)));
    }
    // The Windows font with a path is still the one written for the style's own.
    let path = read_dxf(dxf.replace("verdanab.ttf", "C:\\Windows\\Fonts\\verdanab.ttf").as_bytes()).unwrap();
    assert_eq!(fonts(&path), fonts(&drawing(false)));
    // A font changed in AutoCAD since (the xdata left as it was) is the font read.
    let edited = read_dxf(dxf.replace("verdanab.ttf", "times.ttf").as_bytes()).unwrap();
    assert!(fonts(&edited).contains(&("Notes".into(), "times.ttf".into())));
}

#[test]
fn without_portable_fonts_the_file_has_the_drawings_fonts() {
    let files = style_files(&write_dxf(&drawing(false)));
    assert!(files.contains(&("Notes".into(), "DejaVuSans-Bold.ttf".into())), "{files:?}");
}
