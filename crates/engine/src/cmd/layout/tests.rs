use super::*;

fn session_with_model() -> Session {
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("circle 50,25 20").unwrap();
    s
}

fn viewports(s: &Session, layout: &str) -> Vec<(Handle, Viewport)> {
    s.doc()
        .unwrap()
        .layout(layout)
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some((e.handle, v.clone())) } else { None })
        .collect()
}

#[test]
fn new_layout_gets_viewport_zoomed_to_extents() {
    let mut s = session_with_model();
    let r = s.execute("layout.new", &json!({})).unwrap();
    assert_eq!(r["name"], "Layout3");
    let vps = viewports(&s, "Layout3");
    assert_eq!(vps.len(), 1);
    let (_, v) = &vps[0];
    let sheet = cadcraft_render::sheet(s.doc().unwrap(), "Layout3").unwrap();
    assert!((v.width - sheet.printable.width()).abs() < 1e-9 && (v.height - sheet.printable.height()).abs() < 1e-9);
    assert!(v.id >= 2);
    // Model extents (0,0)..(100,45) are centred and fully visible.
    assert!(v.view_center.near(Vec2::new(50.0, 22.5), 1e-6));
    let model_w = v.view_height * v.width / v.height;
    assert!(model_w >= 100.0 && v.view_height >= 45.0);
    // The render shows model geometry inside the viewport only.
    let list = cadcraft_render::build(s.doc().unwrap(), &Space::Paper("Layout3".into()), &cadcraft_render::Options::default());
    let rect = Bounds2::new(v.center.xy() - Vec2::new(v.width, v.height) / 2.0, v.center.xy() + Vec2::new(v.width, v.height) / 2.0).expand(1e-9);
    let content: Vec<_> = list.prims.iter().filter(|p| p.handle == cadcraft_render::VIEWPORT_CONTENT).collect();
    assert!(content.len() >= 2);
    assert!(content.iter().all(|p| list.points(p).iter().all(|q| rect.contains(*q))));
    assert!(list.sheet.is_some());
    // Undo removes the layout.
    s.undo().unwrap();
    assert!(s.doc().unwrap().layout("Layout3").is_none());
}

#[test]
fn rename_keeps_the_active_layout_whatever_its_case() {
    let mut s = session_with_model();
    s.doc_mut().unwrap().active_layout = Some("layout1".into());
    s.execute("layout.rename", &json!({"from": "Layout1", "to": "Sheet A"})).unwrap();
    assert_eq!(s.doc().unwrap().active_layout.as_deref(), Some("Sheet A"));
}

#[test]
fn rename_copy_delete() {
    let mut s = session_with_model();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.execute("layout.rename", &json!({"to": "Sheet A"})).unwrap();
    assert_eq!(s.space(), Space::Paper("Sheet A".into()));
    assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "Layout2"})).is_err(), "duplicate");
    assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "a/b"})).is_err(), "bad char");
    assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "model"})).is_err(), "reserved");
    s.execute("mview", &json!({"p1": [1, 1], "p2": [5, 4]})).unwrap();
    let r = s.execute("layout.copy", &json!({"from": "Sheet A"})).unwrap();
    assert_eq!(r["name"], "Sheet A (2)");
    let a = viewports(&s, "Sheet A");
    let b = viewports(&s, "Sheet A (2)");
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_ne!(a[0].0, b[0].0, "copied entities get new handles");
    s.execute("layout.delete", &json!({"name": "sheet a"})).unwrap();
    assert_eq!(s.space(), Space::Model, "deleting the current layout returns to Model");
    s.execute("layout", &json!({"option": "delete", "name": "Layout2"})).unwrap();
    assert!(s.execute("layout.delete", &json!({"name": "Sheet A (2)"})).is_err(), "last layout stays");
    let l = s.execute("layout", &json!({"option": "list"})).unwrap();
    assert_eq!(l.as_array().unwrap().len(), 1);
    assert!(s.execute("layout.delete", &json!({"name": "Model"})).is_err());
}

#[test]
fn new_layout_from_the_menu_asks_for_a_name() {
    let mut s = session_with_model();
    s.start("layout.new").unwrap();
    assert_eq!(s.current_prompt().map(|p| p.display()).as_deref(), Some("Enter name of new layout <Layout3>:"));
    s.cmdline("Layout1").unwrap();
    assert!(s.running.is_some(), "a taken name asks again");
    s.cmdline("Sheet A").unwrap();
    // The name is taken; the model area to show is asked next. Enter takes the whole drawing.
    assert!(s.running.is_some() && s.current_prompt().is_some_and(|p| p.message.contains("first corner of the model area")));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    s.start("layout.new").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    let d = s.doc().unwrap();
    assert!(d.layout("Sheet A").is_some() && d.layout("Layout3").is_some(), "Enter takes the default name");
}

