use crate::raster::{RasterOptions, View, render};
use crate::*;
use cadcraft_doc::{Common, Drawing, EntityKind, Hatch, HatchLoop, Space};
use cadcraft_geom::{PolyVertex, Vec3};

fn sample() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(10.0, 0.0, 0.0) }))
        .unwrap();
    d.add(
        &Space::Model,
        Common { color: Color::Index(1), ..Default::default() },
        EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(5.0, 5.0, 0.0), radius: 3.0 }),
    )
    .unwrap();
    d
}

#[test]
fn builds_lines_and_circles() {
    let d = sample();
    let l = build(&d, &Space::Model, &Options { tolerance: 0.01, ..Default::default() });
    assert_eq!(l.prims.len(), 2);
    assert!(l.prims[1].len > 20);
    assert_eq!(l.prims[1].color, Rgb(255, 0, 0));
}

#[test]
fn linetype_dashes_line() {
    let mut d = sample();
    d.linetypes.extend(cadcraft_doc::library::standard_linetypes());
    d.layer_mut("0").unwrap().linetype = "DASHED".into();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.len() > 10);
}

#[test]
fn frozen_layer_hidden() {
    let mut d = sample();
    d.layer_mut("0").unwrap().frozen = true;
    assert!(build(&d, &Space::Model, &Options::default()).prims.is_empty());
}

#[test]
fn text_produces_strokes() {
    let mut d = Drawing::new_imperial();
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Text(cadcraft_doc::Text {
            insert: Vec3::ZERO,
            align_pt: None,
            height: 1.0,
            value: "CAD".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    )
    .unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.len() >= 3);
}

#[test]
fn hatch_pattern_and_solid() {
    let mut d = Drawing::new_imperial();
    let sq = vec![
        PolyVertex::new(Vec2::new(0.0, 0.0)),
        PolyVertex::new(Vec2::new(4.0, 0.0)),
        PolyVertex::new(Vec2::new(4.0, 4.0)),
        PolyVertex::new(Vec2::new(0.0, 4.0)),
    ];
    let h = Hatch {
        pattern: "ANSI31".into(),
        solid: false,
        loops: vec![HatchLoop { vertices: sq.clone(), outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(h.clone())).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    // 45° lines at 0.125 spacing across a 4x4 square: about 4*sqrt(2)/0.125 ≈ 45 lines.
    assert!(l.prims.len() > 35 && l.prims.len() < 60, "{}", l.prims.len());
    let mut d2 = Drawing::new_imperial();
    d2.add(&Space::Model, Common::default(), EntityKind::Hatch(Hatch { solid: true, pattern: "SOLID".into(), ..h })).unwrap();
    let l2 = build(&d2, &Space::Model, &Options::default());
    assert_eq!(l2.prims.len(), 1);
    assert_eq!(l2.prims[0].kind, Kind::Tris);
}

#[test]
fn raster_draws_pixels() {
    let d = sample();
    let l = build(&d, &Space::Model, &Options::default());
    let v = View::fit(&l.bounds, 200, 200, 0.1);
    let pm = render(&l, &v, &RasterOptions::default()).unwrap();
    let bg = pm.pixels().iter().filter(|p| p.red() == 33).count();
    assert!(bg < 200 * 200);
    // A red pixel exists from the circle.
    assert!(pm.pixels().iter().any(|p| p.red() > 200 && p.green() < 60));
}

#[test]
fn raster_rejects_bad_size() {
    let l = DisplayList::default();
    assert!(render(&l, &View { center: Vec2::ZERO, scale: 1.0, width: 0, height: 10 }, &RasterOptions::default()).is_none());
}

fn layout_with_viewport(vp: cadcraft_doc::Viewport) -> Drawing {
    let mut d = sample(); // line (0,0)-(10,0) and red circle at (5,5) r3
    let paper = Space::Paper("Layout1".into());
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    d
}

fn vp(center: Vec2, w: f64, h: f64, view_center: Vec2, view_height: f64, id: u32) -> cadcraft_doc::Viewport {
    cadcraft_doc::Viewport {
        center: center.to3(0.0),
        width: w,
        height: h,
        view_center,
        view_height,
        id,
        locked: false,
        frozen_layers: Vec::new(),
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    }
}

#[test]
fn layout_viewport_shows_model_clipped() {
    // Viewport 4x4 paper units centred at (5,4), showing model window centred at (5,5), 8 high:
    // scale 0.5, so model (5,5) → paper (5,4); the visible model window is (1,1)..(9,9).
    let d = layout_with_viewport(vp(Vec2::new(5.0, 4.0), 4.0, 4.0, Vec2::new(5.0, 5.0), 8.0, 2));
    let l = build(&d, &Space::Paper("Layout1".into()), &Options { tolerance: 0.01, ..Default::default() });
    let sheet = l.sheet.unwrap();
    assert!(sheet.size.x > sheet.size.y, "landscape sheet");
    let rect = Bounds2::new(Vec2::new(3.0, 2.0), Vec2::new(7.0, 6.0)).expand(1e-9);
    let content: Vec<&DPrim> = l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).collect();
    assert!(!content.is_empty(), "model geometry is drawn in the viewport");
    for p in &content {
        for q in l.points(p) {
            assert!(rect.contains(*q), "{q:?} outside the viewport");
        }
    }
    // The circle (red) is visible, scaled by 0.5 about the view centre: radius 1.5 around (5,4).
    let red: Vec<Vec2> = content.iter().filter(|p| p.color == Rgb(255, 0, 0)).flat_map(|p| l.points(p).to_vec()).collect();
    assert!(!red.is_empty());
    assert!(red.iter().all(|q| (q.dist(Vec2::new(5.0, 4.0)) - 1.5).abs() < 0.02));
    // The line y=0 maps to paper y=1.5, below the viewport: clipped away entirely.
    assert!(content.iter().filter(|p| p.color != Rgb(255, 0, 0)).all(|p| l.points(p).iter().all(|q| q.y > 1.9)));
    // The border is drawn with the viewport's own handle.
    assert!(l.prims.iter().any(|p| p.handle != VIEWPORT_CONTENT));
}

#[test]
fn layout_viewport_frozen_layer_and_id1() {
    let mut v = vp(Vec2::new(5.0, 4.0), 20.0, 20.0, Vec2::new(5.0, 5.0), 20.0, 2);
    v.frozen_layers = vec!["0".into()];
    let d = layout_with_viewport(v);
    let l = build(&d, &Space::Paper("Layout1".into()), &Options::default());
    assert!(l.prims.iter().all(|p| p.handle != VIEWPORT_CONTENT), "layer 0 is frozen in this viewport");
    // Viewport id 1 (the paper-space view) draws nothing.
    let d = layout_with_viewport(vp(Vec2::new(5.0, 4.0), 20.0, 20.0, Vec2::new(5.0, 5.0), 20.0, 1));
    assert!(build(&d, &Space::Paper("Layout1".into()), &Options::default()).prims.is_empty());
    // Model space never draws viewport contents; plotting skips non-plot layers.
    let mut d = sample();
    d.layer_mut("0").unwrap().plot = false;
    assert!(build_plot(&d, &Space::Model, &Options::default()).prims.is_empty());
    assert!(!build(&d, &Space::Model, &Options::default()).prims.is_empty());
}

/// The points of each primitive drawn through a viewport.
fn viewport_points(l: &DisplayList) -> Vec<Vec<Vec2>> {
    l.prims.iter().filter(|p| p.handle == VIEWPORT_CONTENT).map(|p| l.points(p).to_vec()).collect()
}

#[test]
fn viewport_contents_can_be_left_out_and_built_for_any_area() {
    let d = layout_with_viewport(vp(Vec2::new(5.0, 4.0), 4.0, 4.0, Vec2::new(5.0, 5.0), 8.0, 2));
    let paper = Space::Paper("Layout1".into());
    let h = d.space(&paper).unwrap().iter().find(|e| matches!(e.kind, EntityKind::Viewport(_))).unwrap().handle;
    let opts = Options { tolerance: 0.01, ..Default::default() };
    // Left out of the layout: only the border remains.
    let skipped = build(&d, &paper, &Options { skip_viewport: Some(h), ..opts.clone() });
    assert!(viewport_points(&skipped).is_empty());
    assert!(skipped.prims.iter().any(|p| p.handle == h));
    // Built for the viewport's own rectangle: what the layout draws there.
    let rect = Bounds2::new(Vec2::new(3.0, 2.0), Vec2::new(7.0, 6.0));
    let full = build(&d, &paper, &opts);
    assert!(!viewport_points(&full).is_empty());
    assert_eq!(viewport_points(&viewport_content(&d, &paper, h, rect, &opts)), viewport_points(&full));
    // Built for a larger area, clipped to it instead of the viewport: the line y=0 (paper y=1.5,
    // below the viewport) is there.
    let wide = viewport_content(&d, &paper, h, rect.expand(2.0), &opts);
    assert!(viewport_points(&wide).iter().any(|pts| pts.iter().all(|q| (q.y - 1.5).abs() < 1e-9)));
    // Not a viewport, or an empty or non-finite area: nothing.
    assert!(viewport_content(&d, &paper, Handle(0xDEAD), rect, &opts).prims.is_empty());
    assert!(viewport_content(&d, &paper, h, Bounds2::EMPTY, &opts).prims.is_empty());
    let nan = Bounds2::new(Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0));
    assert!(viewport_content(&d, &paper, h, nan, &opts).prims.is_empty());
}

