//! A PDF underlay's page is drawn at its place in the drawing order (DRAWORDER): the display
//! list says before which primitive, for the canvas and plotters.

use cadcraft_doc::Handle;
use cadcraft_doc::{Common, Drawing, EntityKind, Line, PdfUnderlay, Space};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_render::{DisplayList, Options, PageClip, build, build_plot};

/// Each underlay's handle and the primitive its page comes before.
fn places(l: &DisplayList) -> Vec<(Handle, u32)> {
    l.underlays.iter().map(|u| (u.handle, u.at)).collect()
}

fn line(y: f64) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) })
}

#[test]
fn the_list_places_each_underlay_among_the_objects() {
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), line(0.0)).unwrap();
    let u = PdfUnderlay { size: Vec2::new(11.69, 8.27), path: "plan.pdf".into(), ..PdfUnderlay::default() };
    let h = d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    d.add(&Space::Model, Common::default(), line(5.0)).unwrap();
    let opts = Options::default();
    // After the first line's one primitive.
    assert_eq!(places(&build(&d, &Space::Model, &opts)), vec![(h, 1)]);
    assert_eq!(places(&build_plot(&d, &Space::Model, &opts)), vec![(h, 1)]);
    // DRAWORDER back: under everything; front: over the two lines.
    d.model.send_to_back(h);
    assert_eq!(places(&build(&d, &Space::Model, &opts)), vec![(h, 0)]);
    d.model.bring_to_front(h);
    assert_eq!(places(&build(&d, &Space::Model, &opts)), vec![(h, 2)]);
    // Not in another space's list.
    assert!(build(&d, &Space::Paper("Layout1".into()), &opts).underlays.is_empty());
}

/// Past 512 objects model space is built in chunks on all cores, then joined.
#[test]
fn a_large_drawing_built_on_all_cores_keeps_the_place() {
    let mut d = Drawing::new_metric();
    for i in 0..700 {
        d.add(&Space::Model, Common::default(), line(f64::from(i))).unwrap();
    }
    let u = PdfUnderlay { size: Vec2::new(11.69, 8.27), path: "plan.pdf".into(), ..PdfUnderlay::default() };
    let h = d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    for i in 0..10 {
        d.add(&Space::Model, Common::default(), line(-f64::from(i))).unwrap();
    }
    let list = build(&d, &Space::Model, &Options::default());
    assert_eq!(places(&list), vec![(h, 700)]);
}

/// A drawing whose model space has an underlay, and Layout1 a viewport at 1:2 on paper at
/// (100, 80) looking at `look` (default: the page's middle third); its underlay's handle and axes.
fn layout_with_viewport(look: Option<Vec2>) -> (Drawing, Handle, Vec2, Vec2) {
    let mut d = Drawing::new_metric();
    let u = PdfUnderlay { size: Vec2::new(11.69, 8.27), path: "plan.pdf".into(), ..PdfUnderlay::default() };
    let (au, av) = u.axes();
    let h = d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
    let vp = cadcraft_doc::Viewport {
        center: Vec3::new(100.0, 80.0, 0.0),
        width: au.len() / 6.0,
        height: av.len() / 6.0,
        view_center: look.unwrap_or((au + av) * 0.5),
        view_height: av.len() / 3.0,
        id: 2,
        ..cadcraft_doc::Viewport::default()
    };
    d.add(&Space::Paper("Layout1".into()), Common::default(), EntityKind::Viewport(vp)).unwrap();
    (d, h, au, av)
}

#[test]
fn a_layout_viewport_shows_the_page_clipped_to_it() {
    let paper = Space::Paper("Layout1".into());
    let (d, h, au, _) = layout_with_viewport(None);
    let list = build(&d, &paper, &Options::default());
    let [lu] = list.underlays.as_slice() else { panic!("one page: {:?}", list.underlays) };
    assert_eq!(lu.handle, h);
    // The page at half size on paper, its middle on the viewport's centre.
    assert!((lu.u.len() - au.len() / 2.0).abs() < 1e-6);
    assert!((lu.origin + (lu.u + lu.v) * 0.5 - Vec2::new(100.0, 80.0)).len() < 1e-6);
    assert!(matches!(lu.clip, PageClip::Rect(_)));
    let shown = lu.shown().unwrap();
    assert_eq!(lu.shown_of(&lu.triangles()), Some(shown), "from triangles already computed: the same part");
    for (got, want) in shown.iter().zip([1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]) {
        assert!((got - want).abs() < 1e-6, "{shown:?}");
    }
    // A viewport looking elsewhere shows none of it.
    let (far, ..) = layout_with_viewport(Some(Vec2::new(-1e4, -1e4)));
    assert!(build(&far, &paper, &Options::default()).underlays.iter().all(|u| u.shown().is_none()));
}
