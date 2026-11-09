//! DIMASSOC is a drawing variable, written as HEADER group 280, independently of DIMSTYLE.
use cadcraft_doc::Drawing;
use cadcraft_io::{SaveOptions, Version, read_dxf, write, write_with};

fn dimassoc_groups(bytes: &[u8]) -> Vec<(i32, i64)> {
    let tags = cadcraft_dxf::parse(bytes).unwrap();
    let header = cadcraft_dxf::sections(&tags).into_iter().find(|s| s.name == "HEADER").unwrap();
    let mut found = Vec::new();
    for pair in header.tags.windows(2) {
        if pair[0].code == 9 && pair[0].str() == "$DIMASSOC" {
            found.push((pair[1].code, pair[1].i64()));
        }
    }
    found
}

#[test]
fn new_drawing_modes_use_group_280_in_ascii_and_binary_dxf() {
    for mode in [0, 1, 2] {
        let mut d = Drawing::new_metric();
        d.header.set_i64("DIMASSOC", mode);
        for version in [Version::R2000, Version::R2004, Version::R2007, Version::R2018] {
            for binary in [false, true] {
                let bytes = write_with(&d, "setting.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
                assert_eq!(dimassoc_groups(&bytes), [(280, mode)], "{version:?}, binary{binary}");
                let back = read_dxf(&bytes).unwrap();
                assert_eq!(back.header.i64("DIMASSOC", -1), mode);
            }
        }
    }
}

#[test]
fn an_imported_mode_can_be_changed_without_a_stale_or_duplicate_header() {
    let source = b"0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$DIMASSOC\n280\n1\n0\nENDSEC\n0\nEOF\n";
    let mut d = read_dxf(source).unwrap();
    assert_eq!(d.header.i64("DIMASSOC", -1), 1);
    for mode in [1, 0, 2] {
        d.header.set_i64("DIMASSOC", mode);
        let bytes = write(&d, "edited.dxf").unwrap();
        assert_eq!(dimassoc_groups(&bytes), [(280, mode)]);
        d = read_dxf(&bytes).unwrap();
        assert_eq!(d.header.i64("DIMASSOC", -1), mode);
    }
}

#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
mod native {
    use super::*;
    use cadcraft_io::read;
    use std::io::Cursor;

    #[test]
    fn dwg_decode_keeps_the_mode_even_when_the_dxf_bridge_omits_it() {
        for mode in [0, 1, 2] {
            let mut source = acadrust::CadDocument::new();
            source.header.dimension_associativity = mode;
            let bytes = acadrust::DwgWriter::write_to_vec(&source).unwrap();
            let back = read(&bytes, "source.dwg").unwrap();
            assert_eq!(back.header.i64("DIMASSOC", -1), i64::from(mode));
        }
    }

    #[test]
    fn dwg_and_dwt_keep_the_mode_in_the_native_header_and_after_reopen() {
        for mode in [0, 1, 2] {
            let mut d = Drawing::new_metric();
            d.header.set_i64("DIMASSOC", mode);
            for name in ["setting.dwg", "setting.dwt"] {
                let bytes = write(&d, name).unwrap();
                let native = acadrust::DwgReader::from_stream(Cursor::new(&bytes)).read().unwrap();
                assert_eq!(i64::from(native.header.dimension_associativity), mode, "{name}: native header");
                let back = read(&bytes, name).unwrap();
                assert_eq!(back.header.i64("DIMASSOC", -1), mode, "{name}: reopened variable");
            }
        }
    }
}