#[test]
fn viewport_contents_built_on_all_cores_match_one_thread() {
    // Enough model objects for the parallel path; plots build on one thread.
    let mut d = layout_with_viewport(vp(Vec2::new(5.0, 4.0), 8.0, 6.0, Vec2::new(30.0, 30.0), 60.0, 2));
    for i in 0..1200 {
        let (x, y) = (f64::from(i % 40) * 1.5, f64::from(i / 40) * 2.0);
        let line = EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 1.0, y + 1.5, 0.0) });
        d.add(&Space::Model, Common::default(), line).unwrap();
    }
    let paper = Space::Paper("Layout1".into());
    let screen = build(&d, &paper, &Options::default());
    let plot = build_plot(&d, &paper, &Options::default());
    let pts = |l: &DisplayList| -> Vec<Vec<Vec2>> {
        let border: Vec<Handle> = d.space(&paper).unwrap().iter().map(|e| e.handle).collect();
        l.prims.iter().filter(|p| !border.contains(&p.handle)).map(|p| l.points(p).to_vec()).collect()
    };
    assert!(pts(&screen).len() > 1000);
    assert_eq!(pts(&screen), pts(&plot));
}

#[test]
fn hostile_viewports_do_not_panic() {
    for v in [
        vp(Vec2::new(f64::NAN, 0.0), 1.0, 1.0, Vec2::ZERO, 1.0, 2),
        vp(Vec2::ZERO, -1.0, 1.0, Vec2::ZERO, 1.0, 2),
        vp(Vec2::ZERO, 1.0, 1.0, Vec2::ZERO, 0.0, 2),
        vp(Vec2::ZERO, 1e308, 1e308, Vec2::new(1e308, -1e308), 1e-308, 2),
        vp(Vec2::ZERO, 1.0, 1.0, Vec2::new(f64::INFINITY, 0.0), 1.0, 2),
    ] {
        let d = layout_with_viewport(v);
        let _ = build(&d, &Space::Paper("Layout1".into()), &Options::default());
    }
}

fn text_doc(style_font: &str, value: &str) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(cadcraft_doc::TextStyle { name: "TT".into(), font: style_font.into(), ..Default::default() });
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::Text(cadcraft_doc::Text {
            insert: Vec3::ZERO,
            align_pt: None,
            height: 1.0,
            value: value.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "TT".into(),
            halign: Default::default(),
            valign: Default::default(),
        }),
    )
    .unwrap();
    d
}

/// The first common system TrueType font, if any is installed (tests skip otherwise).
fn system_font() -> Option<&'static str> {
    ["Arial", "Helvetica", "DejaVuSans", "Verdana", "LiberationSans-Regular"].into_iter().find(|n| cadcraft_fonts::ttf::find(n).is_some())
}

#[test]
fn truetype_text_is_filled_or_outlined() {
    let Some(font) = system_font() else { return };
    let mut d = text_doc(font, "OH");
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.iter().any(|p| p.kind == Kind::Tris), "TEXTFILL=1 fills glyphs");
    // The O's counter is a hole: filled area is well below the glyph box area.
    let area: f64 = l
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Tris)
        .flat_map(|p| l.points(p).chunks(3).map(|t| ((t[1] - t[0]).cross(t[2] - t[0]) / 2.0).abs()).collect::<Vec<_>>())
        .sum();
    assert!(area > 0.1 && area < l.bounds.width() * l.bounds.height() * 0.8, "area {area}");
    assert!((l.bounds.max.y - 1.0).abs() < 0.1, "cap height = text height");
    d.header.set_i64("TEXTFILL", 0);
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.iter().all(|p| p.kind == Kind::Polyline), "TEXTFILL=0 outlines");
    assert!(l.prims.len() >= 3, "O has two contours, H one");
}

#[test]
fn missing_font_falls_back_to_stroke() {
    let d = text_doc("NoSuchFontAnywhere.ttf", "OH");
    let l = build(&d, &Space::Model, &Options::default());
    assert!(!l.prims.is_empty() && l.prims.iter().all(|p| p.kind == Kind::Polyline));
}

