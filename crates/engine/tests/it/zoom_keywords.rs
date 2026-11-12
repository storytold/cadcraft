//! Typed ZOOM Center, Scale, Dynamic and a plain scale factor do what they say, and ZOOM All /
//! Extents in paper space fit the sheet (#388).

use cadcraft_engine::{Session, View};
use cadcraft_geom::Vec2;
use serde_json::json;

fn view(s: &Session) -> View {
    s.state().unwrap().view()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn zoom_center_scale_dynamic_and_paper_space_all() {
    let mut s = Session::new();
    s.viewport_px = (1600.0, 900.0);
    s.execute("limits", &json!({ "min": [0, 0], "max": [420, 297] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 0], [1000, 0]] })).unwrap();

    // Center: point, then a height; nX is a magnification of the current height.
    s.cmdline("ZOOM C").unwrap();
    assert_eq!(s.current_prompt().unwrap().display(), "Specify center point:");
    s.cmdline("500,0").unwrap();
    assert!(s.current_prompt().unwrap().display().starts_with("Enter magnification or height <"));
    s.cmdline("100").unwrap();
    assert!(s.current_prompt().is_none());
    let v = view(&s);
    assert!(v.center.near(Vec2::new(500.0, 0.0), 1e-9) && near(v.height, 100.0), "{v:?}");
    s.script("ZOOM C 0,0 2X\n").unwrap();
    let v = view(&s);
    assert!(v.center.near(Vec2::ZERO, 1e-9) && near(v.height, 50.0), "{v:?}");

    // Scale: nX relative to the view; a plain n relative to the limits (1 fits them).
    s.script("ZOOM S 0.5X\n").unwrap();
    assert!(near(view(&s).height, 100.0), "{:?}", view(&s));
    s.cmdline("ZOOM 1").unwrap();
    // 420 × 297 at 16:9 is limited by the height.
    assert!(near(view(&s).height, 297.0) && view(&s).center.near(Vec2::ZERO, 1e-9), "{:?}", view(&s));
    s.cmdline("ZOOM S 2").unwrap();
    assert!(near(view(&s).height, 148.5), "{:?}", view(&s));

    // Dynamic: a point places the view box (the size of the view before), Enter shows it (#463).
    s.script("ZOOM D 80,45\n\n").unwrap();
    assert!(s.current_prompt().is_none());
    let v = view(&s);
    assert!(v.center.near(Vec2::new(80.0, 45.0), 1e-9) && near(v.height, 148.5), "{v:?}");

    // Paper space: All and Extents (no paper objects) fit the sheet, not the model limits.
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let sheet = s.zoom_limits().unwrap();
    assert!(sheet.min == Vec2::ZERO && !near(sheet.max.x, 420.0), "the sheet, not the limits: {sheet:?}");
    s.cmdline("ZOOM E").unwrap();
    assert!(view(&s).center.near(sheet.center(), 1e-9), "{:?} vs {sheet:?}", view(&s));
    s.cmdline("ZOOM A").unwrap();
    let v = view(&s);
    assert!(v.center.near(sheet.center(), 1e-9) && v.height < 297.0 && v.height >= sheet.height(), "{v:?} vs {sheet:?}");
}
