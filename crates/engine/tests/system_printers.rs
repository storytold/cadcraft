//! System printers (#670 step 2): the host lists them as devices, and PLOT to one sends the
//! plotter's PDF to the host instead of returning a file.

use std::sync::Mutex;

use cadcraft_doc::{DevicePaper, Drawing, PlotterConfig, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::cmd::plotters::{PlotterStore, PrintJob, SystemPrinters, set_plotter_store, set_system_printers};
use serde_json::{Value, json};

/// The jobs printed: printer, paper, copies, title, the options the plot hook got.
static JOBS: Mutex<Vec<(String, String, u32, String, Value)>> = Mutex::new(Vec::new());
/// The tests share [`JOBS`]: one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// A plotter that returns its options as the file, so a print job shows what was plotted.
fn echo_plot(_: &Drawing, _: &Space, opts: &Value) -> Result<Vec<u8>, String> {
    Ok(opts.to_string().into_bytes())
}

/// One system printer, a roll plotter with Letter and a 24 in roll, as the system would list it.
fn printers() -> Vec<PlotterConfig> {
    vec![PlotterConfig {
        name: "Office_T120".into(),
        description: "HP DesignJet T120".into(),
        default_paper: "Letter".into(),
        papers: vec![
            DevicePaper { name: "Letter".into(), width: 215.9, height: 279.4, margins: [5.0; 4] },
            DevicePaper { name: "Roll 24 in".into(), width: 609.6, height: 914.4, margins: [5.0; 4] },
        ],
        system: true,
        system_default: true,
        ..PlotterConfig::default()
    }]
}

/// Records each print job (printer, paper, copies, title and plotted options) and answers with a job id.
fn print(j: &PrintJob) -> Result<String, String> {
    let opts = serde_json::from_slice(j.pdf).map_err(|e| e.to_string())?;
    JOBS.lock().unwrap().push((j.printer.into(), j.paper.into(), j.copies, j.title.into(), opts));
    Ok(format!("{}-7", j.printer))
}

/// A session with the stand-in printers and plotter installed and a line to plot.
fn session() -> Session {
    set_plotter_store(PlotterStore::memory());
    set_system_printers(SystemPrinters { list: printers, print });
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Err("no writer in this test".into()), plot: Some(echo_plot) });
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    s
}

/// System printers are listed after the built-in devices, and PLOT on one sends the job with its paper, copies and title.
#[test]
fn system_printers_are_devices_and_plot_prints_on_them() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    let r = s.execute("plottermanager", &json!({})).unwrap();
    let names: Vec<&str> = r["devices"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["CADCraft PDF", "CADCraft PNG", "CADCraft SVG", "Office_T120"]);

    let r = s.execute("plot", &json!({ "layout": "Model", "device": "office_t120", "paper": "Roll 24 in", "copies": 2 })).unwrap();
    assert_eq!(r["job"], "Office_T120-7");
    assert_eq!(r["copies"], 2);
    assert!(r.get("data").is_none());
    let (printer, paper, copies, title, opts) = JOBS.lock().unwrap().pop().unwrap();
    assert_eq!((printer.as_str(), paper.as_str(), copies), ("Office_T120", "Roll 24 in", 2));
    assert!(title.ends_with(" - Model"), "{title}");
    // The plotter wrote a PDF on the printer's paper.
    assert_eq!(opts["output"], "pdf");
    assert_eq!(opts["width"], 609.6);

    // EXPORTPDF to the same device still returns the PDF.
    let r = s.execute("exportpdf", &json!({ "layout": "Model", "device": "Office_T120" })).unwrap();
    assert!(r.get("data").is_some() && r.get("job").is_none());
    assert!(JOBS.lock().unwrap().is_empty());

    // A printer takes the PDF before the output's own checks: an SVG with a model-space offset
    // (which SVG refuses) still prints.
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "output": "svg", "center": false, "offset": [5, 5] })).unwrap();
    assert_eq!(r["job"], "Office_T120-7", "{r}");
    assert_eq!(JOBS.lock().unwrap().pop().unwrap().4["output"], "pdf");

    // A `printer` passed with the call never prints: EXPORTPDF and PLOT without a printer
    // device write their file.
    let r = s.execute("exportpdf", &json!({ "layout": "Model", "printer": "Office_T120" })).unwrap();
    assert!(r.get("data").is_some() && r.get("job").is_none());
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "CADCraft PDF", "printer": "Office_T120" })).unwrap();
    assert!(r.get("job").is_none(), "{r}");
    assert!(JOBS.lock().unwrap().is_empty());

    // An unknown printer is refused, and copies are bounded.
    assert!(s.execute("plot", &json!({ "layout": "Model", "device": "Nope" })).is_err());
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "copies": 100000 })).unwrap();
    assert_eq!(r["copies"], 999);
    assert_eq!(JOBS.lock().unwrap().pop().unwrap().2, 999);

    // A system printer is listed as one, and PLOTTERMANAGER neither edits nor deletes it.
    let list = s.execute("plottermanager", &json!({ "action": "list" })).unwrap();
    let t120 = list["devices"].as_array().unwrap().iter().find(|d| d["name"] == "Office_T120").cloned().unwrap();
    assert_eq!(t120["system"], true);
    assert_eq!(t120["default"], true, "the system's default printer says so");
    let set = s.execute("plottermanager", &json!({ "action": "set", "name": "Office_T120", "description": "x" }));
    assert!(set.unwrap_err().to_string().contains("system printer"));
    assert!(s.execute("plottermanager", &json!({ "action": "delete", "name": "Office_T120" })).is_err());
}

