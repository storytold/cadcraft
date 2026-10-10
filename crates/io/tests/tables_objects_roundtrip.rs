//! DXF save → reopen keeps table and object-section data: complex linetype elements,
//! multileader styles, layout page setups, named views, UCSs and groups.

use cadcraft_doc::geom::{Vec2, Vec3};
use cadcraft_doc::{Common, DashElement, Drawing, EntityKind, Group, Line, Linetype, MLeaderStyle, NamedView, Space, TextStyle, Ucs, library};
use cadcraft_io::{read_dxf, write_dxf};

fn round_trip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).expect("reopen")
}

#[test]
fn linetype_text_and_shape_elements_round_trip() {
    let mut d = Drawing::new_imperial();
    let fence = library::standard_linetypes().into_iter().find(|l| l.name == "FENCELINE1").expect("FENCELINE1");
    d.linetypes.push(fence.clone());
    let gas = Linetype {
        name: "GAS".into(),
        description: "Gas line ----GAS----".into(),
        pattern: vec![
            DashElement::dash(0.5),
            DashElement {
                text: Some("GAS".into()),
                style: Some("Standard".into()),
                scale: 0.1,
                rotation: 0.25,
                offset: Vec2::new(-0.1, -0.05),
                ..DashElement::dash(-0.2)
            },
        ],
    };
    // A shape from a shape file (not a text style), with an absolute rotation.
    let shape = Linetype {
        name: "BATTING".into(),
        description: String::new(),
        pattern: vec![
            DashElement::dash(0.0001),
            DashElement {
                shape: Some(132),
                style: Some("ltypeshp.shx".into()),
                scale: 0.2,
                rotation: 1.5,
                absolute: true,
                offset: Vec2::new(0.0, 0.1),
                ..DashElement::dash(-0.3)
            },
        ],
    };
    d.linetypes.extend([gas.clone(), shape.clone()]);
    let dxf = write_dxf(&d);
    // The shape file is written as an unnamed shape style (STYLE flag 1).
    assert!(dxf.contains("ltypeshp.shx"), "no shape style record");
    let r = read_dxf(dxf.as_bytes()).expect("reopen");
    for lt in [&fence, &gas, &shape] {
        assert_eq!(r.linetype(&lt.name), Some(lt), "{}", lt.name);
    }
    // The shape style isn't a text style.
    assert!(r.text_styles.iter().all(|s| !s.name.is_empty()));
}

#[test]
fn multileader_styles_and_current_style_round_trip() {
    let mut d = Drawing::new_imperial();
    d.text_styles.push(TextStyle { name: "Slanted".into(), oblique: 0.2, ..TextStyle::default() });
    let callout =
        MLeaderStyle { name: "Callout".into(), arrow_size: 2.0, text_height: 3.0, landing_gap: 1.0, dogleg: 5.0, text_style: "Slanted".into() };
    d.mleader_styles.push(callout);
    d.header.set_str("CMLEADERSTYLE", "Callout");
    let r = round_trip(&d);
    assert_eq!(r.mleader_styles, d.mleader_styles);
    assert_eq!(r.header.str("CMLEADERSTYLE", ""), "Callout");
}

#[test]
fn layout_page_setup_and_paper_view_round_trip() {
    let mut d = Drawing::new_imperial();
    let first = d.layouts.get_mut(0).expect("Layout1");
    first.page.scale = 0.5;
    first.page.plot_style_table = "monochrome.ctb".into();
    first.page.center = true;
    first.page.plot_area = "extents".into();
    first.view = Some((Vec2::new(5.0, 4.0), 12.0));
    let second = d.layouts.get_mut(1).expect("Layout2");
    second.page.scale_to_fit = true;
    second.page.scale = 0.25;
    second.page.lineweights = false;
    second.page.plot_area = "window".into();
    second.page.device = "DWG To PDF.pc3".into();
    let r = round_trip(&d);
    for (a, b) in d.layouts.iter().zip(&r.layouts) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.page, b.page, "{}", a.name);
        assert_eq!(a.view, b.view, "{}", a.name);
    }
    assert_eq!(r.layouts.len(), d.layouts.len());
}

