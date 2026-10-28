//! The rest of the fields of the drawing's records survive saving and reopening as DXF, and a
//! second save: a layout's plot settings and layout data, a model space
//! viewport's view settings, a multileader style's and a table style's fields, a dimension link's
//! snap data, a dimension's arrowhead flips and the registered applications.

use cadcraft_doc::*;
use cadcraft_dxf::{Tag, parse, records, sections};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{Version, read_dxf};

fn dxf(d: &Drawing) -> String {
    let mut d = d.clone();
    d.header.set_str("ACADVER", Version::R2018.code());
    String::from_utf8(cadcraft_io::write(&d, "x.dxf").unwrap()).unwrap()
}

/// Saved, reopened, saved again and reopened again.
fn resaved(d: &Drawing) -> Drawing {
    read_dxf(dxf(&read_dxf(dxf(d).as_bytes()).unwrap()).as_bytes()).unwrap()
}

fn all(text: &str) -> Vec<(String, Vec<Tag>)> {
    let tags = parse(text.as_bytes()).unwrap();
    sections(&tags).into_iter().flat_map(|s| records(&s.tags)).collect()
}

/// The first records of two files that differ, for a readable failure.
fn diff(a: &str, b: &str) -> String {
    let (ra, rb) = (all(a), all(b));
    let show = |g: &[Tag]| g.iter().map(|t| format!("{}={}", t.code, t.str())).collect::<Vec<_>>().join(" ");
    for (x, y) in ra.iter().zip(&rb) {
        if x != y {
            return format!("first difference in {}:\n  {}\n  {}", x.0, show(&x.1), show(&y.1));
        }
    }
    format!("{} records against {}", ra.len(), rb.len())
}

fn group(g: &[Tag], code: i32) -> Option<String> {
    g.iter().find(|t| t.code == code).map(Tag::str)
}

/// A layout with every plot setting and layout field off its default.
fn plotted() -> Drawing {
    let mut d = Drawing::new_imperial();
    let mut l = Layout::new("Sheet A", 1);
    l.page.settings = PlotSettings {
        name: "A-size".into(),
        view_name: "North".into(),
        origin: [1.5, 2.5],
        image_origin: [0.25, 0.75],
        shade_mode: 2,
        shade_resolution: 4,
        shade_dpi: 600,
        shade_plot: Handle(0),
    };
    l.record = LayoutRecord {
        flags: 3,
        limits_min: Vec2::new(-1.0, -2.0),
        limits_max: Vec2::new(30.0, 20.0),
        insbase: Vec3::new(1.0, 2.0, 3.0),
        extents_min: Vec3::new(-5.0, -5.0, 0.0),
        extents_max: Vec3::new(25.0, 15.0, 0.0),
        elevation: 4.0,
        ucs_origin: Vec3::new(10.0, 0.0, 0.0),
        ucs_x: Vec3::new(0.0, 1.0, 0.0),
        ucs_y: Vec3::new(-1.0, 0.0, 0.0),
        ortho_type: 5,
        ..LayoutRecord::default()
    };
    d.layouts = vec![l];
    d.model_layout = Some(Box::new(Layout {
        page: PageSetup {
            settings: PlotSettings { name: "Model setup".into(), shade_mode: 1, ..PlotSettings::default() },
            plot_area: "extents".into(),
            scale_to_fit: true,
            ..PageSetup::default()
        },
        record: LayoutRecord { limits_max: Vec2::new(40.0, 30.0), ..LayoutRecord::default() },
        ..Layout::new("Model", 0)
    }));
    d
}

