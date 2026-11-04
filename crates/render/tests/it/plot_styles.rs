//! Plot style tables applied to the display list (issue #410): colour-dependent tables map an
//! object's colour to its printed colour, screening, linetype and lineweight; layouts show them
//! when "Display plot styles" is on.

use std::sync::Arc;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, Line, Lineweight, PlotStyle, PlotStyleKind, PlotStyleTable, Space};
use cadcraft_geom::Vec3;
use cadcraft_render::{DisplayList, Options, build, build_plot};

const BLACK: Rgb = Rgb(0, 0, 0);

fn line(y: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) })
}

/// Lines in red (1), green (3), colour 7 (ByLayer on layer 0) and a true orange, in `space`.
fn drawing(space: &Space) -> Drawing {
    let mut d = Drawing::new_metric();
    let c = |color| Common { color, ..Default::default() };
    d.add(space, c(Color::Index(1)), line(0.0)).unwrap();
    d.add(space, c(Color::Index(3)), line(1.0)).unwrap();
    d.add(space, Common::default(), line(2.0)).unwrap();
    d.add(space, c(Color::True(Rgb(255, 128, 0))), line(3.0)).unwrap();
    d
}

fn with_table(t: PlotStyleTable) -> Options {
    Options { plot_style_table: Some(Arc::new(t)), lineweights: true, ..Default::default() }
}

fn colours(l: &DisplayList) -> Vec<Rgb> {
    l.prims.iter().map(|p| p.display_rgb(Rgb(255, 255, 255))).collect()
}

#[test]
fn monochrome_prints_every_colour_black() {
    let d = drawing(&Space::Model);
    let l = build_plot(&d, &Space::Model, &with_table(PlotStyleTable::builtin("monochrome.ctb").unwrap()));
    assert_eq!(l.prims.len(), 4);
    assert!(l.prims.iter().all(|p| p.color == BLACK && !p.aci7), "{:?}", l.prims);
    // Without a table the colours are the objects'.
    let plain = build_plot(&d, &Space::Model, &Options::default());
    assert_eq!(colours(&plain), [Rgb(255, 0, 0), Rgb(0, 255, 0), BLACK, Rgb(255, 128, 0)]);
}

#[test]
fn grayscale_and_default_tables() {
    let d = drawing(&Space::Model);
    let l = build_plot(&d, &Space::Model, &with_table(PlotStyleTable::builtin("grayscale.ctb").unwrap()));
    for c in colours(&l) {
        assert!(c.0 == c.1 && c.1 == c.2, "grey: {c:?}");
    }
    let c = colours(&l);
    assert!(c[1].0 > c[0].0, "green is lighter than red");
    assert_eq!(c[2], BLACK, "colour 7 prints black");
    let keep = build_plot(&d, &Space::Model, &with_table(PlotStyleTable::builtin("default.ctb").unwrap()));
    assert_eq!(colours(&keep), colours(&build_plot(&d, &Space::Model, &Options::default())));
}

#[test]
fn screening_lineweight_and_linetype_overrides() {
    let mut d = Drawing::new_metric();
    d.header.set_f64("LTSCALE", 0.05);
    d.add(&Space::Model, Common { color: Color::Index(1), lineweight: Lineweight::Mm100(13), ..Default::default() }, line(0.0)).unwrap();
    d.add(&Space::Model, Common { color: Color::Index(5), ..Default::default() }, line(1.0)).unwrap();
    let table = PlotStyleTable::color_dependent("pens.ctb", "", |i| {
        (i == 1).then(|| PlotStyle {
            color: Some(BLACK),
            screening: 50,
            lineweight: Some(70),
            linetype: Some("DASHED".into()),
            ..PlotStyle::default()
        })
    });
    let l = build_plot(&d, &Space::Model, &with_table(table.clone()));
    let red: Vec<_> = l.prims.iter().filter(|p| p.color == Rgb(128, 128, 128)).collect();
    assert!(red.len() > 1, "dashed into several pieces: {}", red.len());
    assert!(red.iter().all(|p| p.lw == 0.7), "pen lineweight replaces 0.13 mm");
    let blue: Vec<_> = l.prims.iter().filter(|p| p.color == Rgb(0, 0, 255)).collect();
    assert_eq!(blue.len(), 1, "colour 5 keeps its own style");
    assert_eq!(blue[0].lw, 0.25, "object lineweight (LWDEFAULT)");
    // With lineweights off nothing gets a width.
    let thin = build_plot(&d, &Space::Model, &Options { lineweights: false, ..with_table(table) });
    assert!(thin.prims.iter().all(|p| p.lw == 0.0));
}

