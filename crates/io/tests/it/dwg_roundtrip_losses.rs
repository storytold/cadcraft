//! Data that used to be lost only when a drawing is saved as DWG and reopened (the DWG bridge
//! converts our DXF with acadrust), and the paper-space viewport the DXF writer adds for it.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::*;
use cadcraft_geom::{Vec2, Vec3};

fn through_dwg(d: &Drawing) -> Drawing {
    let dwg = cadcraft_io::write(d, "x.dwg").unwrap();
    cadcraft_io::read(&dwg, "x.dwg").unwrap()
}

fn line(d: &mut Drawing, space: &Space, common: Common) -> Handle {
    d.add(space, common, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 5.0, 0.0) })).unwrap()
}

fn viewport(id: u32) -> EntityKind {
    EntityKind::Viewport(Viewport {
        center: Vec3::new(50.0, 40.0, 0.0),
        width: 80.0,
        height: 60.0,
        view_center: Vec2::new(10.0, 10.0),
        view_height: 50.0,
        id,
        locked: false,
        frozen_layers: vec![],
        layer_colors: vec![],
        clip: None,
        on: true,
    })
}

/// The (code, value) pairs of every `kind` record in DXF text.
fn records(text: &str, kind: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut inside = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        if *code == "0" {
            inside = *value == kind;
            if inside {
                out.push(Vec::new());
            }
        } else if inside && let Some(rec) = out.last_mut() {
            rec.push((code.to_string(), value.to_string()));
        }
    }
    out
}

fn group<'a>(rec: &'a [(String, String)], code: &str) -> Option<&'a str> {
    rec.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

fn viewport_ids(l: &Layout) -> Vec<u32> {
    l.entities.iter().filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.id) } else { None }).collect()
}

/// True colours (entity and layer) and entity transparency: a new drawing's DWG is written as
/// AutoCAD 2018 (R2000 DWG has neither), while the DXF the bridge reads stays R2000.
#[test]
fn dwg_keeps_true_colours_and_transparency() {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Fancy".into(), color: Color::True(Rgb(12, 200, 99)), ..Layer::default() });
    let h = line(
        &mut d,
        &Space::Model,
        Common { layer: "Fancy".into(), color: Color::True(Rgb(1, 2, 3)), transparency: Transparency::Percent(40), ..Common::default() },
    );

    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1032"));
    let header = records(&cadcraft_io::write_dxf(&d), "SECTION");
    assert_eq!(header.first().and_then(|h| group(h, "1")), Some("AC1015"), "the DXF stays R2000");

    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let e = back.entity(h).unwrap();
    assert_eq!(e.common.color, Color::True(Rgb(1, 2, 3)));
    assert_eq!(e.common.transparency, Transparency::Percent(40));
    assert_eq!(back.layers.iter().find(|l| l.name == "Fancy").map(|l| l.color), Some(Color::True(Rgb(12, 200, 99))));
}

/// Entities of a layout other than the first stay in that layout.
#[test]
fn dwg_keeps_paper_space_entities_in_their_layout() {
    let mut d = Drawing::new_metric();
    let second = Space::Paper(d.layouts[1].name.clone());
    let h = d.add(&second, Common::default(), EntityKind::Circle(Circle { center: Vec3::new(1.0, 1.0, 0.0), radius: 0.5 })).unwrap();
    let back = through_dwg(&d);
    assert!(back.layouts[1].entities.contains(h), "{:?}", back.layouts.iter().map(|l| l.entities.len()).collect::<Vec<_>>());
    assert!(!back.layouts[0].entities.contains(h));
}

/// Table styles (R2000 DWG has no TABLESTYLE objects).
#[test]
fn dwg_keeps_table_styles() {
    let mut d = Drawing::new_metric();
    let sched = TableStyle { name: "Sched".into(), text_height: 2.5, margin: 1.0, title: false, header: true, ..TableStyle::default() };
    d.table_styles.push(sched.clone());
    let back = through_dwg(&d);
    assert_eq!(back.table_styles.iter().find(|s| s.name == "Sched"), Some(&sched));
}

