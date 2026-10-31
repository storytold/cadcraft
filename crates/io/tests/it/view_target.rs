//! A view's centre is stored in display coordinates, whose origin is the view target (DXF
//! Reference: VIEWPORT 12/22 and 17/27/37, VPORT 12/22 and 17/27/37, VIEW 10/20 and 12/22/32).
//! ArchiCAD's AutoCAD 2018 export puts each layout viewport's location in the target with a
//! centre of 0,0, so reading the centre alone showed model point 0,0 (#2492).

use cadcraft_doc::geom::Vec2;
use cadcraft_doc::{Drawing, EntityKind, Viewport};
use cadcraft_io::{read_dxf, write_dxf};

/// An ASCII DXF of `tags`.
fn dxf(tags: &[(i32, &str)]) -> String {
    tags.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A layout viewport with view centre `center` (12/22) and view target `target` (17/27/37).
fn with_viewport(center: [&str; 2], target: [&str; 3]) -> Drawing {
    let tags: Vec<(i32, &str)> = vec![
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "VIEWPORT"),
        (5, "40"),
        (67, "1"),
        (8, "0"),
        (10, "148.5"),
        (20, "105"),
        (30, "0"),
        (40, "250"),
        (41, "180"),
        (68, "2"),
        (69, "2"),
        (12, center[0]),
        (22, center[1]),
        (16, "0"),
        (26, "0"),
        (36, "1"),
        (17, target[0]),
        (27, target[1]),
        (37, target[2]),
        (45, "48680.2"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    read_dxf(dxf(&tags).as_bytes()).expect("read")
}

fn viewports(d: &Drawing) -> Vec<Viewport> {
    d.layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id > 1 => Some(v.clone()),
            _ => None,
        })
        .collect()
}

fn view_center(d: &Drawing) -> Vec2 {
    let vs = viewports(d);
    assert_eq!(vs.len(), 1, "{vs:?}");
    vs[0].view_center
}

/// `a` and `b` agree to within the rounding of adding two decimals.
fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < 1e-6
}

#[test]
fn a_viewport_looks_at_its_target_plus_its_centre() {
    // The issue's file: centre 0,0, the facade in the target.
    let c = view_center(&with_viewport(["0", "0"], ["230748.7", "-867.4", "0"]));
    assert_eq!(c, Vec2::new(230748.7, -867.4));
    // Both set: their sum. A target alone of 0 leaves the centre as it is.
    assert_eq!(view_center(&with_viewport(["10", "5"], ["100", "50", "7"])), Vec2::new(110.0, 55.0));
    assert_eq!(view_center(&with_viewport(["10", "5"], ["0", "0", "0"])), Vec2::new(10.0, 5.0));
}

/// Saved and reopened, the viewport still looks at the same place: CADCraft writes the whole
/// location as the centre with a target of 0.
#[test]
fn the_view_survives_a_save_and_reopen() {
    let d = with_viewport(["3", "4"], ["84672.2", "25851.5", "0"]);
    let back = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    let c = view_center(&back);
    assert!(close(c, Vec2::new(84675.2, 25855.5)), "{c:?}");
}

/// A target coordinate that is not a finite number reads as 0, as any DXF real does, and a sum
/// that overflows leaves the centre.
#[test]
fn a_hostile_target_leaves_the_centre() {
    for target in [["1e999", "0", "0"], ["nan", "0", "0"], ["1e308", "0", "0"]] {
        assert_eq!(view_center(&with_viewport(["1e308", "2"], target)), Vec2::new(1e308, 2.0), "{target:?}");
    }
    assert_eq!(view_center(&with_viewport(["1", "2"], ["inf", "5", "0"])), Vec2::new(1.0, 7.0));
}

/// The model viewport tiles of a saved configuration (VPORT 12/22 and 17/27/37) and named views
/// (VIEW 10/20 and 12/22/32) are read the same way.
#[test]
fn vport_tiles_and_named_views_add_their_targets() {
    let tags: &[(i32, &str)] = &[
        (0, "SECTION"),
        (2, "TABLES"),
        (0, "TABLE"),
        (2, "VPORT"),
        (0, "VPORT"),
        (2, "TWO"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (11, "0.5"),
        (21, "1"),
        (12, "1"),
        (22, "2"),
        (17, "100"),
        (27, "200"),
        (37, "0"),
        (40, "50"),
        (41, "1"),
        (0, "VPORT"),
        (2, "TWO"),
        (70, "0"),
        (10, "0.5"),
        (20, "0"),
        (11, "1"),
        (21, "1"),
        (12, "0"),
        (22, "0"),
        (17, "-40"),
        (27, "30"),
        (37, "0"),
        (40, "50"),
        (41, "1"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "VIEW"),
        (0, "VIEW"),
        (2, "Facade"),
        (70, "0"),
        (40, "20"),
        (41, "30"),
        (10, "1"),
        (20, "2"),
        (11, "0"),
        (21, "0"),
        (31, "1"),
        (12, "367097.3"),
        (22, "-441.8"),
        (32, "0"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ];
    let d = read_dxf(dxf(tags).as_bytes()).expect("read");
    let two = d.vports.iter().find(|c| c.name.eq_ignore_ascii_case("TWO")).expect("configuration TWO");
    let centers: Vec<Vec2> = two.tiles.iter().map(|t| t.center).collect();
    assert_eq!(centers, [Vec2::new(101.0, 202.0), Vec2::new(-40.0, 30.0)]);
    let facade = d.views.iter().find(|v| v.name == "Facade").expect("view Facade");
    assert!(close(facade.center, Vec2::new(367098.3, -439.8)), "{:?}", facade.center);
    // And again after a save and reopen.
    let back = read_dxf(write_dxf(&d).as_bytes()).expect("reopen");
    let two = back.vports.iter().find(|c| c.name.eq_ignore_ascii_case("TWO")).expect("configuration TWO");
    assert_eq!(two.tiles.iter().map(|t| t.center).collect::<Vec<_>>(), centers);
    let again = back.views.iter().find(|v| v.name == "Facade").map(|v| v.center).expect("view Facade");
    assert!(close(again, Vec2::new(367098.3, -439.8)), "{again:?}");
}