#[test]
fn backwards_and_upside_down_mirror_text() {
    let mut d = text_doc("CADCraft Stroke", "ABC");
    let normal = build(&d, &Space::Model, &Options::default()).bounds;
    assert!(normal.min.x >= -1e-9 && normal.min.y >= -1e-9);
    d.text_styles.iter_mut().for_each(|s| s.backwards = true);
    let back = build(&d, &Space::Model, &Options::default()).bounds;
    assert!(back.max.x <= 1e-9 && (back.width() - normal.width()).abs() < 1e-9, "mirrored to the left");
    d.text_styles.iter_mut().for_each(|s| {
        s.backwards = false;
        s.upside_down = true;
    });
    let up = build(&d, &Space::Model, &Options::default()).bounds;
    assert!(up.max.y <= 1e-9 && up.min.y < -0.9, "mirrored below the baseline");
}

#[test]
fn truetype_mtext_and_dimension_text() {
    let Some(font) = system_font() else { return };
    let mut d = Drawing::new_imperial();
    d.text_styles.iter_mut().for_each(|s| s.font = font.into());
    d.add(
        &Space::Model,
        Common::default(),
        EntityKind::MText(cadcraft_doc::MText {
            insert: Vec3::ZERO,
            height: 1.0,
            width: 0.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "A{\\C1;B}\\P\\S1/2;".into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
    )
    .unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.iter().any(|p| p.kind == Kind::Tris && p.color == Rgb(255, 0, 0)), "red B");
    assert!(l.prims.iter().any(|p| p.kind == Kind::Tris && p.color != Rgb(255, 0, 0)));
    let mut d = Drawing::new_imperial();
    d.text_styles.iter_mut().for_each(|s| s.font = font.into());
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(lin_dim())).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.iter().filter(|p| p.kind == Kind::Tris).count() > 2, "arrows plus filled glyphs");
}

fn lin_dim() -> cadcraft_doc::Dimension {
    cadcraft_doc::Dimension {
        kind: cadcraft_doc::DimKind::Linear { rotation: 0.0 },
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
    }
}

#[test]
fn dimension_colours_by_role() {
    let mut d = Drawing::new_imperial();
    if let Some(st) = d.dim_styles.first_mut() {
        st.dim_line_color = Color::Index(1);
        st.ext_line_color = Color::Index(3);
        st.text_color = Color::Index(5);
    }
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(lin_dim())).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    let has = |c: Rgb, k: Kind| l.prims.iter().any(|p| p.color == c && p.kind == k);
    assert!(has(Rgb(255, 0, 0), Kind::Polyline) && has(Rgb(255, 0, 0), Kind::Tris), "dimension line and arrows red");
    assert!(has(Rgb(0, 255, 0), Kind::Polyline), "extension lines green");
    assert!(has(Rgb(0, 0, 255), Kind::Polyline), "text blue");
    // ByBlock (the default) takes the dimension's own colour.
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common { color: Color::Index(6), ..Default::default() }, EntityKind::Dimension(lin_dim())).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.iter().all(|p| p.color == Rgb(255, 0, 255)));
}

#[test]
fn table_title_is_centred_and_merged() {
    let mut d = Drawing::new_imperial();
    let cell = |t: &str| cadcraft_doc::TableCell { text: t.into(), ..Default::default() };
    let mut title = vec![cell("TITLE"), cell(""), cell("")];
    title[0].merged = Some((1, 3));
    let t = cadcraft_doc::Table {
        insert: Vec3::new(0.0, 0.0, 0.0),
        col_widths: vec![2.0, 2.0, 2.0],
        row_heights: vec![1.0, 1.0],
        cells: vec![title, vec![cell("a"), cell("b"), cell("c")]],
        style: "Standard".into(),
        text_height: 0.2,
        title: true,
        header: false,
        ..Default::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Table(t)).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    // No vertical cell borders cross the merged title row (y in 0..-1) except the outer ones.
    let inner_vertical_in_title = l.prims.iter().filter(|p| p.kind == Kind::Polyline).any(|p| {
        l.points(p).windows(2).any(|w| {
            (w[0].x - w[1].x).abs() < 1e-12
                && (w[0].y - w[1].y).abs() > 0.5
                && w[0].x > 0.1
                && w[0].x < 5.9
                && w[0].y.max(w[1].y) > -0.99
                && w[0].y.min(w[1].y) < -0.01
        })
    });
    assert!(!inner_vertical_in_title);
    // Title text centred on the table width.
    let text: Vec<Vec2> = l
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline)
        .flat_map(|p| l.points(p).to_vec())
        .filter(|q| q.y > -0.95 && q.y < -0.05 && q.x > 0.05 && q.x < 5.95)
        .collect();
    let b = Bounds2::from_points(text);
    assert!((b.center().x - 3.0).abs() < 0.05, "{b:?}");
}

// ---------- hatch level of detail (on-screen builds, issue #544) ----------

fn pattern_hatch(pattern: &str, scale: f64, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Hatch {
    let sq = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y)));
    Hatch {
        pattern: pattern.into(),
        solid: false,
        loops: vec![HatchLoop { vertices: sq.to_vec(), outer: true, sources: Vec::new() }],
        scale,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: Vec::new(),
    }
}

fn drawing_with(h: Hatch) -> Drawing {
    let mut d = Drawing::new_imperial();
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).unwrap();
    d
}

fn loops_of(h: &Hatch) -> Vec<Vec<Vec2>> {
    crate::hatch::fill_loops(h, 0.001)
}

/// On-screen options for a view where one pixel is `px` drawing units.
fn screen(px: f64, view: Option<Bounds2>) -> Options {
    Options { tolerance: px / 2.0, min_dash: px * 2.0, view, ..Default::default() }
}

#[test]
fn dense_hatch_on_screen_is_one_solid_fill() {
    // ANSI31 at scale 0.01: lines 0.00125 apart over a 4x4 square. On a 1000 px view of it a
    // pixel is 0.004, so the lines run together: one solid fill. Plots keep every line.
    let d = drawing_with(pattern_hatch("ANSI31", 0.01, (0.0, 0.0, 4.0, 4.0)));
    let plot = build(&d, &Space::Model, &Options::default());
    assert!(plot.prims.len() > 4000, "plots keep every line: {}", plot.prims.len());
    let l = build(&d, &Space::Model, &screen(0.004, None));
    assert_eq!(l.prims.len(), 1);
    assert_eq!(l.prims[0].kind, Kind::Tris);
}

