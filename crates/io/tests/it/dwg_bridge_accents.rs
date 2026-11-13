//! Accented names and text survive a DWG save. The DWG bridge's R2000 DXF wrote every non-ASCII
//! character as a `\U+XXXX` escape, and acadrust 0.6.7 and 0.6.8 read an escape for a character
//! the code page has (and every escape in a DXF declared 2007 or later) as literal text, so a block
//! named `Média` reached the DWG as `M\U+00E9dia`; the ODA's audit renamed it, a layout tab showed
//! the escape. The DWG's own text, as acadrust reads it back, must hold the characters.

#![cfg(not(target_arch = "wasm32"))]

use cadcraft_doc::{Drawing, EntityKind};

/// A drawing with an accented layer, block, TEXT, MTEXT and layout name,
/// and texts that test the edges: a literal escape, code page bytes that look like UTF-8, and
/// characters ANSI_1252 doesn't have.
fn drawing() -> Drawing {
    let dxf = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  9\n$DWGCODEPAGE\n  3\nANSI_1252\n  0\nENDSEC\n\
  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nElétrica\n 70\n0\n 62\n7\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n\
  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  8\n0\n  2\nTomada Média\n 70\n0\n 10\n0\n 20\n0\n 30\n0\n\
  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n  0\nENDBLK\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n\
  0\nTEXT\n  8\nElétrica\n 10\n0\n 20\n0\n 40\n1\n  1\nAção Ω 漢\n\
  0\nTEXT\n  8\n0\n 10\n0\n 20\n2\n 40\n1\n  1\nliteral \\U+00E9\n\
  0\nTEXT\n  8\n0\n 10\n0\n 20\n4\n 40\n1\n  1\nÃ© e é\n\
  0\nMTEXT\n  8\n0\n 10\n0\n 20\n6\n 40\n1\n  1\nPortão de acesso\n\
  0\nINSERT\n  8\n0\n  2\nTomada Média\n 10\n5\n 20\n5\n\
  0\nENDSEC\n  0\nEOF\n";
    let mut d = cadcraft_io::read(dxf.as_bytes(), "x.dxf").unwrap();
    let layout = d.layouts.iter_mut().find(|l| !l.name.eq_ignore_ascii_case("Model")).expect("a paper layout");
    layout.name = "Planta Elétrica".into();
    d
}

/// `name` as acadrust writes it in the DXF it gives for a DWG: UTF-8 from 2007, before that
/// ANSI_1252 (the Latin-1 range here) with `\U+XXXX` for the other characters.
fn as_written(name: &str, utf8: bool) -> Vec<u8> {
    if utf8 {
        return name.as_bytes().to_vec();
    }
    let mut out = Vec::new();
    for c in name.chars() {
        match u8::try_from(u32::from(c)) {
            Ok(b) if !(0x80..0xA0).contains(&b) => out.push(b),
            _ => out.extend(format!("\\U+{:04X}", u32::from(c)).bytes()),
        }
    }
    out
}

fn contains(bytes: &[u8], part: &[u8]) -> bool {
    bytes.windows(part.len()).any(|w| w == part)
}

fn texts(d: &Drawing) -> Vec<String> {
    let mut out: Vec<String> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            EntityKind::MText(t) => Some(t.contents.clone()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// Saved as DWG of each version, from a drawing that came from a 2018 file (the bridge's DXF is
/// declared 2018) and from one that didn't (read as R2000 in ANSI_1252), the DWG holds the
/// accented names and text as characters, and the drawing reopens with them.
#[test]
fn accented_names_and_text_survive_a_dwg_save() {
    for kept in [Some("AC1032"), None] {
        for version in ["AC1018", "AC1032"] {
            let mut d = drawing();
            std::sync::Arc::make_mut(&mut d.foreign).version = kept.map(str::to_string);
            let v = cadcraft_io::Version::for_acadver(version).unwrap();
            let opts = cadcraft_io::SaveOptions { version: Some(v), binary: false };
            let dwg = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
            let what = format!("kept {kept:?}, {version}");
            let text = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap();
            for name in ["Tomada Média", "Elétrica", "Planta Elétrica", "Ação Ω 漢", "Portão de acesso"] {
                assert!(contains(&text, &as_written(name, version == "AC1032")), "{what}: `{name}` is in the DWG");
            }
            for escaped in ["M\\U+00E9dia", "El\\U+00E9trica", "A\\U+00E7", "A\\U+00C7", "Port\\U+00E3o"] {
                assert!(!contains(&text, escaped.as_bytes()), "{what}: no `{escaped}` in the DWG");
            }
            let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
            assert!(back.block("Tomada Média").is_some(), "{what}");
            assert!(back.layers.iter().any(|l| l.name == "Elétrica"), "{what}");
            assert!(back.layouts.iter().any(|l| l.name == "Planta Elétrica"), "{what}");
            assert_eq!(texts(&back), texts(&d), "{what}");
        }
    }
}

/// An ASCII DXF of `version` whose named object dictionary holds an XRECORD under the key
/// `Média`, with the string `Valor médio`: objects CADCraft keeps as it read them, written back
/// raw through the bridge. `latin1` writes the text in ANSI_1252, otherwise in UTF-8.
fn with_xrecord(version: &str, latin1: bool) -> Vec<u8> {
    let text = format!(
        "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\n{version}\n  9\n$DWGCODEPAGE\n  3\nANSI_1252\n  9\n$HANDSEED\n  5\n100\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n  0\nENDSEC\n\
  0\nSECTION\n  2\nOBJECTS\n  0\nDICTIONARY\n  5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n  3\nMédia\n350\nE0\n\
  0\nXRECORD\n  5\nE0\n102\n{{ACAD_REACTORS\n330\nC\n102\n}}\n330\nC\n100\nAcDbXrecord\n280\n1\n  1\nValor médio\n  0\nENDSEC\n  0\nEOF\n"
    );
    if latin1 { text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect() } else { text.into_bytes() }
}

/// The kept XRECORD and its dictionary key keep their accents through the bridge, from a 2000 file
/// (ANSI_1252) saved as 2018 and from a 2018 file (UTF-8) saved as 2004.
#[test]
fn kept_objects_keep_their_accents_through_the_bridge() {
    for (source, latin1, version) in [("AC1015", true, "AC1032"), ("AC1032", false, "AC1018")] {
        let d = cadcraft_io::read(&with_xrecord(source, latin1), "x.dxf").unwrap();
        let v = cadcraft_io::Version::for_acadver(version).unwrap();
        let dwg = cadcraft_io::write_with(&d, "x.dwg", &cadcraft_io::SaveOptions { version: Some(v), binary: false }).unwrap();
        let text = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap();
        let what = format!("{source} saved as {version}");
        for part in ["Média", "Valor médio"] {
            assert!(contains(&text, &as_written(part, version == "AC1032")), "{what}: `{part}` is in the DWG: {}", String::from_utf8_lossy(&text));
        }
        assert!(!contains(&text, b"\\U+00E9"), "{what}: no escape in the DWG");
    }
}