/// The viewport of layout `name`.
fn only_viewport(s: &Session, name: &str) -> Viewport {
    let v = viewports(s, name);
    assert_eq!(v.len(), 1, "one viewport");
    v.into_iter().next().map(|(_, v)| v).unwrap()
}

/// A `window` zooms the new layout's viewport to that part of the model.
#[test]
fn a_new_layout_can_show_a_window_of_the_model() {
    let mut s = session_with_model();
    s.execute("layout.new", &json!({"name": "Sheet W", "window": [[10, 10], [30, 20]]})).unwrap();
    let v = only_viewport(&s, "Sheet W");
    assert!(v.view_center.dist(Vec2::new(20.0, 15.0)) < 1e-9, "centred on the window: {:?}", v.view_center);
    let aspect = v.width / v.height;
    assert!(v.view_height >= 10.0 && v.view_height * aspect >= 20.0, "the whole window is in view");
    assert!(v.view_height <= (10.0_f64).max(20.0 / aspect) * 1.02 + 1e-9, "and not much more: {}", v.view_height);
    // Without a window the viewport shows the model extents, as before.
    s.execute("layout.new", &json!({"name": "Sheet E"})).unwrap();
    let e = only_viewport(&s, "Sheet E");
    assert!(e.view_height > v.view_height * 2.0, "extents are larger than the window");
}

/// A window without area, or not two corners, is an error that leaves no layout behind.
#[test]
fn a_bad_layout_window_adds_no_layout() {
    let mut s = session_with_model();
    let before = s.doc().unwrap().layouts.len();
    for w in [
        json!([[1, 1], [1, 9]]),
        json!([[1, 1], [9, 1]]),
        json!([[1, 1]]),
        json!("all"),
        json!([[1, 1], ["a", 2]]),
        json!([[f64::MAX, 0], [-f64::MAX, 1]]),
    ] {
        assert!(s.execute("layout.new", &json!({"name": "Bad", "window": w})).is_err(), "{w}");
    }
    assert_eq!(s.doc().unwrap().layouts.len(), before, "no layout is left behind");
}

/// The interactive command shows the Model tab, takes two corners and shows the new layout.
#[test]
fn new_layout_picks_the_model_area_in_the_model_tab() {
    let mut s = session_with_model();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.start("layout.new").unwrap();
    s.cmdline("Sheet P").unwrap();
    assert_eq!(s.space(), Space::Model, "the Model tab is shown to pick the area");
    s.cmdline("10,10").unwrap();
    assert!(s.current_prompt().is_some_and(|p| p.message == "Specify opposite corner"));
    // A window without width re-asks for the first corner and adds nothing.
    s.cmdline("10,30").unwrap();
    assert!(s.current_prompt().is_some_and(|p| p.message.contains("first corner")), "{:?}", s.current_prompt().map(|p| p.message));
    assert!(s.doc().unwrap().layout("Sheet P").is_none());
    s.cmdline("10,10").unwrap();
    s.cmdline("30,20").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.space(), Space::Paper("Sheet P".into()), "the new layout is shown");
    let v = only_viewport(&s, "Sheet P");
    assert!(v.view_center.dist(Vec2::new(20.0, 15.0)) < 1e-9, "{:?}", v.view_center);
    s.undo().unwrap();
    assert!(s.doc().unwrap().layout("Sheet P").is_none(), "one undo step");
}

/// A tab shown while the corners are picked ends the command instead of reading points in that space.
#[test]
fn new_layout_ends_if_another_tab_is_shown_while_picking() {
    let mut s = session_with_model();
    let before = s.doc().unwrap().layouts.len();
    s.start("layout.new").unwrap();
    s.cmdline("Sheet T").unwrap();
    assert_eq!(s.space(), Space::Model);
    // A tab shown meanwhile: the next point must not be taken for a model corner.
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.cmdline("10,10").unwrap();
    assert!(s.running.is_none(), "the command ended");
    assert_eq!(s.layout_space(), Space::Paper("Layout1".into()), "the tab that was shown stays");
    assert_eq!(s.doc().unwrap().layouts.len(), before, "no layout was added");
}