#[test]
fn sparse_dots_keep_their_look_when_lines_are_close() {
    // Dot rows half a pixel apart but dots five pixels apart along them: not solid colour.
    let fam = |dashes: &[f64]| cadcraft_doc::library::PatternLine { angle: 0.0, origin: (0.0, 0.0), delta: (0.0, 0.5), dashes: dashes.to_vec() };
    let sparse = Hatch { pattern_lines: vec![fam(&[0.0, -5.0])], ..pattern_hatch("MYDOTS", 1.0, (0.0, 0.0, 40.0, 40.0)) };
    assert!(!crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &sparse, 1.0));
    // Dots closer than a pixel along rows closer than a pixel: solid.
    let dense = Hatch { pattern_lines: vec![fam(&[0.0, -0.5])], ..sparse.clone() };
    assert!(crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &dense, 1.0));
    // A family with only gaps draws nothing, so it never reads as solid.
    let blank = Hatch { pattern_lines: vec![fam(&[-0.5])], ..sparse.clone() };
    assert!(!crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &blank, 1.0));
    assert!(crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &blank, &loops_of(&blank), 1.0, None).is_empty());
    // Full detail is never solid.
    assert!(!crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &dense, 0.0));
}

#[test]
fn scattered_dot_pattern_reads_solid_once_dots_fill_the_pixels() {
    // AR-SAND at scale 3/16 (the library pattern is drawn 16x coarser since #643, so this is the
    // geometry of the old scale 3): three dot families, no single one dense enough on its own at
    // 0.5 per pixel, but together about four dots on every pixel. Zoomed in 4x the dots stand apart.
    let h = pattern_hatch("AR-SAND", 3.0 / 16.0, (0.0, 0.0, 100.0, 100.0));
    assert!(crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &h, 0.5));
    assert!(!crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &h, 0.125));
}

/// Same polylines up to float noise: the view-limited path rotates into the family frame and back,
/// so a start point can differ by ~1e-16 between platforms (#804). Structure must match exactly.
fn assert_lines_close(a: &[Vec<Vec2>], b: &[Vec<Vec2>], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: line count");
    for (i, (ra, rb)) in a.iter().zip(b).enumerate() {
        assert_eq!(ra.len(), rb.len(), "{what}: run {i} length");
        for (pa, pb) in ra.iter().zip(rb) {
            assert!((pa.x - pb.x).abs() < 1e-9 && (pa.y - pb.y).abs() < 1e-9, "{what}: run {i}: {pa:?} vs {pb:?}");
        }
    }
}

#[test]
fn normal_zoom_pattern_is_unchanged() {
    // Lines and dash repeats a few pixels long or more: the level of detail changes nothing.
    for (pattern, px) in [("ANSI31", 0.05), ("ANSI33", 0.05), ("AR-CONC", 0.02), ("AR-SAND", 0.01), ("DOTS", 0.01)] {
        let h = pattern_hatch(pattern, 1.0, (0.0, 0.0, 6.0, 4.0));
        let loops = loops_of(&h);
        let full = crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &h, &loops, 0.0, None);
        assert!(!full.is_empty(), "{pattern}");
        assert!(!crate::hatch::reads_solid(&cadcraft_doc::Drawing::new_imperial(), &h, px), "{pattern}");
        let same_res = crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &h, &loops, px, None);
        assert_lines_close(&same_res, &full, pattern);
        // A view covering the whole hatch changes nothing either.
        let all = Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(7.0, 5.0));
        let in_view = crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &h, &loops, px, Some(&all));
        assert_lines_close(&in_view, &full, pattern);
    }
}

#[test]
fn dash_repeat_under_two_pixels_is_one_run() {
    // ANSI33's dashed family repeats every 0.1875: at 0.1 per pixel it reads as a continuous line,
    // drawn as one run per span covering the same length as the full-detail span.
    let h = pattern_hatch("ANSI33", 1.0, (0.0, 0.0, 6.0, 4.0));
    let loops = loops_of(&h);
    let full = crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &h, &loops, 0.0, None);
    let lod = crate::hatch::pattern_lines(&cadcraft_doc::Drawing::new_imperial(), &h, &loops, 0.1, None);
    assert!(lod.len() < full.len(), "{} vs {}", lod.len(), full.len());
    assert!(lod.iter().all(|r| r.len() == 2));
    // The continuous family: identical runs in both.
    let solid_runs = |v: &[Vec<Vec2>]| v.iter().filter(|r| r.len() == 2).count();
    assert!(solid_runs(&lod) > 0 && solid_runs(&full) > solid_runs(&lod));
}

#[test]
fn hatch_pattern_only_inside_the_view() {
    // A 40x40 ANSI31 hatch looked at through a 2x2 view in a corner, at full pixel detail.
    let d = drawing_with(pattern_hatch("ANSI31", 1.0, (0.0, 0.0, 40.0, 40.0)));
    let full = build(&d, &Space::Model, &Options::default());
    let far = Some(Bounds2::new(Vec2::new(100.0, 100.0), Vec2::new(110.0, 110.0)));
    assert_eq!(build(&d, &Space::Model, &Options { view: far, ..Default::default() }).prims.len(), 0);
    let corner = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0));
    let part = build(&d, &Space::Model, &Options { view: Some(corner), ..Default::default() });
    assert!(!part.prims.is_empty() && part.prims.len() * 10 < full.prims.len(), "{} of {}", part.prims.len(), full.prims.len());
    // Runs are cut in the family's rotated frame: a 2x2 view at 45° reaches 1 unit out, plus one
    // line spacing (0.125 × √2 along an axis) from rounding the line range outwards.
    let near = corner.expand(1.0 + 0.125 * 2f64.sqrt() + 1e-9);
    assert!(part.verts.iter().all(|p| near.contains(*p)));
    // Nothing is lost inside the view: the drawn length within an inner box is the same.
    let inner = Bounds2::new(Vec2::new(0.2, 0.2), Vec2::new(1.8, 1.8));
    let (a, b) = (length_inside(&full, &inner), length_inside(&part, &inner));
    assert!(a > 1.0 && (a - b).abs() < 1e-9, "{a} vs {b}");
}

/// Total length of a display list's polylines inside an axis-aligned box (Liang–Barsky).
fn length_inside(l: &DisplayList, bx: &Bounds2) -> f64 {
    let mut total = 0.0;
    for p in l.prims.iter().filter(|p| p.kind == Kind::Polyline) {
        for w in l.points(p).windows(2) {
            let (p0, d) = (w[0], w[1] - w[0]);
            let (mut t0, mut t1) = (0.0f64, 1.0f64);
            let mut ok = true;
            for (pp, q) in [(-d.x, p0.x - bx.min.x), (d.x, bx.max.x - p0.x), (-d.y, p0.y - bx.min.y), (d.y, bx.max.y - p0.y)] {
                if pp == 0.0 {
                    ok &= q >= 0.0;
                } else if pp < 0.0 {
                    t0 = t0.max(q / pp);
                } else {
                    t1 = t1.min(q / pp);
                }
            }
            if ok && t1 > t0 {
                total += d.len() * (t1 - t0);
            }
        }
    }
    total
}

