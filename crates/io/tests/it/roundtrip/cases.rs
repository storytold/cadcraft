//! Deterministic drawing inputs shared by DXF and DWG tests, independent of either writer.
//! Each family isolates a property so a known failure need not disable unrelated coverage.
//! Format/version limits and pending failures belong to the callers, not these generators.

pub(super) mod blocks;
pub(super) mod complex;
pub(super) mod geometry;
pub(super) mod properties;

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    AnnoScale, Circle, Common, Drawing, EntityKind, HAlign, Layer, Line, Linetype, Lineweight, RawTag, Space, Text, TextStyle, Transparency, VAlign,
};
use cadcraft_geom::Vec3;

pub(super) struct Case {
    pub name: String,
    pub drawing: Drawing,
}

pub(super) fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Survey_Ä".into(), color: Color::Index(3), ..Layer::default() });
    d.linetypes.push(Linetype::simple("Dash_test", "Original synthetic dash pattern", &[2.0, -0.75, 0.0, -0.25]));
    d.text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    d
}

pub(super) fn text() -> Text {
    Text {
        insert: Vec3::new(2.5, -3.75, 4.125),
        align_pt: None,
        height: 2.25,
        value: "CAD Ø".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    }
}

pub(super) fn entity_case(name: &str, common: Common, kind: EntityKind) -> Case {
    let mut drawing = drawing();
    drawing.add(&Space::Model, common, kind).unwrap();
    Case { name: name.into(), drawing }
}

pub(super) fn common_case(name: &str, common: Common) -> Case {
    let mut drawing = drawing();
    for kind in [
        EntityKind::Line(Line { a: Vec3::new(-1.25, 2.5, 3.75), b: Vec3::new(8.125, -13.5, 21.25) }),
        EntityKind::Circle(Circle { center: Vec3::new(-7.25, 11.5, 4.75), radius: 3.125 }),
        EntityKind::Text(text()),
    ] {
        drawing.add(&Space::Model, common.clone(), kind).unwrap();
    }
    Case { name: name.into(), drawing }
}

pub(super) fn empty_and_defaults() -> Vec<Case> {
    vec![Case { name: "empty-model".into(), drawing: drawing() }, common_case("common/default", Common::default())]
}

pub(super) fn geometry() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, a, b) in [
        ("zero-start", Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)),
        ("zero-end", Vec3::new(-3.0, -2.0, -1.0), Vec3::ZERO),
        ("small", Vec3::new(1e-9, -2e-9, 3e-9), Vec3::new(-4e-9, 5e-9, -6e-9)),
        ("survey", Vec3::new(1e7 + 0.125, -2e7 - 0.25, 123.75), Vec3::new(1e7 + 0.5, -2e7 + 0.75, -42.5)),
    ] {
        for (direction, start, end) in [("forward", a, b), ("reverse", b, a)] {
            cases.push(entity_case(&format!("line/{name}/{direction}"), Common::default(), EntityKind::Line(Line { a: start, b: end })));
        }
        for radius in [1e-9, 0.125, 3.25, 1e8] {
            cases.push(entity_case(&format!("circle/{name}/radius={radius}"), Common::default(), EntityKind::Circle(Circle { center: a, radius })));
        }
    }
    cases
}

pub(super) fn layers() -> Vec<Case> {
    ["0", "Survey_Ä"].map(|layer| common_case(&format!("common/layer={layer}"), Common { layer: layer.into(), ..Common::default() })).into()
}

pub(super) fn layer_linetypes() -> Vec<Case> {
    ["0", "Survey_Ä"]
        .map(|layer| {
            let mut case = common_case(&format!("common/layer={layer}/inherited-linetype"), Common { layer: layer.into(), ..Common::default() });
            case.drawing.layer_mut(layer).unwrap().linetype = "Dash_test".into();
            case
        })
        .into()
}

pub(super) fn colors() -> Vec<Case> {
    [Color::ByLayer, Color::ByBlock]
        .into_iter()
        .chain((1..=255).map(Color::Index))
        .chain([Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(255, 0, 0), Rgb(0, 255, 0), Rgb(0, 0, 255), Rgb(17, 93, 241)].map(Color::True))
        .map(|color| common_case(&format!("common/color={color:?}"), Common { color, ..Common::default() }))
        .collect()
}

pub(super) fn linetypes() -> Vec<Case> {
    ["ByLayer", "ByBlock", "Continuous", "Dash_test"]
        .map(|linetype| common_case(&format!("common/linetype={linetype}"), Common { linetype: linetype.into(), ..Common::default() }))
        .into()
}

