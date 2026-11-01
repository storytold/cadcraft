//! A native layout with no media or paper-space viewport has never been activated.
//! Preserve that distinction from an intentionally empty, configured sheet.
use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::Vec3;

fn native_empty_layout(extra: &str, entities: &str, media: &str) -> Drawing {
    let dxf = format!(
        "0\nSECTION\n2\nHEADER\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n0\nLAYOUT\n5\nA0\n330\nA1\n\
         100\nAcDbPlotSettings\n4\n{media}\n44\n0\n45\n0\n\
         100\nAcDbLayout\n1\nPresentación1\n71\n1\n330\n1F\n{extra}\
         0\nENDSEC\n0\nEOF\n"
    );
    cadcraft_io::read_dxf(dxf.as_bytes()).unwrap()
}

#[test]
fn identifies_only_an_unvisited_unsized_empty_layout() {
    let d = native_empty_layout("", "", "");
    assert!(d.layouts[0].needs_initialization);
    // A paper-space viewport may have been deleted while the overall viewport survives.
    assert!(!native_empty_layout("331\nF0\n", "", "").layouts[0].needs_initialization);
    let paper_line = "0\nLINE\n5\nF0\n67\n1\n8\n0\n10\n0\n20\n0\n11\n10\n21\n10\n";
    assert!(!native_empty_layout("", paper_line, "").layouts[0].needs_initialization);
    // A configured media name alone also distinguishes a deliberately blank sheet.
    assert!(!native_empty_layout("", "", "ISO_A4").layouts[0].needs_initialization);
}

#[test]
fn unvisited_state_survives_saving_without_visiting_the_layout() {
    let d = native_empty_layout("", "", "");
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(read.layouts[0].needs_initialization);
    assert!(read.layouts[0].entities.is_empty());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&d, "unvisited.dwg").unwrap();
        let read = cadcraft_io::read(&dwg, "unvisited.dwg").unwrap();
        assert!(read.layouts[0].needs_initialization);
        assert!(read.layouts[0].entities.is_empty());
    }
}

#[test]
fn saving_existing_content_does_not_mark_the_layout_unvisited() {
    let mut d = Drawing::new_metric();
    d.layouts[0].needs_initialization = true;
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 10.0, 0.0) })).unwrap();
    let read = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(!read.layouts[0].needs_initialization);
    assert_eq!(read.layouts[0].entities.len(), 1);
}
