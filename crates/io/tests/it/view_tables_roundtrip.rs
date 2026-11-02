//! DXF save → reopen keeps the VIEW table's paper space flag (with the view's layout), the view
//! twist and the layer snapshot, and the VPORT table's tiled model viewports and saved viewport
//! configurations (#463).

use cadcraft_doc::geom::Vec2;
use cadcraft_doc::{Drawing, NamedView, VportConfig, VportTile};
use cadcraft_io::{read_dxf, write_dxf};

fn round_trip(d: &Drawing) -> Drawing {
    read_dxf(write_dxf(d).as_bytes()).expect("reopen")
}

#[test]
fn model_and_paper_views_round_trip() {
    let mut d = Drawing::new_imperial();
    let layout = d.layouts.first().map(|l| l.name.clone()).expect("a layout");
    d.views.push(NamedView { name: "Plan".into(), center: Vec2::new(5.0, 5.0), height: 20.0, width: 30.0, twist: 0.5, ..Default::default() });
    d.views.push(NamedView {
        name: "Sheet".into(),
        center: Vec2::new(4.0, 3.0),
        height: 8.0,
        width: 11.0,
        layer_state: Some("*VIEW Sheet".into()),
        layout: Some(layout),
        ..Default::default()
    });
    // A paper space view of an unknown layout stays a paper space view.
    d.views.push(NamedView { name: "Loose".into(), center: Vec2::ZERO, height: 1.0, width: 1.0, layout: Some(String::new()), ..Default::default() });
    let r = round_trip(&d);
    assert_eq!(r.views.len(), 3);
    for (a, b) in d.views.iter().zip(&r.views) {
        assert_eq!(
            (&a.name, a.center, a.height, a.width, &a.layer_state, &a.layout),
            (&b.name, b.center, b.height, b.width, &b.layer_state, &b.layout)
        );
        assert!((a.twist - b.twist).abs() < 1e-12, "{}: {} vs {}", a.name, a.twist, b.twist);
    }
}

/// Another program's paper space view (flag 70 = 1, no CADCraft xdata) and a hostile twist.
#[test]
fn foreign_paper_view_and_hostile_twist() {
    let tags: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "TABLES"),
        (0, "TABLE"),
        (2, "VIEW"),
        (0, "VIEW"),
        (2, "PaperOne"),
        (70, "1"),
        (40, "10"),
        (41, "16"),
        (10, "3"),
        (20, "4"),
        (50, "90"),
        (0, "VIEW"),
        (2, "Odd"),
        (70, "0"),
        (40, "2"),
        (41, "2"),
        (50, "nan"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    let dxf: String = tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect();
    let d = read_dxf(dxf.as_bytes()).expect("read");
    let paper = d.views.iter().find(|v| v.name == "PaperOne").expect("PaperOne");
    assert_eq!(paper.layout.as_deref(), Some(""));
    assert!((paper.twist - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    let odd = d.views.iter().find(|v| v.name == "Odd").expect("Odd");
    assert_eq!((odd.twist, odd.layout.as_deref()), (0.0, None));
    let again = round_trip(&d);
    assert_eq!(again.views, d.views);
}

fn tile(min: (f64, f64), max: (f64, f64), center: (f64, f64), height: f64) -> VportTile {
    VportTile { min: Vec2::new(min.0, min.1), max: Vec2::new(max.0, max.1), center: Vec2::new(center.0, center.1), height, aspect: 0.75 }
}

#[test]
fn tiled_viewports_and_saved_configurations_round_trip() {
    let mut d = Drawing::new_imperial();
    d.header.set("SNAPBASE", cadcraft_doc::HVal::Point(cadcraft_doc::geom::Vec3::new(1.0, 2.0, 0.0)));
    let active = VportConfig {
        name: VportConfig::ACTIVE.into(),
        tiles: vec![tile((0.5, 0.0), (1.0, 1.0), (5.0, 5.0), 10.0), tile((0.0, 0.0), (0.5, 1.0), (-3.0, 4.0), 2.5)],
    };
    let quad = VportConfig {
        name: "Quad".into(),
        tiles: vec![
            tile((0.0, 0.5), (0.5, 1.0), (1.0, 1.0), 1.0),
            tile((0.5, 0.5), (1.0, 1.0), (2.0, 2.0), 2.0),
            tile((0.0, 0.0), (0.5, 0.5), (3.0, 3.0), 3.0),
            tile((0.5, 0.0), (1.0, 0.5), (4.0, 4.0), 4.0),
        ],
    };
    d.vports = vec![active, quad];
    let r = round_trip(&d);
    assert_eq!(r.vports, d.vports);
    // The first *ACTIVE record still carries the snap base.
    assert_eq!(r.header.point("SNAPBASE").map(|p| p.xy()), Some(Vec2::new(1.0, 2.0)));
    // No tiling: one *ACTIVE record, read back as no configuration.
    let mut single = Drawing::new_imperial();
    single.vports = vec![VportConfig { name: VportConfig::ACTIVE.into(), tiles: vec![tile((0.0, 0.0), (1.0, 1.0), (0.0, 0.0), 1.0)] }];
    assert!(round_trip(&single).vports.is_empty());
}

/// VPORT records with corners outside 0..1, no area (or turned around), or an unusable view
/// height are skipped.
#[test]
fn hostile_vport_records_are_skipped() {
    let rec = |name: &str, x0: &str, x1: &str, h: &str| -> Vec<(i32, String)> {
        [(0, "VPORT"), (2, name), (10, x0), (20, "0"), (11, x1), (21, "1"), (12, "1"), (22, "1"), (40, h), (41, "1.5")]
            .iter()
            .map(|(c, v)| (*c, v.to_string()))
            .collect()
    };
    let mut tags: Vec<(i32, String)> = vec![(0, "SECTION".into()), (2, "TABLES".into()), (0, "TABLE".into()), (2, "VPORT".into())];
    tags.extend(rec("*ACTIVE", "0", "0.5", "4"));
    tags.extend(rec("*ACTIVE", "0.5", "1", "8"));
    tags.extend(rec("*ACTIVE", "-3", "0.5", "4"));
    tags.extend(rec("*ACTIVE", "0.2", "0.2", "4"));
    tags.extend(rec("*ACTIVE", "0.9", "0.1", "nan"));
    tags.extend(rec("Saved", "0", "1", "0"));
    tags.extend(rec("Saved", "0", "1", "inf"));
    tags.extend([(0, "ENDTAB".into()), (0, "ENDSEC".into()), (0, "EOF".into())]);
    let dxf: String = tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect();
    let d = read_dxf(dxf.as_bytes()).expect("read");
    assert_eq!(d.vports.len(), 1, "{:?}", d.vports);
    let active = d.vports.first().expect("*Active");
    assert!(active.is_active());
    assert_eq!(active.tiles.iter().map(|t| t.height).collect::<Vec<_>>(), [4.0, 8.0]);
    assert_eq!(round_trip(&d).vports, d.vports);
}