#[test]
fn big_hatch_seen_through_a_wide_view_is_drawn() {
    // ANSI31 (0.125 apart) over ±1e5 seen through a 3000-unit view: about 34,000 lines in the
    // family's rotated frame, past the 20,000-line guard of a whole-hatch sweep, but still drawn
    // when the sweep is limited to the view.
    let d = drawing_with(pattern_hatch("ANSI31", 1.0, (-1e5, -1e5, 1e5, 1e5)));
    let view = Some(Bounds2::new(Vec2::new(-1500.0, -1500.0), Vec2::new(1500.0, 1500.0)));
    let l = build(&d, &Space::Model, &Options { view, ..Default::default() });
    assert!(l.prims.len() > 20_000, "{}", l.prims.len());
}

#[test]
fn hatch_in_a_squashed_block_keeps_lines_that_show() {
    // A block hatch inserted 100x wider than tall: along x its lines are spread far apart on
    // screen, so it is drawn with lines, not merged into a fill.
    let mut d = Drawing::new_imperial();
    let mut blk = cadcraft_doc::Block::new("B");
    let mut e = cadcraft_doc::Entity::new(cadcraft_doc::Handle(1), EntityKind::Hatch(pattern_hatch("ANSI31", 1.0, (0.0, 0.0, 4.0, 4.0))));
    e.common = Common::default();
    blk.entities.push(e);
    d.blocks.insert("B".into(), std::sync::Arc::new(blk));
    let ins = cadcraft_doc::Insert {
        block: "B".into(),
        insert: Vec3::new(0.0, 0.0, 0.0),
        scale: Vec3::new(100.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    // One pixel = 1 unit: 0.125 spacing × 100 along x is far more than a pixel.
    let l = build(&d, &Space::Model, &screen(1.0, None));
    assert!(l.prims.iter().all(|p| p.kind != Kind::Tris), "merged into a fill");
    assert!(l.prims.len() > 10, "{}", l.prims.len());
}

#[test]
fn max_scale_is_the_most_stretched_axis() {
    let m = Mat3::scale(1.0, 0.1);
    assert!((max_scale(&m) - 1.0).abs() < 1e-12);
    let r = Mat3::rotate(0.7).then(Mat3::scale(2.0, 2.0));
    assert!((max_scale(&r) - 2.0).abs() < 1e-12);
    assert!((max_scale(&Mat3::IDENTITY) - 1.0).abs() < 1e-12);
}

// ---------- transparency ----------

fn line_common(c: Common) -> (Common, EntityKind) {
    (c, EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }))
}

fn with_transparency() -> Options {
    Options { transparency: true, ..Default::default() }
}

fn block_entity(handle: u64, common: Common, kind: EntityKind) -> cadcraft_doc::Entity {
    let mut e = cadcraft_doc::Entity::new(cadcraft_doc::Handle(handle), kind);
    e.common = common;
    e
}

fn insert_of(block: &str) -> EntityKind {
    EntityKind::Insert(cadcraft_doc::Insert {
        block: block.into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

#[test]
fn transparency_resolves_by_object_layer_and_block() {
    use cadcraft_doc::Transparency::{ByBlock, ByLayer, Percent};
    let mut d = Drawing::new_imperial();
    d.layers.push(cadcraft_doc::Layer { name: "Walls".into(), transparency: 40, ..Default::default() });
    let add = |d: &mut Drawing, c: Common| {
        let (c, k) = line_common(c);
        d.add(&Space::Model, c, k).unwrap();
    };
    add(&mut d, Common { transparency: Percent(25), ..Default::default() });
    add(&mut d, Common { layer: "Walls".into(), transparency: ByLayer, ..Default::default() });
    // ByBlock outside a block: opaque.
    add(&mut d, Common { transparency: ByBlock, ..Default::default() });
    // Block "Inner": a ByBlock line and a ByLayer line on layer 0 (the reference's layer).
    let mut inner = cadcraft_doc::Block::new("Inner");
    let (c, k) = line_common(Common { transparency: ByBlock, ..Default::default() });
    inner.entities.push(block_entity(1, c, k));
    let (c, k) = line_common(Common::default());
    inner.entities.push(block_entity(2, c, k));
    d.blocks.insert("Inner".into(), std::sync::Arc::new(inner));
    // Block "Outer": a ByBlock reference to Inner, on layer Walls.
    let mut outer = cadcraft_doc::Block::new("Outer");
    outer.entities.push(block_entity(3, Common { layer: "Walls".into(), transparency: ByBlock, ..Default::default() }, insert_of("Inner")));
    d.blocks.insert("Outer".into(), std::sync::Arc::new(outer));
    d.add(&Space::Model, Common { transparency: Percent(70), ..Default::default() }, insert_of("Outer")).unwrap();
    let l = build(&d, &Space::Model, &with_transparency());
    let t: Vec<u8> = l.prims.iter().map(|p| p.transparency).collect();
    // Nested: Inner's ByBlock line takes Outer's reference (70, through the ByBlock reference to
    // Inner); its layer-0 line takes the reference's layer, Walls (40).
    assert_eq!(t, vec![25, 40, 0, 70, 40]);
    // Off (the default): everything is opaque, as before.
    assert!(build(&d, &Space::Model, &Options::default()).prims.iter().all(|p| p.transparency == 0 && p.alpha() == 255));
}

#[test]
fn hostile_transparency_values_are_clamped() {
    let mut d = Drawing::new_imperial();
    d.layers.push(cadcraft_doc::Layer { name: "Odd".into(), transparency: 250, ..Default::default() });
    let (c, k) = line_common(Common { transparency: cadcraft_doc::Transparency::Percent(u8::MAX), ..Default::default() });
    d.add(&Space::Model, c, k).unwrap();
    let (c, k) = line_common(Common { layer: "Odd".into(), ..Default::default() });
    d.add(&Space::Model, c, k).unwrap();
    // A ByLayer object on a layer that isn't in the table: opaque.
    let (c, k) = line_common(Common { layer: "Missing".into(), ..Default::default() });
    d.add(&Space::Model, c, k).unwrap();
    let l = build(&d, &Space::Model, &with_transparency());
    assert_eq!(l.prims.iter().map(|p| p.transparency).collect::<Vec<_>>(), vec![90, 90, 0]);
    assert_eq!((alpha(0), alpha(50), alpha(90), alpha(200)), (255, 128, 26, 26));
}

#[test]
fn transparent_solid_hatch_blends_with_the_line_under_it() {
    let mut d = Drawing::new_imperial();
    // The line runs mid-pixel (y = 2.025 is screen row 99.5) so it fills one row exactly.
    d.add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(-2.0, 2.025, 0.0), b: Vec3::new(6.0, 2.025, 0.0) }))
        .unwrap();
    let solid = Hatch { solid: true, pattern: "SOLID".into(), ..pattern_hatch("SOLID", 1.0, (0.0, 0.0, 4.0, 4.0)) };
    d.add(
        &Space::Model,
        Common { color: Color::Index(1), transparency: cadcraft_doc::Transparency::Percent(50), ..Default::default() },
        EntityKind::Hatch(solid),
    )
    .unwrap();
    let pixel = |o: &Options, x: f64, y: f64| {
        let l = build(&d, &Space::Model, o);
        let v = View { center: Vec2::new(2.0, 2.0), scale: 20.0, width: 200, height: 200 };
        let pm = render(&l, &v, &RasterOptions { background: Rgb(0, 0, 0), antialias: false, ..Default::default() }).unwrap();
        let (px, py) = v.to_screen(Vec2::new(x, y));
        let c = pm.pixel(px as u32, py as u32).unwrap();
        (c.red(), c.green(), c.blue())
    };
    let near = |a: (u8, u8, u8), b: (u8, u8, u8)| [(a.0, b.0), (a.1, b.1), (a.2, b.2)].iter().all(|(x, y)| x.abs_diff(*y) <= 2);
    // 50 %: half red over black, and half red over the white line, which shows through.
    let inside = pixel(&with_transparency(), 1.0, 1.0);
    assert!(near(inside, (128, 0, 0)), "{inside:?}");
    let on_line = pixel(&with_transparency(), 1.0, 2.025);
    assert!(near(on_line, (255, 127, 127)), "{on_line:?}");
    // Transparency off: the hatch hides the line, as before.
    assert_eq!(pixel(&Options::default(), 1.0, 2.025), (255, 0, 0));
}

