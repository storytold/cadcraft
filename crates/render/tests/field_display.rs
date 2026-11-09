//! Fields are drawn as their values, on a grey background while FIELDDISPLAY is 1 and never
//! when plotting (#532).

use cadcraft_doc::fields;
use cadcraft_doc::{Common, Drawing, EntityKind, HVal, Space, Table, TableCell, Text};
use cadcraft_geom::Vec3;
use cadcraft_render::{Options, build, build_plot};

fn drawing(value: &str) -> Drawing {
    let mut d = Drawing::new_metric();
    let t = Text {
        insert: Vec3::new(0.0, 0.0, 0.0),
        align_pt: None,
        height: 2.5,
        value: value.into(),
        rotation: 0.3,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Text(t.clone())).unwrap();
    let mt = serde_json::from_value(serde_json::json!({"insert": {"x": 0.0, "y": 10.0, "z": 0.0}, "height": 2.5, "contents": value})).unwrap();
    d.add(&Space::Model, Common::default(), EntityKind::MText(mt)).unwrap();
    d
}

#[test]
fn values_on_a_grey_background_on_screen_only() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    let with_field = drawing(&format!("A {code}"));
    let plain = drawing("A PLATE");
    let o = Options::default();
    let f = build(&with_field, &Space::Model, &o);
    let p = build(&plain, &Space::Model, &o);
    // The same glyphs as the plain value, plus a grey box (two triangles) per text.
    assert_eq!(f.verts.len(), p.verts.len());
    assert_eq!(f.tris.len(), p.tris.len() + 12);
    assert!(build_plot(&with_field, &Space::Model, &o).tris.len() == p.tris.len());
    let mut off = with_field.clone();
    off.header.set("FIELDDISPLAY", HVal::Int(0));
    assert_eq!(build(&off, &Space::Model, &o).tris.len(), p.tris.len());
}

#[test]
fn layout_matching_fields_keep_glyphs_and_background_upright() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    let upright = |value: &str, rotation: f64| {
        let mut d = drawing(value);
        let entities: Vec<_> = d.model.iter().map(|e| (**e).clone()).collect();
        for mut e in entities {
            e.common.xdata =
                vec![cadcraft_doc::RawTag { code: 1001, value: "AcadAnnoPO".into() }, cadcraft_doc::RawTag { code: 1070, value: "1".into() }];
            if let EntityKind::Text(t) = &mut e.kind {
                t.rotation = rotation;
            }
            if let EntityKind::MText(t) = &mut e.kind {
                t.rotation = rotation;
            }
            d.model.push(e);
        }
        d
    };
    let points = |d: &Drawing, plot: bool| {
        let list = if plot { build_plot(d, &Space::Model, &Options::default()) } else { build(d, &Space::Model, &Options::default()) };
        list.prims.iter().flat_map(|p| list.points(p).iter().copied()).collect::<Vec<_>>()
    };
    let rotated = upright(&code, std::f64::consts::PI);
    let zero = upright(&code, 0.0);
    let plain = upright("PLATE", 0.0);
    assert!(!points(&rotated, false).is_empty());
    assert_eq!(points(&rotated, false), points(&zero, false), "glyphs and field backgrounds ignore stored rotation");
    assert_eq!(points(&rotated, true), points(&plain, true), "plotting draws the displayed field value upright without a background");
}

/// The grey box is measured from the glyphs where they are drawn: vertical text gets a tall box
/// that contains every glyph point (#551).
#[test]
fn background_follows_rotated_text_and_attributes() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    for deg in [0.0_f64, 90.0, 30.0] {
        let mut d = Drawing::new_metric();
        let t = Text {
            insert: Vec3::new(5.0, 7.0, 0.0),
            align_pt: None,
            height: 2.5,
            value: format!("A {code}"),
            rotation: deg.to_radians(),
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        };
        d.add(&Space::Model, Common::default(), EntityKind::Text(t)).unwrap();
        let list = build(&d, &Space::Model, &Options::default());
        // The background is drawn first: two triangles; the glyphs follow.
        let grey: Vec<_> = list.tris[..6].to_vec();
        let (gx0, gx1) = grey.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.x), a.1.max(p.x)));
        let (gy0, gy1) = grey.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.y), a.1.max(p.y)));
        if deg == 90.0 {
            assert!(gy1 - gy0 > 2.0 * (gx1 - gx0), "vertical text needs a tall box: {grey:?}");
        }
        let glyphs: Vec<_> = list.prims.iter().skip(1).flat_map(|p| list.points(p).iter().copied()).collect();
        assert!(!glyphs.is_empty());
        let eps = 1e-6;
        // A rotated box is not axis-aligned, so test containment in its own frame.
        let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
        let local = |p: cadcraft_geom::Vec2| (p.x * c + p.y * s, -p.x * s + p.y * c);
        let (lx0, lx1, ly0, ly1) = grey.iter().map(|p| local(*p)).fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |a, (x, y)| {
            (a.0.min(x), a.1.max(x), a.2.min(y), a.3.max(y))
        });
        for g in glyphs {
            let (x, y) = local(g);
            assert!(x >= lx0 - eps && x <= lx1 + eps && y >= ly0 - eps && y <= ly1 + eps, "{deg} deg: glyph point {g:?} outside the box");
        }
    }
}

/// A table cell with a field draws the field's cached value, on screen and plotted, as the same
/// cell with the plain value does.
#[test]
fn table_cells_draw_the_cached_value_of_a_field() {
    let (code, _) = fields::set_values(&fields::var_code("Title", None), |_| Some("PLATE".into()));
    let table = |text: &str| {
        let mut d = Drawing::new_metric();
        let t = Table {
            insert: Vec3::new(0.0, 0.0, 0.0),
            col_widths: vec![40.0],
            row_heights: vec![8.0],
            cells: vec![vec![TableCell { text: text.into(), ..Default::default() }]],
            style: "Standard".into(),
            text_height: 2.5,
            title: false,
            header: false,
            row_styles: Vec::new(),
            breaks: None,
            data_link: None,
        };
        d.add(&Space::Model, Common::default(), EntityKind::Table(t)).unwrap();
        d
    };
    let with_field = table(&format!("A {code}"));
    let plain = table("A PLATE");
    let o = Options::default();
    let points = |l: &cadcraft_render::DisplayList| l.prims.iter().flat_map(|p| l.points(p).iter().copied()).collect::<Vec<_>>();
    let (f, p) = (build(&with_field, &Space::Model, &o), build(&plain, &Space::Model, &o));
    assert_eq!(points(&f), points(&p), "screen");
    assert_eq!(points(&build_plot(&with_field, &Space::Model, &o)), points(&build_plot(&plain, &Space::Model, &o)), "plot");
    // And the field code is not drawn: the plain code text would be many more strokes.
    let raw = table(&format!("A {code}").replace("%<", "").replace(">%", ""));
    assert_ne!(points(&f).len(), points(&build(&raw, &Space::Model, &o)).len());
}
