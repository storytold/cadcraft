//! An associative array (a block reference to an anonymous array block whose items refer to a
//! source block) keeps its parameters through a DXF save and reopen; other readers just see the
//! blocks. Damaged parameter data is ignored, never a crash.

use std::sync::Arc;

use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{read_dxf, write_dxf};

fn params(source: &str) -> ArrayParams {
    ArrayParams {
        source: source.into(),
        kind: ArrayKind::Polar(PolarArray {
            center: Vec2::new(-10.0, 0.0),
            items: 5,
            fill: 270.0,
            rotate: false,
            rows: 2,
            row_spacing: 3.5,
            row_elevation: 0.25,
        }),
        levels: 2,
        level_spacing: 4.0,
        replaced: vec![ReplacedItem { i: 1, j: 0, level: 0, block: "Größe".into() }],
        editing: None,
    }
}

/// A circle as the source, an array of it, and a block "Größe" replacing one item.
fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    let mut src = Block::new("*U1");
    let h = d.new_handle();
    src.entities.push(Entity::new(h, EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert("*U1".into(), Arc::new(src));
    let mut other = Block::new("Größe");
    let h = d.new_handle();
    other.entities.push(Entity::new(h, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })));
    d.blocks.insert("Größe".into(), Arc::new(other));
    let p = params("*U1");
    let items = p.item_entities(&mut d).unwrap();
    let mut arr = Block::new("*U2");
    for e in items {
        arr.entities.push(e);
    }
    arr.array = Some(p);
    d.blocks.insert("*U2".into(), Arc::new(arr));
    let ins = Insert {
        block: "*U2".into(),
        insert: Vec3::new(20.0, 5.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.3,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    d
}

#[test]
fn array_parameters_survive_a_dxf_round_trip() {
    let d = drawing();
    let text = write_dxf(&d).replace("\r\n", "\n");
    // Kept as CADCraft xdata; the items are ordinary block references.
    assert!(text.contains("\n1001\nCADCRAFT\n1000\nARRAY\n1002\n{\n"), "array xdata missing");
    let back = read_dxf(text.as_bytes()).unwrap();
    let arr = back.block("*U2").unwrap();
    assert_eq!(arr.array.as_ref(), Some(&params("*U1")));
    assert_eq!(arr.entities.len(), 20);
    assert!(back.block("*U1").unwrap().array.is_none());
    let ins = back.model.iter().next().unwrap();
    assert_eq!(back.array_params(ins), Some(params("*U1")));
    // A second round trip keeps them too.
    let again = read_dxf(write_dxf(&back).as_bytes()).unwrap();
    assert_eq!(again.block("*U2").unwrap().array.as_ref(), Some(&params("*U1")));
}

#[test]
fn damaged_array_xdata_is_ignored() {
    let text = write_dxf(&drawing()).replace("\r\n", "\n");
    let start = text.find("\n1000\nARRAY\n1002\n{\n").unwrap() + "\n1000\nARRAY\n1002\n{\n".len();
    for bad in [
        "1000\n{\"source\":\"*U1\"\n",
        "1000\nnot json at all\n",
        "1000\n{\"source\":\"*U1\",\"kind\":{\"type\":\"rect\",\"cols\":4294967295,\"rows\":4294967295,\"colSpacing\":1,\"rowSpacing\":1}}\n",
        "1000\n{\"source\":\"*U1\",\"kind\":{\"type\":\"rect\",\"cols\":0,\"rows\":1,\"colSpacing\":1,\"rowSpacing\":1}}\n",
        "1000\n{\"source\":\"*U1\",\"kind\":{\"type\":\"rect\",\"cols\":2,\"rows\":1,\"colSpacing\":1e999,\"rowSpacing\":1}}\n",
    ] {
        let end = start + text[start..].find("1002\n}\n").unwrap();
        let hostile = format!("{}{}{}", &text[..start], bad, &text[end..]);
        let back = read_dxf(hostile.as_bytes()).unwrap();
        let arr = back.block("*U2").unwrap();
        assert!(arr.array.is_none(), "{bad}");
        // The items are still there: the array reads as a plain block reference.
        assert_eq!(arr.entities.len(), 20);
    }
}

#[test]
fn renamed_blocks_are_followed() {
    // Another program renamed the anonymous source block and the replacement block: the
    // parameters follow the names the items use.
    let text = write_dxf(&drawing()).replace("\r\n", "\n");
    let renamed: String = text
        .lines()
        .map(|l| match l {
            "*U1" => "*U9",
            "Gr\\U+00F6\\U+00DFe" => "Other",
            l => l,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let back = read_dxf(renamed.as_bytes()).unwrap();
    let p = back.block("*U2").unwrap().array.clone().unwrap();
    assert_eq!(p.source, "*U9");
    assert_eq!(p.replaced[0].block, "Other");

    // Items another program added or removed: the block is kept as a plain block.
    let mut d = drawing();
    let mut arr = (**d.block("*U2").unwrap()).clone();
    let first = arr.entities.iter().next().unwrap().handle;
    arr.entities.remove(first);
    d.blocks.insert("*U2".into(), Arc::new(arr));
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert!(back.block("*U2").unwrap().array.is_none());
    assert_eq!(back.block("*U2").unwrap().entities.len(), 19);
}
