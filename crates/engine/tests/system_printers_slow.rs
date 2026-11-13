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

/// Refuses every job: this test never gets as far as printing.
fn print(_: &PrintJob) -> Result<String, String> {
    Err("no printing in this test".into())
}

/// A hung listing costs one wait: the unknown device's error, a second plot and a plot with no device don't wait again.
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
    // After a wait that ran out, later plots don't wait again while the listings hang.
    let start = std::time::Instant::now();
    assert!(s.execute("plot", &json!({ "layout": "Model", "device": "Nope2" })).is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(2), "no second wait: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    let _ = s.execute("plot", &json!({ "layout": "Model", "device": "None" }));
    assert!(start.elapsed() < std::time::Duration::from_secs(2), "no device needs no printer list: {:?}", start.elapsed());
}
