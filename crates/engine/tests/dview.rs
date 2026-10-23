//! DVIEW starts with "Select objects or <use DVIEWBLOCK>:", then the option prompt;
//! TWist, PAn, Zoom, Off and Undo on a 2D view; the 3D options not available yet. It works on the
//! Model tab and inside an active layout viewport (whose twist is an undoable edit of the
//! viewport), and not on a layout sheet.

use cadcraft_doc::{EntityKind, Handle, Viewport};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session, View};
use serde_json::json;

const SELECT: &str = "Select objects or <use DVIEWBLOCK>:";
const OPTION: &str = "Enter option [CAmera/TArget/Distance/POints/PAn/Zoom/TWist/CLip/Hide/Off/Undo]:";
const PAPER: &str = "This command is not available in Paper space.";

fn session() -> Session {
    let mut s = Session::new();
    s.viewport_px = (1200.0, 800.0);
    s.execute("line", &json!({ "points": [[0, 0], [100, 50]] })).unwrap();
    s.execute("view.set", &json!({ "center": [10, 10], "height": 20 })).unwrap();
    s
}

fn prompt(s: &Session) -> String {
    s.current_prompt().map(|p| p.display()).unwrap_or_default()
}

fn view(s: &Session) -> View {
    s.state().unwrap().view()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn twist_deg(s: &Session) -> f64 {
    view(s).twist.to_degrees()
}

fn logged(s: &Session, m: &str) -> bool {
    s.log.iter().any(|l| l == m)
}

#[test]
fn every_prompt_step_on_the_model_tab() {
    let mut s = session();
    let undo = s.state().unwrap().undo.len();
    s.cmdline("dview").unwrap();
    assert_eq!(prompt(&s), SELECT);
    // The whole drawing shows while objects are picked.
    assert!(s.display_only().is_none());
    // Enter with nothing selected: the DVIEWBLOCK sample (a house of lines) is the preview set.
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), OPTION);
    let only = s.display_only().unwrap();
    assert!(only.handles.is_empty());
    assert!(only.extra.len() > 10 && only.extra.iter().all(|e| matches!(e.kind, EntityKind::Line(_))));

    // TWist: the current twist is the default; a typed angle turns the view.
    s.cmdline("tw").unwrap();
    assert_eq!(prompt(&s), "Specify view twist angle <0>:");
    s.cmdline("30").unwrap();
    assert!(near(twist_deg(&s), 30.0));
    assert_eq!(prompt(&s), OPTION);
    s.cmdline("TWist").unwrap();
    assert_eq!(prompt(&s), "Specify view twist angle <30>:");
    // Enter keeps it.
    s.cmdline("").unwrap();
    assert!(near(twist_deg(&s), 30.0));

    // PAn: base point, second point.
    s.cmdline("pa").unwrap();
    assert_eq!(prompt(&s), "Specify displacement base point:");
    s.cmdline("0,0").unwrap();
    assert_eq!(prompt(&s), "Specify second point:");
    s.cmdline("4,0").unwrap();
    assert_eq!(view(&s).center, Vec2::new(6.0, 10.0));
    assert_eq!(prompt(&s), OPTION);

    // Zoom: a scale factor of the current view.
    s.cmdline("z").unwrap();
    assert_eq!(prompt(&s), "Specify zoom scale factor <1>:");
    s.cmdline("2").unwrap();
    assert!(near(view(&s).height, 10.0));
    // The sample house stays where it was put: the view moves over it.
    assert_eq!(s.display_only().unwrap(), only);

    // Off: perspective is never on in a 2D view, so nothing changes.
    let before = view(&s);
    s.cmdline("o").unwrap();
    assert_eq!(prompt(&s), OPTION);
    assert_eq!(view(&s), before);

    // The options that need a 3D camera are not available yet and stay at the option prompt.
    for (typed, name) in [("ca", "CAmera"), ("ta", "TArget"), ("d", "Distance"), ("po", "POints"), ("cl", "CLip"), ("h", "Hide")] {
        s.cmdline(typed).unwrap();
        assert!(logged(&s, &format!("DVIEW {name} is not available yet.")), "{name}: {:?}", s.log);
        assert_eq!(prompt(&s), OPTION);
    }
    assert!(!s.log.iter().any(|l| l.contains("Only the view twist")));

    // Undo takes back one DVIEW action at a time: zoom, pan, twist.
    s.cmdline("u").unwrap();
    assert!(near(view(&s).height, 20.0));
    s.cmdline("u").unwrap();
    assert_eq!(view(&s).center, Vec2::new(10.0, 10.0));
    assert!(near(twist_deg(&s), 30.0));
    s.cmdline("u").unwrap();
    assert_eq!(view(&s).twist, 0.0);
    // Nothing left to undo: still at the option prompt.
    s.cmdline("u").unwrap();
    assert_eq!(prompt(&s), OPTION);

    // An unknown option is refused and asked again.
    assert!(s.cmdline("bogus").is_ok());
    assert_eq!(prompt(&s), OPTION);

    // Enter ends DVIEW; the whole drawing shows again, and a Model tab twist is no undo step.
    s.cmdline("tw").unwrap();
    s.cmdline("45").unwrap();
    s.cmdline("").unwrap();
    assert!(s.current_prompt().is_none());
    assert!(s.display_only().is_none());
    assert!(near(twist_deg(&s), 45.0));
    assert_eq!(s.state().unwrap().undo.len(), undo);
}

