//! Dimensions take viewport layer colours, leader arrows their style's DIMSCALE, attributes
//! their own properties (issue #313).

use std::sync::Arc;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    Attrib, Block, Common, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, HAlign, Handle, Insert, Leader, Line, Lineweight, Space, Text,
    VAlign, Viewport,
};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use cadcraft_render::{Kind, Options, VIEWPORT_CONTENT, build};

fn rgb(i: u8) -> Rgb {
    Color::Index(i).resolve(Color::Index(7), Color::Index(7))
}

fn layer(d: &mut Drawing, name: &str, color: u8, lw: Lineweight) {
    d.ensure_layer(name);
    let l = d.layer_mut(name).unwrap();
    l.color = Color::Index(color);
    l.lineweight = lw;
}

fn on(layer: &str) -> Common {
    Common { layer: layer.into(), ..Default::default() }
}

#[test]
fn dimensions_take_the_viewport_layer_colour() {
    let mut d = Drawing::new_imperial();
    layer(&mut d, "A", 1, Lineweight::ByLayer);
    let dim = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 2.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    d.add(&Space::Model, on("A"), EntityKind::Dimension(dim)).unwrap();
    d.add(&Space::Model, on("A"), EntityKind::Line(Line { a: Vec3::new(0.0, -1.0, 0.0), b: Vec3::new(10.0, -1.0, 0.0) })).unwrap();
    let vp = Viewport {
        center: Vec3::new(10.0, 10.0, 0.0),
        width: 20.0,
        height: 10.0,
        view_center: Vec2::new(5.0, 1.0),
        view_height: 10.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: vec![("a".into(), Color::Index(3))],
        clip: None,
        on: true,
    };
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
    let l = build(&d, &Space::Paper("Layout1".into()), &Options::default());
    let seen: Vec<Rgb> = l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).map(|p| p.color).collect();
    assert!(seen.iter().filter(|c| **c == rgb(3)).count() > 3, "dimension lines, arrows and text: {seen:?}");
    assert!(seen.iter().all(|c| *c == rgb(3)), "everything on layer A is green in the viewport: {seen:?}");
    // Model space keeps the layer's own colour.
    let m = build(&d, &Space::Model, &Options::default());
    assert!(m.prims.iter().all(|p| p.color == rgb(1)));
}

/// Length of the leader's arrowhead along the leader (x).
fn arrow_length(d: &Drawing) -> f64 {
    let l = build(d, &Space::Model, &Options::default());
    let tris: Vec<Vec2> = l.prims.iter().filter(|p| p.kind == Kind::Tris).flat_map(|p| l.points(p).to_vec()).collect();
    Bounds2::from_points(tris).width()
}

#[test]
fn leader_arrows_use_their_style_dimscale() {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(DimStyle { name: "BIG".into(), scale: 10.0, arrow_size: 0.18, ..DimStyle::default() });
    let leader = Leader { vertices: vec![Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0)], arrow: true, spline: false, style: "BIG".into() };
    d.add(&Space::Model, Common::default(), EntityKind::Leader(leader)).unwrap();
    d.header.set_f64("DIMSCALE", 1.0);
    assert!((arrow_length(&d) - 1.8).abs() < 1e-6, "{}", arrow_length(&d));
    // A style DIMSCALE of 0 falls back to the drawing's.
    d.dim_styles.iter_mut().filter(|s| s.name == "BIG").for_each(|s| s.scale = 0.0);
    d.header.set_f64("DIMSCALE", 2.0);
    assert!((arrow_length(&d) - 0.36).abs() < 1e-6, "{}", arrow_length(&d));
    // Both 0: never a zero-size arrow.
    d.header.set_f64("DIMSCALE", 0.0);
    assert!((arrow_length(&d) - 0.18).abs() < 1e-6, "{}", arrow_length(&d));
}

fn text(x: f64, value: &str) -> Text {
    Text {
        insert: Vec3::new(x, 0.0, 0.0),
        align_pt: None,
        height: 1.0,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    }
}

fn attdef(handle: u64, tag: &str, x: f64, common: Common) -> Entity {
    let a = Attrib {
        tag: tag.into(),
        text: text(x, tag),
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    Entity { common, ..Entity::new(Handle(handle), EntityKind::AttDef(a)) }
}

/// Colour and lineweight of the attribute text drawn between x0 and x0 + 5 (none = not drawn).
fn attrib_at(d: &Drawing, x0: f64) -> Option<(Rgb, f32)> {
    let l = build(d, &Space::Model, &Options { lineweights: true, ..Options::default() });
    let mut it = l.prims.iter().filter(|p| l.points(p).iter().all(|q| q.x >= x0 - 0.5 && q.x < x0 + 5.0)).map(|p| (p.color, p.lw));
    let first = it.next()?;
    assert!(it.all(|x| x == first), "one colour and lineweight per attribute");
    Some(first)
}

#[test]
fn attributes_draw_with_their_own_properties() {
    let mut d = Drawing::new_imperial();
    layer(&mut d, "TXT", 2, Lineweight::Mm100(50));
    layer(&mut d, "INS", 5, Lineweight::Mm100(18));
    let mut blk = Block::new("TAGS");
    // ByLayer on its own layer; ByBlock; ByLayer on layer 0 (= the reference's layer); explicit.
    blk.entities.push(attdef(0x10, "BYLAYER", 0.0, on("TXT")));
    blk.entities.push(attdef(0x11, "BYBLOCK", 10.0, Common { color: Color::ByBlock, lineweight: Lineweight::ByBlock, ..on("0") }));
    blk.entities.push(attdef(0x12, "LAYER0", 20.0, on("0")));
    blk.entities.push(attdef(0x13, "OWN", 30.0, Common { color: Color::Index(6), lineweight: Lineweight::Mm100(70), ..on("TXT") }));
    d.blocks.insert(blk.name.clone(), Arc::new(blk));
    let attribs = [("bylayer", 0.0), ("ByBlock", 10.0), ("LAYER0", 20.0), ("OWN", 30.0), ("NODEF", 40.0)]
        .map(|(tag, x)| Attrib {
            tag: tag.into(),
            text: text(x, "X"),
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        })
        .to_vec();
    let ins = Insert {
        block: "TAGS".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let common = Common { color: Color::Index(1), lineweight: Lineweight::Mm100(35), ..on("INS") };
    d.add(&Space::Model, common, EntityKind::Insert(ins)).unwrap();

    assert_eq!(attrib_at(&d, 0.0), Some((rgb(2), 0.5)), "ByLayer: the attribute's layer");
    assert_eq!(attrib_at(&d, 10.0), Some((rgb(1), 0.35)), "ByBlock: the block reference's");
    assert_eq!(attrib_at(&d, 20.0), Some((rgb(5), 0.18)), "layer 0: the block reference's layer");
    assert_eq!(attrib_at(&d, 30.0), Some((rgb(6), 0.7)), "explicit");
    assert_eq!(attrib_at(&d, 40.0), Some((rgb(1), 0.35)), "no definition: the block reference's");

    // An attribute on a layer that is off is hidden; the others stay.
    d.layer_mut("TXT").unwrap().on = false;
    assert_eq!(attrib_at(&d, 0.0), None);
    assert_eq!(attrib_at(&d, 10.0), Some((rgb(1), 0.35)));
}
