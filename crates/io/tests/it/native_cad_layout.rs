//! Synthetic native DXF records: attributes use group 74, and a top-view centre is relative
//! to the WCS target. No reference application files or assets are fixtures here.

use cadcraft_doc::{EntityKind, HAlign, Space, VAlign};
use cadcraft_geom::Vec2;
use cadcraft_io::{read_dxf, write_dxf};

fn dxf(body: &str) -> String {
    format!("0\nSECTION\n2\nENTITIES\n{body}0\nENDSEC\n0\nEOF\n")
}

#[test]
fn attributes_ignore_field_length_and_use_their_vertical_justification() {
    let source = dxf(concat!(
        "0\nINSERT\n5\n10\n2\nLABEL\n66\n1\n10\n0\n20\n0\n",
        "0\nATTRIB\n5\n11\n2\nTITLE\n1\nTitle\n40\n5\n10\n90\n20\n20\n11\n100\n21\n22.5\n72\n1\n73\n0\n74\n2\n",
        "0\nATTRIB\n5\n12\n2\nBASE\n1\nBaseline\n40\n2\n10\n10\n20\n10\n73\n3\n",
        "0\nSEQEND\n",
        "0\nATTDEF\n5\n13\n2\nDEF\n1\nDefinition\n40\n5\n10\n90\n20\n20\n11\n100\n21\n22.5\n72\n1\n73\n3\n74\n2\n",
        "0\nTEXT\n5\n14\n1\nText\n40\n5\n10\n90\n20\n20\n11\n100\n21\n22.5\n72\n1\n73\n2\n74\n3\n",
    ));
    let doc = read_dxf(source.as_bytes()).unwrap();
    let entities: Vec<_> = doc.model.iter().collect();
    let EntityKind::Insert(ins) = &entities[0].kind else { panic!("insert") };
    let title = &ins.attribs[0].text;
    assert_eq!((title.halign, title.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(ins.attribs[1].text.valign, VAlign::Baseline);
    let EntityKind::AttDef(def) = &entities[1].kind else { panic!("definition") };
    assert_eq!(def.text.valign, VAlign::Middle);
    let EntityKind::Text(text) = &entities[2].kind else { panic!("text") };
    assert_eq!(text.valign, VAlign::Middle);
    let (_, bounds) = cadcraft_render::place_text_entity(&doc, title, &title.value);
    // Middle justification centres the cap height, leaving room for descenders below it.
    assert!((bounds.max.y - 25.0).abs() < 1e-6);
    for reopened in
        [read_dxf(write_dxf(&doc).as_bytes()).unwrap(), cadcraft_io::read(&cadcraft_io::write(&doc, "test.dwg").unwrap(), "test.dwg").unwrap()]
    {
        let EntityKind::Insert(ins) = &reopened.model.iter().next().unwrap().kind else { panic!("insert") };
        assert_eq!(ins.attribs[0].text.valign, VAlign::Middle);
        assert_eq!(ins.attribs[1].text.valign, VAlign::Baseline);
    }
}

#[test]
fn top_view_target_moves_the_model_window_and_survives_saving() {
    let source = dxf(concat!(
        "0\nLINE\n5\n10\n10\n0\n20\n0\n11\n10\n21\n10\n",
        "0\nVIEWPORT\n5\n20\n67\n1\n410\nLayout1\n10\n105\n20\n148.5\n40\n210\n41\n297\n",
        "12\n5\n22\n10\n17\n210\n27\n148.5\n37\n0\n16\n0\n26\n0\n36\n873\n45\n297\n69\n2\n",
    ));
    let doc = read_dxf(source.as_bytes()).unwrap();
    let space = Space::Paper("Layout1".into());
    for doc in [doc.clone(), read_dxf(write_dxf(&doc).as_bytes()).unwrap()] {
        let EntityKind::Viewport(vp) = &doc.space(&space).unwrap().iter().next().unwrap().kind else { panic!("viewport") };
        assert_eq!(vp.view_center, Vec2::new(215.0, 158.5));
        let display = cadcraft_render::build(&doc, &space, &cadcraft_render::Options::default());
        // Only the viewport border: the model line around the origin is outside its window.
        assert!(display.prims.iter().all(|p| p.handle == cadcraft_doc::Handle(0x20)));
    }
}

#[test]
fn top_view_direction_length_and_rounding_noise_do_not_change_the_model_center() {
    for z in [0.000001, 1.0, 873.0, 63_379.0] {
        let source = dxf(&format!(
            "0\nVIEWPORT\n5\n20\n67\n1\n410\nLayout1\n10\n100\n20\n100\n40\n200\n41\n150\n12\n-4000\n22\n3000\n17\n4300\n27\n4000\n37\n-4\n16\n{}\n26\n{}\n36\n{z}\n45\n6000\n69\n2\n",
            2e-16 * z,
            -1e-16 * z
        ));
        let doc = read_dxf(source.as_bytes()).unwrap();
        let space = Space::Paper("Layout1".into());
        for reopened in [
            doc.clone(),
            read_dxf(write_dxf(&doc).as_bytes()).unwrap(),
            cadcraft_io::read(&cadcraft_io::write(&doc, "view.dwg").unwrap(), "view.dwg").unwrap(),
        ] {
            let vp = reopened
                .space(&space)
                .unwrap()
                .iter()
                .find_map(|e| match &e.kind {
                    EntityKind::Viewport(v) if v.id == 2 => Some(v),
                    _ => None,
                })
                .unwrap();
            assert_eq!(vp.view_center, Vec2::new(300.0, 7000.0), "direction length {z}");
        }
    }
}

#[test]
fn overall_viewport_freezes_paper_entities_only_in_its_layout() {
    use cadcraft_doc::{Common, Drawing, Entity, Handle, Layer, Line, Viewport};
    use cadcraft_geom::Vec3;
    let mut doc = Drawing::new_metric();
    doc.layers.push(Layer { name: "Notes".into(), ..Layer::default() });
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 10.0, 0.0) });
    doc.add(&Space::Model, Common { layer: "Notes".into(), ..Common::default() }, line.clone()).unwrap();
    for layout in &mut doc.layouts {
        layout.entities.push(Entity::new(
            Handle(0x1000 + layout.tab_order as u64),
            EntityKind::Viewport(Viewport { id: 1, frozen_layers: vec!["Notes".into()], ..Viewport::default() }),
        ));
        layout.entities.push(Entity {
            common: Common { layer: "Notes".into(), ..Common::default() },
            ..Entity::new(Handle(0x2000 + layout.tab_order as u64), line.clone())
        });
    }
    let dxf = write_dxf(&doc);
    assert!(dxf.contains("\n331\n") || dxf.contains("\r\n331\r\n"));
    // Native 331 alone, without CADCraft's redundant freeze-name xdata, is enough.
    let tags = cadcraft_dxf::parse(dxf.as_bytes()).unwrap();
    let mut native = Vec::new();
    let mut skip = false;
    for tag in tags {
        if tag.code == 0 {
            skip = false;
        }
        if tag.code == 1001 && tag.str() == "CADCRAFT" {
            skip = true;
        }
        if !skip && tag.code != 341 {
            native.push(tag);
        }
    }
    let native = cadcraft_dxf::write_ascii(&native);
    let from_native = read_dxf(native.as_bytes()).unwrap();
    for reopened in [from_native, cadcraft_io::read(&cadcraft_io::write(&doc, "freeze.dwg").unwrap(), "freeze.dwg").unwrap()] {
        for layout in &reopened.layouts {
            let overall = layout
                .entities
                .iter()
                .find_map(|e| match &e.kind {
                    EntityKind::Viewport(v) if v.id == 1 => Some(v),
                    _ => None,
                })
                .unwrap();
            assert_eq!(overall.frozen_layers, vec!["Notes"]);
            assert!(cadcraft_render::build(&reopened, &Space::Paper(layout.name.clone()), &cadcraft_render::Options::default()).prims.is_empty());
        }
        assert!(!cadcraft_render::build(&reopened, &Space::Model, &cadcraft_render::Options::default()).prims.is_empty());
    }
}