pub(super) fn lineweights() -> Vec<Case> {
    [Lineweight::ByLayer, Lineweight::ByBlock, Lineweight::Default]
        .into_iter()
        .chain(Lineweight::STANDARD.map(Lineweight::Mm100))
        .map(|lineweight| common_case(&format!("common/lineweight={lineweight:?}"), Common { lineweight, ..Common::default() }))
        .collect()
}

pub(super) fn linetype_scales() -> Vec<Case> {
    [0.125, 1.0, 3.25, 1e6].map(|ltscale| common_case(&format!("common/ltscale={ltscale}"), Common { ltscale, ..Common::default() })).into()
}

pub(super) fn transparencies() -> Vec<Case> {
    [Transparency::ByLayer, Transparency::ByBlock]
        .into_iter()
        .chain((0..=90).map(Transparency::Percent))
        .map(|transparency| common_case(&format!("common/transparency={transparency:?}"), Common { transparency, ..Common::default() }))
        .collect()
}

pub(super) fn visibility() -> Vec<Case> {
    [false, true].map(|visible| common_case(&format!("common/visible={visible}"), Common { visible, ..Common::default() })).into()
}

pub(super) fn thicknesses() -> Vec<Case> {
    [-2.5, 0.0, 3.75].map(|thickness| common_case(&format!("common/thickness={thickness}"), Common { thickness, ..Common::default() })).into()
}

pub(super) fn plot_styles() -> Vec<Case> {
    ["ByLayer", "Review"]
        .map(|plot_style| common_case(&format!("common/plot_style={plot_style}"), Common { plot_style: plot_style.into(), ..Common::default() }))
        .into()
}

pub(super) fn byblock_plot_style() -> Vec<Case> {
    vec![common_case("common/plot_style=ByBlock", Common { plot_style: "ByBlock".into(), ..Common::default() })]
}

pub(super) fn extrusions() -> Vec<Case> {
    [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 1.0, 0.0)]
        .map(|extrusion| common_case(&format!("common/extrusion={extrusion:?}"), Common { extrusion, ..Common::default() }))
        .into()
}

pub(super) fn foreign_xdata() -> Vec<Case> {
    let xdata = vec![
        RawTag { code: 1001, value: "ROUNDTRIP_TEST".into() },
        RawTag { code: 1000, value: "foreign text Ø 😀".into() },
        RawTag { code: 1070, value: "-17".into() },
        RawTag { code: 1040, value: "3.25".into() },
        RawTag { code: 1004, value: "00FF80".into() },
    ];
    vec![common_case("common/foreign-xdata", Common { xdata, ..Common::default() })]
}

pub(super) fn common_interactions(plot_style: &str) -> Vec<Case> {
    // Full local product; retain the original DXF case with a named plot style as well as
    // an inherited-style counterpart, so a named-style loss does not hide other interactions.
    let mut cases = Vec::new();
    for layer in ["0", "Survey_Ä"] {
        for color in [Color::ByLayer, Color::ByBlock, Color::True(Rgb(21, 89, 233))] {
            for visible in [false, true] {
                let common = Common {
                    layer: layer.into(),
                    color,
                    visible,
                    linetype: "Dash_test".into(),
                    lineweight: Lineweight::Mm100(53),
                    ltscale: 2.75,
                    transparency: Transparency::Percent(37),
                    thickness: -1.25,
                    plot_style: plot_style.into(),
                    ..Common::default()
                };
                cases.push(common_case(&format!("common/product/{layer}/{color:?}/visible={visible}/plot_style={plot_style}"), common));
            }
        }
    }
    cases
}

pub(super) fn text_values() -> Vec<Case> {
    [("empty", ""), ("whitespace", "  a\tb  "), ("unicode", "Café 图号 😀 e\u{301}")]
        .map(|(name, value)| entity_case(&format!("text/value/{name}"), Common::default(), EntityKind::Text(Text { value: value.into(), ..text() })))
        .into()
}

pub(super) fn text_literal_escapes() -> Vec<Case> {
    vec![entity_case("text/value/literal-escapes", Common::default(), EntityKind::Text(Text { value: r"\U+0041 \M+183A4 \P %%c".into(), ..text() }))]
}

pub(super) fn text_heights() -> Vec<Case> {
    [1e-9, 0.125, 2.25, 1e5]
        .map(|height| entity_case(&format!("text/height={height}"), Common::default(), EntityKind::Text(Text { height, ..text() })))
        .into()
}

