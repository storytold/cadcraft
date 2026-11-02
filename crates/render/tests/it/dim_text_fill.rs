//! DIMTFILL and DIMTFILLCLR: a dimension's text background hides what is drawn under the text,
//! in the drawing's background color (1) or in DIMTFILLCLR (2).

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Block, Circle, Common, DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, Handle, Line, MText, Space};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{DisplayList, Kind, Options, build, dimension_geometry};

/// A linear dimension from (0, 0) to (10, 0) with its line at y = 5, in style "Fill".
fn dimension() -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Fill".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

/// DIMGAP 0.5 leaves room above the text: a line at y = 5.4 passes under the background but
/// clear of the text.
fn style(fill: u8, color: Color) -> DimStyle {
    DimStyle { name: "Fill".into(), text_gap: 0.5, text_fill: fill, text_fill_color: color, ..DimStyle::default() }
}

/// A line at y = 5.4 drawn first, then the dimension.
fn drawing(fill: u8, color: Color) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(style(fill, color));
    let line = EntityKind::Line(Line { a: Vec3::new(-2.0, 5.4, 0.0), b: Vec3::new(12.0, 5.4, 0.0) });
    d.add(&Space::Model, Common::default(), line).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dimension())).unwrap();
    d
}

/// Dark pixels on the row of the line at y = 5.4, near the text (x 4.6 to 5.4) and far from it
/// (x -1.5 to -0.5), on white paper.
fn dark_on_line(list: &DisplayList) -> (usize, usize) {
    let view = View { center: Vec2::new(5.0, 5.0), scale: 20.0, width: 400, height: 200 };
    let o = RasterOptions { background: Rgb(255, 255, 255), antialias: false, ..Default::default() };
    let pm = render(list, &view, &o).unwrap();
    let (mut near, mut far) = (0, 0);
    // World y = 5.4 is screen row 100 - 0.4 * 20 = 92; world x = 5 + (px - 200) / 20.
    for y in 91..=93u32 {
        for x in 0..400u32 {
            let wx = 5.0 + (f64::from(x) + 0.5 - 200.0) / 20.0;
            if pm.pixel(x, y).unwrap().red() > 128 {
                continue;
            }
            if (4.6..5.4).contains(&wx) {
                near += 1;
            } else if (-1.5..-0.5).contains(&wx) {
                far += 1;
            }
        }
    }
    (near, far)
}

#[test]
fn text_fill_gives_the_text_a_background_box() {
    let d = dimension();
    assert!(dimension_geometry(&d, &style(0, Color::ByBlock), 1.0).text_fill.is_empty(), "DIMTFILL 0: no background");
    let g = dimension_geometry(&d, &style(1, Color::ByBlock), 1.0);
    let [a, b, c, e] = g.text_fill.as_slice() else { panic!("four corners: {:?}", g.text_fill) };
    // The box is centered on the text and reaches DIMGAP (0.5) past the text height (0.18).
    let mid = (*a + *b + *c + *e) * 0.25;
    assert!(mid.dist(g.text_pos) < 1e-9, "{mid:?} vs {:?}", g.text_pos);
    assert!(((c.y - a.y) - (0.18 + 1.0)).abs() < 0.05, "height {}", c.y - a.y);
    assert!(b.x - a.x > 1.0, "width {}", b.x - a.x);
}

#[test]
fn text_fill_hides_what_is_drawn_under_the_text() {
    let opts = Options::default();
    let (near, far) = dark_on_line(&build(&drawing(0, Color::ByBlock), &Space::Model, &opts));
    assert!(near > 0 && far > 0, "DIMTFILL 0: the line shows under the text ({near}, {far})");
    let list = build(&drawing(1, Color::ByBlock), &Space::Model, &opts);
    assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 1);
    let (near, far) = dark_on_line(&list);
    assert_eq!(near, 0, "DIMTFILL 1: the background hides the line under the text");
    assert!(far > 0, "the line shows away from the text");
}

