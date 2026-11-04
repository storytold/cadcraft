//! Only colour 7 flips between white and black with the background; every other colour,
//! however light or dark, is drawn as its own RGB (issue #272).

use cadcraft_color::{Color, Rgb, aci_rgb};
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::raster::{RasterOptions, View, render};
use cadcraft_render::{Options, build};

const WHITE: Rgb = Rgb(255, 255, 255);
const BLACK: Rgb = Rgb(0, 0, 0);
const DARK: Rgb = Rgb(33, 40, 48);

fn line(y: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) })
}

/// An insert of block "B" (one ByBlock line along y = 0) at height `y`.
fn insert(y: f64) -> EntityKind {
    EntityKind::Insert(Insert {
        block: "B".into(),
        insert: Vec3::new(0.0, y, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

/// One horizontal line per case at y = 0, 1, 2, ... and, per case, its name and the expected
/// colour on paper (white) and on the dark canvas.
fn cases() -> (Drawing, Vec<(&'static str, Rgb, Rgb)>) {
    let mut d = Drawing::new_imperial();
    let mut blk = Block::new("B");
    let mut by_block = Entity::new(Handle(1), line(0.0));
    by_block.common.color = Color::ByBlock;
    blk.entities.push(by_block);
    d.blocks.insert("B".into(), std::sync::Arc::new(blk));
    let light_yellow = aci_rgb(51);
    let dark_grey = aci_rgb(250);
    let c = |color| Common { color, ..Default::default() };
    let rows: Vec<(&str, Common, EntityKind, Rgb, Rgb)> = vec![
        ("ByLayer on layer 0 (colour 7)", Common::default(), line(0.0), BLACK, WHITE),
        ("index 7", c(Color::Index(7)), line(1.0), BLACK, WHITE),
        ("ByBlock in a colour-7 insert", c(Color::Index(7)), insert(2.0), BLACK, WHITE),
        ("index 51 (light yellow)", c(Color::Index(51)), line(3.0), light_yellow, light_yellow),
        ("index 255 (white)", c(Color::Index(255)), line(4.0), WHITE, WHITE),
        ("true-colour white", c(Color::True(WHITE)), line(5.0), WHITE, WHITE),
        ("index 250 (dark grey)", c(Color::Index(250)), line(6.0), dark_grey, dark_grey),
        ("true-colour black", c(Color::True(BLACK)), line(7.0), BLACK, BLACK),
        ("ByBlock in an index-255 insert", c(Color::Index(255)), insert(8.0), WHITE, WHITE),
    ];
    let mut out = Vec::new();
    for (name, common, kind, paper, canvas) in rows {
        d.add(&Space::Model, common, kind).unwrap();
        out.push((name, paper, canvas));
    }
    (d, out)
}

#[test]
fn only_colour_7_flips_with_the_background() {
    let (d, cases) = cases();
    let l = build(&d, &Space::Model, &Options::default());
    assert_eq!(l.prims.len(), cases.len());
    for (i, (name, paper, canvas)) in cases.iter().enumerate() {
        let p = l.prims.iter().find(|p| (l.points(p)[0].y - i as f64).abs() < 1e-9).unwrap();
        assert_eq!(p.display_rgb(WHITE), *paper, "{name} on paper");
        assert_eq!(p.display_rgb(DARK), *canvas, "{name} on the dark canvas");
    }
}

#[test]
fn png_draws_true_colours() {
    let (d, cases) = cases();
    let l = build(&d, &Space::Model, &Options::default());
    let view = View { center: Vec2::new(5.0, 4.0), scale: 10.0, width: 120, height: 120 };
    for (bg, paper) in [(WHITE, true), (DARK, false)] {
        let pm = render(&l, &view, &RasterOptions { background: bg, antialias: false, ..Default::default() }).unwrap();
        for (i, (name, on_paper, on_canvas)) in cases.iter().enumerate() {
            let (x, y) = view.to_screen(Vec2::new(5.0, i as f64));
            let px = pm.pixel(x as u32, y as u32).unwrap();
            let want = if paper { on_paper } else { on_canvas };
            assert_eq!(Rgb(px.red(), px.green(), px.blue()), *want, "{name} on {bg:?}");
        }
    }
}
