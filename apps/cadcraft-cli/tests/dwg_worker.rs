//! The DWG reader and writer run in a child process, so a decoder that dies ends one open or save,
//! not the app. The worker setting is process-wide, so everything runs in one test.

use cadcraft_io::dwg_worker;

#[test]
fn a_real_worker_reads_and_writes_and_its_death_is_an_error_here() {
    let drawing = cadcraft_engine::sample::bracket();
    let in_process = cadcraft_io::write(&drawing, "a.dwg").unwrap();

    dwg_worker::use_worker(env!("CARGO_BIN_EXE_cadcraft-cli").into(), vec![dwg_worker::WORKER_ARG.into()]);
    let written = cadcraft_io::write(&drawing, "a.dwg").unwrap();
    assert_eq!(written.get(..6), in_process.get(..6), "the same version of file");
    let read = cadcraft_io::read(&written, "a.dwg").unwrap();
    assert_eq!(read.model.len(), drawing.model.len());

    // A decode error comes back as that error, not as the worker stopping.
    let mut bad = in_process.clone();
    bad.truncate(64);
    let e = cadcraft_io::read(&bad, "a.dwg").unwrap_err().to_string();
    assert!(!e.contains("worker"), "{e}");

    // The worker aborts as the third-party code can. This process goes on.
    let e = dwg_worker::crash_worker_for_test().unwrap_err();
    assert!(e.contains("worker stopped"), "{e}");
    assert!(cadcraft_io::read(&written, "a.dwg").is_ok(), "the next file gets a new worker");

    // No worker to start: the work happens here, as without one.
    dwg_worker::use_worker("no-such-program-here".into(), Vec::new());
    assert!(cadcraft_io::read(&written, "a.dwg").is_ok());
}
