//! A lightweight polyline keeps its vertex identifiers (LWPOLYLINE group 91, DXF 2010 and later:
//! "vertex identifier (if not 0)") through a save while its vertices are where they were read.
//! Geometric constraints name a polyline's segments by these identifiers. CADCraft's polyline has
//! none, so every save wrote the polyline without them, and what the constraints name was gone.
//!
//! The DWG is written here with acadrust, the identifiers out of order and the first vertex not
//! the smallest, as on a polyline that was edited in another program.
#![cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]

use std::io::Cursor;

use acadrust::entities::{EntityType, LwPolyline, LwVertex};
use acadrust::{CadDocument, DxfVersion, Vector2};
use cadcraft_io::{SaveOptions, Version, read, write_with};

const VERTEX_IDS: [i32; 4] = [2, 5, 7, 1];

fn polyline_dwg() -> Vec<u8> {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut polyline = LwPolyline::new();
    polyline.is_closed = true;
    for (id, (x, y)) in VERTEX_IDS.iter().zip([(0.0, 0.0), (40.0, 0.0), (40.0, 25.0), (0.0, 25.0)]) {
        polyline.vertices.push(LwVertex { vertex_id: *id, ..LwVertex::new(Vector2::new(x, y)) });
    }
    doc.add_entity(EntityType::LwPolyline(polyline)).expect("add the polyline");
    acadrust::DwgWriter::write_to_vec(&doc).expect("write the DWG")
}

/// The vertex identifiers of the one lightweight polyline in DWG bytes.
fn vertex_ids_of(dwg: &[u8]) -> Vec<i32> {
    let doc = acadrust::DwgReader::from_stream(Cursor::new(dwg.to_vec())).read().unwrap();
    doc.entities()
        .find_map(|e| match e {
            EntityType::LwPolyline(p) => Some(p.vertices.iter().map(|v| v.vertex_id).collect()),
            _ => None,
        })
        .expect("the polyline is in the DWG")
}

/// The vertex identifiers (groups 91) of the one LWPOLYLINE record in a DXF text.
fn dxf_vertex_ids(dxf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(dxf);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut pairs = lines.chunks(2).filter_map(|pair| Some((*pair.first()?, *pair.get(1)?)));
    let record = pairs.by_ref().skip_while(|pair| *pair != ("0", "LWPOLYLINE")).skip(1);
    record.take_while(|(code, _)| *code != "0").filter(|(code, _)| *code == "91").map(|(_, value)| value.to_string()).collect()
}

fn save(d: &cadcraft_doc::Drawing, name: &str, version: Version) -> Vec<u8> {
    write_with(d, name, &SaveOptions { version: Some(version), binary: false }).unwrap()
}

#[test]
fn a_polyline_keeps_its_vertex_identifiers_through_a_save() {
    cadcraft_io::dwg_worker::allow_in_process();
    let dwg = polyline_dwg();
    assert_eq!(vertex_ids_of(&dwg), VERTEX_IDS, "the fixture as acadrust reads it");
    let d = read(&dwg, "polyline.dwg").unwrap();
    for version in [Version::R2010, Version::R2018] {
        assert_eq!(vertex_ids_of(&save(&d, "saved.dwg", version)), VERTEX_IDS, "DWG {version:?}");
    }
    assert_eq!(dxf_vertex_ids(&save(&d, "saved.dxf", Version::R2018)), ["2", "5", "7", "1"]);
    // A DXF before 2010 has no such group.
    assert!(dxf_vertex_ids(&save(&d, "saved.dxf", Version::R2000)).is_empty());
    let back = read(&save(&d, "saved.dxf", Version::R2018), "saved.dxf").unwrap();
    assert_eq!(dxf_vertex_ids(&save(&back, "again.dxf", Version::R2018)), ["2", "5", "7", "1"], "read back from a DXF");
}

/// Once a vertex is elsewhere, the order is another or a vertex is gone, the identifiers no longer
/// say which vertex is which: none are written.
#[test]
fn an_edited_polyline_drops_its_vertex_identifiers() {
    cadcraft_io::dwg_worker::allow_in_process();
    let mut d = read(&polyline_dwg(), "polyline.dwg").unwrap();
    let handle = d.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::LwPolyline(_))).map(|e| e.handle).unwrap();
    let mut after = |edit: fn(&mut cadcraft_doc::LwPolyline)| {
        assert!(d.model.modify(handle, |e: &mut cadcraft_doc::Entity| {
            if let cadcraft_doc::EntityKind::LwPolyline(p) = &mut e.kind {
                edit(p);
            }
        }));
        vertex_ids_of(&save(&d, "edited.dwg", Version::R2018))
    };
    assert_ne!(after(|p| p.vertices.reverse()), VERTEX_IDS, "reversed");
    assert_eq!(after(|p| p.vertices.reverse()), VERTEX_IDS, "reversed back: every vertex is where it was read");
    assert_ne!(after(|p| p.vertices[1].p.x += 1.0), VERTEX_IDS, "a vertex moved");
}
