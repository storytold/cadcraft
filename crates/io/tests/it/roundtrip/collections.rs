//! Drawing-collection contracts beyond model space. Assertions cover explicitly authored data,
//! not generated timestamps/extents, allocation seeds or extra implementation-owned objects.
//! Named tables are compared by identity; paper entities retain order and handles. These are
//! focused collection contracts, not a claim that every Drawing/session field is persistent.

use super::{
    cases::{self, Case},
    compare,
};
use cadcraft_color::Color;
use cadcraft_doc::{
    Common, Drawing, EntityKind, Group, HVal, Layer, LayerState, Layout, Line, Linetype, NamedView, Space, TextStyle, Ucs, Viewport, VportConfig,
    VportTile,
};
use cadcraft_geom::{Vec2, Vec3};

pub(super) fn foreign_records() -> Case {
    use cadcraft_doc::{Handle, RawTag, foreign::ForeignObject};
    let mut drawing = cases::drawing();
    let foreign = std::sync::Arc::make_mut(&mut drawing.foreign);
    foreign.dictionary.push(("SYNTHETIC_PAYLOAD".into(), Handle(0xA000)));
    foreign.targets.insert(Handle(0xA001), ("NOD".into(), String::new()));
    foreign.objects.push(ForeignObject {
        handle: Handle(0xA000),
        kind: "XRECORD".into(),
        tags: [
            (5, "A000"),
            (330, "A001"),
            (100, "AcDbXrecord"),
            (280, "1"),
            (1, "Original Ø payload"),
            (90, "-7"),
            (310, "00FF80"),
            (10, "1.25"),
            (20, "-2.5"),
            (30, "3.75"),
        ]
        .map(|(code, value)| RawTag { code, value: value.into() })
        .into(),
    });
    drawing.handseed = 0xA100;
    Case { name: "drawing/foreign-xrecord".into(), drawing }
}

pub(super) fn assert_foreign_records(a: &Drawing, b: &Drawing, context: &str) {
    // Rereads may add implementation-owned dictionaries; this case owns one explicit key.
    let key = "SYNTHETIC_PAYLOAD";
    let (_, handle) = a.foreign.dictionary.iter().find(|(name, _)| name == key).expect("authored dictionary entry");
    let (_, target) = b.foreign.dictionary.iter().find(|(name, _)| name == key).unwrap_or_else(|| panic!("{context}.dictionary[{key}]: missing key"));
    let source = a.foreign.objects.iter().find(|o| o.handle == *handle).unwrap();
    let back = b.foreign.objects.iter().find(|o| o.handle == *target).expect("dictionary target must resolve");
    assert_eq!(back.kind, source.kind, "{context}.object.kind");
    let owner = back.tags.iter().find(|t| t.code == 330).and_then(|t| cadcraft_doc::Handle::parse_hex(&t.value)).expect("XRECORD owner");
    assert_eq!(b.foreign.targets.get(&owner), Some(&("NOD".into(), String::new())), "{context}.object.owner");
    // Foreign's documented contract reallocates object handles and remaps references.
    // Identity here is the NOD key and its resolved target, not the numeric allocation.
    // Preserve every payload tag (including its order) after the subclass marker.
    fn payload(tags: &[cadcraft_doc::RawTag]) -> &[cadcraft_doc::RawTag] {
        let start =
            tags.iter().position(|t| t.code == 100 && t.value == "AcDbXrecord").unwrap_or_else(|| panic!("missing XRECORD subclass: {tags:?}"));
        &tags[start + 1..]
    }
    assert_eq!(payload(&back.tags), payload(&source.tags), "{context}.object.payload");
}

const HEADER_KEYS: &[&str] = &["LTSCALE", "CELTSCALE", "LUPREC", "AUPREC", "INSBASE", "TEXTSIZE", "CLAYER", "TEXTSTYLE"];

pub(super) fn headers() -> Case {
    let mut drawing = cases::drawing();
    drawing.header.set_f64("LTSCALE", 2.75);
    drawing.header.set_f64("CELTSCALE", 0.125);
    drawing.header.set_i64("LUPREC", 7);
    drawing.header.set_i64("AUPREC", 3);
    drawing.header.set("INSBASE", HVal::Point(Vec3::new(1.25, -2.5, 3.75)));
    drawing.header.set_f64("TEXTSIZE", 3.25);

    Case { name: "drawing/header-variables".into(), drawing }
}