/// DWG doesn't store viewport ids; readers number a layout's viewports in order, and id 1 is the
/// paper-space viewport (the sheet, not drawn). The writer puts that viewport ahead of a layout's
/// own viewports, so a viewport made in CADCraft (id 2) keeps its id instead of becoming the
/// sheet; our reader drops the added viewport again, so a DXF round trip is unchanged.
#[test]
fn viewport_ids_survive_dwg_and_the_paper_viewport_stays_implicit() {
    let mut d = Drawing::new_metric();
    let first = Space::Paper(d.layouts[0].name.clone());
    let h = d.add(&first, Common::default(), viewport(2)).unwrap();
    d.add(&first, Common::default(), viewport(3)).unwrap();

    let text = cadcraft_io::write_dxf(&d);
    let vps = records(&text, "VIEWPORT");
    assert_eq!(vps.len(), 3, "{text}");
    assert_eq!(group(&vps[0], "69"), Some("1"));
    assert_eq!(group(&vps[0], "1000"), Some("PAPERVIEW"));
    // Viewports are written "on": status 68 positive, and 90 with the always-set bit 32768.
    assert_eq!(group(&vps[1], "68"), Some("2"));
    assert_eq!(group(&vps[1], "90"), Some("32768"));

    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(viewport_ids(&back.layouts[0]), [2, 3]);
    assert_eq!(back.layouts[0].entities.len(), 2);

    let back = through_dwg(&d);
    assert_eq!(viewport_ids(&back.layouts[0]), [2, 3]);
    assert!(back.layouts[0].entities.contains(h));

    // A layout that has its paper-space viewport (from another program) gets no second one.
    let mut d = Drawing::new_metric();
    d.add(&first, Common::default(), viewport(1)).unwrap();
    d.add(&first, Common::default(), viewport(2)).unwrap();
    let text = cadcraft_io::write_dxf(&d);
    assert!(!text.contains("PAPERVIEW"));
    assert_eq!(viewport_ids(&cadcraft_io::read_dxf(text.as_bytes()).unwrap().layouts[0]), [1, 2]);
}

/// The paper-space viewport (id 1) of a drawing from another program can come after a model
/// viewport in the layout, e.g. when the draw order (SORTENTS) put it behind. DWG numbers in
/// order, so the writer puts it first: otherwise the model viewport became the sheet on reopen.
#[test]
fn a_paper_viewport_behind_a_model_viewport_stays_id_1_through_dwg() {
    let mut d = Drawing::new_metric();
    let names: Vec<String> = d.layouts.iter().map(|l| l.name.clone()).collect();
    for name in &names {
        let space = Space::Paper(name.clone());
        let model = d.add(&space, Common::default(), viewport(2)).unwrap();
        let sheet = d.add(&space, Common::default(), viewport(1)).unwrap();
        let back = through_dwg(&d);
        let l = back.layouts.iter().find(|l| &l.name == name).unwrap();
        let id = |h: Handle| match &l.entities.get(h).map(|e| &e.kind) {
            Some(EntityKind::Viewport(v)) => Some(v.id),
            _ => None,
        };
        assert_eq!((id(model), id(sheet)), (Some(2), Some(1)), "layout {name}");
    }
}

