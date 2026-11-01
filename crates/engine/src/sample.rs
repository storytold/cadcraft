//! Sample drawings generated in code (original CADCraft content, used for demos and tests).

use cadcraft_color::Color;
use cadcraft_doc::{Common, DimKind, Dimension, Drawing, EntityKind, Hatch, HatchLoop, Layer, Lineweight, MText, Space, Text};
use cadcraft_geom::{PolyVertex, Vec2, Vec3, arc_to_bulge};

use crate::cmd::helpers::{arc, circle, line, lwpoly, rect_vertices, v3};

fn layer(name: &str, color: u8, lt: &str, lw: u16) -> Layer {
    Layer { name: name.into(), color: Color::Index(color), linetype: lt.into(), lineweight: Lineweight::Mm100(lw), ..Layer::default() }
}

fn on(layer: &str) -> Common {
    Common { layer: layer.into(), ..Common::default() }
}

fn dim(kind: DimKind, p13: Vec2, p14: Vec2, defpt: Vec2) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt: v3(defpt),
        text_mid: Vec3::ZERO,
        p13: v3(p13),
        p14: v3(p14),
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
    })
}

fn text(at: Vec2, h: f64, s: &str) -> EntityKind {
    EntityKind::Text(Text {
        insert: v3(at),
        align_pt: None,
        height: h,
        value: s.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
        backwards: false,
        upside_down: false,
    })
}

