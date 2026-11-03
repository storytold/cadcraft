//! #1057: in a DWG, entities of a layout other than the first (`*Paper_Space`) are written owned
//! by their own block (entity mode 0 and its handle). Written as "paper space" (mode 1), AutoCAD
//! put them on the first layout and left theirs empty.

use std::io::Cursor;

use cadcraft_doc::{Circle, Common, Drawing, EntityKind, Layout, Line, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

#[test]
fn entities_of_a_second_layout_are_owned_by_its_block() {
    let mut d = Drawing::new_metric();
    let order = d.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0) + 1;
    d.layouts.push(Layout::new("SHEET_A", order));
    let line = |a: f64| EntityKind::Line(Line { a: Vec3::new(a, 0.0, 0.0), b: Vec3::new(a + 10.0, 10.0, 0.0) });
    d.add(&Space::Model, Common::default(), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 5.0 })).unwrap();
    let first = d.layouts.iter().min_by_key(|l| l.tab_order).unwrap().name.clone();
    d.add(&Space::Paper(first.clone()), Common::default(), line(0.0)).unwrap();
    d.add(&Space::Paper("SHEET_A".into()), Common::default(), line(20.0)).unwrap();
    d.add(&Space::Paper("SHEET_A".into()), Common::default(), line(40.0)).unwrap();
    let dwg = write_with(&d, "case.dwg", &SaveOptions { version: Some(Version::R2018), binary: false }).unwrap();

    // As written: entity mode and owner of each entity.
    let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg.clone())).read().unwrap();
    let block_of = |h: acadrust::Handle| doc.block_records.iter().find(|b| b.handle == h).map(|b| b.name.to_ascii_uppercase());
    let sheet_a = doc
        .objects
        .iter()
        .find_map(|(_, o)| match o {
            acadrust::objects::ObjectType::Layout(l) if l.name == "SHEET_A" => Some(l.block_record),
            _ => None,
        })
        .unwrap();
    let mut on_sheet_a = 0;
    for e in doc.entities() {
        let c = e.common();
        let block = block_of(c.owner_handle).unwrap_or_default();
        if c.owner_handle == sheet_a {
            on_sheet_a += 1;
            assert_eq!(c.entity_mode, Some(0), "an entity of SHEET_A ({block}) is owned by its block");
        } else if block == "*PAPER_SPACE" {
            assert_eq!(c.entity_mode, Some(1), "the first layout's entities stay paper space");
        }
    }
    assert_eq!(on_sheet_a, 2);

    // Read back, each layout has its own.
    let back = read(&dwg, "case.dwg").unwrap();
    let count = |n: &str| back.layouts.iter().find(|l| l.name == n).map_or(0, |l| l.entities.len());
    assert_eq!((count(&first), count("SHEET_A"), back.model.len()), (1, 2, 1));
}
