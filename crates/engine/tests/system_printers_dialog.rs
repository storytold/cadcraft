//! Dialog edits use cached devices even while the refresh worker is blocked.
use cadcraft_doc::{PageSetup, PlotterConfig};
use cadcraft_engine::cmd::plotters::{self, PrintJob, SystemPrinters};
use serde_json::json;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
static RELEASE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
fn list() -> Vec<PlotterConfig> {
    if CALLS.fetch_add(1, Ordering::SeqCst) > 0 {
        let _ = RELEASE.1.wait_timeout_while(RELEASE.0.lock().unwrap(), std::time::Duration::from_secs(3), |ready| !*ready).unwrap();
    }
    vec![PlotterConfig { name: "Cached printer".into(), system: true, ..PlotterConfig::default() }]
}
fn print(_: &PrintJob) -> Result<String, String> {
    Err("not printing".into())
}
#[test]
fn dialog_device_lookup_does_not_wait_for_refresh() {
    plotters::set_system_printers(SystemPrinters { list, print });
    // The command path waits for the initial list.
    let mut page = PageSetup::default();
    plotters::page_device("pagesetup", &json!({ "device": "Cached printer" }), &mut page).unwrap();
    plotters::refresh_system_printers();
    let start = std::time::Instant::now();
    plotters::page_device_now("plot", &json!({ "device": "Cached printer" }), &mut page).unwrap();
    let took = start.elapsed();
    *RELEASE.0.lock().unwrap() = true;
    RELEASE.1.notify_all();
    assert!(took < std::time::Duration::from_secs(1), "dialog waited {took:?}");
}