fn attrib(tag: &str) -> cadcraft_doc::Attrib {
    cadcraft_doc::Attrib {
        tag: tag.into(),
        text: cadcraft_doc::Text {
            insert: Vec3::new(1.0, 1.0, 0.0),
            align_pt: None,
            height: 1.0,
            value: "A1".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        },
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    }
}

/// A block "B" with a line (layer 0, ByLayer) and an attribute definition with `def`'s
/// properties, and a reference to it with `reference`'s, carrying the attribute.
fn attributed(def: Common, reference: Common) -> Drawing {
    let mut d = Drawing::new_imperial();
    let mut blk = cadcraft_doc::Block::new("B");
    let (c, k) = line_common(Common::default());
    blk.entities.push(block_entity(1, c, k));
    blk.entities.push(block_entity(2, def, EntityKind::AttDef(attrib("TAG"))));
    d.blocks.insert("B".into(), std::sync::Arc::new(blk));
    let EntityKind::Insert(mut ins) = insert_of("B") else { panic!("an insert") };
    ins.attribs.push(attrib("TAG"));
    d.add(&Space::Model, reference, EntityKind::Insert(ins)).unwrap();
    d
}

/// The transparency of the block's line and of the attribute's text, from a list built with
/// transparency on (the line is drawn first, then the text).
fn line_and_text(d: &Drawing) -> (u8, Vec<u8>) {
    let l = build(d, &Space::Model, &with_transparency());
    let (first, rest) = l.prims.split_first().unwrap();
    assert!(!rest.is_empty(), "the attribute is drawn");
    (first.transparency, rest.iter().map(|p| p.transparency).collect())
}

#[test]
fn attributes_draw_with_their_own_transparency() {
    use cadcraft_doc::Transparency::{ByBlock, Percent};
    // A reference at 60 % on an opaque layer: layer-0 ByLayer contents (the line and the
    // attribute) take the reference's layer, so both are opaque.
    let d = attributed(Common::default(), Common { transparency: Percent(60), ..Default::default() });
    let (line, text) = line_and_text(&d);
    assert_eq!(line, 0);
    assert!(text.iter().all(|t| *t == 0), "{text:?}");
    // An attribute definition with its own 50 % in an opaque reference.
    let d = attributed(Common { transparency: Percent(50), ..Default::default() }, Common::default());
    let (line, text) = line_and_text(&d);
    assert_eq!(line, 0);
    assert!(text.iter().all(|t| *t == 50), "{text:?}");
    // ByBlock follows the reference.
    let d = attributed(Common { transparency: ByBlock, ..Default::default() }, Common { transparency: Percent(30), ..Default::default() });
    assert!(line_and_text(&d).1.iter().all(|t| *t == 30));
    // Off: everything opaque.
    assert!(build(&d, &Space::Model, &Options::default()).prims.iter().all(|p| p.transparency == 0));
}

#[test]
fn attributes_follow_the_reference_layer_and_have_no_definition() {
    use cadcraft_doc::Transparency::Percent;
    // A layer-0 ByLayer definition in a reference on a 40 % layer: the line and the attribute
    // both take the reference's layer.
    let mut d = attributed(Common::default(), Common { layer: "Glass".into(), ..Default::default() });
    // Adding the reference made the layer.
    d.layer_mut("Glass").unwrap().transparency = 40;
    let (line, text) = line_and_text(&d);
    assert_eq!(line, 40);
    assert!(text.iter().all(|t| *t == 40), "{text:?}");
    // No definition in the block: the attribute follows the reference (30 %), with or without
    // properties of its own.
    for props in [cadcraft_doc::AttribProps::default(), cadcraft_doc::AttribProps { color: Some(Color::Index(3)), ..Default::default() }] {
        let mut d = Drawing::new_imperial();
        let mut blk = cadcraft_doc::Block::new("B");
        let (c, k) = line_common(Common::default());
        blk.entities.push(block_entity(1, c, k));
        d.blocks.insert("B".into(), std::sync::Arc::new(blk));
        let EntityKind::Insert(mut ins) = insert_of("B") else { panic!("an insert") };
        ins.attribs.push(cadcraft_doc::Attrib { props: props.clone(), ..attrib("TAG") });
        d.add(&Space::Model, Common { transparency: Percent(30), ..Default::default() }, EntityKind::Insert(ins)).unwrap();
        let (line, text) = line_and_text(&d);
        assert_eq!(line, 0, "layer-0 ByLayer contents take the reference's opaque layer");
        assert!(text.iter().all(|t| *t == 30), "{props:?}: {text:?}");
    }
}

#[test]
fn mtext_exact_spacing_ignores_taller_lines() {
    // #711: a doubled middle line pushes the next baseline down with At least, not with Exactly.
    let mtext = |contents: &str, line_spacing: f64, line_spacing_exact: bool| cadcraft_doc::MText {
        insert: Vec3::ZERO,
        height: 1.0,
        width: 10.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing,
        line_spacing_exact,
        columns: None,
    };
    let d = Drawing::new_imperial();
    let height = |contents: &str, ls: f64, exact: bool| layout_mtext_entity(&d, &mtext(contents, ls, exact)).bounds.height();
    // The plain layout and the paragraph layout (`\p`) both honour the style.
    for mixed in ["A\\P{\\H2x;BB}\\PCC", "\\pxi1;A\\P{\\H2x;BB}\\PCC"] {
        assert!((height(mixed, 1.0, false) - (1.0 + 2.0 * 5.0 / 3.0 + 5.0 / 3.0)).abs() < 1e-9, "{mixed}: at least");
        assert!((height(mixed, 1.0, true) - (1.0 + 2.0 * 5.0 / 3.0)).abs() < 1e-9, "{mixed}: exactly");
        assert!((height(mixed, 2.0, true) - (1.0 + 2.0 * 5.0 / 3.0 * 2.0)).abs() < 1e-9, "{mixed}: exactly, factor 2");
    }
    for exact in [false, true] {
        assert!((height("A\\PBB\\PCC", 1.0, exact) - (1.0 + 2.0 * 5.0 / 3.0)).abs() < 1e-9, "uniform lines, exact {exact}");
    }
}