#[test]
fn plot_settings_and_layout_data_round_trip() {
    let d = plotted();
    let text = dxf(&d);
    for back in [read_dxf(text.as_bytes()).unwrap(), resaved(&d)] {
        let l = back.layouts.first().unwrap();
        assert_eq!(l.page.settings, d.layouts[0].page.settings);
        assert_eq!(l.record, d.layouts[0].record);
        let m = back.model_layout.as_deref().expect("model layout");
        assert_eq!(m.page.settings, d.model_layout.as_ref().unwrap().page.settings);
        assert_eq!(m.record, d.model_layout.as_ref().unwrap().record);
    }
    // Nothing drifts on a second save.
    let again = dxf(&read_dxf(text.as_bytes()).unwrap());
    let pick = |t: &str| {
        all(t)
            .into_iter()
            .filter(|(k, _)| k == "LAYOUT")
            .map(|(_, g)| g.iter().map(|t| (t.code, t.str())).filter(|(c, _)| *c != 5 && *c != 330).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    assert_eq!(pick(&text), pick(&again));
}

#[test]
fn plot_record_values_are_kept_until_the_page_changes() {
    let mut d = plotted();
    let text = dxf(&d);
    // The file names its own device, paper, flags and a plot type CADCraft has no word for.
    let mut tags = parse(text.as_bytes()).unwrap();
    let at = (0..tags.len())
        .find(|i| tags[*i].code == 100 && tags[*i].str() == "AcDbPlotSettings" && tags.get(i + 1).is_some_and(|n| n.code == 1 && n.str() == "A-size"))
        .unwrap();
    let end = at + tags[at..].iter().position(|t| t.code == 100 && t.str() == "AcDbLayout").unwrap();
    for t in &mut tags[at..end] {
        match t.code {
            2 => *t = Tag::s(2, "DWG To PDF.pc3"),
            74 => *t = Tag::i(74, 3),
            _ => {}
        }
    }
    let edited = cadcraft_dxf::write_ascii(&tags);
    let back = read_dxf(edited.as_bytes()).unwrap();
    let page = &back.layouts[0].page;
    assert!(page.read.0.is_some(), "the record's values are kept");
    let again = dxf(&back);
    let g = all(&again).into_iter().find(|(k, g)| k == "LAYOUT" && group(g, 1).as_deref() == Some("A-size")).unwrap().1;
    assert_eq!(group(&g, 2).as_deref(), Some("DWG To PDF.pc3"), "the plot configuration keeps its spelling");
    assert_eq!(group(&g, 74).as_deref().map(str::trim), Some("3"), "the plot type keeps its value");
    // Edit the page: the record follows the page.
    let mut edited = back.clone();
    edited.layouts[0].page.landscape = !edited.layouts[0].page.landscape;
    edited.layouts[0].page.device = "None".into();
    let after = dxf(&edited);
    let g = all(&after).into_iter().find(|(k, g)| k == "LAYOUT" && group(g, 1).as_deref() == Some("A-size")).unwrap().1;
    assert_eq!(group(&g, 2).as_deref(), Some("none_device"));
    assert_eq!(group(&g, 74).as_deref().map(str::trim), Some("5"));
    d.layouts.clear();
}

#[test]
fn viewport_settings_round_trip() {
    let mut d = Drawing::new_imperial();
    let settings = VportSettings {
        direction: Vec3::new(0.0, 0.0, 1.0),
        target: Vec3::new(1.0, 2.0, 0.0),
        lens: 35.0,
        front_clip: 1.0,
        back_clip: 2.0,
        twist: 0.5,
        view_mode: 8,
        circle_zoom: 500,
        fast_zoom: 0,
        ucs_icon: 1,
        snap_on: 1,
        grid_on: 1,
        snap_style: 1,
        snap_isopair: 2,
        style_sheet: Some("acad.ctb".into()),
        render_mode: 2,
        ucs_per_viewport: 0,
        ucs_origin: Vec3::new(3.0, 4.0, 0.0),
        ucs_x: Vec3::new(0.0, 1.0, 0.0),
        ucs_y: Vec3::new(-1.0, 0.0, 0.0),
        ortho_type: 3,
        elevation: 1.5,
        named_ucs: Handle(0),
        base_ucs: Handle(0),
        shade_plot: 2,
        grid_flags: 1,
        major_grid: 10,
        background: Handle(0),
        shade_plot_object: Handle(0),
        visual_style: Handle(0),
        default_lighting: false,
        lighting_type: 2,
        brightness: 0.25,
        contrast: 0.5,
        ambient: Some(7),
        ambient_rgb: Some(0x00FF8800),
    };
    let tile = |min: Vec2, max: Vec2, s: &VportSettings| VportTile {
        settings: s.clone(),
        handle: Default::default(),
        min,
        max,
        center: Vec2::new(5.0, 5.0),
        height: 10.0,
        aspect: 1.5,
    };
    // A saved two-tile configuration keeps each tile's settings.
    d.vports.push(VportConfig {
        name: "Two".into(),
        tiles: vec![
            tile(Vec2::new(0.0, 0.0), Vec2::new(0.5, 1.0), &settings),
            tile(Vec2::new(0.5, 0.0), Vec2::new(1.0, 1.0), &VportSettings::default()),
        ],
    });
    // And a lone *Active tile keeps its settings across a save.
    d.active_vport = Some(Box::new(tile(Vec2::ZERO, Vec2::new(1.0, 1.0), &settings)));
    let text = dxf(&d);
    for back in [read_dxf(text.as_bytes()).unwrap(), resaved(&d)] {
        let two = back.vports.iter().find(|c| c.name == "Two").expect("saved configuration");
        assert_eq!(two.tiles.first().map(|t| &t.settings), Some(&settings));
        assert_eq!(two.tiles.get(1).map(|t| &t.settings), Some(&VportSettings::default()));
        assert_eq!(back.active_vport.as_deref().map(|t| &t.settings), Some(&settings));
    }
    let back = read_dxf(text.as_bytes()).unwrap();
    let once = dxf(&back);
    let twice = dxf(&read_dxf(once.as_bytes()).unwrap());
    assert!(once == twice, "a second save is the same: {}", diff(&once, &twice));
}

#[test]
fn multileader_and_table_style_fields_round_trip() {
    let mut d = Drawing::new_imperial();
    d.mleader_styles.push(MLeaderStyle {
        props: MLeaderStyleProps {
            description: "Callouts".into(),
            content_type: 1,
            max_leader_points: 4,
            first_angle: 0.5,
            second_angle: 0.25,
            leader_type: 2,
            leader_color: 5,
            leader_lineweight: 35,
            landing: false,
            dogleg_on: false,
            default_text: "TBD".into(),
            text_left_attachment: 3,
            text_right_attachment: 4,
            text_angle: 2,
            text_alignment: 1,
            text_color: 3,
            text_frame: true,
            text_align_left: true,
            align_space: 2.5,
            block_color: 1,
            block_scale: [2.0, 3.0, 4.0],
            block_scale_on: false,
            block_rotation: 0.5,
            block_rotation_on: false,
            block_connection: 1,
            scale: 2.0,
            overwrite: true,
            break_gap: 0.5,
            attachment_direction: 1,
            bottom_attachment: 3,
            top_attachment: 4,
            group_298: Some(true),
            ..MLeaderStyleProps::default()
        },
        ..d.mleader_styles.first().cloned().unwrap_or_else(|| MLeaderStyle { name: "Callouts".into(), ..MLeaderStyle::default() })
    });
    if let Some(s) = d.mleader_styles.last_mut() {
        s.name = "Callouts".into();
    }
    let mut ts = d.table_styles.first().cloned().unwrap_or_else(|| TableStyle { name: "Schedule".into(), ..TableStyle::default() });
    ts.name = "Schedule".into();
    ts.props = TableStyleProps {
        version: Some(1),
        description: "Door schedule".into(),
        flow_up: true,
        flags: 1,
        cells: [
            TableCellData { data_type: 2, unit_type: 1, format: "%lu2%pr2".into() },
            TableCellData { data_type: 512, unit_type: 0, format: String::new() },
            TableCellData { data_type: 4, unit_type: 2, format: "%dd".into() },
        ],
    };
    d.table_styles.retain(|s| !s.name.eq_ignore_ascii_case("Schedule"));
    d.table_styles.push(ts);
    let text = dxf(&d);
    for back in [read_dxf(text.as_bytes()).unwrap(), resaved(&d)] {
        let m = back.mleader_styles.iter().find(|s| s.name == "Callouts").expect("mleader style");
        let want = d.mleader_styles.iter().find(|s| s.name == "Callouts").unwrap();
        assert_eq!(m.props, want.props);
        let t = back.table_styles.iter().find(|s| s.name == "Schedule").expect("table style");
        let want = d.table_styles.iter().find(|s| s.name == "Schedule").unwrap();
        assert_eq!(t.props, want.props);
    }
    // Once a file is read nothing drifts (a new drawing's first save names its handle seed afresh).
    let once = dxf(&read_dxf(text.as_bytes()).unwrap());
    let twice = dxf(&read_dxf(once.as_bytes()).unwrap());
    assert!(once == twice, "{}", diff(&once, &twice));
}

#[test]
fn dimension_links_keep_their_snap_data_until_they_change() {
    let mut d = Drawing::new_imperial();
    let c = Common::default;
    let l1 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 5.0, 0.0) })).unwrap();
    let l2 = d.add(&Space::Model, c(), EntityKind::Line(Line { a: Vec3::new(10.0, 0.0, 0.0), b: Vec3::new(10.0, 5.0, 0.0) })).unwrap();
    let read = OsnapRead {
        handle: l1,
        snap: Some(AssocSnap::Start),
        osnap: 1,
        subent: 7,
        gs_marker: 42,
        xref: Some("XR".into()),
        param: 0.5,
        other: Handle(0),
    };
    let dim = |first: DimAssoc| Dimension {
        attachment: Default::default(),
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, -2.0, 0.0),
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
        assoc: vec![first, DimAssoc { read: Default::default(), point: "p14".into(), handle: l2, snap: AssocSnap::Start }],
        marks: Default::default(),
        flip_arrows: [true, false],
        flag_73: true,
    };
    let kept = DimAssoc { read: AsRead::new(read.clone()), point: "p13".into(), handle: l1, snap: AssocSnap::Start };
    let mut d1 = d.clone();
    d1.add(&Space::Model, c(), EntityKind::Dimension(dim(kept.clone()))).unwrap();
    let refs = |t: &str| all(t).into_iter().find(|(k, _)| k == "DIMASSOC").map(|(_, g)| g).unwrap();
    let g = refs(&dxf(&d1));
    assert!(g.iter().any(|t| t.code == 73 && t.i64() == 7), "subentity type kept");
    assert!(g.iter().any(|t| t.code == 91 && t.i64() == 42), "GS marker kept");
    assert!(g.iter().any(|t| t.code == 301 && t.str() == "XR"), "xref handle kept");
    // The same link moved to the other end: the file's values no longer describe it.
    let mut moved = kept.clone();
    moved.snap = AssocSnap::End;
    let mut d2 = d.clone();
    d2.add(&Space::Model, c(), EntityKind::Dimension(dim(moved))).unwrap();
    let g = refs(&dxf(&d2));
    assert!(!g.iter().any(|t| t.code == 73 && t.i64() == 7));
    assert!(g.iter().any(|t| t.code == 301 && t.str().is_empty()));
    // The flips and the flag before them come back.
    let back = read_dxf(dxf(&d1).as_bytes()).unwrap();
    let dm = back.model.iter().find_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None }).unwrap();
    assert_eq!((dm.flip_arrows, dm.flag_73), ([true, false], true));
    let dm = resaved(&d1).model.iter().find_map(|e| if let EntityKind::Dimension(x) = &e.kind { Some(x.clone()) } else { None }).unwrap();
    assert_eq!((dm.flip_arrows, dm.flag_73), ([true, false], true));
}