/// A mechanical part drawing: a flanged mounting bracket in two views with dimensions,
/// centre lines, hidden lines, a section hatch and a title block. Units: inches.
pub fn bracket() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(cadcraft_doc::library::standard_linetypes());
    d.layers.extend([
        layer("Outline", 7, "Continuous", 50),
        layer("Center", 1, "CENTER", 18),
        layer("Hidden", 2, "HIDDEN", 25),
        layer("Dimensions", 4, "Continuous", 18),
        layer("Hatch", 8, "Continuous", 13),
        layer("Notes", 3, "Continuous", 18),
        layer("Title", 7, "Continuous", 35),
    ]);
    d.header.set_f64("LTSCALE", 0.5);
    d.header.set_str("CLAYER", "Outline");
    if let Some(ds) = d.dim_styles.first_mut() {
        ds.arrow_size = 0.12;
        ds.text_height = 0.14;
        ds.decimals = 2;
        ds.text_above = 1;
        ds.text_gap = 0.05;
        ds.ext_offset = 0.06;
        ds.ext_extend = 0.1;
    }
    let m = Space::Model;
    let mut add = |c: Common, k: EntityKind| {
        let _ = d.add(&m, c, k);
    };

    // ---- Front view: plate 8 x 5 with filleted corners, origin (1, 1).
    let (x0, y0, w, h, r) = (1.0, 1.0, 8.0, 5.0, 0.5);
    let b = arc_to_bulge(std::f64::consts::FRAC_PI_2);
    let outline = vec![
        PolyVertex::new(Vec2::new(x0 + r, y0)),
        PolyVertex::with_bulge(Vec2::new(x0 + w - r, y0), b),
        PolyVertex::new(Vec2::new(x0 + w, y0 + r)),
        PolyVertex::with_bulge(Vec2::new(x0 + w, y0 + h - r), b),
        PolyVertex::new(Vec2::new(x0 + w - r, y0 + h)),
        PolyVertex::with_bulge(Vec2::new(x0 + r, y0 + h), b),
        PolyVertex::new(Vec2::new(x0, y0 + h - r)),
        PolyVertex::with_bulge(Vec2::new(x0, y0 + r), b),
    ];
    add(on("Outline"), lwpoly(outline, true));
    // Bolt holes.
    let holes = [Vec2::new(2.0, 2.0), Vec2::new(8.0, 2.0), Vec2::new(2.0, 5.0), Vec2::new(8.0, 5.0)];
    for hc in holes {
        add(on("Outline"), circle(hc, 0.3125));
        add(on("Center"), line(hc - Vec2::X * 0.55, hc + Vec2::X * 0.55));
        add(on("Center"), line(hc - Vec2::Y * 0.55, hc + Vec2::Y * 0.55));
    }
    // Central boss with bore.
    let c = Vec2::new(5.0, 3.5);
    add(on("Outline"), circle(c, 1.5));
    add(on("Outline"), circle(c, 0.875));
    add(on("Hidden"), circle(c, 1.125));
    add(on("Center"), line(c - Vec2::X * 1.9, c + Vec2::X * 1.9));
    add(on("Center"), line(c - Vec2::Y * 1.9, c + Vec2::Y * 1.9));
    // Slot.
    let slot = vec![
        PolyVertex::new(Vec2::new(3.25, 1.6)),
        PolyVertex::with_bulge(Vec2::new(6.75, 1.6), 1.0),
        PolyVertex::new(Vec2::new(6.75, 1.1) + Vec2::Y * 0.0),
    ];
    let _ = slot;
    let slot = vec![
        PolyVertex::new(Vec2::new(4.0, 1.5)),
        PolyVertex::with_bulge(Vec2::new(6.0, 1.5), 1.0),
        PolyVertex::new(Vec2::new(6.0, 1.9)),
        PolyVertex::with_bulge(Vec2::new(4.0, 1.9), 1.0),
    ];
    add(on("Outline"), lwpoly(slot, true));

    // ---- Side view (section A-A) to the right: plate 0.5 thick + boss 1.5 tall.
    let sx = 11.0;
    let sec_plate = rect_vertices(Vec2::new(sx, y0), Vec2::new(sx + 0.5, y0 + h));
    add(on("Outline"), lwpoly(sec_plate.clone(), true));
    let boss = rect_vertices(Vec2::new(sx + 0.5, c.y - 1.5), Vec2::new(sx + 2.0, c.y + 1.5));
    add(on("Outline"), lwpoly(boss.clone(), true));
    // Bore (hidden in side view → shown as section void).
    add(on("Outline"), line(Vec2::new(sx, c.y - 0.875), Vec2::new(sx + 2.0, c.y - 0.875)));
    add(on("Outline"), line(Vec2::new(sx, c.y + 0.875), Vec2::new(sx + 2.0, c.y + 0.875)));
    add(on("Center"), line(Vec2::new(sx - 0.4, c.y), Vec2::new(sx + 2.4, c.y)));
    // Section hatch on the solid parts (two regions around the bore).
    let hatch = |a: Vec2, bb: Vec2| {
        EntityKind::Hatch(Hatch {
            pattern: "ANSI31".into(),
            solid: false,
            loops: vec![HatchLoop { vertices: rect_vertices(a, bb), outer: true, sources: Vec::new() }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        })
    };
    add(on("Hatch"), hatch(Vec2::new(sx, c.y + 0.875), Vec2::new(sx + 0.5, y0 + h)));
    add(on("Hatch"), hatch(Vec2::new(sx, y0), Vec2::new(sx + 0.5, c.y - 0.875)));
    add(on("Hatch"), hatch(Vec2::new(sx + 0.5, c.y + 0.875), Vec2::new(sx + 2.0, c.y + 1.5)));
    add(on("Hatch"), hatch(Vec2::new(sx + 0.5, c.y - 1.5), Vec2::new(sx + 2.0, c.y - 0.875)));

    // ---- Dimensions.
    add(on("Dimensions"), dim(DimKind::Linear { rotation: 0.0 }, Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(5.0, 0.35)));
    add(
        on("Dimensions"),
        dim(DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 }, Vec2::new(x0, y0), Vec2::new(x0, y0 + h), Vec2::new(0.3, 3.5)),
    );
    add(on("Dimensions"), dim(DimKind::Linear { rotation: 0.0 }, Vec2::new(2.0, 5.0), Vec2::new(8.0, 5.0), Vec2::new(5.0, 6.6)));
    add(on("Dimensions"), dim(DimKind::Linear { rotation: 0.0 }, Vec2::new(sx, y0 + h), Vec2::new(sx + 2.0, c.y + 1.5), Vec2::new(sx + 1.0, 6.6)));
    add(
        on("Dimensions"),
        EntityKind::Dimension(Dimension {
            kind: DimKind::Diameter,
            defpt: v3(c + Vec2::from_angle(0.8) * 1.5),
            text_mid: Vec3::ZERO,
            p13: Vec3::ZERO,
            p14: Vec3::ZERO,
            p15: v3(c - Vec2::from_angle(0.8) * 1.5),
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
        }),
    );
    add(
        on("Dimensions"),
        EntityKind::Dimension(Dimension {
            kind: DimKind::Radius,
            defpt: v3(Vec2::new(8.0, 2.0)),
            text_mid: Vec3::ZERO,
            p13: Vec3::ZERO,
            p14: Vec3::ZERO,
            p15: v3(Vec2::new(8.0, 2.0) + Vec2::from_angle(-0.7) * 0.3125),
            p16: Vec3::ZERO,
            text: "4X <>".into(),
            style: "Standard".into(),
            measurement: 0.0,
            text_rotation: 0.0,
            user_text_pos: false,
            block: None,
            overrides: Default::default(),
            assoc: Vec::new(),
            marks: Default::default(),
        }),
    );

    // ---- Notes and title block.
    add(on("Notes"), text(Vec2::new(sx - 0.2, -0.3), 0.16, "SECTION A-A"));
    add(on("Notes"), text(Vec2::new(4.1, -0.3), 0.16, "FRONT VIEW"));
    add(
        on("Notes"),
        EntityKind::MText(MText { insert: v3(Vec2::new(14.2, 6.2)), height: 0.12, width: 3.4, attach: 1, rotation: 0.0, style: "Standard".into(), contents: "NOTES:\\P1. MATERIAL: 6061-T6 ALUMINUM.\\P2. BREAK ALL SHARP EDGES 0.02 MAX.\\P3. BORE %%c1.750 THRU, H7.\\P4. FINISH: CLEAR ANODIZE.".into(), line_spacing: 1.0, line_spacing_exact: false, columns: None, direction: Default::default(), background: None }),
    );
    let tb0 = Vec2::new(14.0, -0.5);
    add(on("Title"), lwpoly(rect_vertices(tb0, tb0 + Vec2::new(4.0, 2.0)), true));
    add(on("Title"), line(tb0 + Vec2::new(0.0, 1.2), tb0 + Vec2::new(4.0, 1.2)));
    add(on("Title"), line(tb0 + Vec2::new(0.0, 0.6), tb0 + Vec2::new(4.0, 0.6)));
    add(on("Title"), line(tb0 + Vec2::new(2.0, 0.0), tb0 + Vec2::new(2.0, 0.6)));
    add(on("Title"), text(tb0 + Vec2::new(0.15, 1.5), 0.22, "MOUNTING BRACKET"));
    add(on("Title"), text(tb0 + Vec2::new(0.15, 0.82), 0.12, "CADCRAFT SAMPLE  DWG NO. CC-0001"));
    add(on("Title"), text(tb0 + Vec2::new(0.15, 0.22), 0.12, "SCALE 1:1"));
    add(on("Title"), text(tb0 + Vec2::new(2.15, 0.22), 0.12, "SHEET 1 OF 1"));
    // Drawing border.
    add(on("Title"), lwpoly(rect_vertices(Vec2::new(-0.5, -0.5), Vec2::new(18.0, 7.5)), true));
    let _ = arc;
    d
}