pub(super) fn text_rotations() -> Vec<Case> {
    [0.0, -0.7, std::f64::consts::FRAC_PI_2, 5.9]
        .map(|rotation| entity_case(&format!("text/rotation={rotation}"), Common::default(), EntityKind::Text(Text { rotation, ..text() })))
        .into()
}

pub(super) fn text_widths() -> Vec<Case> {
    [0.125, 1.0, 2.75]
        .map(|width_factor| {
            entity_case(&format!("text/width_factor={width_factor}"), Common::default(), EntityKind::Text(Text { width_factor, ..text() }))
        })
        .into()
}

pub(super) fn text_obliques() -> Vec<Case> {
    [-0.3, 0.0, 0.4]
        .map(|oblique| entity_case(&format!("text/oblique={oblique}"), Common::default(), EntityKind::Text(Text { oblique, ..text() })))
        .into()
}

pub(super) fn text_styles() -> Vec<Case> {
    ["Standard", "Notes"]
        .map(|style| entity_case(&format!("text/style={style}"), Common::default(), EntityKind::Text(Text { style: style.into(), ..text() })))
        .into()
}

pub(super) fn annotation_scales() -> Vec<Case> {
    [vec![], vec![AnnoScale::unit()], vec![AnnoScale::unit(), AnnoScale::new("1:50", 1.0, 50.0)]]
        .map(|scales| {
            entity_case(&format!("text/anno_scales={scales:?}"), Common { anno_scales: scales, ..Common::default() }, EntityKind::Text(text()))
        })
        .into()
}

pub(super) fn coplanar_text_alignment() -> Vec<Case> {
    // Construct additional valid single-plane inputs before serialization. Keep the original
    // differing-elevation cases separately: DXF can store both XYZ points, DWG TEXT has one Z.
    let mut cases = text_alignment(true);
    for case in &mut cases {
        case.name.push_str("/coplanar");
        let handles: Vec<_> = case.drawing.model.iter().map(|e| e.handle).collect();
        for handle in handles {
            case.drawing.model.modify(handle, |e| {
                if let EntityKind::Text(t) = &mut e.kind {
                    t.align_pt = t.align_pt.map(|p| Vec3::new(p.x, p.y, t.insert.z));
                }
            });
        }
    }
    cases
}

pub(super) fn noncoplanar_text_alignment() -> Vec<Case> {
    text_alignment(true)
        .into_iter()
        .filter(|case| case.drawing.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.align_pt.is_some_and(|p| p.z != t.insert.z))))
        .collect()
}

pub(super) fn text_alignment(with_point: bool) -> Vec<Case> {
    let mut cases = Vec::new();
    let points = if with_point { vec![Some(Vec3::ZERO), Some(Vec3::new(17.25, -23.5, 6.75))] } else { vec![None] };
    for halign in [HAlign::Left, HAlign::Center, HAlign::Right, HAlign::Aligned, HAlign::Middle, HAlign::Fit] {
        for valign in [VAlign::Baseline, VAlign::Bottom, VAlign::Middle, VAlign::Top] {
            if matches!(halign, HAlign::Aligned | HAlign::Middle | HAlign::Fit) && valign != VAlign::Baseline {
                continue;
            }
            for &align_pt in &points {
                // The unused second point of left-baseline TEXT has a separate DXF read fixture.
                if halign == HAlign::Left && valign == VAlign::Baseline && align_pt.is_some() {
                    continue;
                }
                let mut t = Text { halign, valign, align_pt, rotation: 0.7, oblique: -0.2, width_factor: 1.25, style: "Notes".into(), ..text() };
                // Build a consistent baseline before serialization. No writer-derived expected
                // data: the font layout API determines a valid input; both points compare exactly.
                if let Some(anchor) = align_pt
                    && !matches!(halign, HAlign::Aligned | HAlign::Fit)
                {
                    let (h, v) = cadcraft_render::text_align(halign, valign);
                    let params = cadcraft_fonts::TextParams {
                        insert: t.insert.xy(),
                        align_pt: Some(anchor.xy()),
                        height: t.height,
                        rotation: t.rotation,
                        width_factor: t.width_factor,
                        oblique: t.oblique,
                        h,
                        v,
                        backwards: false,
                        upside_down: false,
                    };
                    t.insert = cadcraft_fonts::baseline_start(&cadcraft_fonts::TextFont::Stroke, &t.value, &params).to3(anchor.z);
                }
                cases.push(entity_case(&format!("text/alignment/{halign:?}/{valign:?}/point={align_pt:?}"), Common::default(), EntityKind::Text(t)));
            }
        }
    }
    cases
}