/// The opposite-corner prompt is a window corner, so ortho does not bend it.
#[test]
fn the_opposite_corner_is_not_bent_by_ortho() {
    let mut s = session_with_model();
    s.start("layout.new").unwrap();
    s.cmdline("Sheet O").unwrap();
    s.cmdline("10,10").unwrap();
    let p = s.current_prompt().unwrap();
    assert!(p.corner && p.base == Some(Vec2::new(10.0, 10.0)), "a window's opposite corner prompt");
}

/// Esc puts back the tab the command started in and adds no layout.
#[test]
fn cancelling_new_layout_puts_the_tab_back() {
    let mut s = session_with_model();
    s.execute("layout.set", &json!({"name": "Layout2"})).unwrap();
    let before = s.doc().unwrap().layouts.len();
    s.start("layout.new").unwrap();
    s.cmdline("Sheet C").unwrap();
    assert_eq!(s.space(), Space::Model);
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(s.space(), Space::Paper("Layout2".into()), "back in the tab it started in: {:?}", s.log);
    assert_eq!(s.doc().unwrap().layouts.len(), before);
    // Esc at the name prompt changes nothing either.
    s.start("layout.new").unwrap();
    s.cancel();
    assert_eq!(s.space(), Space::Paper("Layout2".into()));
}

/// Esc after the user chose another tab leaves that tab shown, not the one the command started in.
#[test]
fn cancelling_new_layout_keeps_a_tab_chosen_meanwhile() {
    let mut s = session_with_model();
    s.execute("layout.set", &json!({"name": "Layout2"})).unwrap();
    s.start("layout.new").unwrap();
    s.cmdline("Sheet K").unwrap();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(s.space(), Space::Paper("Layout1".into()), "the tab chosen meanwhile stays: {:?}", s.log);
}
#[test]
fn mview_arrangements_and_interactive() {
    let mut s = session_with_model();
    assert!(s.execute("mview", &json!({"p1": [0, 0], "p2": [1, 1]})).is_err(), "not in model space");
    s.execute("mview", &json!({"layout": "Layout2", "count": 4})).unwrap();
    assert_eq!(viewports(&s, "Layout2").len(), 4);
    s.execute("vports.3", &json!({"layout": "Layout2", "p1": [0, 0], "p2": [9, 6]})).unwrap();
    let v = viewports(&s, "Layout2");
    assert_eq!(v.len(), 7);
    let total: f64 = v[4..].iter().map(|(_, v)| v.width * v.height).sum();
    assert!((total - 54.0).abs() < 1e-9, "three viewports tile the window");
    // Interactive MVIEW: two corners.
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    s.cmdline("mview").unwrap();
    s.cmdline("1,1").unwrap();
    s.cmdline("6,4").unwrap();
    assert!(s.running.is_none());
    let v = viewports(&s, "Layout1");
    assert_eq!(v.len(), 1);
    assert!((v[0].1.width - 5.0).abs() < 1e-9 && (v[0].1.height - 3.0).abs() < 1e-9);
    // MVIEW 2 → arrangement (Enter: Vertical) → Fit (Enter).
    s.cmdline("mview").unwrap();
    s.cmdline("2").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(viewports(&s, "Layout1").len(), 3);
    // One undo step per MVIEW.
    s.undo().unwrap();
    assert_eq!(viewports(&s, "Layout1").len(), 1);
    // In model space the interactive command ends at once.
    s.execute("layout.set", &json!({"name": "Model"})).unwrap();
    s.cmdline("vports.4").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn viewport_scale_lock_and_layers() {
    let mut s = session_with_model();
    s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
    let r = s.execute("mview", &json!({"p1": [1, 1], "p2": [5, 4]})).unwrap();
    let h = r["viewports"][0].as_str().unwrap().to_string();
    let out = s.execute("viewport.set", &json!({"handle": h, "scale": "1:20", "center": [50, 20]})).unwrap();
    assert!((out[0]["viewHeight"].as_f64().unwrap() - 60.0).abs() < 1e-9);
    assert!((out[0]["scale"].as_f64().unwrap() - 0.05).abs() < 1e-12);
    s.execute("viewport.set", &json!({"handle": h, "locked": true})).unwrap();
    assert!(s.execute("viewport.set", &json!({"handle": h, "scale": 0.1})).is_err(), "locked");
    s.execute("viewport.set", &json!({"handle": h, "scale": 0.1, "locked": false})).unwrap();
    s.execute("vplayer", &json!({"handle": h, "freeze": ["0", "Walls"]})).unwrap();
    s.execute("vplayer", &json!({"handle": h, "thaw": "walls"})).unwrap();
    let v = viewports(&s, "Layout1");
    assert_eq!(v[0].1.frozen_layers, vec!["0".to_string()]);
    s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": "red"}})).unwrap();
    assert!(s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": "bylayer"}})).is_err());
    assert!(s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": [1]}})).is_err());
    let vv = viewports(&s, "Layout1");
    assert_eq!(vv[0].1.layer_colors, vec![("Walls".to_string(), cadcraft_color::Color::Index(1))]);
    s.execute("vplayer", &json!({"handle": h, "colors": {"walls": null}})).unwrap();
    assert!(viewports(&s, "Layout1")[0].1.layer_colors.is_empty());
    assert!((v[0].1.view_height - 30.0).abs() < 1e-9);
    // Layer 0 frozen in the viewport: nothing of the model shows.
    let list = cadcraft_render::build(s.doc().unwrap(), &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
    assert!(list.prims.iter().all(|p| p.handle != cadcraft_render::VIEWPORT_CONTENT));
    for bad_p in
        [json!({"handle": h, "scale": "0:0"}), json!({"handle": h, "scale": -1}), json!({"handle": h, "viewHeight": 0}), json!({"handle": "FFFFFF"})]
    {
        assert!(s.execute("viewport.set", &bad_p).is_err(), "{bad_p}");
    }
}

#[test]
fn pagesetup_paper_table() {
    let mut s = Session::new();
    let r = s.execute("pagesetup", &json!({"layout": "Layout1", "paper": "A3", "landscape": true, "margins": [5, 5, 5, 5]})).unwrap();
    assert_eq!(r["page"]["widthMm"], 297.0);
    let sheet = cadcraft_render::sheet(s.doc().unwrap(), "Layout1").unwrap();
    // Imperial drawing: paper units are inches.
    assert!((sheet.size.x - 420.0 / 25.4).abs() < 1e-9 && (sheet.size.y - 297.0 / 25.4).abs() < 1e-9);
    assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "paper": "Z12"})).is_err());
    assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "margins": [1, 2]})).is_err());
    assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "width": 1e308, "height": 5})).is_err());
    assert!(s.execute("pagesetup", &json!({"paper": "A4"})).is_err(), "model tab needs a layout");
    for area in ["extents", "display", "window", "limits"] {
        let e = s.execute("pagesetup", &json!({"layout": "Layout1", "plotArea": area})).unwrap_err();
        assert!(e.to_string().contains("not available yet"), "{area}: {e}");
    }
    let r = s.execute("pagesetup", &json!({"layout": "Layout1", "plotArea": "Layout"})).unwrap();
    assert_eq!(r["page"]["plotArea"], "layout");
    let report = s.execute("pagesetup", &Value::Null).unwrap();
    assert!(report["papers"].as_array().unwrap().len() >= 10);
    s.execute("pagesetup", &json!({"layout": "Layout2", "width": 500, "height": 300})).unwrap();
    let p = &s.doc().unwrap().layout("Layout2").unwrap().page;
    assert_eq!((p.width_mm, p.height_mm), (300.0, 500.0));
}