#[test]
fn selected_objects_are_the_preview_set() {
    let mut s = session();
    let line = s.doc().unwrap().model.iter().next().map(|e| e.handle).unwrap();
    s.execute("line", &json!({ "points": [[0, 10], [10, 10]] })).unwrap();
    s.cmdline("dview").unwrap();
    s.input(Input::Pick(vec![line])).unwrap();
    assert_eq!(prompt(&s), SELECT);
    s.cmdline("").unwrap();
    assert_eq!(prompt(&s), OPTION);
    let only = s.display_only().unwrap();
    assert_eq!(only.handles, vec![line]);
    assert!(only.extra.is_empty(), "no sample house with a selection");
    // Picked at the prompt, the objects don't stay selected.
    assert!(s.selection().is_empty());
    // Escape ends it too, and the drawing shows again.
    s.input(Input::Cancel).unwrap();
    assert!(s.current_prompt().is_none());
    assert!(s.display_only().is_none());
}

#[test]
fn a_dviewblock_of_the_drawings_own_is_previewed() {
    let mut s = session();
    s.execute("circle", &json!({ "center": [0.5, 0.5], "radius": 0.5 })).unwrap();
    let c = s.doc().unwrap().model.iter().last().map(|e| e.handle).unwrap();
    s.execute("block", &json!({ "name": "DVIEWBLOCK", "base": [0, 0], "handles": [c.hex()], "keep": "delete" })).unwrap();
    s.cmdline("dview").unwrap();
    s.cmdline("").unwrap();
    let only = s.display_only().unwrap();
    assert_eq!(only.extra.len(), 1);
    match &only.extra[0].kind {
        EntityKind::Insert(i) => {
            assert_eq!(i.block, "DVIEWBLOCK");
            // Scaled from its 1 by 1 unit area to half the view's height.
            assert!(near(i.scale.x, 10.0) && near(i.scale.y, 10.0));
        }
        k => panic!("{k:?}"),
    }
    s.cmdline("").unwrap();
}

/// A layout with one viewport, `(session, viewport handle)`.
fn layout_session() -> (Session, Handle) {
    let mut s = session();
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let r = s.execute("mview", &json!({ "p1": [20, 20], "p2": [220, 120] })).unwrap();
    let h = Handle::parse_hex(r["viewports"][0].as_str().unwrap()).unwrap();
    (s, h)
}

fn vp(s: &Session, h: Handle) -> Viewport {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Viewport(v) => v.clone(),
        _ => panic!("not a viewport"),
    }
}

#[test]
fn on_a_layout_sheet_it_is_not_available() {
    let (mut s, _) = layout_session();
    assert_eq!(s.execute("dview", &json!({ "twist": 10 })).unwrap_err().to_string(), PAPER);
    assert_eq!(s.cmdline("dview").unwrap_err().to_string(), PAPER);
    assert!(s.current_prompt().is_none());
    // view.set can't turn the sheet either; an untwisted view is fine.
    assert_eq!(s.execute("view.set", &json!({ "center": [0, 0], "height": 5, "twist": 15 })).unwrap_err().to_string(), PAPER);
    s.execute("view.set", &json!({ "center": [0, 0], "height": 5 })).unwrap();
}

