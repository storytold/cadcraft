//! Raster images (IMAGE): the pixels are drawn over the image's rectangle, rotated with it and
//! cut to its clip boundary; an image without loaded pixels is a crossed-out frame.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Image, Insert, Line, Raster, Space, Transparency, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{Kind, Options, build};

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BG: Rgb = Rgb(33, 40, 48);

/// Two by two pixels: red and green on top, blue and white below.
fn pixels() -> Arc<Raster> {
    Arc::new(Raster::new(2, 2, [RED, GREEN, BLUE, WHITE].concat()).unwrap())
}

/// A 10 x 10 unit image of `map.png` with its lower-left corner at the origin.
fn image() -> Image {
    Image {
        insert: Vec3::ZERO,
        u: Vec3::new(5.0, 0.0, 0.0),
        v: Vec3::new(0.0, 5.0, 0.0),
        size: Vec2::new(2.0, 2.0),
        path: "map.png".into(),
        ..Image::default()
    }
}

fn drawing(i: Image, loaded: bool) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
    if loaded {
        d.rasters.insert("map.png", pixels());
    }
    d
}

/// The colour at the centre of each quarter of the 10 x 10 square: top left, top right, bottom
/// left, bottom right.
fn quarters(d: &Drawing) -> [[u8; 4]; 4] {
    let list = build(d, &Space::Model, &Options::default());
    let view = View { center: Vec2::new(5.0, 5.0), scale: 10.0, width: 100, height: 100 };
    let pm = render(&list, &view, &RasterOptions { antialias: false, ..Default::default() }).unwrap();
    [(25, 25), (75, 25), (25, 75), (75, 75)].map(|(x, y)| {
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    })
}

fn bg() -> [u8; 4] {
    [BG.0, BG.1, BG.2, 255]
}

/// The quarters show `want` (the filter blends a little of the neighbouring pixel in).
#[track_caller]
fn shows(got: [[u8; 4]; 4], want: [[u8; 4]; 4]) {
    let near = got.iter().flatten().zip(want.iter().flatten()).all(|(a, b)| a.abs_diff(*b) <= 6);
    assert!(near, "{got:?} is not {want:?}");
}

#[test]
fn the_pixels_are_drawn_over_the_rectangle() {
    let d = drawing(image(), true);
    shows(quarters(&d), [RED, GREEN, BLUE, WHITE]);
    let list = build(&d, &Space::Model, &Options::default());
    assert_eq!(list.images.len(), 1);
    let p = list.prims.iter().find(|p| matches!(p.kind, Kind::Image { .. })).unwrap();
    assert_eq!(list.points(p), [Vec2::new(0.0, 10.0), Vec2::new(10.0, 10.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.0)]);
    assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Polyline).count(), 1, "the frame, not crossed out");
}

#[test]
fn an_image_without_pixels_is_a_crossed_out_frame() {
    for d in [drawing(image(), false), drawing(Image { display: 6, ..image() }, true)] {
        let list = build(&d, &Space::Model, &Options::default());
        assert!(list.images.is_empty());
        assert_eq!(list.prims.iter().filter(|p| p.kind == Kind::Polyline).count(), 3, "frame and two diagonals");
    }
}

#[test]
fn the_image_turns_with_its_axes() {
    // Turned a quarter turn counter-clockwise about its lower-left corner, then moved back over
    // the square: the image's top edge is now the square's left edge.
    let i = Image { insert: Vec3::new(10.0, 0.0, 0.0), u: Vec3::new(0.0, 5.0, 0.0), v: Vec3::new(-5.0, 0.0, 0.0), ..image() };
    shows(quarters(&drawing(i, true)), [GREEN, WHITE, RED, BLUE]);
}

