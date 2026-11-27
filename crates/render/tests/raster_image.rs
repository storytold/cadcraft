//! Raster images (IMAGE): the pixels are drawn over the image's rectangle, rotated with it and
//! cut to its clip boundary; an image without loaded pixels is a crossed-out frame.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Image, Insert, Line, Raster, Space, Transparency, Viewport};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{DisplayList, Kind, Options, build, build_plot};

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
        clip: None,
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
        twist: 0.0,
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
    // The image (10..110 across) is wider than the viewport (15..105): only what shows counts
    // towards the sheet's bounds, which a plot fits to.
    assert!(list.bounds.min.x >= 15.0 - 1e-9 && list.bounds.max.x <= 105.0 + 1e-9, "{:?}", list.bounds);

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

// ---------------- IMAGEFRAME: what the mode changes, and what it must not ----------------

/// Pixels that are not opaque, so the raster's own alpha is part of what is drawn: red and blue
/// (the left column) and green and white (the right).
fn translucent_pixels() -> Arc<Raster> {
    Arc::new(Raster::new(2, 2, [[255, 0, 0, 128], [0, 255, 0, 200], [0, 0, 255, 255], [255, 255, 255, 128]].concat()).unwrap())
}

/// The picture the preservation matrix draws: the same 10 x 10 square as [`image`], clipped to its
/// left column (a real boundary, not the whole image), half faded, drawn half transparent.
fn rich_image() -> Image {
    Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(0.5, 1.5)], clipping: true, fade: 25, ..image() }
}

/// The half transparency `rich_image` is drawn with, on its entity.
fn rich_common() -> Common {
    Common { transparency: Transparency::Percent(50), ..Common::default() }
}

/// Where the picture is shown: model space, a layout's own objects, a layout's viewport onto model
/// space, or a block reference.
#[derive(Clone, Copy, Debug)]
enum Place {
    Model,
    Paper,
    Viewport,
    Block,
}

impl Place {
    const ALL: [Place; 4] = [Place::Model, Place::Paper, Place::Viewport, Place::Block];

    fn space(self) -> Space {
        match self {
            Place::Model | Place::Block => Space::Model,
            Place::Paper | Place::Viewport => Space::Paper("Layout1".into()),
        }
    }

    /// A drawing whose picture is drawn with `mode` in its header (`None`: no IMAGEFRAME at all),
    /// with the image's pixels when `loaded`, and with the clip boundary, fade and transparency
    /// [`rich_image`] describes when `rich`.
    fn drawing(self, mode: Option<i64>, loaded: bool, rich: bool) -> Drawing {
        let mut d = Drawing::new_imperial();
        if let Some(mode) = mode {
            d.header.set_i64("IMAGEFRAME", mode);
        }
        let common = if rich { rich_common() } else { Common::default() };
        let image = if rich { rich_image() } else { image() };
        match self {
            Place::Model | Place::Paper => {
                d.add(&self.space(), common, EntityKind::Image(image)).unwrap();
            }
            Place::Viewport => {
                d.add(&Space::Model, common.clone(), EntityKind::Image(image)).unwrap();
                // Larger than the picture: the viewport cuts none of it, so its frame is one stroke.
                let vp = Viewport {
                    center: Vec3::new(5.0, 5.0, 0.0),
                    width: 20.0,
                    height: 20.0,
                    view_center: Vec2::new(5.0, 5.0),
                    view_height: 20.0,
                    id: 2,
                    locked: false,
                    frozen_layers: Vec::new(),
                    layer_colors: Vec::new(),
                    clip: None,
                    on: true,
                    twist: 0.0,
                };
                d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
            }
            Place::Block => {
                // The block reference scales its contents 5: the picture's axes and the corners of
                // its clip boundary turn with the block.
                let mut blk = Block::new("MAP");
                blk.entities.push(Entity { handle: Handle(900), common, kind: EntityKind::Image(image) });
                d.blocks.insert("MAP".into(), Arc::new(blk));
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
                    clip: None,
                };
                d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
            }
        }
        if loaded {
            d.rasters.insert("map.png", if rich { translucent_pixels() } else { pixels() });
        }
        d
    }

    /// The placement of the picture's top-left corner, one pixel to the right and one down, in the
    /// space it is drawn in: what the block's scale changes, and nothing else does.
    fn expected_geometry(self) -> (Vec2, Vec2, Vec2) {
        match self {
            Place::Model | Place::Paper | Place::Viewport => (Vec2::new(0.0, 10.0), Vec2::new(5.0, 0.0), Vec2::new(0.0, -5.0)),
            Place::Block => (Vec2::new(0.0, 50.0), Vec2::new(25.0, 0.0), Vec2::new(0.0, -25.0)),
        }
    }

    /// The corners of [`rich_image`]'s clip boundary: the left column of the picture, in the space
    /// it is drawn in.
    fn expected_clip(self) -> Vec<Vec2> {
        let s = if matches!(self, Place::Block) { 5.0 } else { 1.0 };
        [(0.0, 10.0), (5.0, 10.0), (5.0, 0.0), (0.0, 0.0)].map(|(x, y)| Vec2::new(x * s, y * s)).to_vec()
    }
}

