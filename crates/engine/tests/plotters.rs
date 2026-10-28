//! Plotter configurations (issue #564): PLOTTERMANAGER keeps user devices in the host's folder,
//! PAGESETUP takes a device and its papers with their printable areas, and PLOT hands the
//! plotter the device's output, paper and margins.

use cadcraft_doc::{Drawing, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_decode, set_io};
use cadcraft_engine::cmd::plotters::{PlotterStore, set_plotter_store};
use cadcraft_engine::snap::tracking::CursorQuery;
use cadcraft_engine::{Input, geom::Vec2};
use serde_json::{Value, json};

/// The plot hook echoes the options it gets.
fn echo_plot(_: &Drawing, _: &Space, opts: &Value) -> Result<Vec<u8>, String> {
    Ok(opts.to_string().into_bytes())
}

/// A session with in-memory plotters and I/O hooks, holding one line so layouts have content.
fn session() -> Session {
    set_plotter_store(PlotterStore::memory());
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Err("no writer in this test".into()), plot: Some(echo_plot) });
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    s
}

/// The device names the PLOTTERMANAGER report lists, in the order it lists them.
fn names(s: &mut Session) -> Vec<String> {
    let r = s.execute("plottermanager", &json!({})).unwrap();
    r["devices"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap().to_string()).collect()
}

