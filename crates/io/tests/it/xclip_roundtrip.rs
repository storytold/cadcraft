//! XCLIP boundaries (gap F2) through DXF and DWG: SPATIAL_FILTER objects in the references'
//! ACAD_FILTER extension dictionaries.

use std::sync::Arc;

use cadcraft_doc::{Block, ClipBoundary, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_dxf::Version;
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{SaveOptions, read, write_with};

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    let mut block = Block::new("Plan");
    block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::new(-10.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) })));
    block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::new(0.0, -10.0, 0.0), b: Vec3::new(0.0, 10.0, 0.0) })));
    d.blocks.insert(block.name.clone(), Arc::new(block));
    let clips = [
        // A rectangle on a moved, scaled, rotated reference.
        (Vec3::new(10.0, 20.0, 0.0), 2.0, 0.3, ClipBoundary::new(vec![Vec2::new(-2.0, -1.0), Vec2::new(2.0, 1.0)])),
        // A triangle, inverted.
        (
            Vec3::new(100.0, 0.0, 0.0),
            1.0,
            0.0,
            ClipBoundary { inverted: true, ..ClipBoundary::new(vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(0.0, 5.0)]) },
        ),
        // Turned off.
        (
            Vec3::new(200.0, 0.0, 0.0),
            0.5,
            1.0,
            ClipBoundary { enabled: false, ..ClipBoundary::new(vec![Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)]) },
        ),
    ];
    for (at, scale, rotation, clip) in clips {
        let ins = Insert { block: "Plan".into(), insert: at, scale: Vec3::new(scale, scale, 1.0), rotation, clip: Some(clip), ..Insert::default() };
        d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    }
    // An unclipped reference stays unclipped.
    d.add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "Plan".into(), ..Insert::default() })).unwrap();
    d
}

fn clips(d: &Drawing) -> Vec<Option<ClipBoundary>> {
    d.model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.clip.clone()),
            _ => None,
        })
        .collect()
}

fn same(a: &[Option<ClipBoundary>], b: &[Option<ClipBoundary>]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        match (x, y) {
            (None, None) => {}
            (Some(x), Some(y)) => {
                assert_eq!((x.enabled, x.inverted, x.points.len()), (y.enabled, y.inverted, y.points.len()));
                for (p, q) in x.points.iter().zip(&y.points) {
                    assert!(p.near(*q, 1e-9), "{p:?} {q:?}");
                }
            }
            _ => panic!("{x:?} != {y:?}"),
        }
    }
}

fn save(d: &Drawing, name: &str, version: Version) -> Vec<u8> {
    write_with(d, name, &SaveOptions { version: Some(version), ..Default::default() }).unwrap()
}

#[test]
fn clips_round_trip_through_dxf() {
    let d = drawing();
    let bytes = save(&d, "a.dxf", Version::R2018);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert_eq!(text.matches("SPATIAL_FILTER").count(), 3, "one filter per clipped reference");
    assert!(text.contains("ACAD_FILTER") && text.contains("CADCRAFT_XCLIP"));
    let first = read(&bytes, "a.dxf").unwrap();
    same(&clips(&first), &clips(&d));
    // The model owns the filters now: nothing of them is left as foreign data.
    assert!(!first.foreign.objects.iter().any(|o| o.kind == "SPATIAL_FILTER"));
    for v in [Version::R2000, Version::R2004, Version::R2010, Version::R2018] {
        let back = read(&save(&first, "a.dxf", v), "a.dxf").unwrap();
        same(&clips(&back), &clips(&d));
        assert_eq!(back.model.len(), first.model.len(), "{}", v.year());
        assert!(!back.foreign.objects.iter().any(|o| o.kind == "SPATIAL_FILTER"), "{}", v.year());
        let again = String::from_utf8(save(&back, "a.dxf", v)).unwrap();
        assert_eq!(again.matches("SPATIAL_FILTER").count(), 3, "no duplicate filters on resave");
    }
}

#[test]
fn clips_round_trip_through_dwg() {
    let d = drawing();
    let first = read(&save(&d, "a.dxf", Version::R2018), "a.dxf").unwrap();
    for v in [Version::R2004, Version::R2010, Version::R2018] {
        let back = read(&save(&first, "a.dwg", v), "a.dwg").unwrap();
        same(&clips(&back), &clips(&d));
        assert_eq!(back.model.len(), first.model.len(), "{}", v.year());
        let blk = |d: &Drawing| d.block("Plan").map(|b| b.entities.len());
        assert_eq!(blk(&back), blk(&first), "{}", v.year());
    }
}

#[test]
fn deleting_a_clip_read_from_a_file_removes_its_filter() {
    let mut d = read(&save(&drawing(), "a.dxf", Version::R2018), "a.dxf").unwrap();
    let hs: Vec<Handle> = d.model.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.clip.is_some())).map(|e| e.handle).collect();
    for h in &hs {
        d.modify_entity(*h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.clip = None;
            }
        })
        .unwrap();
    }
    let text = String::from_utf8(save(&d, "a.dxf", Version::R2018)).unwrap();
    assert!(!text.contains("SPATIAL_FILTER") && !text.contains("CADCRAFT_XCLIP"));
    assert!(clips(&read(text.as_bytes(), "a.dxf").unwrap()).iter().all(Option::is_none));
}

#[test]
fn filter_matrices_are_the_inverse_reference_transform() {
    // A reference at (150, 0), unrotated: the inverse moves by -150; vertices in drawing units.
    let mut d = Drawing::new_metric();
    let mut block = Block::new("P");
    block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 1.0, 0.0) })));
    d.blocks.insert("P".into(), Arc::new(block));
    let clip = ClipBoundary::new(vec![Vec2::new(20.0, 20.0), Vec2::new(80.0, 20.0), Vec2::new(80.0, 80.0), Vec2::new(20.0, 80.0)]);
    let h = d
        .add(
            &Space::Model,
            Common::default(),
            EntityKind::Insert(Insert { block: "P".into(), insert: Vec3::new(150.0, 0.0, 0.0), clip: Some(clip), ..Insert::default() }),
        )
        .unwrap();
    // Read back without CADCraft taking the filter: look at the DXF groups.
    let text = String::from_utf8(save(&d, "a.dxf", Version::R2018)).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines.windows(2).position(|w| w[0] == "0" && w[1] == "SPATIAL_FILTER").unwrap();
    let end = (at + 2..lines.len()).step_by(2).find(|i| lines[*i] == "0").unwrap();
    let rec = &lines[at..end];
    let values = |code: &str| -> Vec<f64> { rec.chunks(2).filter(|w| w[0] == code).map(|w| w[1].parse().unwrap()).collect() };
    assert_eq!(values("70"), vec![4.0]);
    assert_eq!(values("10"), vec![170.0, 230.0, 230.0, 170.0]);
    assert_eq!(
        values("40"),
        vec![1.0, 0.0, 0.0, -150.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    );
    let _ = h;
}

#[test]
fn hostile_spatial_filters_do_not_panic() {
    let text = String::from_utf8(save(&drawing(), "a.dxf", Version::R2018)).unwrap();
    // Vertex counts, coordinates and matrices replaced by nonsense.
    for (from, to) in [(" 70\n3\n", " 70\n999999999\n"), ("\n 40\n1.0\n", "\n 40\nnan\n"), ("\n 10\n", "\n 10\n1e999\n 10\n")] {
        let bad = text.replacen(from, to, 1);
        let _ = read(bad.as_bytes(), "a.dxf");
    }
}