/// What one picture holds: the strokes drawn (the frame and its diagonals, a viewport's border),
/// each image's placement, clip outlines and fade, the opacity each image draws with, and the
/// colours at the centres of the four quarters of the picture.
#[derive(Debug, PartialEq)]
struct Shot {
    strokes: usize,
    count: usize,
    geometry: Vec<(Vec2, Vec2, Vec2)>,
    clips: Vec<Vec<Vec<Vec2>>>,
    fades: Vec<u8>,
    opacity: Vec<u8>,
    pixels: [[u8; 4]; 4],
}

/// The view that fits `list`'s picture into the sample grid, so the four quarters are sampled at the
/// same place whatever space, block scale or viewport the picture is drawn through.
fn fitted(list: &DisplayList) -> View {
    let b = list.images.first().map(|i| Bounds2::from_points(i.corners())).unwrap_or(list.bounds);
    let size = b.width().max(b.height());
    View { center: b.center(), scale: if size > 0.0 { 100.0 / size } else { 1.0 }, width: 100, height: 100 }
}

fn shot(place: Place, mode: Option<i64>, loaded: bool, rich: bool, plotting: bool, transparency: bool) -> Shot {
    let d = Place::drawing(place, mode, loaded, rich);
    let space = place.space();
    let opts = Options { transparency, ..Options::default() };
    let list = if plotting { build_plot(&d, &space, &opts) } else { build(&d, &space, &opts) };
    let view = fitted(&list);
    let pm = render(&list, &view, &RasterOptions { antialias: false, ..Default::default() }).unwrap();
    let pixels = [(25, 25), (75, 25), (25, 75), (75, 75)].map(|(x, y)| {
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    });
    Shot {
        strokes: list.prims.iter().filter(|p| p.kind == Kind::Polyline).count(),
        count: list.images.len(),
        geometry: list.images.iter().map(|i| (i.origin, i.right, i.down)).collect(),
        clips: list.images.iter().map(|i| i.clips.clone()).collect(),
        fades: list.images.iter().map(|i| i.fade).collect(),
        opacity: list.prims.iter().filter(|p| matches!(p.kind, Kind::Image { .. })).map(|p| p.alpha()).collect(),
        pixels,
    }
}

/// How far a colour is from the canvas, channel by channel.
fn from_bg(p: [u8; 4]) -> u32 {
    p.iter().zip(bg()).map(|(a, b)| u32::from(a.abs_diff(b))).sum()
}

/// The mode changes the frame and nothing else: the pixels, the image's placement and axes, its
/// clip boundary, its fade and the opacity it draws with are the same in every mode, on screen and
/// in a plot, with transparency drawn and not, in all four places.
#[test]
fn every_mode_draws_the_same_picture() {
    for place in Place::ALL {
        for plotting in [false, true] {
            for transparency in [false, true] {
                let shots: [Shot; 3] = [0, 1, 2].map(|m| shot(place, Some(m), true, true, plotting, transparency));
                for (m, s) in shots.iter().enumerate() {
                    let what = format!("{place:?} mode {m} plotting {plotting} transparency {transparency}");
                    assert_eq!(s.count, 1, "{what}: one picture");
                    assert_eq!(s.geometry, shots[1].geometry, "{what}: placement and axes");
                    assert_eq!(s.clips, shots[1].clips, "{what}: clip outlines");
                    assert_eq!(s.fades, shots[1].fades, "{what}: fade");
                    assert_eq!(s.opacity, shots[1].opacity, "{what}: the opacity it draws with");
                    assert_eq!(s.pixels, shots[1].pixels, "{what}: pixels");
                    // What the pixels hold is the picture itself: the two samples inside the clip
                    // are its content, and the two outside it are the canvas.
                    assert_ne!(s.pixels[0], bg(), "{what}: the first quarter is the picture");
                    assert_ne!(s.pixels[2], bg(), "{what}: the third quarter is the picture");
                    assert_eq!(s.pixels[1], bg(), "{what}: the second quarter is clipped away");
                    assert_eq!(s.pixels[3], bg(), "{what}: the fourth quarter is clipped away");
                }
            }
        }
    }
}