/// The options the plot hook got.
fn plotted(r: Value) -> Value {
    let bytes = base64_decode(r["data"].as_str().unwrap()).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
/// PLOTTERMANAGER creates, edits, saves, loads and deletes user devices; built-ins are fixed,
/// duplicate names and papers with margins larger than the page are refused.
fn manage_devices() {
    let mut s = session();
    let n = names(&mut s);
    assert_eq!(n[..3], ["CADCraft PDF", "CADCraft PNG", "CADCraft SVG"], "built-in devices first");
    let pdf = s.execute("plottermanager", &json!({"action": "get", "name": "cadcraft pdf"})).unwrap();
    assert!(pdf["builtin"].as_bool().unwrap());
    let papers = pdf["papers"].as_array().unwrap();
    assert!(papers.iter().any(|p| p["name"] == "ISO B1 (707.00 x 1000.00 MM)"));
    let a4 = papers.iter().find(|p| p["name"] == "ISO A4 (210.00 x 297.00 MM)").unwrap();
    assert_eq!(a4["margins"], json!([5.0, 5.0, 5.0, 5.0]));
    let letter = papers.iter().find(|p| p["name"] == "ANSI A (8.50 x 11.00 Inches)").unwrap();
    assert_eq!(letter["margins"], json!([6.35, 6.35, 6.35, 6.35]));

    // A user device: a PNG copy with its own borderless paper.
    s.execute("plottermanager", &json!({"action": "new", "name": "Office Raster", "output": "png", "resolution": 150})).unwrap();
    let paper = json!({"name": "A3 borderless", "width": 420, "height": 297, "margins": [0, 0, 0, 0]});
    let r = s.execute("plottermanager", &json!({"action": "set", "name": "office raster", "paper": paper, "defaultPaper": "A3 borderless"})).unwrap();
    assert_eq!(r["defaultPaper"], "A3 borderless");
    assert_eq!(r["resolution"], 150);
    assert!(names(&mut s).contains(&"Office Raster".to_string()));
    assert!(s.execute("plottermanager", &json!({"action": "new", "name": "OFFICE RASTER"})).is_err(), "duplicate");
    assert!(s.execute("plottermanager", &json!({"action": "set", "name": "CADCraft PDF", "resolution": 100})).is_err(), "built-ins are fixed");
    assert!(s.execute("plottermanager", &json!({"action": "delete", "name": "CADCraft SVG"})).is_err());
    let bad_margins = json!({"name": "x", "width": 100, "height": 100, "margins": [60, 0, 60, 0]});
    assert!(s.execute("plottermanager", &json!({"action": "set", "name": "Office Raster", "paper": bad_margins})).is_err());

    // Save as text, load under another name, delete.
    let text = s.execute("plottermanager", &json!({"action": "save", "name": "Office Raster"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(text.contains("\"cadcraftPlotter\": 1"), "{text}");
    s.execute("plottermanager", &json!({"action": "load", "text": text, "name": "Office Copy"})).unwrap();
    let copy = s.execute("plottermanager", &json!({"action": "get", "name": "Office Copy"})).unwrap();
    assert_eq!(copy["output"], "png");
    assert!(s.execute("plottermanager", &json!({"action": "load", "text": "{}"})).is_err());
    s.execute("plottermanager", &json!({"action": "delete", "name": "office copy"})).unwrap();
    assert!(!names(&mut s).contains(&"Office Copy".to_string()));
    assert!(s.execute("plottermanager", &json!({"action": "nope"})).is_err());
}

#[test]
/// PAGESETUP copies the device's papers and margins into the layout's page, keeps a paper the new
/// device also has, and falls back to the device default when it does not.
fn page_setup_takes_a_device_and_its_papers() {
    let mut s = session();
    s.execute("plottermanager", &json!({"action": "new", "name": "Plan Printer", "output": "pdf"})).unwrap();
    let paper = json!({"name": "Strip", "width": 200, "height": 600, "margins": [10, 20, 10, 20]});
    s.execute("plottermanager", &json!({"action": "set", "name": "Plan Printer", "paper": paper})).unwrap();
    let r = s.execute("pagesetup", &json!({"layout": "Layout1", "device": "plan printer", "paper": "strip", "landscape": false})).unwrap();
    assert_eq!(r["page"]["device"], "Plan Printer");
    assert_eq!(r["page"]["paper"], "Strip");
    let page = &s.doc().unwrap().layout("Layout1").unwrap().page;
    assert_eq!((page.width_mm, page.height_mm, page.margins_mm), (200.0, 600.0, [10.0, 20.0, 10.0, 20.0]));
    // The printable area shown in the layout follows the device paper.
    let sheet = cadcraft_render::sheet(s.doc().unwrap(), "Layout1").unwrap();
    let mm = |v: f64| v * sheet.unit_mm;
    let got = [mm(sheet.printable.min.x), mm(sheet.printable.min.y), mm(sheet.printable.max.x), mm(sheet.printable.max.y)];
    assert!(got.iter().zip([10.0, 20.0, 190.0, 580.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{got:?}");

    // Switching to a built-in device keeps a paper it has, else takes its default (A4).
    s.execute("pagesetup", &json!({"layout": "Layout1", "device": "CADCraft PDF"})).unwrap();
    let page = s.doc().unwrap().layout("Layout1").unwrap().page.clone();
    assert_eq!((page.paper.as_str(), page.margins_mm), ("ISO A4 (210.00 x 297.00 MM)", [5.0; 4]));
    s.execute("pagesetup", &json!({"layout": "Layout1", "paper": "A3"})).unwrap();
    let page = s.doc().unwrap().layout("Layout1").unwrap().page.clone();
    assert_eq!((page.width_mm, page.height_mm), (297.0, 420.0));

    assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "device": "Nope"})).is_err());
    assert!(
        s.execute("pagesetup", &json!({"layout": "Layout1", "device": "Plan Printer", "paper": "Tabloid Extra"})).is_err(),
        "not a paper of the device"
    );
    s.execute("pagesetup", &json!({"layout": "Layout1", "device": "None", "paper": "Letter"})).unwrap();
    let page = s.doc().unwrap().layout("Layout1").unwrap().page.clone();
    assert_eq!((page.device.as_str(), page.paper.as_str()), ("None", "ANSI A (8.50 x 11.00 Inches)"));
    let report = s.execute("pagesetup", &Value::Null).unwrap();
    assert!(report["devices"].as_array().unwrap().iter().any(|d| d == "Plan Printer"));
}

#[test]
/// PLOT and EXPORTPDF hand the hook the device's output, resolution, paper and margins, and a
/// layout plots with its own page setup; unknown devices and papers are refused.
fn plot_uses_the_device() {
    let mut s = session();
    s.execute("plottermanager", &json!({"action": "new", "name": "Sketch PNG", "output": "png", "resolution": 96})).unwrap();
    let o = plotted(s.execute("plot", &json!({"layout": "Model", "device": "Sketch PNG", "paper": "A3"})).unwrap());
    assert_eq!(o["output"], "png");
    assert_eq!(o["resolution"], 96);
    assert_eq!((o["width"].as_f64(), o["height"].as_f64()), (Some(297.0), Some(420.0)));
    assert_eq!(o["margins"], json!([5.0, 5.0, 5.0, 5.0]));
    assert_eq!(o["landscape"], true, "the device's default orientation");
    assert!(o.get("paper").is_none() && o.get("device").is_none());
    // EXPORTPDF always writes PDF.
    let o = plotted(s.execute("exportpdf", &json!({"layout": "Model", "device": "Sketch PNG"})).unwrap());
    assert_eq!(o["output"], "pdf");
    // A layout plots with its page setup's device and paper.
    s.execute("pagesetup", &json!({"layout": "Layout1", "device": "Sketch PNG", "paper": "A4"})).unwrap();
    let o = plotted(s.execute("plot", &json!({"layout": "Layout1"})).unwrap());
    assert_eq!(o["output"], "png");
    assert!(o.get("width").is_none(), "the page setup already holds the paper");
    // No device: options pass through unchanged.
    let o = plotted(s.execute("plot", &json!({"layout": "Model", "paper": "A4"})).unwrap());
    assert!(o.get("output").is_none());
    assert!(s.execute("plot", &json!({"layout": "Model", "device": "Nope"})).is_err());
    assert!(s.execute("plot", &json!({"layout": "Model", "device": "Sketch PNG", "paper": "Nope"})).is_err());
}

#[test]
/// The typed -PLOTTERMANAGER prompt machine creates a device, adds a paper to it and deletes it.
fn typed_plotter_manager() {
    let mut s = session();
    for line in ["-plottermanager", "New", "Typed Plotter", "Svg"] {
        s.cmdline(line).unwrap();
    }
    let d = s.execute("plottermanager", &json!({"action": "get", "name": "Typed Plotter"})).unwrap();
    assert_eq!(d["output"], "svg");
    for line in ["-plottermanager", "Paper", "Typed Plotter", "Card", "100,150"] {
        s.cmdline(line).unwrap();
    }
    let d = s.execute("plottermanager", &json!({"action": "get", "name": "Typed Plotter"})).unwrap();
    assert!(d["papers"].as_array().unwrap().iter().any(|p| p["name"] == "Card" && p["width"] == 100.0));
    for line in ["-plottermanager", "Delete", "Typed Plotter"] {
        s.cmdline(line).unwrap();
    }
    assert!(!names(&mut s).contains(&"Typed Plotter".to_string()));
}

#[test]
/// Hostile names and values (empty or path-like names, unknown output, huge resolution, negative
/// or enormous paper sizes, oversized loads) are refused instead of panicking.
fn hostile_names_and_values_are_refused() {
    let mut s = session();
    let r = s.execute("plottermanager", &json!({"action": "new", "name": "../../etc/passwd"})).unwrap();
    assert_eq!(r["name"], "../../etc/passwd", "the name is kept; only the file name is cleaned");
    for p in [
        json!({"action": "new", "name": ""}),
        json!({"action": "new", "name": "x", "output": "dwf"}),
        json!({"action": "new", "name": "y", "resolution": 1e12}),
        json!({"action": "set", "name": "../../etc/passwd", "paper": {"name": "p", "width": f64::MAX, "height": 1}}),
        json!({"action": "set", "name": "../../etc/passwd", "paper": {"name": "p", "width": -1, "height": 1}}),
        json!({"action": "set", "name": "../../etc/passwd", "removePaper": "nope"}),
        json!({"action": "load", "text": "x".repeat(2 << 20)}),
        json!({"action": "get"}),
    ] {
        assert!(s.execute("plottermanager", &p).is_err(), "{p}");
    }
}

#[test]
/// PLOTWINDOW keeps both picked corners for the plot dialog, previews the rubber-band rectangle
/// after the first pick, and refuses a window with no area (JSON form included).
fn plotwindow_keeps_the_picked_window_for_the_plot_dialog() {
    let mut s = Session::new();
    // Typed: two corners, with the window's rectangle following the cursor after the first.
    s.cmdline("plotwindow").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify first corner");
    s.cmdline("10,20").unwrap();
    assert_eq!(s.preview(cadcraft_engine::geom::Vec2::new(40.0, 5.0)).len(), 4, "the rubber-band rectangle");
    s.cmdline("40,5").unwrap();
    assert!(s.running.is_none());
    let w = s.plot_window.expect("the picked window");
    assert_eq!((w.min.x, w.min.y, w.max.x, w.max.y), (10.0, 5.0, 40.0, 20.0));
    // JSON, and a window without area is refused.
    let r = s.execute("plotwindow", &json!({"p1": [0, 0], "p2": [3, 4]})).unwrap();
    assert_eq!(r["window"], json!([[0.0, 0.0], [3.0, 4.0]]));
    assert!(s.execute("plotwindow", &json!({"p1": [0, 0], "p2": [3, 0]})).is_err());
}

/// Snap `raw` at the current prompt the way the canvas does, so a test sees the pick ORTHO and
/// object snap would really produce.
fn hover(s: &mut Session, raw: Vec2) -> Vec2 {
    let p = s.current_prompt().expect("a prompt");
    let q = CursorQuery { base: p.base, deferred: p.deferred, corner: p.corner, aperture: 1e-6, tol: 1e-6, now: 0.0, shift: false };
    s.snap_cursor(raw, &q).point
}

#[test]
/// Regression (#1641): with ORTHO on, PLOTWINDOW's opposite corner stays where the cursor is so
/// the window keeps both extents, while an ordinary point prompt still follows ortho.
fn plotwindow_picks_both_corners_with_ortho_on() {
    let mut s = Session::new();
    s.execute("setvar", &json!({"name": "OSMODE", "value": 0})).unwrap();
    s.execute("setvar", &json!({"name": "ORTHOMODE", "value": 1})).unwrap();
    s.cmdline("plotwindow").unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify first corner");
    let a = hover(&mut s, Vec2::new(10.0, 20.0));
    s.input(Input::Point(a)).unwrap();
    assert_eq!(s.current_prompt().unwrap().message, "Specify opposite corner");
    let c = hover(&mut s, Vec2::new(40.0, 5.0));
    assert!(c.near(Vec2::new(40.0, 5.0), 1e-9), "the opposite corner is where the cursor is: {c:?}");
    s.input(Input::Point(c)).unwrap();
    assert!(s.running.is_none());
    let w = s.plot_window.expect("the picked window");
    assert!(w.width() > 0.0 && w.height() > 0.0, "the window has both extents: {w:?}");
    assert_eq!((w.min.x, w.min.y, w.max.x, w.max.y), (10.0, 5.0, 40.0, 20.0));

    // An ordinary point prompt still follows ortho: a line's next point stays on its axis.
    s.cmdline("line 0,0").unwrap();
    let c = hover(&mut s, Vec2::new(10.0, 4.0));
    assert!(c.near(Vec2::new(10.0, 0.0), 1e-9), "ortho keeps the line horizontal: {c:?}");
}