#[test]
fn layout_display_plot_styles_flag() {
    let space = Space::Paper("Layout1".into());
    let mut d = drawing(&space);
    d.layouts[0].page.plot_style_table = "monochrome.ctb".into();
    let red = |l: &DisplayList| l.prims.iter().any(|p| p.color == Rgb(255, 0, 0));
    assert!(red(&build(&d, &space, &Options::default())), "flag off: object colours on screen");
    d.layouts[0].page.show_plot_styles = true;
    assert!(!red(&build(&d, &space, &Options::default())), "flag on: monochrome on screen");
    // Plotting takes its table from the options (the plotter passes the page's).
    assert!(red(&build_plot(&d, &space, &Options::default())));
    // Model space and unknown tables draw object colours.
    d.layouts[0].page.plot_style_table = "unknown.ctb".into();
    assert!(red(&build(&d, &space, &Options::default())));
}

#[test]
fn hostile_tables_do_not_panic() {
    let d = drawing(&Space::Model);
    for styles in [Vec::new(), vec![PlotStyle { screening: 255, lineweight: Some(u16::MAX), linetype: Some("NOPE".into()), ..PlotStyle::default() }]]
    {
        let t = PlotStyleTable { name: "x.ctb".into(), styles, ..PlotStyleTable::default() };
        let l = build_plot(&d, &Space::Model, &with_table(t));
        assert_eq!(l.prims.len(), 4);
        assert!(l.prims.iter().all(|p| p.lw <= 2.11));
    }
    let named = PlotStyleTable { kind: PlotStyleKind::Named, styles: vec![PlotStyle::object("Normal")], ..PlotStyleTable::default() };
    assert_eq!(build_plot(&d, &Space::Model, &with_table(named)).prims.len(), 4);
}

#[test]
fn named_styles_by_layer_by_block_and_name() {
    use cadcraft_doc::{Block, Entity, Handle, Insert, Layer};
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), plot_style: "Black".into(), ..Layer::new("Walls") });
    let red = |plot_style: &str| Common { layer: "Walls".into(), plot_style: plot_style.into(), ..Default::default() };
    d.add(&Space::Model, red("ByLayer"), line(0.0)).unwrap();
    d.add(&Space::Model, red("Normal"), line(1.0)).unwrap();
    d.add(&Space::Model, red("Screened 50%"), line(2.0)).unwrap();
    d.add(&Space::Model, red("No Such Style"), line(3.0)).unwrap();
    // A ByBlock line inside a block whose reference is "Thick".
    let mut blk = Block::new("B");
    blk.entities.push(Entity {
        common: Common { color: Color::Index(1), plot_style: "ByBlock".into(), ..Default::default() },
        ..Entity::new(Handle(1), line(0.0))
    });
    d.blocks.insert("B".into(), Arc::new(blk));
    let ins = EntityKind::Insert(Insert {
        block: "B".into(),
        insert: Vec3::new(0.0, 4.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    d.add(&Space::Model, Common { plot_style: "Thick".into(), ..Default::default() }, ins).unwrap();
    let l = build_plot(&d, &Space::Model, &with_table(PlotStyleTable::builtin("default.stb").unwrap()));
    let got: Vec<(Rgb, f32)> = l.prims.iter().map(|p| (p.display_rgb(Rgb(255, 255, 255)), p.lw)).collect();
    let red_rgb = Rgb(255, 0, 0);
    assert_eq!(
        got,
        [(BLACK, 0.25), (red_rgb, 0.25), (Rgb(255, 128, 128), 0.25), (red_rgb, 0.25), (red_rgb, 0.5)],
        "ByLayer → Black, Normal, Screened 50%, unknown → Normal, ByBlock → the reference's Thick"
    );
    // A colour-dependent table ignores the property.
    let ctb = build_plot(&d, &Space::Model, &with_table(PlotStyleTable::builtin("default.ctb").unwrap()));
    assert!(ctb.prims.iter().all(|p| p.color == red_rgb));
}