pub(super) fn named_header_reference(key: &str) -> Case {
    let mut case = headers();
    case.name = format!("drawing/header-reference/{key}");
    case.drawing.header.set_str(
        key,
        match key {
            "CLAYER" => "Survey_Ä",
            "TEXTSTYLE" => "Notes",
            _ => panic!("unknown header reference"),
        },
    );
    case
}

pub(super) fn assert_headers(a: &Drawing, b: &Drawing, context: &str) {
    for key in HEADER_KEYS {
        assert_eq!(b.header.get(key), a.header.get(key), "{context}.header.{key}");
    }
}

pub(super) fn symbol_tables() -> Case {
    let mut drawing = cases::drawing();
    drawing.layers.push(Layer {
        name: "UnusedLayer".into(),
        color: Color::Index(5),
        locked: true,
        description: "Unused Ø".into(),
        ..Layer::default()
    });
    drawing.linetypes.push(Linetype::simple("UnusedDash", "Unused synthetic", &[3.0, -0.5, 0.0, -0.25]));
    drawing.text_styles.push(TextStyle { name: "UnusedStyle".into(), font: "SyntheticSans.ttf".into(), width_factor: 1.25, ..TextStyle::default() });
    Case { name: "drawing/unreferenced-tables".into(), drawing }
}

pub(super) fn assert_symbol_tables(a: &Drawing, b: &Drawing, context: &str) {
    for layer in &a.layers {
        assert_eq!(b.layer(&layer.name), Some(layer), "{context}.layer[{}]", layer.name);
    }
    for linetype in &a.linetypes {
        assert_eq!(b.linetype(&linetype.name), Some(linetype), "{context}.linetype[{}]", linetype.name);
    }
    for style in &a.text_styles {
        assert_eq!(b.text_style(&style.name), Some(style), "{context}.style[{}]", style.name);
    }
}

pub(super) fn views_and_ucs() -> Case {
    let mut drawing = cases::drawing();
    drawing.views.push(NamedView { name: "SurveyView".into(), center: Vec2::new(1.25, -2.5), height: 25.0, width: 40.0, ..NamedView::default() });
    drawing.ucss.push(Ucs {
        name: "RotatedUCS".into(),
        origin: Vec3::new(3.0, 4.0, 5.0),
        x_axis: Vec3::new(0.0, 1.0, 0.0),
        y_axis: Vec3::new(-1.0, 0.0, 0.0),
    });
    Case { name: "drawing/views-and-ucs".into(), drawing }
}

pub(super) fn assert_views_and_ucs(a: &Drawing, b: &Drawing, context: &str) {
    assert_eq!(b.views, a.views, "{context}.views");
    assert_eq!(b.ucss, a.ucss, "{context}.ucss");
}

pub(super) fn vports() -> Case {
    let mut drawing = cases::drawing();
    drawing.vports.push(VportConfig {
        name: "Split".into(),
        tiles: vec![
            VportTile { min: Vec2::ZERO, max: Vec2::new(0.5, 1.0), center: Vec2::new(2.0, 3.0), height: 20.0, aspect: 0.75 },
            VportTile { min: Vec2::new(0.5, 0.0), max: Vec2::new(1.0, 1.0), center: Vec2::new(7.0, 8.0), height: 30.0, aspect: 0.75 },
        ],
    });
    Case { name: "drawing/tiled-viewports".into(), drawing }
}

pub(super) fn assert_vports(a: &Drawing, b: &Drawing, context: &str) {
    for viewport in &a.vports {
        assert_eq!(b.vports.iter().find(|v| v.name == viewport.name), Some(viewport), "{context}.vport[{}]", viewport.name);
    }
}

pub(super) fn groups() -> Case {
    let mut case = cases::common_case("drawing/groups", Common::default());
    let handles: Vec<_> = case.drawing.model.iter().map(|e| e.handle).collect();
    case.drawing.groups.push(Group {
        name: "Synthetic".into(),
        description: "Ordered overlapping membership Ø".into(),
        selectable: true,
        members: vec![handles[2], handles[0]],
    });
    case.drawing.groups.push(Group { name: "Other".into(), description: String::new(), selectable: false, members: vec![handles[1], handles[2]] });
    case
}

