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

/// How many times the system was asked for its printers.
static LISTINGS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One system printer, a roll plotter with Letter and a 24 in roll, as the system would list it.
fn printers() -> Vec<PlotterConfig> {
    LISTINGS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    vec![PlotterConfig {
        name: "Office_T120".into(),
        description: "HP DesignJet T120".into(),
        default_paper: "Letter".into(),
        papers: vec![
            DevicePaper { native_key: None, name: "Letter".into(), width: 215.9, height: 279.4, margins: [5.0; 4] },
            DevicePaper { native_key: None, name: "Roll 24 in".into(), width: 609.6, height: 914.4, margins: [5.0; 4] },
        ],
        system: true,
        system_default: true,
        ..PlotterConfig::default()
    }]
}

/// Records each print job (printer, paper, copies, title and plotted options) and answers with a job id.
fn print(j: &PrintJob) -> Result<String, String> {
    // Thirteen copies stand for a printer that refuses the job.
    if j.copies == 13 {
        return Err("the printer refused the job".into());
    }
    let opts = serde_json::from_slice(j.pdf).map_err(|e| e.to_string())?;
    JOBS.lock().unwrap().push((j.printer.into(), j.paper.into(), j.copies, j.title.into(), opts));
    Ok(format!("{}-7", j.printer))
}