#[test]
fn the_clip_boundary_cuts_the_image() {
    // The left pixel column only (pixel coordinates from pixel centres, y down from the top).
    let i = Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(0.5, 1.5)], clipping: true, ..image() };
    shows(quarters(&drawing(i, true)), [RED, bg(), BLUE, bg()]);
    // A triangle over the top-left half.
    let i = Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(1.5, -0.5), Vec2::new(-0.5, 1.5)], clipping: true, ..image() };
    let q = quarters(&drawing(i, true));
    shows([q[0], q[3], q[0], q[3]], [RED, bg(), RED, bg()]);
    // The boundary is kept but switched off.
    let i = Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(0.5, 1.5)], clipping: false, ..image() };
    shows(quarters(&drawing(i, true)), [RED, GREEN, BLUE, WHITE]);
    // The boundary a file holds for an unclipped image is the whole image.
    let i = Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(1.5, 1.5)], clipping: true, ..image() };
    shows(quarters(&drawing(i, true)), [RED, GREEN, BLUE, WHITE]);
}

#[test]
fn a_file_of_another_size_fills_the_same_rectangle() {
    // The entity says 4 x 4 pixels of 2.5 units; the file found has 2 x 2.
    let i = Image { u: Vec3::new(2.5, 0.0, 0.0), v: Vec3::new(0.0, 2.5, 0.0), size: Vec2::new(4.0, 4.0), ..image() };
    shows(quarters(&drawing(i, true)), [RED, GREEN, BLUE, WHITE]);
}

#[test]
fn fade_and_degenerate_images_do_not_panic() {
    let faded = quarters(&drawing(Image { fade: 100, ..image() }, true));
    assert_eq!(faded[0], bg(), "fully faded shows the background");
    for i in [
        Image { u: Vec3::ZERO, ..image() },
        Image { u: Vec3::new(f64::NAN, 0.0, 0.0), ..image() },
        Image { size: Vec2::new(0.0, -1.0), ..image() },
        Image { insert: Vec3::new(1e308, 1e308, 0.0), u: Vec3::new(1e308, 0.0, 0.0), ..image() },
        Image { clip: vec![Vec2::new(f64::INFINITY, 0.0), Vec2::new(1.0, f64::NAN)], clipping: true, ..image() },
    ] {
        let _ = quarters(&drawing(i, true));
    }
}

#[test]
fn a_one_pixel_image_at_its_own_size_does_not_panic() {
    // One pixel drawn one screen pixel wide, at whole and at half pixel positions, with and
    // without smoothing (tiny-skia asserts on narrow anti-aliased rectangles).
    for x in [0.0, 0.25, 0.5] {
        let mut d = Drawing::new_imperial();
        let i = Image {
            insert: Vec3::new(x, x, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(1.0, 1.0),
            path: "dot.png".into(),
            ..Image::default()
        };
        d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
        d.rasters.insert("dot.png", Arc::new(Raster::new(1, 1, RED.to_vec()).unwrap()));
        let list = build(&d, &Space::Model, &Options::default());
        for antialias in [true, false] {
            let view = View { center: Vec2::new(8.0, 8.0), scale: 1.0, width: 16, height: 16 };
            assert!(render(&list, &view, &RasterOptions { antialias, ..Default::default() }).is_some());
        }
    }
}

/// The colour of one pixel of the 100 x 100 picture of the 10 x 10 square.
fn pixel_at(d: &Drawing, opts: &Options, x: u32, y: u32) -> [u8; 4] {
    let list = build(d, &Space::Model, opts);
    let view = View { center: Vec2::new(5.0, 5.0), scale: 10.0, width: 100, height: 100 };
    let c = render(&list, &view, &RasterOptions { antialias: false, ..Default::default() }).unwrap().pixel(x, y).unwrap();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

#[track_caller]
fn near(got: [u8; 4], want: [u8; 4]) {
    assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 6), "{got:?} is not {want:?}");
}