/// A small architectural floor plan (feet-inches, drawn in inches).
pub fn floor_plan() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(cadcraft_doc::library::standard_linetypes());
    d.layers.extend([
        layer("A-WALL", 7, "Continuous", 50),
        layer("A-DOOR", 3, "Continuous", 25),
        layer("A-GLAZ", 4, "Continuous", 18),
        layer("A-ANNO", 2, "Continuous", 18),
        layer("A-FURN", 8, "Continuous", 13),
    ]);
    d.header.set_i64("LUNITS", 4);
    d.header.set_i64("LUPREC", 4);
    let m = Space::Model;
    let mut add = |c: Common, k: EntityKind| {
        let _ = d.add(&m, c, k);
    };
    let ft = 12.0;
    let t = 6.0;
    // Outer walls 30' x 20'.
    add(on("A-WALL"), lwpoly(rect_vertices(Vec2::ZERO, Vec2::new(30.0 * ft, 20.0 * ft)), true));
    add(on("A-WALL"), lwpoly(rect_vertices(Vec2::new(t, t), Vec2::new(30.0 * ft - t, 20.0 * ft - t)), true));
    // Interior wall.
    add(on("A-WALL"), lwpoly(rect_vertices(Vec2::new(14.0 * ft, t), Vec2::new(14.0 * ft + 4.0, 20.0 * ft - t)), true));
    // Door swing.
    let hinge = Vec2::new(14.0 * ft + 4.0, 6.0 * ft);
    add(on("A-DOOR"), line(hinge, hinge + Vec2::X * 36.0));
    add(on("A-DOOR"), arc(&cadcraft_geom::Arc::new(hinge, 36.0, 0.0, std::f64::consts::FRAC_PI_2)));
    // Windows.
    for x in [5.0, 20.0] {
        add(on("A-GLAZ"), line(Vec2::new(x * ft, 0.0), Vec2::new(x * ft + 48.0, 0.0)));
        add(on("A-GLAZ"), line(Vec2::new(x * ft, t), Vec2::new(x * ft + 48.0, t)));
        add(on("A-GLAZ"), line(Vec2::new(x * ft, t / 2.0), Vec2::new(x * ft + 48.0, t / 2.0)));
    }
    add(on("A-ANNO"), text(Vec2::new(5.0 * ft, 10.0 * ft), 9.0, "LIVING ROOM"));
    add(on("A-ANNO"), text(Vec2::new(19.0 * ft, 10.0 * ft), 9.0, "BEDROOM"));
    add(on("A-ANNO"), dim(DimKind::Linear { rotation: 0.0 }, Vec2::ZERO, Vec2::new(30.0 * ft, 0.0), Vec2::new(15.0 * ft, -3.0 * ft)));
    // Furniture.
    add(on("A-FURN"), lwpoly(rect_vertices(Vec2::new(20.0 * ft, 14.0 * ft), Vec2::new(25.0 * ft, 19.0 * ft)), true));
    add(on("A-FURN"), circle(Vec2::new(7.0 * ft, 14.0 * ft), 24.0));
    d
}

/// The sample used by `--sample`.
pub fn default_sample() -> Drawing {
    bracket()
}

pub fn _unused() -> EntityKind {
    line(Vec2::ZERO, Vec2::X)
}