/// Each layout has its own printable origin. Changing the paper's margins/offset must not
/// translate its entities, and neither DXF nor native DWG saving may reset those offsets.
#[test]
fn two_layouts_keep_distinct_paper_origins_without_moving_viewports() {
    let mut d = Drawing::new_metric();
    d.layouts[0].page.margins_mm = [6.35; 4];
    d.layouts[1].page.margins_mm = [7.5, 20.0, 7.5, 20.0];
    d.layouts[1].page.offset_mm = [3.0, -2.0];
    let mut handles = Vec::new();
    for name in d.layouts.iter().map(|l| l.name.clone()).collect::<Vec<_>>() {
        handles.push(d.add(&Space::Paper(name), Common::default(), viewport(2)).unwrap());
    }
    for back in [cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap(), through_dwg(&d)] {
        for (before, after) in d.layouts.iter().zip(&back.layouts) {
            assert_eq!(after.page, before.page);
            assert_eq!(cadcraft_render::sheet(&back, &after.name), cadcraft_render::sheet(&d, &before.name));
        }
        for h in &handles {
            assert_eq!(back.entity(*h).unwrap().kind, d.entity(*h).unwrap().kind);
        }
        let first = cadcraft_render::sheet(&back, &back.layouts[0].name).unwrap();
        let second = cadcraft_render::sheet(&back, &back.layouts[1].name).unwrap();
        assert_eq!(first.origin, Vec2::new(-6.35, -6.35));
        assert_eq!(second.origin, Vec2::new(-23.0, -5.5));
    }
}
/// Foreign class versions are interpreted against the source DXF version before the
/// bridge converts them to the requested DWG. A 2013 record labelled 2000 was shifted twice.
#[test]
fn dwg_bridge_decodes_foreign_render_settings_at_the_source_version() {
    let source = concat!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n0\nENDSEC\n",
        "0\nSECTION\n2\nCLASSES\n",
        "0\nCLASS\n1\nMENTALRAYRENDERSETTINGS\n2\nAcDbMentalRayRenderSettings\n3\nSCENEOE\n90\n1024\n91\n1\n280\n0\n281\n0\n",
        "0\nCLASS\n1\nRAPIDRTRENDERSETTINGS\n2\nAcDbRapidRTRenderSettings\n3\nObjectDBX Classes\n90\n1024\n91\n1\n280\n0\n281\n0\n0\nENDSEC\n",
        "0\nSECTION\n2\nOBJECTS\n",
        "0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n3\nSyntheticMental\n350\n300\n3\nSyntheticRapid\n350\n301\n",
        "0\nMENTALRAYRENDERSETTINGS\n5\n300\n330\nC\n100\nAcDbRenderSettings\n90\n2\n1\nSynthetic\n",
        "100\nAcDbMentalRayRenderSettings\n90\n2\n",
        "0\nRAPIDRTRENDERSETTINGS\n5\n301\n330\nC\n100\nAcDbRenderSettings\n90\n2\n1\nSynthetic\n",
        "100\nAcDbRapidRTRenderSettings\n90\n3\n",
        "0\nENDSEC\n0\nEOF\n",
    );
    let doc = cadcraft_io::read_dxf(source.as_bytes()).unwrap();
    for version in [cadcraft_io::Version::R2013, cadcraft_io::Version::R2018] {
        let dwg = cadcraft_io::write_with(&doc, "settings.dwg", &cadcraft_io::SaveOptions { version: Some(version), binary: false }).unwrap();
        let native = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
        for (kind, expected) in
            [("MENTALRAYRENDERSETTINGS", "2"), ("RAPIDRTRENDERSETTINGS", if version == cadcraft_io::Version::R2013 { "2" } else { "3" })]
        {
            let settings = records(&native, kind);
            assert_eq!(settings.len(), 1, "{kind} at {version:?}");
            assert_eq!(group(&settings[0], "90"), Some(expected), "{kind} at {version:?}");
        }
    }
}

#[test]
fn saved_layout_points_to_its_overall_viewport_in_both_formats() {
    for explicit in [false, true] {
        let mut d = Drawing::new_metric();
        let paper = Space::Paper(d.layouts[0].name.clone());
        if explicit {
            d.add(&paper, Common::default(), viewport(1)).unwrap();
        }
        d.add(&paper, Common::default(), viewport(2)).unwrap();
        let dwg = cadcraft_io::write(&d, "sheet.dwg").unwrap();
        let native = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).unwrap();
        for text in [cadcraft_io::write_dxf(&d), native] {
            let overall = records(&text, "VIEWPORT").into_iter().find(|r| group(r, "69") == Some("1")).unwrap();
            let block = group(&overall, "330").unwrap();
            let layout =
                records(&text, "LAYOUT").into_iter().find(|r| r.iter().rev().find(|(c, _)| c == "330").is_some_and(|(_, h)| h == block)).unwrap();
            assert_eq!(group(&layout, "331"), group(&overall, "5"));
        }
    }
}
