//! Dialog edits use cached devices even while the refresh worker is blocked.
use cadcraft_doc::{PageSetup, PlotterConfig};
use cadcraft_engine::cmd::plotters::{self, PrintJob, SystemPrinters};
use serde_json::json;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
/// Refresh listings that have returned (the blocked one only after RELEASE or its timeout).
static RETURNED: AtomicUsize = AtomicUsize::new(0);
static RELEASE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
fn list() -> Vec<PlotterConfig> {
    if CALLS.fetch_add(1, Ordering::SeqCst) > 0 {
        let _ = RELEASE.1.wait_timeout_while(RELEASE.0.lock().unwrap(), std::time::Duration::from_secs(10), |ready| !*ready).unwrap();
        RETURNED.fetch_add(1, Ordering::SeqCst);
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
    // Wait (bounded, so a broken refresh fails instead of hanging) until the refresh worker is
    // inside the blocked listing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while CALLS.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(CALLS.load(Ordering::SeqCst) >= 2, "the refresh started a listing");
    plotters::page_device_now("plot", &json!({ "device": "Cached printer" }), &mut page).unwrap();
    let still_blocked = RETURNED.load(Ordering::SeqCst) == 0;
    *RELEASE.0.lock().unwrap() = true;
    RELEASE.1.notify_all();
    assert!(still_blocked, "the dialog lookup returned while the refresh was still blocked");
}