pub(super) fn assert_groups(a: &Drawing, b: &Drawing, context: &str) {
    assert_eq!(b.groups.len(), a.groups.len(), "{context}.groups.len");
    for group in &a.groups {
        assert_eq!(b.groups.iter().find(|g| g.name == group.name), Some(group), "{context}.group[{}]", group.name);
    }
}

pub(super) fn layer_states() -> Case {
    let mut drawing = cases::drawing();
    let mut layers = drawing.layers.clone();
    for layer in &mut layers {
        layer.on = false;
        layer.locked = true;
    }
    drawing.layer_states.push(LayerState { name: "Saved".into(), layers });
    Case { name: "drawing/layer-state".into(), drawing }
}

pub(super) fn assert_layer_states(a: &Drawing, b: &Drawing, context: &str) {
    assert_eq!(b.layer_states, a.layer_states, "{context}.layer_states");
}

pub(super) fn layouts() -> Case {
    let mut drawing = cases::drawing();
    drawing.layouts = vec![Layout::new("Sheet_A", 1), Layout::new("Sheet_B", 2)];
    for (index, layout) in drawing.layouts.iter_mut().enumerate() {
        layout.page.paper_mm = true;
        // PageSetup stores portrait dimensions; landscape is a separate orientation flag.
        layout.page.width_mm = 297.0;
        layout.page.height_mm = 420.0;
        layout.page.scale = 0.5;
        layout.page.plot_area = "window".into();
        layout.page.plot_window = [1.0, 2.0, 201.0, 142.0];
        layout.view = Some((Vec2::new(10.0, 20.0), 100.0 + index as f64));
    }
    drawing.active_layout = Some("Sheet_B".into());
    for name in ["Sheet_A", "Sheet_B"] {
        drawing.add(&Space::Paper(name.into()), Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 20.0, 0.0) })).unwrap();
        drawing
            .add(
                &Space::Paper(name.into()),
                Common::default(),
                EntityKind::Viewport(Viewport {
                    center: Vec3::new(50.0, 60.0, 0.0),
                    width: 80.0,
                    height: 50.0,
                    view_center: Vec2::new(5.0, 7.0),
                    view_height: 25.0,
                    id: 2,
                    locked: true,
                    frozen_layers: vec!["Survey_Ä".into()],
                    ..Viewport::default()
                }),
            )
            .unwrap();
    }
    Case { name: "drawing/layouts-and-paper-entities".into(), drawing }
}

#[test]
fn collection_assertions_reject_lost_authored_data() {
    type Check = fn(&Drawing, &Drawing, &str);
    type Mutate = fn(&mut Drawing);
    let cases: [(Case, Check, Mutate); 6] = [
        (headers(), assert_headers, |d| d.header.set_f64("TEXTSIZE", 9.0)),
        (symbol_tables(), assert_symbol_tables, |d| d.layers.retain(|l| l.name != "UnusedLayer")),
        (views_and_ucs(), assert_views_and_ucs, |d| d.ucss.clear()),
        (groups(), assert_groups, |d| d.groups.first_mut().unwrap().members.reverse()),
        (layouts(), assert_layouts, |d| d.layouts.first_mut().unwrap().page.scale = 2.0),
        (foreign_records(), assert_foreign_records, |d| std::sync::Arc::make_mut(&mut d.foreign).objects.clear()),
    ];
    for (case, verify, mutate) in cases {
        verify(&case.drawing, &case.drawing, &case.name);
        let mut changed = case.drawing.clone();
        mutate(&mut changed);
        assert!(std::panic::catch_unwind(|| verify(&case.drawing, &changed, &case.name)).is_err(), "{}: lost data must fail", case.name);
    }
}

pub(super) fn assert_layouts(a: &Drawing, b: &Drawing, context: &str) {
    assert_eq!(b.layouts.len(), a.layouts.len(), "{context}.layouts.len");
    assert_eq!(b.active_layout, a.active_layout, "{context}.active_layout");
    for layout in &a.layouts {
        let back = b.layouts.iter().find(|l| l.name == layout.name).expect("layout must survive");
        assert_eq!(back.tab_order, layout.tab_order, "{context}.{}.tab_order", layout.name);
        assert_eq!(back.page, layout.page, "{context}.{}.page", layout.name);
        assert_eq!(back.view, layout.view, "{context}.{}.view", layout.name);
        compare::entities(a, b, &layout.entities, &back.entities, &format!("{context}.layout[{}]", layout.name));
    }
}
