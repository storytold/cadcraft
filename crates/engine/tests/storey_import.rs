//! STOREY.IMPORT: another office's plan becomes a storey of its own, at its own coordinates and
//! with its own layers, shown as the ghost beneath the current storey, in one undo step.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Drawing, Entity, EntityKind, HVal, Handle, Layer, Line, StoreyView};
use serde_json::json;

/// A test "file": its bytes are the $INSBASE "x,y". Model space: 30 lines on 30 layers (another
/// office's layer names), the first from 100,200 to 110,200.
fn read(bytes: &[u8], _name: &str) -> Result<Drawing, String> {
    let text = String::from_utf8_lossy(bytes);
    let (x, y) = text.trim().split_once(',').ok_or("bad test file")?;
    let (x, y): (f64, f64) = (x.parse().map_err(|_| "x")?, y.parse().map_err(|_| "y")?);
    let mut d = Drawing::new_metric();
    d.header.set("INSBASE", HVal::Point(Vec3::new(x, y, 0.0)));
    for i in 0..30u32 {
        let layer = format!("A-WALL-{i:02}-their naming");
        d.layers.push(Layer::new(&layer));
        let y = 200.0 + f64::from(i);
        let mut e = Entity::new(Handle(0x100 + i as u64), EntityKind::Line(Line { a: Vec3::new(100.0, y, 0.0), b: Vec3::new(110.0, y, 0.0) }));
        e.common.layer = layer;
        d.model.push(e);
    }
    // Their DOOR: a circle, referenced once.
    let mut door = cadcraft_engine::doc::Block::new("DOOR");
    door.entities.push(Entity::new(Handle(0x50), EntityKind::Circle(cadcraft_engine::doc::Circle { center: Vec3::ZERO, radius: 9.0 })));
    d.blocks.insert("DOOR".into(), std::sync::Arc::new(door));
    let ins = cadcraft_engine::doc::Insert {
        block: "DOOR".into(),
        insert: Vec3::new(100.0, 300.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    // A multileader showing their DOOR in place of text.
    let ml = cadcraft_engine::doc::MLeader {
        leaders: vec![vec![Vec3::new(120.0, 300.0, 0.0), Vec3::new(130.0, 310.0, 0.0)]],
        landing: Vec3::new(130.0, 310.0, 0.0),
        dogleg: 2.0,
        text: None,
        style: "Standard".into(),
        arrow_size: 1.0,
        spline: false,
        block: Some(cadcraft_engine::doc::Insert { insert: Vec3::new(132.0, 310.0, 0.0), ..ins.clone() }),
    };
    d.model.push(Entity::new(Handle(0x200), EntityKind::Insert(ins)));
    d.model.push(Entity::new(Handle(0x201), EntityKind::MLeader(ml)));
    Ok(d)
}

/// The plan file, in a folder of its own per call: the tests run at once, and one writing the
/// file while another reads it would fail the import.
fn file(base: &str) -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("cadcraft-storey-import-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("Bestand Nachbar.dxf");
    std::fs::write(&p, base).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn a_plan_becomes_a_ghost_storey_at_its_own_coordinates() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut s = Session::new();
    let own = s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    let own = Handle::parse_hex(own["handles"][0].as_str().unwrap()).unwrap();
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    let undo_depth = s.state().unwrap().undo.len();
    // $INSBASE 5,5 in the file must not shift the plan.
    let r = s.execute("storey.import", &json!({ "file": file("5,5") })).unwrap();
    assert_eq!(r["storey"], "Bestand Nachbar");
    assert_eq!(r["objects"], 32);
    let d = s.doc().unwrap();
    assert_eq!(d.storeys.current_storey().unwrap().name, "EG", "still drawing on EG");
    let plan = d.storeys.by_name("Bestand Nachbar").unwrap().id;
    assert_eq!(d.ghost_storey, Some(plan));
    let imported: Vec<&Entity> = d.model.iter().filter(|e| e.common.storey() == Some(plan)).map(|e| e.as_ref()).collect();
    assert_eq!(imported.len(), 32);
    assert!(imported.iter().all(|e| d.storey_view(e) == StoreyView::Ghost));
    let EntityKind::Line(l) = &imported[0].kind else { panic!("a line") };
    assert!(l.a.xy().near(cadcraft_engine::doc::geom::Vec2::new(100.0, 200.0), 1e-9), "at the file's own coordinates: {:?}", l.a);
    assert!(d.layer("A-WALL-07-their naming").is_some(), "their layers come along");
    assert_eq!(d.entity(own).unwrap().common.storey(), d.storeys.by_name("EG").map(|x| x.id));
    // One undo step takes it all back.
    assert_eq!(s.state().unwrap().undo.len(), undo_depth + 1);
    s.undo().unwrap();
    let d = s.doc().unwrap();
    assert!(d.storeys.by_name("Bestand Nachbar").is_none());
    assert_eq!(d.model.len(), 1);
    // A file that can't be read leaves nothing behind.
    assert!(s.execute("storey.import", &json!({ "file": "/no/such/plan.dxf" })).is_err());
    assert_eq!(s.doc().unwrap().storeys.list.len(), 1);
}

#[test]
fn a_plan_named_like_a_block_of_the_drawing_leaves_the_block_alone() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut s = Session::new();
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    // The drawing's own block "Bestand Nachbar" (the file's name): one circle.
    let c = s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap();
    let c = c.get("handle").or_else(|| c["handles"].get(0)).cloned().unwrap();
    s.execute("block", &json!({ "name": "Bestand Nachbar", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    let blocks = s.doc().unwrap().blocks.len();
    s.execute("storey.import", &json!({ "file": file("0,0") })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.block("Bestand Nachbar").unwrap().entities.len(), 1, "not redefined from the file");
    assert_eq!(d.blocks.len(), blocks + 1, "their DOOR came along, no import block left behind");
}

#[test]
fn their_door_and_ours_stay_apart() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    let mut s = Session::new();
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    let c = s.execute("line", &json!({ "points": [[0, 0], [1, 0]] })).unwrap();
    let c = c.get("handle").or_else(|| c["handles"].get(0)).cloned().unwrap();
    s.execute("block", &json!({ "name": "DOOR", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    s.execute("storey.import", &json!({ "file": file("0,0") })).unwrap();
    let d = s.doc().unwrap();
    assert!(matches!(d.block("DOOR").unwrap().entities.iter().next().unwrap().kind, EntityKind::Line(_)), "ours unchanged");
    let theirs = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Insert(i) => Some(i.block.clone()),
        _ => None,
    });
    assert_eq!(theirs.as_deref(), Some("DOOR$IMPORT1"));
    let leader = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::MLeader(m) => m.block.as_ref().map(|i| i.block.clone()),
        _ => None,
    });
    assert_eq!(leader.as_deref(), Some("DOOR$IMPORT1"), "a multileader's block follows the rename too");
    assert!(matches!(d.block("DOOR$IMPORT1").unwrap().entities.iter().next().unwrap().kind, EntityKind::Circle(_)));
}