/// A session with the stand-in printers and plotter installed and a line to plot.
fn session() -> Session {
    set_plotter_store(PlotterStore::memory());
    set_system_printers(SystemPrinters { list: printers, print });
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Err("no writer in this test".into()), plot: Some(echo_plot) });
    // Wait for the first listing, so every test starts with the printer listed, whichever runs first.
    cadcraft_engine::cmd::plotters::page_device("pagesetup", &json!({ "device": "Office_T120" }), &mut cadcraft_doc::PageSetup::default()).unwrap();
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
    JOBS.lock().unwrap().clear();
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
/// written stops the plot before anything prints (a missing folder is made, see
/// `plot_to_file_creates_missing_folders_before_submitting`). REFRESH asks for the printers again.
#[test]
fn plot_to_file_and_print_and_refresh_the_printers() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
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

    // A folder can't be made under a file: the PDF can't be written, so nothing is printed.
    let bad = path.join("plan.pdf");
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
    // Without a device, the stamp names the built-in device of the format written.
    let r = s.execute("plot", &json!({ "layout": "Model", "output": "png", "stamp": "Checked {device}" })).unwrap();
    let opts: Value = serde_json::from_slice(&cadcraft_engine::cmd::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(opts["stamp"], "Checked CADCraft PNG");
    let r = s.execute("exportpdf", &json!({ "layout": "Model", "stamp": false })).unwrap();
    let opts: Value = serde_json::from_slice(&cadcraft_engine::cmd::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
    assert!(opts.get("stamp").is_none());
    assert!(s.execute("plot", &json!({ "layout": "Model", "stamp": 5 })).is_err());

    let asked = LISTINGS.load(std::sync::atomic::Ordering::SeqCst);
    let r = s.execute("plottermanager", &json!({ "action": "refresh" })).unwrap();
    assert_eq!(r["printers"], true);
    // A refresh asks the system again rather than answering from the cache (bounded wait for
    // the background listing).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while LISTINGS.load(std::sync::atomic::Ordering::SeqCst) == asked && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(LISTINGS.load(std::sync::atomic::Ordering::SeqCst) > asked, "refresh listed the printers again");
    let list = s.execute("plottermanager", &json!({ "action": "list" })).unwrap();
    assert!(list["devices"].as_array().unwrap().iter().any(|d| d["name"] == "Office_T120"));
}

#[test]
fn an_inferred_image_output_does_not_submit_to_a_layouts_printer() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
    s.execute("pagesetup", &json!({ "layout": "Layout1", "device": "Office_T120" })).unwrap();
    let dir = std::env::temp_dir().join(format!("cadcraft-image-no-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for output in ["png", "svg"] {
        let path = dir.join(format!("plan.{output}"));
        let r = s.execute("plot", &json!({ "layout": "Layout1", "path": path })).unwrap();
        let options: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(options["output"], output);
        assert!(r.get("job").is_none());
        assert!(JOBS.lock().unwrap().is_empty());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plot_to_file_creates_missing_folders_before_submitting() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
    let dir = std::env::temp_dir().join(format!("cadcraft-new-plot-folder-{}", std::process::id()));
    let path = dir.join("nested/plan.pdf");
    let r = s.execute("plot", &json!({ "device": "Office_T120", "path": path })).unwrap();
    assert!(path.is_file());
    assert!(r.get("createdFolder").is_some(), "{r}");
    // Take the job: the tests share JOBS and run in any order.
    assert!(JOBS.lock().unwrap().pop().is_some());
    assert!(JOBS.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_multi_sheet_printer_plot_is_refused_before_writing_or_submitting() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
    let dir = std::env::temp_dir().join(format!("cadcraft-multi-print-{}", std::process::id()));
    let path = dir.join("new-folder/plot.pdf");
    let e = s.execute("plot", &json!({ "layouts": ["Layout1", "Layout2"], "device": "Office_T120", "path": path })).unwrap_err();
    assert!(e.to_string().contains("several sheets"), "{e}");
    assert!(!dir.exists());
    assert!(JOBS.lock().unwrap().is_empty());
    let r = s.execute("plot", &json!({ "layouts": ["Layout1", "Layout2"], "device": "CADCraft PDF" })).unwrap();
    assert_eq!(r["pages"], 2);
    s.execute("plot", &json!({ "layouts": ["Layout1"], "device": "Office_T120", "paper": "Letter" })).unwrap();
    assert_eq!(JOBS.lock().unwrap().len(), 1);
}

#[test]
fn printer_pdf_file_output_obeys_the_write_root_before_creating_folders() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
    let base = std::env::temp_dir().join(format!("cadcraft-printer-root-{}", std::process::id()));
    let allowed = base.join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let roots = cadcraft_engine::file_access::AutomationRoots::new(None, Some(&allowed)).unwrap();
    cadcraft_engine::file_access::confine(roots.as_ref(), || {
        let outside = base.join("outside/new/plot.pdf");
        let e = s.execute("plot", &json!({ "device": "Office_T120", "path": outside })).unwrap_err();
        assert!(e.to_string().contains("outside the write root"), "{e}");
        assert!(!base.join("outside").exists());
        assert!(JOBS.lock().unwrap().is_empty());
        let inside = allowed.join("new/plot.pdf");
        s.execute("plot", &json!({ "device": "Office_T120", "path": inside })).unwrap();
        assert!(inside.is_file());
        assert_eq!(JOBS.lock().unwrap().len(), 1);
    });
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_copy_of_a_system_printer_is_a_user_device() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    let made = s.execute("plottermanager", &json!({ "action": "new", "name": "My T120", "from": "Office_T120" })).unwrap();
    let got = s.execute("plottermanager", &json!({ "action": "get", "name": "My T120" })).unwrap();
    for r in [&made, &got] {
        assert_eq!((&r["system"], &r["default"]), (&json!(false), &json!(false)), "{r}");
    }
    let printer = s.execute("plottermanager", &json!({ "action": "get", "name": "Office_T120" })).unwrap();
    assert_eq!((&printer["system"], &printer["default"]), (&json!(true), &json!(true)), "the printer keeps its flags");
    // The device store is shared by the tests: leave it as it was.
    s.execute("plottermanager", &json!({ "action": "delete", "name": "My T120" })).unwrap();
}

#[test]
fn a_refused_print_job_reports_the_pdf_already_saved() {
    let _one = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = session();
    JOBS.lock().unwrap().clear();
    let dir = std::env::temp_dir().join(format!("cadcraft-refused-print-{}", std::process::id()));
    let path = dir.join("plot.pdf");
    let e = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "path": path, "copies": 13 })).unwrap_err().to_string();
    assert!(path.is_file());
    assert!(e.contains(path.to_str().unwrap()) && e.contains("refused"), "{e}");
    let e = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120", "copies": 13 })).unwrap_err().to_string();
    assert!(!e.contains("saved"), "nothing was saved without a path: {e}");
    std::fs::remove_dir_all(dir).unwrap();
}