#[test]
fn inspection_frame_is_drawn_over_the_background() {
    // Polylines drawn after the background, for the dimension with and without an inspection
    // frame (the text strokes come after it either way).
    let after_mask = |dm: Dimension| {
        let mut d = Drawing::new_imperial();
        d.dim_styles.push(style(1, Color::ByBlock));
        d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
        let list = build(&d, &Space::Model, &Options::default());
        let mask = list.prims.iter().position(|p| p.kind == Kind::Mask).unwrap();
        list.prims[mask..].iter().filter(|p| p.kind == Kind::Polyline).count()
    };
    let mut dm = dimension();
    // No label or rate: the same text, and a frame of top, bottom and two ends.
    dm.marks.inspect = Some(cadcraft_doc::Inspection { shape: 1, label: String::new(), rate: String::new() });
    let frame = dimension_geometry(&dm, &style(1, Color::ByBlock), 1.0).text_marks.len();
    assert_eq!(frame, 4);
    let (plain, inspected) = (after_mask(dimension()), after_mask(dm));
    assert_eq!(inspected, plain + frame, "the frame lines come after the background");
    // An arc length dimension's symbol over the text (DIMARCSYM 1) is a mark of the text too.
    let arc = Dimension {
        kind: DimKind::ArcLength,
        defpt: Vec3::new(0.0, 6.0, 0.0),
        p13: Vec3::new(5.0, 0.0, 0.0),
        p14: Vec3::new(-5.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        ..dimension()
    };
    let st = DimStyle { arc_symbol: 1, ..style(1, Color::ByBlock) };
    let g = dimension_geometry(&arc, &st, 1.0);
    let [sym] = g.text_marks.as_slice() else { panic!("one mark: {:?}", g.text_marks) };
    assert!(g.lines[*sym].len() == 13, "the 13-point arc over the text");
}

#[test]
fn a_dimension_read_with_its_block_keeps_the_block_and_gets_the_background() {
    // The block (as read from a file) holds a circle the dimension's own geometry does not have,
    // then the text, placed away from where CADCraft would put it.
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(style(1, Color::ByBlock));
    let mut blk = Block::new("*D1");
    let circle = EntityKind::Circle(Circle { center: Vec3::new(5.0, 5.0, 0.0), radius: 3.0 });
    blk.entities.push(Entity::new(Handle(1), circle));
    let text = MText {
        insert: Vec3::new(20.0, 20.0, 0.0),
        height: 0.18,
        width: 0.0,
        attach: 5,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "10.0000".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    blk.entities.push(Entity::new(Handle(2), EntityKind::MText(text)));
    d.blocks.insert("*D1".into(), std::sync::Arc::new(blk));
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { block: Some("*D1".into()), ..dimension() })).unwrap();
    let list = build(&d, &Space::Model, &Options::default());
    let mask = list.prims.iter().position(|p| p.kind == Kind::Mask).expect("a background");
    let circle = list.prims.iter().position(|p| list.points(p).iter().any(|q| q.dist(Vec2::new(8.0, 5.0)) < 0.05)).expect("the block's circle");
    assert!(circle < mask, "the circle before the background, the text after it");
    // The background is around the block's text.
    let pts = list.points(&list.prims[mask]);
    let mid = pts.iter().fold(Vec2::ZERO, |s, p| s + *p) * (1.0 / pts.len() as f64);
    assert!(mid.dist(Vec2::new(20.0, 20.0)) < 0.05, "{mid:?}");
}

#[test]
fn every_text_of_a_block_gets_a_background() {
    // A measurement and a tolerance as two MTEXTs of the block.
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(style(1, Color::ByBlock));
    let mut blk = Block::new("*D1");
    for (i, (y, s)) in [(20.0, "10.0000"), (22.0, "+0.1")].into_iter().enumerate() {
        let text = MText {
            insert: Vec3::new(20.0, y, 0.0),
            height: 0.18,
            width: 0.0,
            attach: 5,
            rotation: 0.0,
            style: "Standard".into(),
            contents: s.into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        blk.entities.push(Entity::new(Handle(i as u64 + 1), EntityKind::MText(text)));
    }
    d.blocks.insert("*D1".into(), std::sync::Arc::new(blk));
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { block: Some("*D1".into()), ..dimension() })).unwrap();
    let list = build(&d, &Space::Model, &Options::default());
    assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 2);
}

