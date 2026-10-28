//! Objects CADCraft didn't change are saved as the DWG had them (`dwg_check::restore_unchanged`):
//! what the bridge (acadrust) can't carry survives open and save, and what CADCraft changed is
//! saved as CADCraft wrote it.

use cadcraft_doc::Drawing;
use cadcraft_dwg::codec::{edit, read_container, type_name};

/// The handle of the first object of type `kind`.
fn first_of(bytes: &[u8], kind: &str) -> u64 {
    let c = read_container(bytes).unwrap();
    c.objects_iter().find_map(|(r, o)| (type_name(o.ok()?.type_code, &c.classes).0 == kind).then_some(r.handle)).unwrap()
}

/// The references of the object with handle `h`.
fn refs_of(bytes: &[u8], h: u64) -> Vec<u64> {
    let c = read_container(bytes).unwrap();
    let r = c.map.iter().find(|r| r.handle == h).unwrap();
    edit::handle_refs(&c.object_at(r.offset).unwrap(), c.version).unwrap()
}

/// A DWG whose dimension style names the UCS table as its text style, as some AutoCAD files do:
/// a drawing CADCraft saved, with that one reference rewritten.
fn odd_dimstyle_dwg(version: cadcraft_dxf::Version) -> (Vec<u8>, u64, u64) {
    let d = Drawing::new_imperial();
    let opts = cadcraft_io::SaveOptions { version: Some(version), binary: false };
    let bytes = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
    let (dimstyle, style, ucs) = (first_of(&bytes, "DIMSTYLE"), first_of(&bytes, "STYLE"), first_of(&bytes, "UCS_CONTROL"));
    let c = read_container(&bytes).unwrap();
    let r = c.map.iter().find(|r| r.handle == dimstyle).unwrap();
    let o = c.object_at(r.offset).unwrap();
    let at = edit::handle_refs(&o, c.version).unwrap().iter().position(|h| *h == style).unwrap();
    let stored = edit::with_reference(&o, c.version, at, ucs).unwrap();
    let file = edit::replace_objects(&bytes, &[(dimstyle, stored)].into_iter().collect()).unwrap();
    (file, dimstyle, ucs)
}

/// The bridge reads the reference and writes the standard text style instead; the object comes
/// back as the file had it, in every version the container writer rewrites.
#[test]
fn an_unchanged_dimension_style_keeps_a_reference_the_bridge_changes() {
    for version in [cadcraft_dxf::Version::R2004, cadcraft_dxf::Version::R2010, cadcraft_dxf::Version::R2013, cadcraft_dxf::Version::R2018] {
        let (file, dimstyle, ucs) = odd_dimstyle_dwg(version);
        assert!(refs_of(&file, dimstyle).contains(&ucs), "{version:?}: the test file");
        let d = cadcraft_io::read(&file, "x.dwg").unwrap();
        let saved = cadcraft_io::write(&d, "x.dwg").unwrap();
        assert_eq!(refs_of(&saved, dimstyle), refs_of(&file, dimstyle), "{version:?}: saved");
        // And the drawing it reads back is the one that was opened.
        let back = cadcraft_io::read(&saved, "x.dwg").unwrap();
        assert_eq!(back.dim_styles, d.dim_styles, "{version:?}");
    }
}

/// A dimension style CADCraft changed is saved as CADCraft writes it, not as the file had it.
#[test]
fn a_changed_dimension_style_is_saved_as_changed() {
    let (file, dimstyle, ucs) = odd_dimstyle_dwg(cadcraft_dxf::Version::R2018);
    let mut d = cadcraft_io::read(&file, "x.dwg").unwrap();
    let st = d.dim_styles.first_mut().unwrap();
    st.text_height = 0.5;
    let saved = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert!(!refs_of(&saved, dimstyle).contains(&ucs), "the bridge's object");
    let back = cadcraft_io::read(&saved, "x.dwg").unwrap();
    assert_eq!(back.dim_styles.first().map(|s| s.text_height), Some(0.5));
}

/// A drawing that wasn't read from a DWG, or was read from one of another version, is saved as
/// the bridge writes it.
#[test]
fn a_dwg_of_another_version_is_not_copied_from() {
    let (file, dimstyle, ucs) = odd_dimstyle_dwg(cadcraft_dxf::Version::R2018);
    let d = cadcraft_io::read(&file, "x.dwg").unwrap();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_dxf::Version::R2013), binary: false };
    let saved = cadcraft_io::write_with(&d, "x.dwg", &opts).unwrap();
    assert!(!refs_of(&saved, dimstyle).contains(&ucs));
}