#[test]
fn inside_an_active_viewport_it_twists_that_viewport_as_one_undo_step() {
    let (mut s, h) = layout_session();
    s.execute("mspace", &json!({ "handle": h.hex() })).unwrap();
    let undo = s.state().unwrap().undo.len();
    let pv = s.state().unwrap().paper_view();
    s.cmdline("dview").unwrap();
    assert_eq!(prompt(&s), SELECT);
    s.cmdline("").unwrap();
    s.cmdline("tw").unwrap();
    s.cmdline("30").unwrap();
    // The viewport entity carries the twist, and the model view inside it is turned by it.
    assert!(near(vp(&s, h).twist.to_degrees(), 30.0));
    assert!(near(twist_deg(&s), 30.0));
    // The sheet itself is not turned.
    assert_eq!(s.state().unwrap().paper_view(), pv);
    s.cmdline("tw").unwrap();
    s.cmdline("50").unwrap();
    s.cmdline("").unwrap();
    assert!(s.current_prompt().is_none());
    assert!(near(vp(&s, h).twist.to_degrees(), 50.0));
    // One undo step for the whole DVIEW, which turns the viewport back.
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(vp(&s, h).twist, 0.0);

    // The JSON form and view.set twist the viewport the same way, each one undo step.
    s.execute("dview", &json!({ "twist": -90 })).unwrap();
    assert!(near(vp(&s, h).twist.to_degrees(), 270.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    let v = view(&s);
    s.execute("view.set", &json!({ "center": [v.center.x, v.center.y], "height": v.height, "twist": 15 })).unwrap();
    assert!(near(vp(&s, h).twist.to_degrees(), 15.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 2);
    // Setting a view without a twist keeps the viewport's.
    s.execute("view.set", &json!({ "center": [v.center.x, v.center.y], "height": v.height })).unwrap();
    assert!(near(vp(&s, h).twist.to_degrees(), 15.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 2);

    // Back on the sheet, DVIEW is not available again.
    s.execute("pspace", &json!({})).unwrap();
    assert_eq!(s.cmdline("dview").unwrap_err().to_string(), PAPER);
}

#[test]
fn escape_inside_an_active_viewport_keeps_the_twist_as_one_undo_step() {
    let (mut s, h) = layout_session();
    s.execute("mspace", &json!({ "handle": h.hex() })).unwrap();
    let undo = s.state().unwrap().undo.len();
    for l in ["dview", "", "tw", "30"] {
        s.cmdline(l).unwrap();
    }
    s.input(Input::Cancel).unwrap();
    assert!(s.current_prompt().is_none());
    assert!(near(vp(&s, h).twist.to_degrees(), 30.0));
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(vp(&s, h).twist, 0.0);
}

#[test]
fn the_view_inside_a_twisted_viewport_maps_the_screen_like_the_sheet() {
    let (mut s, h) = layout_session();
    s.execute("mspace", &json!({ "handle": h.hex() })).unwrap();
    s.execute("dview", &json!({ "twist": 30 })).unwrap();
    let st = s.state().unwrap();
    let (vpe, pv, v) = (vp(&s, h), st.paper_view(), st.view());
    // The model view's centre is the model point at the sheet's view centre.
    assert!((vpe.model_to_paper(v.center) - pv.center).len() < 1e-9);
    assert!(near(v.twist, vpe.twist));
    // Panning the view inside the viewport moves its model view, keeping the twist.
    let moved = View { center: v.center + Vec2::new(5.0, -2.0), ..v };
    s.state_mut().unwrap().set_view(moved);
    let after = view(&s);
    assert!((after.center - moved.center).len() < 1e-9, "{after:?}");
    assert!(near(after.twist, vpe.twist));
    assert_eq!(s.state().unwrap().paper_view(), pv);
    // ZOOM Extents fits the turned box of the drawing in the viewport.
    s.execute("zoom", &json!({ "mode": "extents" })).unwrap();
    assert!(near(view(&s).twist, vpe.twist));
}

#[test]
fn a_viewport_twist_survives_a_dxf_round_trip() {
    let (mut s, h) = layout_session();
    s.execute("mspace", &json!({ "handle": h.hex() })).unwrap();
    s.execute("dview", &json!({ "twist": 30 })).unwrap();
    let dxf = cadcraft_io::write_dxf(s.doc().unwrap());
    let back = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let twists: Vec<f64> = back
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(v) if v.id != 1 => Some(v.twist.to_degrees()),
            _ => None,
        })
        .collect();
    assert_eq!(twists.len(), 1);
    assert!((twists[0] - 30.0).abs() < 1e-9, "{twists:?}");
}