/// Plot to file with a printer: the PDF is written first, then printed; a file that can't be
/// written stops the plot before anything prints. REFRESH asks for the printers again.
#[test]
fn plot_to_file_and_print_and_refresh_the_printers() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("cadcraft-plot-to-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plan.pdf");
    let path_s = path.to_string_lossy().to_string();
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "path": path_s })).unwrap();
    assert_eq!(r["job"], "Office_T120-7");
    assert_eq!(r["path"], path_s.as_str());
    let written = std::fs::read(&path).unwrap();
    let (printer, _, _, _, opts) = JOBS.lock().unwrap().pop().unwrap();
    assert_eq!(printer, "Office_T120");
    // The file is the PDF that was printed.
    assert_eq!(serde_json::from_slice::<Value>(&written).unwrap(), opts);

    let bad = dir.join("missing-folder").join("plan.pdf");
    let e = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "path": bad.to_string_lossy() })).unwrap_err();
    assert!(e.to_string().contains("plan.pdf"), "{e}");
    assert!(JOBS.lock().unwrap().is_empty(), "nothing printed when the file couldn't be written");
    let _ = std::fs::remove_dir_all(&dir);

    // The plot stamp: the drawing and the device named here, the rest left to the plotter.
    s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "stamp": true })).unwrap();
    let stamp = JOBS.lock().unwrap().pop().unwrap().4["stamp"].as_str().unwrap().to_string();
    let title = s.state().unwrap().title.clone();
    assert_eq!(stamp, format!("{title}, {{layout}}, Office_T120, Letter, {{scale}}"), "the device's paper by its name");
    let r = s.execute("exportpdf", &json!({ "layout": "Model", "stamp": "Checked {device}\u{7}" })).unwrap();
    let opts: Value = serde_json::from_slice(&cadcraft_engine::cmd::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(opts["stamp"], "Checked CADCraft PDF");
    let r = s.execute("exportpdf", &json!({ "layout": "Model", "stamp": false })).unwrap();
    let opts: Value = serde_json::from_slice(&cadcraft_engine::cmd::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    assert!(opts.get("stamp").is_none());
    assert!(s.execute("plot", &json!({ "layout": "Model", "stamp": 5 })).is_err());

    let r = s.execute("plottermanager", &json!({ "action": "refresh" })).unwrap();
    assert_eq!(r["printers"], true);
    let list = s.execute("plottermanager", &json!({ "action": "list" })).unwrap();
    assert!(list["devices"].as_array().unwrap().iter().any(|d| d["name"] == "Office_T120"));
}