fn fake_plot(d: &Drawing, space: &Space, opts: &Value) -> std::result::Result<Vec<u8>, String> {
    let _ = d;
    Ok(format!("%PDF-1.4 fake {space:?} {opts}").into_bytes())
}

#[test]
fn plot_goes_through_the_hook() {
    super::super::file::set_io(super::super::file::IoHooks {
        read: |_, _| Err("no reader in tests".into()),
        write: |_, _| Err("no writer in tests".into()),
        plot: Some(fake_plot),
    });
    let mut s = session_with_model();
    if io().and_then(|h| h.plot).is_none() {
        return; // another test installed hooks without a plotter first
    }
    let r = s.execute("plot", &json!({"layout": "Layout1", "paper": "A4"})).unwrap();
    let data = super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap();
    let text = String::from_utf8(data).unwrap();
    assert!(text.starts_with("%PDF") && text.contains("Paper(\"Layout1\")") && text.contains("A4"));
    let r = s.execute("exportpdf", &json!({})).unwrap();
    assert_eq!(r["layout"], "Model");
    let baseline = String::from_utf8(super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    assert!(!baseline.contains("\"center\"") && !baseline.contains("\"offset\""), "omitted options remain omitted: {baseline}");
    assert!(s.execute("plot", &json!({"layout": "Nope"})).is_err());
    // Plot area and a ratio scale reach the plotter as a model-space window and a number.
    let opts = |r: Value| String::from_utf8(super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    let p = json!({"layout": "Model", "plotArea": "window", "window": [[10, 0], [0, 5]], "fit": false, "scale": "1:2"});
    let text = opts(s.execute("plot", &p).unwrap());
    assert!(text.contains(r#""window":[[0.0,0.0],[10.0,5.0]]"#) && text.contains(r#""scale":0.5"#), "{text}");
    let text = opts(s.execute("plot", &json!({"layout": "Model", "center": false, "offset": [12.5, -7.0]})).unwrap());
    assert!(text.contains(r#""center":false"#) && text.contains(r#""offset":[12.5,-7.0]"#), "{text}");
    for offset in [json!([1]), json!([1, 2, 3]), json!([f64::NAN, 0]), json!([100_001, 0])] {
        assert!(s.execute("plot", &json!({"layout": "Model", "center": false, "offset": offset})).is_err());
    }
    assert!(s.execute("plot", &json!({"layout": "Model", "output": "svg", "center": false, "offset": [0, 0]})).is_err());
    assert!(opts(s.execute("plot", &json!({"layout": "Model", "plotArea": "limits"})).unwrap()).contains(r#""window":[[0.0,0.0],"#));
    assert!(s.execute("plot", &json!({"layout": "Model", "plotArea": "window"})).is_err(), "window needs corners");
    assert!(s.execute("plot", &json!({"layout": "Layout1", "plotArea": "display"})).is_err(), "layouts plot their sheet");
    assert!(s.execute("plot", &json!({"layout": "Model", "scale": -1})).is_err());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = std::env::temp_dir().join(format!("cadcraft-plot-test-{}.pdf", std::process::id()));
        let ps = path.to_string_lossy().to_string();
        let r = s.execute("plot", &json!({"path": ps})).unwrap();
        assert!(r["bytes"].as_u64().unwrap() > 0);
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn hostile_layout_params() {
    let mut s = session_with_model();
    let long = "x".repeat(300);
    for (c, p) in [
        ("layout.new", json!({"name": ""})),
        ("layout.new", json!({"name": long})),
        ("layout.new", json!({"name": "a\u{0}b"})),
        ("layout.new", json!({"name": "Layout1"})),
        ("mview", json!({"layout": "Layout1", "p1": [1e308, -1e308], "p2": [-1e308, 1e308]})),
        ("mview", json!({"layout": "Layout1", "p1": [1, 1], "p2": [1, 1]})),
        ("mview", json!({"layout": "Layout1", "count": 99})),
        ("mview", json!({"layout": "Layout1", "count": -1})),
        ("layout.rename", json!({"from": "Layout1"})),
        ("layout.copy", json!({"from": "nope"})),
        ("viewport.set", json!({"handles": ["zz"]})),
    ] {
        assert!(s.execute(c, &p).is_err(), "{c} {p}");
    }
}

#[test]
fn mspace_edits_model_through_viewport() {
    let mut s = session_with_model();
    let before = s.doc().unwrap().model.len();
    s.execute("layout.new", &json!({"name": "L"})).unwrap();
    s.execute("layout.set", &json!({"name": "L"})).unwrap();
    assert!(s.execute("mspace", &json!({"at": [-1000, -1000]})).is_err());
    let vps = viewports(&s, "L");
    let (h, vp) = vps.iter().find(|(_, v)| v.id != 1).cloned().unwrap();
    s.execute("mspace", &json!({"at": [vp.center.x, vp.center.y]})).unwrap();
    assert_eq!(s.space(), Space::Model);
    assert_eq!(s.layout_space(), Space::Paper("L".into()));
    // The screen view in MSPACE maps the viewport centre to its model view centre.
    let st = s.state().unwrap();
    let (pv, v) = (st.paper_view(), st.view());
    let k = vp.view_height / vp.height;
    assert!((v.height - pv.height * k).abs() < 1e-9);
    // New geometry goes to model space.
    s.cmdline("circle 10,10 3").unwrap();
    assert_eq!(s.doc().unwrap().model.len(), before + 1);
    // Zooming changes the viewport's model view, not the sheet.
    s.state_mut().unwrap().set_view(crate::View { center: v.center, height: v.height / 2.0 });
    let vp2 = viewports(&s, "L").into_iter().find(|(x, _)| *x == h).unwrap().1;
    assert!((vp2.view_height - vp.view_height / 2.0).abs() < 1e-9);
    assert_eq!(s.state().unwrap().paper_view(), pv);
    s.execute("pspace", &json!({})).unwrap();
    assert_eq!(s.space(), Space::Paper("L".into()));
    s.execute("mspace", &json!({})).unwrap();
    s.execute("layout.set", &json!({"name": "Model"})).unwrap();
    assert!(s.state().unwrap().mspace.is_none());
}