#[test]
fn the_image_primitive_belongs_to_its_entity_and_widens_the_bounds() {
    let mut d = Drawing::new_imperial();
    let i = Image { insert: Vec3::new(100.0, 200.0, 0.0), ..image() };
    let h = d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
    d.rasters.insert("map.png", pixels());
    let list = build(&d, &Space::Model, &Options::default());
    let p = list.prims.iter().find(|p| matches!(p.kind, Kind::Image { .. })).unwrap();
    assert_eq!(p.handle, h, "picking and highlighting find the entity");
    assert_eq!((list.bounds.min, list.bounds.max), (Vec2::new(100.0, 200.0), Vec2::new(110.0, 210.0)));
    let img = &list.images[0];
    assert_eq!((img.origin, img.right, img.down), (Vec2::new(100.0, 210.0), Vec2::new(5.0, 0.0), Vec2::new(0.0, -5.0)));
    assert_eq!(img.at(2.0, 2.0), Vec2::new(110.0, 200.0));
}

#[test]
fn draw_order_is_kept_in_pictures() {
    // A white line across the top half (y = 7.5, picture row 25).
    let line = || EntityKind::Line(Line { a: Vec3::new(0.0, 7.5, 0.0), b: Vec3::new(10.0, 7.5, 0.0) });
    let build_with = |image_last: bool| {
        let mut d = Drawing::new_imperial();
        let order = if image_last { [line(), EntityKind::Image(image())] } else { [EntityKind::Image(image()), line()] };
        for k in order {
            d.add(&Space::Model, Common::default(), k).unwrap();
        }
        d.rasters.insert("map.png", pixels());
        d
    };
    near(pixel_at(&build_with(true), &Options::default(), 25, 25), RED);
    near(pixel_at(&build_with(false), &Options::default(), 25, 25), WHITE);
}

#[test]
fn an_image_in_a_block_follows_the_block_reference() {
    let mut d = Drawing::new_imperial();
    let mut blk = Block::new("MAP");
    // A 2 x 2 unit image in the block.
    let small = Image { u: Vec3::new(1.0, 0.0, 0.0), v: Vec3::new(0.0, 1.0, 0.0), ..image() };
    blk.entities.push(Entity::new(Handle(900), EntityKind::Image(small)));
    d.blocks.insert("MAP".into(), Arc::new(blk));
    // Inserted five times larger at the origin: the same 10 x 10 square.
    let ins = Insert {
        block: "MAP".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(5.0, 5.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let h = d.add(&Space::Model, Common::default(), EntityKind::Insert(ins.clone())).unwrap();
    d.rasters.insert("map.png", pixels());
    shows(quarters(&d), [RED, GREEN, BLUE, WHITE]);
    let list = build(&d, &Space::Model, &Options::default());
    assert_eq!(list.prims.iter().find(|p| matches!(p.kind, Kind::Image { .. })).map(|p| p.handle), Some(h), "selected with its block reference");

    // A quarter turn about the origin, moved back over the square.
    let mut d2 = d.clone();
    let turned = Insert { insert: Vec3::new(10.0, 0.0, 0.0), rotation: std::f64::consts::FRAC_PI_2, ..ins };
    d2.model = Default::default();
    d2.add(&Space::Model, Common::default(), EntityKind::Insert(turned)).unwrap();
    shows(quarters(&d2), [GREEN, WHITE, RED, BLUE]);
}

#[test]
fn a_layout_viewport_cuts_the_image_to_its_outline() {
    let mut d = Drawing::new_metric();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Image(Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(1.5, 0.5)], clipping: true, ..image() }),
    )
    .unwrap();
    d.rasters.insert("map.png", pixels());
    // 90 x 100 paper units at 10:1, looking at the middle of the image: model x 0.5..9.5.
    let vp = Viewport {
        center: Vec3::new(60.0, 60.0, 0.0),
        width: 90.0,
        height: 100.0,
        view_center: Vec2::new(5.0, 5.0),
        view_height: 10.0,
        id: 2,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
    let list = build(&d, &Space::Paper("Layout1".into()), &Options::default());
    assert_eq!(list.images.len(), 1);
    let img = &list.images[0];
    // In paper units: ten times larger, the image's top-left corner at (10, 110).
    assert_eq!((img.origin, img.right, img.down), (Vec2::new(10.0, 110.0), Vec2::new(50.0, 0.0), Vec2::new(0.0, -50.0)));
    assert_eq!(img.clips.len(), 2, "its own boundary, then the viewport");
    assert_eq!(img.clips[0], [Vec2::new(10.0, 110.0), Vec2::new(110.0, 110.0), Vec2::new(110.0, 60.0), Vec2::new(10.0, 60.0)], "the top pixel row");
    assert_eq!(img.clips[1], [Vec2::new(15.0, 10.0), Vec2::new(105.0, 10.0), Vec2::new(105.0, 110.0), Vec2::new(15.0, 110.0)]);
    let p = list.prims.iter().find(|p| matches!(p.kind, Kind::Image { index: 0 })).unwrap();
    assert_eq!(list.points(p).len(), 4);

    // Drawn: inside the viewport and the image's top row there is the picture; outside the
    // viewport, or in the clipped-away bottom row, there is none.
    let view = View { center: Vec2::new(60.0, 60.0), scale: 1.0, width: 120, height: 120 };
    let pm = render(&list, &view, &RasterOptions { antialias: false, ..Default::default() }).unwrap();
    let px = |x: u32, y: u32| {
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    };
    // (Sampled at pixel centres: an enlarged picture is smoothed between them.)
    near(px(35, 35), RED);
    near(px(85, 35), GREEN);
    near(px(12, 35), bg());
    near(px(107, 35), bg());
    near(px(35, 85), bg());
    near(px(85, 85), bg());

    // A viewport looking elsewhere shows nothing of it.
    let mut away = d.clone();
    let Some(l) = away.layouts.iter_mut().find(|l| l.name == "Layout1") else { panic!("no Layout1") };
    l.entities = Default::default();
    let far = Viewport {
        view_center: Vec2::new(500.0, 500.0),
        ..match &d.layouts.iter().find(|l| l.name == "Layout1").unwrap().entities.iter().last().unwrap().kind {
            EntityKind::Viewport(v) => v.clone(),
            _ => panic!("not a viewport"),
        }
    };
    away.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(far)).unwrap();
    assert!(build(&away, &Space::Paper("Layout1".into()), &Options::default()).images.is_empty());
}

