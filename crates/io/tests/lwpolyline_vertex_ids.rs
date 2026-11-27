//! A lightweight polyline keeps its vertex identifiers (LWPOLYLINE group 91 after a vertex, DXF
//! 2010 and later) through a save while its vertices are where they were read.
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
    polyline_dwg_with([(0.0, 0.0), (40.0, 0.0), (40.0, 25.0), (0.0, 25.0)], 1.0)
}

/// A DWG with one closed polyline at `points`, its normal along `normal_z` Z.
fn polyline_dwg_with(points: [(f64, f64); 4], normal_z: f64) -> Vec<u8> {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut polyline = LwPolyline::new();
    polyline.is_closed = true;
    polyline.normal = acadrust::Vector3::new(0.0, 0.0, normal_z);
    for (id, (x, y)) in VERTEX_IDS.iter().zip(points) {
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
    assert_eq!(after(|p| p.vertices.reverse()), [0; 4], "reversed");
    assert_eq!(after(|p| p.vertices.reverse()), VERTEX_IDS, "reversed back: every vertex is where it was read");
    assert_eq!(after(|p| p.vertices[1].p.x += 1.0), [0; 4], "a vertex moved");
}

/// A polyline whose normal points down is read mirrored in x; untouched, it keeps its identifiers.
#[test]
fn a_polyline_facing_down_keeps_its_vertex_identifiers() {
    cadcraft_io::dwg_worker::allow_in_process();
    let d = read(&polyline_dwg_with([(0.0, 0.0), (40.0, 0.0), (40.0, 25.0), (0.0, 25.0)], -1.0), "down.dwg").unwrap();
    assert_eq!(vertex_ids_of(&save(&d, "saved.dwg", Version::R2018)), VERTEX_IDS);
}

/// Reversed, a polyline symmetric about x = 0 has its mirror image's points: the identifiers still
/// no longer say which vertex is which.
#[test]
fn a_reversed_symmetric_polyline_drops_its_vertex_identifiers() {
    cadcraft_io::dwg_worker::allow_in_process();
    let mut d = read(&polyline_dwg_with([(-20.0, 0.0), (-20.0, 25.0), (20.0, 25.0), (20.0, 0.0)], 1.0), "sym.dwg").unwrap();
    let handle = d.model.iter().find(|e| matches!(e.kind, cadcraft_doc::EntityKind::LwPolyline(_))).map(|e| e.handle).unwrap();
    assert!(d.model.modify(handle, |e: &mut cadcraft_doc::Entity| {
        if let cadcraft_doc::EntityKind::LwPolyline(p) = &mut e.kind {
            p.vertices.reverse();
        }
    }));
    assert_eq!(vertex_ids_of(&save(&d, "saved.dwg", Version::R2018)), [0; 4]);
}

/// A group 91 inside an application's `102 {` group of the record is not a vertex identifier.
#[test]
fn a_91_inside_an_application_group_is_not_an_identifier() {
    cadcraft_io::dwg_worker::allow_in_process();
    let d = read(&polyline_dwg(), "polyline.dwg").unwrap();
    let dxf = String::from_utf8(save(&d, "saved.dxf", Version::R2018)).unwrap();
    let mut lines: Vec<&str> = dxf.lines().collect();
    let start = lines.windows(2).position(|pair| pair[0].trim() == "0" && pair[1].trim() == "LWPOLYLINE").unwrap();
    // After the first vertex's identifier.
    let at = (start..lines.len()).step_by(2).find(|i| lines[*i].trim() == "91").unwrap() + 2;
    lines.splice(at..at, ["102", "{SOMEAPP", " 91", "9", "102", "}"]);
    let back = read(lines.join("\n").as_bytes(), "app.dxf").unwrap();
    assert_eq!(dxf_vertex_ids(&save(&back, "again.dxf", Version::R2018)), ["2", "5", "7", "1"]);
}
