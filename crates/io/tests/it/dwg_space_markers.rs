//! The space blocks' markers (BLOCK and ENDBLK) keep their handles through a DWG save and open,
//! as CADCraft's own DWG reader sees them in the files.

use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Drawing, EntityKind, Handle, KeptHandle, Space};
use cadcraft_dwg::codec::read_container;

/// A drawing saved as DWG by CADCraft: a line in model space, and the default layout with the
/// paper space block's markers at 1B and 1D (a gap between them, as files AutoCAD saves have).
fn dwg() -> Vec<u8> {
    let mut d = Drawing::new_imperial();
    let l = d.layouts.first_mut().unwrap();
    (l.begin_handle, l.end_handle) = (KeptHandle(Handle(0x1B)), KeptHandle(Handle(0x1D)));
    let line = cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) };
    d.add(&Space::Model, Default::default(), EntityKind::Line(line)).unwrap();
    cadcraft_io::write(&d, "x.dwg").unwrap()
}

#[test]
fn the_paper_space_end_marker_keeps_the_files_handle() {
    let bytes = dwg();
    let m = read_container(&bytes).unwrap().space_markers();
    assert_eq!((m.paper_begin, m.paper_end), (Some(0x1B), Some(0x1D)), "the file as written");
    let (Some(begin), Some(end)) = (m.paper_begin, m.paper_end) else { panic!("no paper space markers: {m:?}") };
    let d = cadcraft_io::read(&bytes, "x.dwg").unwrap();
    let l = d.layouts.iter().find(|l| l.begin_handle.get().0 == begin).expect("the paper space layout");
    assert_eq!(l.end_handle.get().0, end, "the layout's ENDBLK");
    // Saved again, the file has the same markers.
    let again = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert_eq!(read_container(&again).unwrap().space_markers(), m);
}