#[test]
fn object_transparency_and_pixel_alpha_blend_with_what_is_under() {
    // Half transparent as an object.
    let mut d = Drawing::new_imperial();
    let c = Common { transparency: Transparency::Percent(50), ..Default::default() };
    d.add(&Space::Model, c, EntityKind::Image(image())).unwrap();
    d.rasters.insert("map.png", pixels());
    let half = |c: u8, b: u8| ((u16::from(c) + u16::from(b)) / 2) as u8;
    let mixed = [half(255, BG.0), half(0, BG.1), half(0, BG.2), 255];
    near(pixel_at(&d, &Options { transparency: true, ..Options::default() }, 25, 25), mixed);
    near(pixel_at(&d, &Options::default(), 25, 25), RED);

    // Half transparent pixels in the file.
    let mut d = drawing(image(), false);
    d.rasters.insert("map.png", Arc::new(Raster::new(2, 2, [[255, 0, 0, 128], GREEN, BLUE, [0, 0, 0, 0]].concat()).unwrap()));
    let q = quarters(&d);
    near(q[0], mixed);
    near(q[1], GREEN);
    near(q[3], bg());

    // Half faded.
    let d = drawing(Image { fade: 50, ..image() }, true);
    near(quarters(&d)[0], mixed);
}

#[test]
fn two_references_share_one_set_of_pixels() {
    let mut d = drawing(image(), true);
    d.add(&Space::Model, Common::default(), EntityKind::Image(Image { insert: Vec3::new(20.0, 0.0, 0.0), ..image() })).unwrap();
    let list = build(&d, &Space::Model, &Options::default());
    assert_eq!(list.images.len(), 2);
    assert!(Arc::ptr_eq(&list.images[0].raster, &list.images[1].raster));
    let kinds: Vec<Kind> = list.prims.iter().map(|p| p.kind).filter(|k| matches!(k, Kind::Image { .. })).collect();
    assert_eq!(kinds, [Kind::Image { index: 0 }, Kind::Image { index: 1 }]);
}
