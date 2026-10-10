//! Data that used to be lost only when a drawing is saved as DWG and reopened (the DWG bridge
//! converts our DXF with acadrust), and the paper-space viewport the DXF writer adds for it.
#![cfg(not(target_arch = "wasm32"))]

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

/// True colours (entity and layer) and entity transparency: the DWG is written as AutoCAD 2004
/// (R2000 DWG has neither), while the DXF stays R2000.
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
    assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1018"));
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
    let sched = TableStyle { name: "Sched".into(), text_height: 2.5, margin: 1.0, title: false, header: true };
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