#[test]
fn table_cell_text_height_scales_with_the_table() {
    let height = |table_h: f64, own: Option<f64>| {
        let d = Drawing::new_imperial();
        let st = crate::table_style_of(&d, &cadcraft_doc::Table::default());
        let cell = cadcraft_doc::TableCell { text: "a".into(), text_height: own, ..Default::default() };
        let t = cadcraft_doc::Table {
            col_widths: vec![500.0],
            row_heights: vec![500.0],
            cells: vec![vec![cell]],
            text_height: table_h * st.text_height,
            ..Default::default()
        };
        crate::table_geometry(&d, &t).texts.first().map(|x| x.height)
    };
    let base = height(1.0, None).unwrap();
    assert_eq!(height(1.0, Some(150.0)), Some(150.0));
    assert_eq!(height(2.0, Some(150.0)), Some(300.0));
    assert_eq!(height(2.0, None), Some(base * 2.0));
}

#[test]
fn cancelled_build_stops_early() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let d = sample();
    let set = Some(Arc::new(AtomicBool::new(true)));
    let full = build(&d, &Space::Model, &Options::default());
    assert!(!full.prims.is_empty());
    let opts = Options { cancel: set.clone(), ..Default::default() };
    assert!(opts.cancelled());
    assert!(build(&d, &Space::Model, &opts).prims.is_empty(), "no entity is drawn once cancelled");
    // A hatch sweep stops at its next pattern line.
    let h = pattern_hatch("ANSI31", 1.0, (0.0, 0.0, 6.0, 4.0));
    let loops = loops_of(&h);
    let drawing = cadcraft_doc::Drawing::new_imperial();
    assert!(!crate::hatch::pattern_lines_until(&drawing, &h, &loops, 0.0, None, None).is_empty());
    assert!(crate::hatch::pattern_lines_until(&drawing, &h, &loops, 0.0, None, set.as_deref()).is_empty());
}

/// #1434: a closed polyline with a global width narrower than a pixel loses whole sides at
/// some zooms, while the same polyline with width 0 always shows every side.
#[test]
fn thin_wide_polyline_keeps_every_side_at_every_zoom() {
    let square = |const_width: f64| {
        let mut d = Drawing::new_imperial();
        let vertices = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec();
        let p = cadcraft_doc::LwPolyline { vertices, closed: true, const_width, elevation: 0.0, plinegen: false };
        d.add(&Space::Model, Common::default(), EntityKind::LwPolyline(p)).unwrap();
        d
    };
    // Side midpoints and the direction across each side.
    let sides = [("bottom", Vec2::new(5.0, 0.0)), ("right", Vec2::new(10.0, 5.0)), ("top", Vec2::new(5.0, 10.0)), ("left", Vec2::new(0.0, 5.0))];
    let mut missing = Vec::new();
    for const_width in [0.0, 0.01, 0.05] {
        let d = square(const_width);
        for step in 0..40 {
            let scale = 20.0 + f64::from(step) * 0.37;
            let px = 1.0 / scale;
            let l = build(&d, &Space::Model, &screen(px, None));
            let v = View { center: Vec2::new(5.17, 4.62), scale, width: 400, height: 400 };
            // No anti-aliasing: pixel-centre coverage, as the GPU canvas rasterises (no MSAA).
            let pm = render(&l, &v, &RasterOptions { background: Rgb(0, 0, 0), antialias: false, ..Default::default() }).unwrap();
            for (name, mid) in sides {
                let (sx, sy) = v.to_screen(mid);
                let lit = (-2i32..=2)
                    .flat_map(|dy| (-2i32..=2).map(move |dx| (dx, dy)))
                    .any(|(dx, dy)| pm.pixel((sx as i32 + dx) as u32, (sy as i32 + dy) as u32).is_some_and(|c| c.red() > 0));
                if !lit {
                    missing.push(format!("width {const_width} scale {scale:.2} ({:.2} px wide): {name} side missing", const_width * scale));
                }
            }
        }
    }
    assert!(missing.is_empty(), "{} misses:\n{}", missing.len(), missing.join("\n"));
}

/// #1434: on screen a wide polyline also draws its centre line (a hairline in its colour, solid and
/// dashed alike); plots and exports (`min_dash` 0) draw the band alone.
#[test]
fn wide_polyline_centre_line_is_drawn_on_screen_only() {
    for lt in ["Continuous", "DASHED"] {
        let drawing = |transparency: u8| {
            let mut d = Drawing::new_imperial();
            d.linetypes.extend(cadcraft_doc::library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
            let vertices = [(0.0, 0.0), (3.0, 0.0), (3.0, 3.0)].map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).to_vec();
            let p = cadcraft_doc::LwPolyline { vertices, closed: false, const_width: 0.2, elevation: 0.0, plinegen: false };
            let common = Common {
                color: Color::Index(1),
                linetype: lt.into(),
                lineweight: cadcraft_doc::Lineweight::Mm100(50),
                transparency: cadcraft_doc::Transparency::Percent(transparency),
                ..Default::default()
            };
            d.add(&Space::Model, common, EntityKind::LwPolyline(p)).unwrap();
            d
        };
        let d = drawing(0);
        let opts = Options { lineweights: true, transparency: true, ..screen(0.01, None) };
        let lines = |l: &DisplayList| l.prims.iter().filter(|p| p.kind == Kind::Polyline).count();
        let on_screen = build(&d, &Space::Model, &opts);
        assert!(on_screen.prims.iter().any(|p| p.kind == Kind::Tris), "{lt}: the band");
        let hair: Vec<&DPrim> = on_screen.prims.iter().filter(|p| p.kind == Kind::Polyline).collect();
        assert!(!hair.is_empty(), "{lt}: the centre line on screen");
        assert!(hair.iter().all(|p| p.color == Rgb(255, 0, 0) && p.lw == 0.0), "{lt}: hairline in the polyline's colour");
        assert!(on_screen.points(hair[0]).iter().all(|q| q.y.abs() < 1e-9 || (q.x - 3.0).abs() < 1e-9), "{lt}: on the centre line");
        assert_eq!(lines(&build(&d, &Space::Model, &Options { min_dash: 0.0, ..opts.clone() })), 0, "{lt}: exports draw the band alone");
        assert_eq!(lines(&build_plot(&d, &Space::Model, &opts)), 0, "{lt}: plots draw the band alone");
        // Its alpha would add up along the centre: a transparent band gets no hairline.
        let see_through = build(&drawing(50), &Space::Model, &opts);
        assert!(see_through.prims.iter().any(|p| p.kind == Kind::Tris && p.transparency == 50), "{lt}: the transparent band");
        assert_eq!(lines(&see_through), 0, "{lt}: a transparent band gets no hairline");
    }
}