/// The clip boundary, the fade and the transparency reach the drawing, and are what they say.
#[test]
fn the_picture_keeps_its_clip_fade_and_transparency() {
    for place in Place::ALL {
        let s = shot(place, Some(1), true, true, false, true);
        let what = format!("{place:?}");
        assert_eq!(s.geometry, [place.expected_geometry()], "{what}: placement and axes");
        // A viewport adds its own outline to the picture's, so the picture is cut to both.
        let outlines = if matches!(place, Place::Viewport) { 2 } else { 1 };
        assert_eq!(s.clips.first().map(Vec::len), Some(outlines), "{what}: outlines");
        let own = s.clips.first().and_then(|outlines| outlines.first()).cloned().unwrap_or_default();
        assert_eq!(own.len(), place.expected_clip().len(), "{what}: clip corners");
        for (got, want) in own.iter().zip(place.expected_clip()) {
            assert!((*got - want).len() < 1e-9, "{what}: clip corner {got:?} is not {want:?}");
        }
        assert_eq!(s.fades, [25], "{what}: the fade");
        // The source pixels are not opaque, so the raster's alpha is part of the picture drawn.
        assert_eq!(translucent_pixels().rgba().get(3), Some(&128), "{what}: a translucent source pixel");

        // The entity's 50 % transparency is only drawn when transparency is on, and it then lets
        // more of the canvas through.
        let opaque_entity = shot(place, Some(1), true, true, false, false);
        assert_eq!(opaque_entity.opacity, [255], "{what}: an opaque entity");
        assert_eq!(s.opacity, [128], "{what}: a 50 % entity");
        assert!(from_bg(s.pixels[0]) < from_bg(opaque_entity.pixels[0]), "{what}: the canvas shows through");
        // And the pixel is not the source colour: the fade blends it towards the canvas.
        assert_ne!(s.pixels[0], [255, 0, 0, 128], "{what}: an unfaded, untransparent pixel");
    }
}

/// The frame is all the mode adds or takes away: one rectangle with pixels, the rectangle and its
/// two diagonals without. Mode 0 draws none, 2 draws it on screen but not in a plot.
#[test]
fn the_mode_changes_the_frame_strokes_only() {
    for place in Place::ALL {
        for loaded in [true, false] {
            let per_mode = if loaded { 1 } else { 3 };
            for plotting in [false, true] {
                let base = shot(place, Some(0), loaded, false, plotting, false);
                for m in [0, 1, 2] {
                    let s = shot(place, Some(m), loaded, false, plotting, false);
                    let want = match (m, plotting) {
                        (0, _) | (2, true) => 0,
                        _ => per_mode,
                    };
                    assert_eq!(s.strokes, base.strokes + want, "{place:?} mode {m} plotting {plotting} loaded {loaded}");
                    if !loaded {
                        assert!(s.count == 0 && s.geometry.is_empty(), "{place:?} mode {m}: no pixels, no picture");
                    }
                }
            }
        }
    }
}

/// A drawing whose header holds no IMAGEFRAME shows the frame, on screen and in a plot, exactly as
/// mode 1 does: the default is 1.
#[test]
fn a_drawing_without_the_setting_shows_the_frame() {
    for place in Place::ALL {
        for loaded in [true, false] {
            for plotting in [false, true] {
                let unset = shot(place, None, loaded, false, plotting, false);
                let one = shot(place, Some(1), loaded, false, plotting, false);
                assert_eq!(unset, one, "{place:?} plotting {plotting} loaded {loaded}");
            }
        }
    }
}
