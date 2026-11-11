//! The Block Editor's construction look. It must not inherit a user's gapless redefinition of
//! DASHED, and the objects that work out colors and linetypes of their own (a dimension drawn
//! from its definition points, a feature control frame, a multiline) take it like any other.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    DimKind, Dimension, Drawing, Entity, EntityKind, Handle, Line, Linetype, MLine, MLineCaps, MLineElement, MLineJust, MLineStyle, RawTag, Space,
    Tolerance,
};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{DisplayList, Kind, Options, build};

/// The extended data that marks an object as construction geometry.
fn marker() -> Vec<RawTag> {
    vec![RawTag { code: 1001, value: "ACAD_BCONSTRUCTION".into() }, RawTag { code: 1070, value: "1".into() }]
}

/// A Block Editor drawing whose first dimension style has colors and linetypes of its own:
/// dimension lines red, extension lines green, text blue, every line continuous. Returns the
/// drawing and the style's name.
fn editor_with_a_colored_dimension_style() -> (Drawing, String) {
    let mut d = Drawing::new_imperial();
    d.block_editor = true;
    let st = d.dim_styles.first_mut().unwrap();
    st.dim_line_color = Color::Index(1);
    st.ext_line_color = Color::Index(3);
    st.text_color = Color::Index(5);
    for linetype in [&mut st.dim_linetype, &mut st.ext_linetype1, &mut st.ext_linetype2] {
        *linetype = "Continuous".into();
    }
    let name = st.name.clone();
    (d, name)
}

/// `kind`, colored magenta, drawn in `editor` as an ordinary object and as construction geometry.
fn ordinary_and_construction(editor: &Drawing, kind: EntityKind) -> (DisplayList, DisplayList) {
    let mut e = Entity::new(Handle(1), kind);
    e.common.color = Color::Index(6);
    let mut ordinary = editor.clone();
    ordinary.model.push(e.clone());
    e.common.xdata = marker();
    let mut construction = editor.clone();
    construction.model.push(e);
    (build(&ordinary, &Space::Model, &Options::default()), build(&construction, &Space::Model, &Options::default()))
}

/// The gray the Block Editor draws construction geometry in: what a marked line gets.
fn construction_gray() -> Rgb {
    let mut d = Drawing::new_imperial();
    d.block_editor = true;
    let mut e = Entity::new(Handle(1), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) }));
    e.common.xdata = marker();
    d.model.push(e);
    build(&d, &Space::Model, &Options::default()).prims[0].color
}

/// Whether `list` draws something and all of it in `color`.
fn all_in(list: &DisplayList, color: Rgb) -> bool {
    !list.prims.is_empty() && list.prims.iter().all(|p| p.color == color)
}

/// Whether `list` draws something in `color`.
fn has(list: &DisplayList, color: Rgb) -> bool {
    list.prims.iter().any(|p| p.color == color)
}

/// The length of the longest stroke of `list`.
fn longest_stroke(list: &DisplayList) -> f64 {
    let length = |p| list.points(p).windows(2).map(|pair| pair[0].dist(pair[1])).sum::<f64>();
    list.prims.iter().filter(|p| p.kind == Kind::Polyline).map(length).fold(0.0, f64::max)
}

/// Construction dashes keep gaps when the drawing redefines dashed as solid.
#[test]
fn construction_dashes_keep_gaps_when_the_drawing_redefines_dashed_as_solid() {
    let mut d = Drawing::new_metric();
    d.block_editor = true;
    d.linetypes.push(Linetype::simple("DASHED", "User-defined adjacent strokes", &[0.5, 0.25]));
    let mut e = Entity::new(Handle(1), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) }));
    e.common.xdata = marker();
    d.model.push(e);

    let list = build(&d, &Space::Model, &Options::default());
    assert!(list.prims.len() > 1, "construction has several visible dash strokes");
    let ink: f64 = list
        .prims
        .iter()
        .filter(|p| p.kind == Kind::Polyline)
        .map(|p| list.points(p).windows(2).map(|pair| pair[0].dist(pair[1])).sum::<f64>())
        .sum();
    assert!(ink > 0.0 && ink < 99.0, "a 100-unit construction line needs actual gaps, not contiguous pieces: ink={ink}");
}