#[test]
fn named_views_ucss_and_groups_round_trip() {
    let mut d = Drawing::new_imperial();
    let line = d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })).expect("line");
    d.views.push(NamedView { name: "Front".into(), center: Vec2::new(5.0, 5.0), height: 20.0, width: 30.0, layer_state: Some("Plan".into()) });
    d.ucss.push(Ucs { name: "Tilted".into(), origin: Vec3::new(1.0, 2.0, 0.0), x_axis: Vec3::new(0.6, 0.8, 0.0), y_axis: Vec3::new(-0.8, 0.6, 0.0) });
    d.groups.push(Group { name: "G1".into(), description: "grp".into(), selectable: false, members: vec![line] });
    // A member that no longer exists is not written.
    let gone = d.new_handle();
    d.groups.push(Group { name: "*A1".into(), description: String::new(), selectable: true, members: vec![line, gone] });
    let r = round_trip(&d);
    assert_eq!(r.views, d.views);
    assert_eq!(r.ucss, d.ucss);
    assert_eq!(r.groups.len(), 2);
    assert_eq!(r.groups.first(), d.groups.first());
    let anon = r.groups.get(1).expect("*A1");
    assert_eq!((anon.name.as_str(), anon.members.as_slice()), ("*A1", [line].as_slice()));
}

/// A minimal file in the shape other programs write: an unnamed shape style, a VIEW, a UCS and a
/// GROUP in ACAD_GROUP. Hostile values (NaN, out-of-range shape numbers, missing groups) are
/// skipped without panicking.
#[test]
fn foreign_and_hostile_tables_objects() {
    let tags: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "TABLES"),
        (0, "TABLE"),
        (2, "STYLE"),
        (0, "STYLE"),
        (5, "11"),
        (2, ""),
        (70, "1"),
        (3, "shapes.shx"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "LTYPE"),
        (0, "LTYPE"),
        (5, "12"),
        (2, "ODD"),
        (73, "3"),
        (49, "0.5"),
        (74, "4"),
        (75, "70000"),
        (340, "11"),
        (46, "nan"),
        (49, "-0.25"),
        (74, "2"),
        (340, "FFFF"),
        (9, "X"),
        (45, "1e400"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "VIEW"),
        (0, "VIEW"),
        (2, "Top"),
        (40, "10"),
        (41, "16"),
        (10, "3"),
        (20, "4"),
        (0, "VIEW"),
        (2, "Bad"),
        (40, "-1"),
        (41, "nan"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "UCS"),
        (0, "UCS"),
        (2, "Side"),
        (10, "1"),
        (20, "2"),
        (30, "3"),
        (11, "0"),
        (21, "1"),
        (31, "0"),
        (12, "0"),
        (22, "0"),
        (32, "1"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "LINE"),
        (5, "2A"),
        (8, "0"),
        (10, "0"),
        (20, "0"),
        (11, "1"),
        (21, "1"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "OBJECTS"),
        (0, "DICTIONARY"),
        (5, "C"),
        (3, "ACAD_GROUP"),
        (350, "D"),
        (3, "ACAD_MLEADERSTYLE"),
        (350, "E"),
        (0, "DICTIONARY"),
        (5, "D"),
        (3, "Doors"),
        (350, "20"),
        (0, "GROUP"),
        (5, "20"),
        (330, "D"),
        (300, "the doors"),
        (70, "0"),
        (71, "1"),
        (340, "2A"),
        (340, "zz"),
        (340, "99"),
        (0, "DICTIONARY"),
        (5, "E"),
        (3, "Odd"),
        (350, "21"),
        (0, "MLEADERSTYLE"),
        (5, "21"),
        (42, "-2"),
        (43, "nan"),
        (45, "-3"),
        (342, "404"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    let dxf: String = tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect();
    let d = read_dxf(dxf.as_bytes()).expect("read");
    let odd = d.linetype("ODD").expect("ODD");
    let first = odd.pattern.first().expect("shape element");
    assert_eq!((first.shape, first.style.as_deref()), (Some(0), Some("shapes.shx")));
    assert!(first.scale.is_finite());
    let second = odd.pattern.get(1).expect("text element");
    assert_eq!((second.text.as_deref(), second.style.as_deref()), (Some("X"), None));
    assert_eq!(d.views.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Top"]);
    assert_eq!(d.ucss.first().map(|u| u.origin), Some(Vec3::new(1.0, 2.0, 3.0)));
    let g = d.groups.first().expect("group");
    assert_eq!((g.name.as_str(), g.description.as_str(), g.members.len()), ("Doors", "the doors", 1));
    let odd_style = d.mleader_styles.iter().find(|s| s.name == "Odd").expect("mleader style");
    // Unusable values fall back to the defaults (NaN reads as 0).
    assert_eq!(odd_style, &MLeaderStyle { name: "Odd".into(), dogleg: 0.0, ..MLeaderStyle::default() });
    // Saving it again keeps everything readable.
    let again = round_trip(&d);
    assert_eq!(again.groups, d.groups);
    assert_eq!(again.views, d.views);
}
