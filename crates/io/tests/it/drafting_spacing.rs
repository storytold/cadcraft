//! Snap/grid spacing uses legacy HEADER variables and the current *ACTIVE VPORT (#607).

use cadcraft_doc::{Drawing, HVal};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_geom::{Vec2, Vec3};

fn header(snap: Vec2, grid: Vec2) -> Vec<Tag> {
    let mut tags = Vec::new();
    for (name, unit) in [("$SNAPUNIT", snap), ("$GRIDUNIT", grid)] {
        tags.extend([Tag::s(9, name), Tag::f(10, unit.x), Tag::f(20, unit.y)]);
    }
    tags
}

fn vport(name: &str, snap: Vec2, grid: Vec2) -> Vec<Tag> {
    vec![Tag::s(0, "VPORT"), Tag::s(2, name), Tag::i(70, 0), Tag::f(14, snap.x), Tag::f(24, snap.y), Tag::f(15, grid.x), Tag::f(25, grid.y)]
}

fn read(header: &[Tag], vports: &[Vec<Tag>]) -> Drawing {
    let mut tags = vec![Tag::s(0, "SECTION"), Tag::s(2, "HEADER"), Tag::s(9, "$ACADVER"), Tag::s(1, "AC1015")];
    tags.extend_from_slice(header);
    tags.extend([Tag::s(0, "ENDSEC"), Tag::s(0, "SECTION"), Tag::s(2, "TABLES"), Tag::s(0, "TABLE"), Tag::s(2, "VPORT")]);
    for vp in vports {
        tags.extend_from_slice(vp);
    }
    tags.extend([Tag::s(0, "ENDTAB"), Tag::s(0, "ENDSEC"), Tag::s(0, "EOF")]);
    cadcraft_io::read_dxf(cadcraft_dxf::write_ascii(&tags).as_bytes()).unwrap()
}

fn assert_units(d: &Drawing, snap: Vec2, grid: Vec2) {
    assert_eq!(d.header.point("SNAPUNIT").unwrap().xy(), snap);
    assert_eq!(d.header.point("GRIDUNIT").unwrap().xy(), grid);
}

fn number(tags: &[Tag], code: i32) -> f64 {
    tags.iter().find(|t| t.code == code).expect("spacing component").f64()
}

#[test]
fn writes_spacing_to_header_and_active_vport_and_roundtrips() {
    let (snap, grid) = (Vec2::new(0.25, 0.125), Vec2::new(1.0, 2.0));
    let mut d = Drawing::new_imperial();
    d.header.set("SNAPUNIT", HVal::Point(snap.to3(0.0)));
    d.header.set("GRIDUNIT", HVal::Point(grid.to3(0.0)));
    let text = cadcraft_io::write_dxf(&d);
    let tags = cadcraft_dxf::parse_ascii(&text).unwrap();
    let sections = sections(&tags);
    let header = &sections.iter().find(|s| s.name == "HEADER").unwrap().tags;
    for (name, unit) in [("$SNAPUNIT", snap), ("$GRIDUNIT", grid)] {
        let index = header.iter().position(|t| t.code == 9 && t.str() == name).expect("spacing header variable");
        let components = &header[index + 1..index + 3];
        assert_eq!(components.iter().map(|t| t.code).collect::<Vec<_>>(), [10, 20]);
        assert_eq!((number(components, 10), number(components, 20)), (unit.x, unit.y));
    }
    let table = &sections.iter().find(|s| s.name == "TABLES").unwrap().tags;
    let records = records(table);
    let (_, active) =
        records.iter().find(|(kind, t)| kind == "VPORT" && t.iter().any(|t| t.code == 2 && t.str().eq_ignore_ascii_case("*ACTIVE"))).unwrap();
    assert_eq!((number(active, 14), number(active, 24)), (snap.x, snap.y));
    assert_eq!((number(active, 15), number(active, 25)), (grid.x, grid.y));
    assert_units(&cadcraft_io::read_dxf(text.as_bytes()).unwrap(), snap, grid);
}

#[test]
fn reads_legacy_spacing_without_an_active_vport() {
    let (snap, grid) = (Vec2::new(0.25, 0.125), Vec2::new(0.75, 1.0));
    assert_units(&read(&header(snap, grid), &[]), snap, grid);
}

#[test]
fn first_active_vport_overrides_header_and_named_viewports() {
    let (snap, grid) = (Vec2::new(0.25, 0.125), Vec2::new(0.75, 1.5));
    let other = Vec2::new(2.0, 3.0);
    let ports = [vport("Saved", other, other), vport(" *active ", snap, grid), vport("*ACTIVE", other, other)];
    assert_units(&read(&header(other, other), &ports), snap, grid);
}

#[test]
fn incomplete_or_invalid_active_spacing_preserves_legacy_values() {
    let (snap, grid) = (Vec2::new(0.25, 0.125), Vec2::new(0.75, 1.5));
    let mut incomplete = vport("*ACTIVE", Vec2::new(2.0, 3.0), Vec2::new(-1.0, 0.5));
    incomplete.retain(|t| t.code != 24);
    assert_units(&read(&header(snap, grid), &[incomplete]), snap, grid);
    let invalid = vport("*ACTIVE", Vec2::new(0.0, 0.5), Vec2::new(0.5, -1.0));
    assert_units(&read(&header(snap, grid), &[invalid]), snap, grid);
}

#[test]
fn missing_or_invalid_legacy_spacing_uses_the_default() {
    let default = Vec2::new(0.5, 0.5);
    assert_units(&read(&[], &[]), default, default);
    assert_units(&read(&header(Vec2::ZERO, Vec2::new(-1.0, 0.5)), &[]), default, default);
    assert_units(&read(&[Tag::s(9, "$SNAPUNIT"), Tag::f(10, 0.25), Tag::s(9, "$GRIDUNIT"), Tag::f(20, 0.75)], &[]), default, default);
}

#[test]
fn zero_grid_components_roundtrip_as_follow_snap_spacing() {
    let mut d = Drawing::new_imperial();
    d.header.set("GRIDUNIT", HVal::Point(Vec3::new(0.0, 0.75, 0.0)));
    let text = cadcraft_io::write_dxf(&d);
    assert_units(&cadcraft_io::read_dxf(text.as_bytes()).unwrap(), Vec2::new(0.5, 0.5), Vec2::new(0.0, 0.75));
}

#[test]
fn invalid_in_memory_spacing_exports_safe_defaults() {
    let mut d = Drawing::new_imperial();
    d.header.set("SNAPUNIT", HVal::Point(Vec3::new(f64::NAN, f64::INFINITY, 0.0)));
    d.header.set("GRIDUNIT", HVal::Point(Vec3::new(-1.0, 0.75, 0.0)));
    let text = cadcraft_io::write_dxf(&d);
    assert_units(&cadcraft_io::read_dxf(text.as_bytes()).unwrap(), Vec2::new(0.5, 0.5), Vec2::new(0.5, 0.5));
}