/// A construction dimension without a dimension block is gray and dashed.
#[test]
fn a_construction_dimension_without_a_dimension_block_is_gray_and_dashed() {
    // A dimension made or edited here has no block: it is drawn from its definition points, with
    // the colors and linetypes of its style (DIMCLRD, DIMCLRE, DIMCLRT, DIMLTYPE, DIMLTEX1 and 2).
    let (editor, style) = editor_with_a_colored_dimension_style();
    let dimension = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 2.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style,
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
        insertion_point: Vec3::ZERO,
    };
    let (ordinary, construction) = ordinary_and_construction(&editor, EntityKind::Dimension(dimension));
    assert!(
        has(&ordinary, Rgb(255, 0, 0)) && has(&ordinary, Rgb(0, 255, 0)) && has(&ordinary, Rgb(0, 0, 255)),
        "an ordinary dimension keeps its style's colors in the editor"
    );
    assert!(longest_stroke(&ordinary) > 1.5, "and its continuous lines: {}", longest_stroke(&ordinary));
    assert!(all_in(&construction, construction_gray()), "lines, arrowheads and text are the editor's gray");
    assert!(longest_stroke(&construction) < 1.0, "every line is dashed, whatever linetype the style names: {}", longest_stroke(&construction));
}

/// A construction feature control frame is gray.
#[test]
fn a_construction_feature_control_frame_is_gray() {
    // Its frame is drawn in DIMCLRD, its symbols and text in DIMCLRT.
    let (editor, style) = editor_with_a_colored_dimension_style();
    let frame = Tolerance { insert: Vec3::ZERO, dir: Vec3::new(1.0, 0.0, 0.0), style, text: "{\\Fgdt;j}%%v0.05%%vA".into() };
    let (ordinary, construction) = ordinary_and_construction(&editor, EntityKind::Tolerance(frame));
    assert!(has(&ordinary, Rgb(255, 0, 0)) && has(&ordinary, Rgb(0, 0, 255)), "an ordinary frame keeps its style's colors in the editor");
    assert!(all_in(&construction, construction_gray()), "frame, symbols and text are the editor's gray");
    assert_eq!(construction.prims.len(), ordinary.prims.len(), "nothing else about the frame changes");
}

/// A construction multiline is gray and dashed.
#[test]
fn a_construction_multiline_is_gray_and_dashed() {
    // Each element is drawn in the color and linetype of the multiline style (the first one here
    // green, the second following the multiline), the caps in the multiline's own.
    let mut editor = Drawing::new_imperial();
    editor.block_editor = true;
    let style = MLineStyle {
        elements: vec![
            MLineElement { offset: 0.5, color: Color::Index(3), ..Default::default() },
            MLineElement { offset: -0.5, ..Default::default() },
        ],
        start_caps: MLineCaps { line: true, ..Default::default() },
        end_caps: MLineCaps { line: true, ..Default::default() },
        ..Default::default()
    };
    // Scale 2: the caps are 2 units long, the elements 100.
    let multiline = MLine::new(&style, &[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], false, MLineJust::Zero, 2.0);
    let (ordinary, construction) = ordinary_and_construction(&editor, EntityKind::MLine(multiline));
    assert!(has(&ordinary, Rgb(0, 255, 0)) && has(&ordinary, Rgb(255, 0, 255)), "an ordinary multiline keeps its colors in the editor");
    assert_eq!(ordinary.prims.len(), 4, "two continuous elements and two caps");
    assert!(all_in(&construction, construction_gray()), "elements and caps are the editor's gray");
    assert!(longest_stroke(&construction) < 1.0, "elements and caps are dashed: {}", longest_stroke(&construction));
}
