//! A printer listing that doesn't end within the wait: PLOT naming an unknown device waits for it
//! once, not again for its fallback and its error message. Its own test binary: the system
//! printers are set once per process.

use cadcraft_doc::PlotterConfig;
use cadcraft_engine::Session;
use cadcraft_engine::cmd::plotters::{PlotterStore, PrintJob, SystemPrinters, set_plotter_store, set_system_printers};
use serde_json::json;

/// A listing that hangs far past the wait.
fn printers() -> Vec<PlotterConfig> {
    std::thread::sleep(std::time::Duration::from_secs(120));
    Vec::new()
}

fn print(_: &PrintJob) -> Result<String, String> {
    Err("no printing in this test".into())
}

#[test]
fn an_unknown_device_waits_for_a_hung_listing_only_once() {
    set_plotter_store(PlotterStore::memory());
    set_system_printers(SystemPrinters { list: printers, print });
    let mut s = Session::new();
    s.cmdline("line 0,0 100,0").unwrap();
    s.cmdline("").unwrap();
    let start = std::time::Instant::now();
    let r = s.execute("plot", &json!({ "layout": "Model", "device": "Nope" }));
    let took = start.elapsed();
    assert!(r.unwrap_err().to_string().contains("unknown plotter `Nope`"));
    // One wait of PRINTERS_WAIT (10 s), not three.
    assert!(took < std::time::Duration::from_secs(15), "{took:?}");
}