#[test]
fn a_block_that_holds_its_background_gets_no_second_one() {
    // A block with a SOLID, then its text at (20, 20), away from where CADCraft would put it: the
    // number of masks drawn.
    let masks = |solid: [(f64, f64); 4]| {
        let mut d = Drawing::new_imperial();
        d.dim_styles.push(style(1, Color::ByBlock));
        let mut blk = Block::new("*D1");
        let solid = cadcraft_doc::Solid { corners: solid.map(|(x, y)| Vec3::new(x, y, 0.0)) };
        blk.entities.push(Entity::new(Handle(1), EntityKind::Solid(solid)));
        let text = MText {
            insert: Vec3::new(20.0, 20.0, 0.0),
            height: 0.18,
            width: 0.0,
            attach: 5,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "10.0000".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        blk.entities.push(Entity::new(Handle(2), EntityKind::MText(text)));
        d.blocks.insert("*D1".into(), std::sync::Arc::new(blk));
        d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { block: Some("*D1".into()), ..dimension() })).unwrap();
        let list = build(&d, &Space::Model, &Options::default());
        list.prims.iter().filter(|p| p.kind == Kind::Mask).count()
    };
    assert_eq!(masks([(19.0, 19.5), (21.0, 19.5), (19.0, 20.5), (21.0, 20.5)]), 0, "the SOLID covers the text");
    // A small SOLID (an arrowhead) over the middle of the text only: the background is drawn.
    assert_eq!(masks([(19.9, 19.95), (20.1, 20.0), (19.9, 20.05), (19.9, 20.05)]), 1, "the SOLID covers part of the text");
}

#[test]
fn a_block_with_turned_text_gets_a_turned_background() {
    // A block whose text is a TEXT turned 90 degrees, at (20, 20): the background is taller than
    // wide and holds every point of the text.
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(style(1, Color::ByBlock));
    let mut blk = Block::new("*D1");
    let text = cadcraft_doc::Text {
        insert: Vec3::new(20.0, 20.0, 0.0),
        align_pt: None,
        height: 0.5,
        value: "10.00".into(),
        rotation: std::f64::consts::FRAC_PI_2,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    blk.entities.push(Entity::new(Handle(1), EntityKind::Text(text)));
    d.blocks.insert("*D1".into(), std::sync::Arc::new(blk));
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { block: Some("*D1".into()), ..dimension() })).unwrap();
    let list = build(&d, &Space::Model, &Options::default());
    let mask = list.prims.iter().position(|p| p.kind == Kind::Mask).expect("a background");
    let quad = list.points(&list.prims[mask]);
    let (lo, hi) = quad.iter().fold((quad[0], quad[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    assert!(hi.y - lo.y > 1.5 * (hi.x - lo.x), "turned: {lo:?} {hi:?}");
    let text: Vec<Vec2> = list.prims[mask + 1..].iter().flat_map(|p| list.points(p).to_vec()).collect();
    assert!(!text.is_empty() && text.iter().all(|p| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y), "the text inside {lo:?} {hi:?}");
}

#[test]
fn text_fill_2_paints_dimtfillclr() {
    let list = build(&drawing(2, Color::Index(1)), &Space::Model, &Options::default());
    assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Mask).count(), 0);
    let red: Vec<_> = list.prims.iter().filter(|p| p.kind == Kind::Tris && p.color == Rgb(255, 0, 0)).collect();
    assert_eq!(red.len(), 1, "one red background");
    // Over the line, next to the text: the background is drawn after the line.
    let view = View { center: Vec2::new(5.0, 5.0), scale: 20.0, width: 400, height: 200 };
    let o = RasterOptions { background: Rgb(255, 255, 255), antialias: false, ..Default::default() };
    let pm = render(&list, &view, &o).unwrap();
    let c = pm.pixel(194, 92).unwrap();
    assert_eq!((c.red(), c.green(), c.blue()), (255, 0, 0));
}
