//! A dimension style edited in the Dimension Style Manager keeps every value its four tabs set
//! (Lines & Arrows, Text, Primary Units, Tolerances) through saving as DXF and as DWG and
//! reopening, and the dimension that uses it still names it.

use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, Space, TextStyle};
use cadcraft_geom::Vec3;
use cadcraft_render::Arrowhead;

fn edited() -> DimStyle {
    DimStyle {
        name: "Mech".into(),
        // Lines & Arrows.
        dim_line_extend: 0.125,
        baseline_spacing: 0.5,
        suppress_dim1: true,
        ext_extend: 0.25,
        ext_offset: 0.1,
        suppress_ext2: true,
        arrow_block: "_ArchTick".into(),
        arrow_size: 0.3,
        center_mark: -0.15,
        // Text.
        text_style: "Notes".into(),
        text_height: 0.35,
        text_above: 1,
        text_just: 2,
        text_gap: 0.05,
        text_inside_horizontal: false,
        text_outside_horizontal: false,
        scale: 2.0,
        // Primary Units.
        linear_unit: 4,
        decimals: 2,
        decimal_separator: ",".into(),
        round: 0.25,
        post: "<> mm".into(),
        linear_factor: 0.5,
        zero_suppression: 12,
        angular_unit: 1,
        angular_decimals: 3,
        // Tolerances.
        tolerance: true,
        tol_decimals: 3,
        tol_plus: 0.02,
        tol_minus: 0.01,
        tol_scale: 0.75,
        tol_just: 0,
        tol_zero_suppression: 8,
        ..DimStyle::default()
    }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Notes".into(), ..TextStyle::default() });
    d.dim_styles.push(edited());
    let dim = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 3.0, 0.0),
        text_mid: Vec3::new(5.0, 3.0, 0.0),
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Mech".into(),
        measurement: 10.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
        insertion_point: Vec3::ZERO,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dim)).unwrap();
    d
}

fn check(format: &str, r: &Drawing) {
    let want = edited();
    let got = r.dim_style("Mech").unwrap_or_else(|| panic!("{format}: the style is gone"));
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let reals = [
        ("DIMDLE", got.dim_line_extend, want.dim_line_extend),
        ("DIMDLI", got.baseline_spacing, want.baseline_spacing),
        ("DIMEXE", got.ext_extend, want.ext_extend),
        ("DIMEXO", got.ext_offset, want.ext_offset),
        ("DIMASZ", got.arrow_size, want.arrow_size),
        ("DIMCEN", got.center_mark, want.center_mark),
        ("DIMTXT", got.text_height, want.text_height),
        ("DIMGAP", got.text_gap, want.text_gap),
        ("DIMSCALE", got.scale, want.scale),
        ("DIMRND", got.round, want.round),
        ("DIMLFAC", got.linear_factor, want.linear_factor),
        ("DIMTP", got.tol_plus, want.tol_plus),
        ("DIMTM", got.tol_minus, want.tol_minus),
        ("DIMTFAC", got.tol_scale, want.tol_scale),
    ];
    for (var, g, w) in reals {
        assert!(near(g, w), "{format}: {var} read back as {g}, not {w}");
    }
    let ints = [
        ("DIMTAD", got.text_above, want.text_above),
        ("DIMJUST", got.text_just, want.text_just),
        ("DIMLUNIT", got.linear_unit, want.linear_unit),
        ("DIMDEC", got.decimals, want.decimals),
        ("DIMZIN", got.zero_suppression, want.zero_suppression),
        ("DIMAUNIT", got.angular_unit, want.angular_unit),
        ("DIMADEC", got.angular_decimals, want.angular_decimals),
        ("DIMTDEC", got.tol_decimals, want.tol_decimals),
        ("DIMTOLJ", got.tol_just, want.tol_just),
        ("DIMTZIN", got.tol_zero_suppression, want.tol_zero_suppression),
    ];
    for (var, g, w) in ints {
        assert_eq!(g, w, "{format}: {var}");
    }
    let flags = [
        ("DIMSD1", got.suppress_dim1, want.suppress_dim1),
        ("DIMSD2", got.suppress_dim2, want.suppress_dim2),
        ("DIMSE1", got.suppress_ext1, want.suppress_ext1),
        ("DIMSE2", got.suppress_ext2, want.suppress_ext2),
        ("DIMTIH", got.text_inside_horizontal, want.text_inside_horizontal),
        ("DIMTOH", got.text_outside_horizontal, want.text_outside_horizontal),
        ("DIMTOL", got.tolerance, want.tolerance),
        ("DIMLIM", got.limits, want.limits),
    ];
    for (var, g, w) in flags {
        assert_eq!(g, w, "{format}: {var}");
    }
    assert_eq!(Arrowhead::parse(&got.arrow_block), Arrowhead::ArchTick, "{format}: DIMBLK {:?}", got.arrow_block);
    assert!(got.text_style.eq_ignore_ascii_case("Notes"), "{format}: DIMTXSTY {:?}", got.text_style);
    assert_eq!(got.decimal_separator, ",", "{format}: DIMDSEP");
    assert_eq!(got.post, "<> mm", "{format}: DIMPOST");
    let uses = r.model.iter().any(|e| matches!(&e.kind, EntityKind::Dimension(dm) if dm.style.eq_ignore_ascii_case("Mech")));
    assert!(uses, "{format}: the dimension keeps its style");
}

#[test]
fn an_edited_dimension_style_survives_dxf() {
    let r = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&drawing()).as_bytes()).unwrap();
    check("DXF", &r);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn an_edited_dimension_style_survives_dwg() {
    let dwg = cadcraft_io::write(&drawing(), "x.dwg").unwrap();
    let r = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    check("DWG", &r);
}
