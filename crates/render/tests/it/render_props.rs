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
        insertion_point: Vec3::ZERO,
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
        twist: 0.0,
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
        clip: None,
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

/// Whether anything is drawn between x0 and x0 + 5.
fn drawn_at(d: &Drawing, x0: f64) -> bool {
    let l = build(d, &Space::Model, &Options::default());
    l.prims.iter().any(|p| l.points(p).iter().all(|q| q.x >= x0 - 0.5 && q.x < x0 + 5.0))
}

/// A block `KKS` with a line on layer 0 (x 0..4), a line on layer OWN (x 10..14), an attribute
/// definition on TXT, referenced on layer INS with that attribute (x 20) and one without a
/// definition (x 30), which follows the reference.
fn kks_drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    layer(&mut d, "TXT", 2, Lineweight::ByLayer);
    layer(&mut d, "OWN", 3, Lineweight::ByLayer);
    layer(&mut d, "INS", 5, Lineweight::ByLayer);
    let line = |x: f64, layer: &str| Entity {
        common: on(layer),
        ..Entity::new(Handle(0), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 4.0, 0.0, 0.0) }))
    };
    let mut blk = Block::new("KKS");
    blk.entities.push(Entity { handle: Handle(0x20), ..line(0.0, "0") });
    blk.entities.push(Entity { handle: Handle(0x21), ..line(10.0, "OWN") });
    blk.entities.push(attdef(0x22, "TAG", 20.0, on("TXT")));
    d.blocks.insert(blk.name.clone(), Arc::new(blk));
    let attribs = [("TAG", 20.0), ("NODEF", 30.0)]
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
        block: "KKS".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    d.add(&Space::Model, on("INS"), EntityKind::Insert(ins)).unwrap();
    d
}

/// Change the block reference of [`kks_drawing`].
fn modify_reference(d: &mut Drawing, f: impl FnOnce(&mut Entity)) {
    let h = d.model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().handle;
    assert!(d.model.modify(h, f));
}

/// What a plot of model space draws between x0 and x0 + 5: the layers (indices) it is on.
fn plotted_at(d: &Drawing, x0: f64) -> Vec<u32> {
    let l = cadcraft_render::build_plot(d, &Space::Model, &Options::default());
    let mut v: Vec<u32> =
        (0..l.prims.len()).filter(|&i| l.points(&l.prims[i]).iter().all(|q| q.x >= x0 - 0.5 && q.x < x0 + 5.0)).map(|i| l.prim_layers[i]).collect();
    v.dedup();
    v
}

fn layer_index(d: &Drawing, name: &str) -> u32 {
    d.layers.iter().position(|l| l.name == name).map(|i| i as u32).unwrap()
}

const XS: [f64; 4] = [0.0, 10.0, 20.0, 30.0];

/// A block reference on a layer that is off hides what its block has on layer 0 (and its
/// attributes without properties of their own), as AutoCAD does, but still shows the objects and
/// attributes on layers of their own that are on. A frozen layer hides all of it.
#[test]
fn a_block_reference_on_a_layer_that_is_off_still_shows_its_own_layers() {
    let mut d = kks_drawing();
    assert_eq!(XS.map(|x| drawn_at(&d, x)), [true; 4], "layer on: all of it");

    d.layer_mut("INS").unwrap().on = false;
    assert_eq!(
        XS.map(|x| drawn_at(&d, x)),
        [false, true, true, false],
        "layer off: layer 0 and the attribute that follows the reference go, the own layers stay"
    );
    // In a plot each part is on its own PDF layer: the attribute on TXT, not on the reference's
    // layer that is off (whose PDF layer starts hidden); the hidden parts aren't plotted at all.
    assert_eq!(plotted_at(&d, 0.0), Vec::<u32>::new());
    assert_eq!(plotted_at(&d, 10.0), vec![layer_index(&d, "OWN")]);
    assert_eq!(plotted_at(&d, 20.0), vec![layer_index(&d, "TXT")]);
    assert_eq!(plotted_at(&d, 30.0), Vec::<u32>::new());

    d.layer_mut("INS").unwrap().frozen = true;
    assert_eq!(XS.map(|x| drawn_at(&d, x)), [false; 4], "frozen: none of it");
}

/// A reference on a layer that isn't plotted is drawn in full on screen; in a plot it keeps its
/// parts on plotted layers of their own, as a layer that is off does.
#[test]
fn a_block_reference_on_a_layer_that_is_not_plotted_still_plots_its_own_layers() {
    let mut d = kks_drawing();
    d.layer_mut("INS").unwrap().plot = false;
    assert_eq!(XS.map(|x| drawn_at(&d, x)), [true; 4], "on screen: all of it");
    let plotted = XS.map(|x| !plotted_at(&d, x).is_empty());
    assert_eq!(plotted, [false, true, true, false]);
}

/// An invisible reference hides all of its block, whatever the layers.
#[test]
fn an_invisible_block_reference_shows_nothing_of_its_block() {
    let mut d = kks_drawing();
    modify_reference(&mut d, |e| e.common.visible = false);
    assert_eq!(XS.map(|x| drawn_at(&d, x)), [false; 4]);
    d.layer_mut("INS").unwrap().on = false;
    assert_eq!(XS.map(|x| drawn_at(&d, x)), [false; 4]);
}

/// A layer frozen in a viewport hides a reference on it there entirely, as a frozen layer does.
#[test]
fn a_block_reference_on_a_layer_frozen_in_the_viewport_shows_nothing_there() {
    let seen = |frozen: &[&str]| {
        let mut d = kks_drawing();
        let vp = Viewport {
            center: Vec3::new(50.0, 50.0, 0.0),
            width: 80.0,
            height: 40.0,
            view_center: Vec2::new(17.0, 0.0),
            view_height: 40.0,
            id: 2,
            locked: false,
            frozen_layers: frozen.iter().map(|s| s.to_string()).collect(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
            twist: 0.0,
        };
        d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
        let l = build(&d, &Space::Paper("Layout1".into()), &Options::default());
        l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).count()
    };
    let all = seen(&[]);
    assert!(all > 0);
    assert!(seen(&["OWN"]) < all, "a part's own layer frozen there hides that part");
    assert_eq!(seen(&["INS"]), 0, "the reference's layer frozen there hides all of it");
}

/// The XCLIP frame belongs to the reference: plotted with XCLIPFRAME 1 while its layer is on,
/// not while it is off, when the parts on layers of their own are still plotted.
#[test]
fn the_clip_frame_of_a_block_reference_on_a_layer_that_is_off_is_not_plotted() {
    let mut d = kks_drawing();
    d.header.set_i64("XCLIPFRAME", 1);
    modify_reference(&mut d, |e| {
        if let EntityKind::Insert(ins) = &mut e.kind {
            ins.clip = Some(cadcraft_doc::ClipBoundary::new(vec![Vec2::new(-2.0, -3.0), Vec2::new(40.0, 3.0)]));
        }
    });
    let frame = |d: &Drawing| {
        let l = cadcraft_render::build_plot(d, &Space::Model, &Options::default());
        l.prims.iter().any(|p| {
            let b = Bounds2::from_points(l.points(p).to_vec());
            b.min.x < -1.0 && b.max.x > 39.0
        })
    };
    assert!(frame(&d), "layer on: the frame is plotted");
    d.layer_mut("INS").unwrap().on = false;
    assert!(!frame(&d), "layer off: no frame");
    assert_eq!(plotted_at(&d, 10.0), vec![layer_index(&d, "OWN")], "the part on its own layer still is");
}