/// Each primitive stroked or filled on its own, as `render` did before it batched hairlines.
fn render_one_by_one(list: &DisplayList, view: &View, o: &RasterOptions) -> tiny_skia::Pixmap {
    use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let mut pm = Pixmap::new(view.width, view.height).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(o.background.0, o.background.1, o.background.2, 255));
    for p in &list.prims {
        let c = p.display_rgb(o.background);
        let mut paint = Paint::default();
        paint.set_color_rgba8(c.0, c.1, c.2, p.alpha());
        paint.anti_alias = o.antialias;
        let mut pb = PathBuilder::new();
        match p.kind {
            Kind::Polyline => {
                for (i, q) in list.points(p).iter().enumerate() {
                    let (x, y) = view.to_screen(*q);
                    if i == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                if let Some(path) = pb.finish() {
                    let w = if o.px_per_mm > 0.0 && p.lw > 0.0 { (p.lw * o.px_per_mm).max(o.hairline) } else { o.hairline };
                    let stroke = Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Kind::Tris => {
                for t in list.points(p).chunks(3) {
                    let [a, b, c] = t else { continue };
                    let ((ax, ay), (bx, by), (cx, cy)) = (view.to_screen(*a), view.to_screen(*b), view.to_screen(*c));
                    pb.move_to(ax, ay);
                    pb.line_to(bx, by);
                    pb.line_to(cx, cy);
                    pb.close();
                }
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
            _ => panic!("not drawn by this reference"),
        }
    }
    pm
}

fn push_prim(l: &mut DisplayList, kind: Kind, color: Rgb, transparency: u8, lw: f32, pts: &[Vec2]) {
    let arr = if kind == Kind::Tris { &mut l.tris } else { &mut l.verts };
    let start = arr.len() as u32;
    arr.extend_from_slice(pts);
    for p in pts.iter().filter(|p| p.is_finite() && p.x.abs() < 1e6) {
        l.bounds.add(*p);
    }
    l.prims.push(DPrim { handle: VIEWPORT_CONTENT, color, aci7: false, lw, transparency, kind, start, len: pts.len() as u32 });
}

/// Hairline polylines with the same paint are stroked as one path; the pixels must be exactly
/// those of stroking each on its own, whatever breaks a run (colour, transparency, lineweight,
/// fills, unusable coordinates) and with overlaps, caps and dots.
#[test]
fn raster_hairline_batches_match_one_by_one() {
    let red = Rgb(255, 0, 0);
    let blue = Rgb(0, 0, 255);
    let mut l = DisplayList::default();
    // A hatch-like run of crossing, overlapping and touching lines in one colour.
    for i in 0..40 {
        let x = f64::from(i) * 0.37;
        push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(x, 0.0), Vec2::new(x + 6.0, 9.0)]);
        push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(x + 3.0, 0.0), Vec2::new(x - 2.0, 9.0), Vec2::new(x + 1.0, 4.0)]);
    }
    // Zero-length (a dot), single-point (nothing), and repeated lines.
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(2.0, 2.0), Vec2::new(2.0, 2.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(3.0, 3.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(1.0, 1.0), Vec2::new(8.0, 2.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(1.0, 1.0), Vec2::new(8.0, 2.0)]);
    // Transparent lines build up where they overlap, within a run and across runs.
    for i in 0..10 {
        let y = f64::from(i) * 0.2;
        push_prim(&mut l, Kind::Polyline, blue, 50, 0.0, &[Vec2::new(0.0, y), Vec2::new(12.0, 8.0 - y)]);
    }
    push_prim(&mut l, Kind::Polyline, blue, 20, 0.0, &[Vec2::new(0.0, 5.0), Vec2::new(12.0, 5.0)]);
    // A lineweight (a filled outline when lineweights are shown), a fill, then the run again.
    push_prim(&mut l, Kind::Polyline, red, 0, 0.5, &[Vec2::new(0.0, 6.0), Vec2::new(12.0, 7.0), Vec2::new(5.0, 2.0)]);
    push_prim(&mut l, Kind::Tris, blue, 30, 0.0, &[Vec2::new(4.0, 4.0), Vec2::new(9.0, 4.0), Vec2::new(6.0, 8.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(0.0, 8.0), Vec2::new(12.0, 1.0)]);
    // Unusable or far-out coordinates are stroked on their own, as before.
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(1.0, 1.0), Vec2::new(f64::NAN, 3.0), Vec2::new(9.0, 9.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(-1e12, 4.0), Vec2::new(1e12, 4.5)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(5.0, -1e7), Vec2::new(5.5, 1e7)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(0.0, 0.5), Vec2::new(12.0, 0.5)]);
    // Partly and wholly off screen.
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(-30.0, -30.0), Vec2::new(40.0, 30.0)]);
    push_prim(&mut l, Kind::Polyline, red, 0, 0.0, &[Vec2::new(100.0, 100.0), Vec2::new(120.0, 101.0)]);
    let view = View::fit(&l.bounds, 160, 120, 0.05);
    let bg = Rgb(33, 40, 48);
    for o in [
        RasterOptions::default(),
        RasterOptions { antialias: false, ..Default::default() },
        RasterOptions { hairline: 0.5, ..Default::default() },
        RasterOptions { hairline: 0.0, antialias: false, ..Default::default() },
        RasterOptions { px_per_mm: 4.0, background: Rgb(255, 255, 255), ..Default::default() },
        RasterOptions { hairline: 1.5, background: bg, ..Default::default() },
    ] {
        let fast = render(&l, &view, &o).unwrap();
        let reference = render_one_by_one(&l, &view, &o);
        assert!(fast.data() == reference.data(), "pixels differ with {o:?}");
    }
}

/// A real hatch pattern (many parallel lines in one colour) renders exactly as stroked line by
/// line.
#[test]
fn raster_hatch_pattern_matches_one_by_one() {
    let mut d = sample();
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(pattern_hatch("ANSI31", 0.2, (0.0, 0.0, 10.0, 10.0)))).unwrap();
    let l = build(&d, &Space::Model, &Options::default());
    assert!(l.prims.len() > 20);
    let view = View::fit(&l.bounds, 300, 200, 0.05);
    for o in [RasterOptions::default(), RasterOptions { background: Rgb(255, 255, 255), ..Default::default() }] {
        assert!(render(&l, &view, &o).unwrap().data() == render_one_by_one(&l, &view, &o).data());
    }
}