#[test]
fn registered_applications_keep_their_case_and_flags() {
    let mut d = Drawing::new_imperial();
    d.appids = vec![
        AppId { name: "AcadTC_Custom".into(), flags: 1, ..AppId::default() },
        AppId { name: "Acad_TC".into(), flags: 0, ..AppId::default() },
        AppId { name: "ACAD_PSEXT".into(), flags: 0, ..AppId::default() },
    ];
    let text = dxf(&d);
    let names: Vec<(String, i64)> = all(&text)
        .into_iter()
        .filter(|(k, _)| k == "APPID")
        .map(|(_, g)| (group(&g, 2).unwrap_or_default(), group(&g, 70).and_then(|v| v.trim().parse().ok()).unwrap_or(0)))
        .collect();
    assert!(names.contains(&("AcadTC_Custom".to_string(), 1)));
    assert!(names.contains(&("Acad_TC".to_string(), 0)));
    assert!(names.contains(&("ACAD_PSEXT".to_string(), 0)));
    assert_eq!(names.iter().filter(|(n, _)| n.eq_ignore_ascii_case("acad_tc")).count(), 1, "no second record in another case");
    let back = read_dxf(text.as_bytes()).unwrap();
    for a in &d.appids {
        let got = back.appids.iter().find(|x| x.name == a.name).unwrap_or_else(|| panic!("{} comes back", a.name));
        assert_eq!(got.flags, a.flags);
    }
    // Saved a second time, each one is still there once, with its flags.
    let again = resaved(&d);
    for a in &d.appids {
        let same: Vec<_> = again.appids.iter().filter(|x| x.name.eq_ignore_ascii_case(&a.name)).collect();
        assert_eq!(same.len(), 1, "{} once", a.name);
        assert_eq!((same[0].name.as_str(), same[0].flags), (a.name.as_str(), a.flags));
    }
}
