//! 3D views in DXF and DWG: a layout VIEWPORT's and the *Active VPORT's view direction (groups
//! 16/26/36) and target (17/27/37) are read and written back.

use cadcraft_doc::{Common, Drawing, EntityKind, Space, ViewDir, Viewport, VportConfig, VportTile};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{read_dxf, write_dxf};

fn view() -> ViewDir {
    ViewDir { dir: Vec3::new(1.0, -1.0, 1.0), target: Vec3::new(10.0, 20.0, 0.0) }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    let paper = Space::Paper(d.layouts[0].name.clone());
    let vp = Viewport { center: Vec3::new(100.0, 80.0, 0.0), width: 150.0, height: 100.0, id: 2, view3d: Some(view()), ..Viewport::default() };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let tile = VportTile { max: Vec2::new(1.0, 1.0), height: 10.0, aspect: 1.5, view3d: Some(view()), ..VportTile::default() };
    d.vports.push(VportConfig { name: VportConfig::ACTIVE.into(), tiles: vec![tile] });
    d
}

fn viewport_view(d: &Drawing) -> Option<ViewDir> {
    d.layouts[0].entities.iter().find_map(|e| match &e.kind {
        EntityKind::Viewport(v) if v.id == 2 => Some(v.view3d),
        _ => None,
    })?
}

fn active_view(d: &Drawing) -> Option<ViewDir> {
    d.vports.iter().find(|c| c.is_active())?.tiles.first()?.view3d
}

fn near(a: Option<ViewDir>, b: ViewDir) -> bool {
    a.is_some_and(|a| (a.dir - b.dir).len() < 1e-9 && (a.target - b.target).len() < 1e-9)
}

#[test]
fn view_directions_round_trip_through_dxf() {
    let text = write_dxf(&drawing());
    let back = read_dxf(text.as_bytes()).unwrap();
    assert!(near(viewport_view(&back), view()));
    assert!(near(active_view(&back), view()));
    // A plan view writes no 3D view and reads back as none.
    let plain = read_dxf(write_dxf(&Drawing::new_metric()).as_bytes()).unwrap();
    assert!(active_view(&plain).is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn view_directions_round_trip_through_dwg() {
    let dwg = cadcraft_io::write(&drawing(), "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert!(near(viewport_view(&back), view()));
}

#[test]
fn a_plan_viewport_centred_on_its_target_reads_as_a_model_centre() {
    let mut d = Drawing::new_metric();
    let paper = Space::Paper(d.layouts[0].name.clone());
    // As DWG files often store it: the view in the target, the centre relative to it.
    let in_target = ViewDir { dir: Vec3::Z, target: Vec3::new(10.0, 20.0, 0.0) };
    let vp = Viewport { width: 150.0, height: 100.0, view_center: Vec2::new(1.0, 2.0), id: 2, view3d: Some(in_target), ..Viewport::default() };
    d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let v = back.layouts[0]
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id == 2 => Some(v.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!((v.view_center, v.view3d), (Vec2::new(11.0, 22.0), None));
}
