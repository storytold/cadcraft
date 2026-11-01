//! A plot that names a system printer while the printers are still being listed (the first plot
//! after the app starts) waits for the list, and the printer wins over a user device of the same
//! name. Its own test binary: the system printers are set once per process.

use cadcraft_doc::{DevicePaper, Drawing, PlotterConfig, Space};
use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::cmd::plotters::{PlotterStore, PrintJob, SystemPrinters, set_plotter_store, set_system_printers};
use serde_json::{Value, json};

fn echo_plot(_: &Drawing, _: &Space, opts: &Value) -> Result<Vec<u8>, String> {
    Ok(opts.to_string().into_bytes())
}

/// A slow listing: the system answers after 300 ms.
fn printers() -> Vec<PlotterConfig> {
    std::thread::sleep(std::time::Duration::from_millis(300));
    vec![PlotterConfig {
        name: "Office_T120".into(),
        default_paper: "Letter".into(),
        papers: vec![DevicePaper { name: "Letter".into(), width: 215.9, height: 279.4, margins: [5.0; 4] }],
        system: true,
        ..PlotterConfig::default()
    }]
}

fn print(j: &PrintJob) -> Result<String, String> {
    Ok(format!("{}-1", j.printer))
}

#[test]
fn a_printer_still_being_listed_is_waited_for_and_wins_over_a_user_device() {
    set_plotter_store(PlotterStore::memory());
    set_system_printers(SystemPrinters { list: printers, print });
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Err("no writer in this test".into()), plot: Some(echo_plot) });
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    // A user plotter with the printer's name, made while the listing is under way.
    s.execute("plottermanager", &json!({ "action": "new", "name": "Office_T120" })).unwrap();
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "Office_T120" })).unwrap();
    assert_eq!(r["job"], "Office_T120-1", "the printer, not the user plotter: {r}");
}
