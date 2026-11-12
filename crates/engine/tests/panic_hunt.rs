//! Fuzz every registered command with adversarial params: a command must return `Err`, never panic
//! or hang. Each call gets a fresh, populated session on its own thread with a deadline, so one bad
//! command can't change what the next one sees or wedge the run.
//!
//! `Session::execute` contains a command's panic and returns an ordinary `Err`, so looking at the
//! result can't find one, and neither can `catch_unwind` here. A panic hook counts them instead,
//! on whichever thread they happen. `contained_panics_count_and_graceful_errors_do_not` proves it.

use cadcraft_engine::{Session, command_specs};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once, PoisonError, mpsc};
use std::time::Duration;

/// A drawing with something to act on, so commands reach their real work: with nothing in it most
/// of them stop at "nothing to do", and with no drawing at all they don't run.
fn fresh() -> Session {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0], [10, 10]]})).unwrap();
    s.execute("circle", &json!({"center": [5, 5], "radius": 2})).unwrap();
    s
}

/// Wall-clock budget for one command on the small drawing. Real work takes milliseconds, so this
/// only catches hangs and work that grows with a number in the params.
const DEADLINE: Duration = Duration::from_secs(4);

/// Budget for the self-test, which checks how panics are counted: the hook prints a backtrace
/// under `RUST_BACKTRACE=1`, and the first symbol lookup can be slow on a loaded runner.
const SELF_TEST_DEADLINE: Duration = Duration::from_secs(120);

/// Panics anywhere in the process, counted by a hook.
static PANICS: AtomicUsize = AtomicUsize::new(0);
/// One run at a time, so a panic is charged to the run that caused it.
static RUN: Mutex<()> = Mutex::new(());

/// Run `f` on a worker thread. `None` if it took longer than `deadline`, else whether it finished
/// without panicking, contained or not (returning `Ok` or a graceful `Err` both count).
fn run_watched(deadline: Duration, f: impl FnOnce() + Send + 'static) -> Option<bool> {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::SeqCst);
            default(info);
        }));
    });
    let _one = RUN.lock().unwrap_or_else(PoisonError::into_inner);
    let before = PANICS.load(Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = catch_unwind(AssertUnwindSafe(f));
        let _ = tx.send(r.is_ok());
    });
    rx.recv_timeout(deadline).ok().map(|ok| ok && PANICS.load(Ordering::SeqCst) == before)
}

fn run_once(id: &str, p: &Value) -> Option<bool> {
    let (id, p) = (id.to_string(), p.clone());
    run_watched(DEADLINE, move || {
        let mut s = fresh();
        let _ = s.execute(&id, &p);
    })
}

/// `Err` with a reason on a panic or a hang. A real hang reproduces and one-time setup (the first
/// font scan) doesn't, so a timeout counts only if a rerun on a fresh session times out too.
fn run_guarded(id: &str, p: &Value) -> Result<(), String> {
    match run_once(id, p).or_else(|| run_once(id, p)) {
        Some(true) => Ok(()),
        Some(false) => Err("panicked".into()),
        None => Err(format!("hung (>{}s, twice)", DEADLINE.as_secs())),
    }
}

/// Shapes and sizes a hostile script, drawing or agent could send. No param names a real path:
/// the hunt must not write files.
fn adversarial() -> Vec<Value> {
    let long = "x".repeat(100_000);
    vec![
        json!(null),
        json!({}),
        json!([]),
        json!("x"),
        json!({"points": [[1e308, -1e308], ["a"]], "handles": ["zz", 99999]}),
        json!({"points": [], "handles": [], "center": [], "from": [], "to": [], "name": "", "layer": "", "style": "", "text": ""}),
        json!({"center": [1e308, 1e308], "radius": 1e308, "angle": 1e308, "scale": 1e308, "height": 1e308, "width": 1e308, "distance": 1e308}),
        json!({"center": [0, 0], "radius": -1, "angle": -1e308, "scale": 0, "height": 0, "width": 0, "distance": -1, "factor": 0}),
        json!({"count": 1e9, "rows": 1e9, "columns": 1e9, "levels": 1e9, "items": 1e9, "sides": 1e9, "segments": 1e9, "index": 1e9}),
        json!({"count": -1, "rows": -1, "columns": -1, "levels": -1, "items": -1, "sides": -1, "segments": -1, "index": -1}),
        json!({"name": long, "layer": long, "text": long, "style": long, "mode": long}),
        json!({"name": "\u{0}\u{202e}\u{1f4a5}", "text": r"%%%\P{\H0;", "gradient": {"name": 5, "color1": [1], "angle": "x", "centered": "y"}}),
    ]
}

#[test]
fn no_command_panics_or_hangs_on_adversarial_params() {
    let params = adversarial();
    let mut bad: Vec<String> = Vec::new();
    for spec in command_specs() {
        for p in &params {
            if let Err(why) = run_guarded(spec.id, p) {
                let shown: String = p.to_string().chars().take(120).collect();
                bad.push(format!("{} [{why}] <- {shown}", spec.id));
                break;
            }
        }
    }
    assert!(bad.is_empty(), "{} commands panicked or hung:\n{}", bad.len(), bad.join("\n"));
}

/// A panic the code under test contains still fails the run; a graceful `Err` does not. Without
/// this the hunt could quietly stop detecting anything.
#[test]
fn contained_panics_count_and_graceful_errors_do_not() {
    let contained = run_watched(SELF_TEST_DEADLINE, || {
        let _ = catch_unwind(|| panic!("contained, as Session::execute does"));
    });
    assert_eq!(contained, Some(false));
    let graceful = run_watched(SELF_TEST_DEADLINE, || {
        let mut s = fresh();
        assert!(s.execute("no-such-command", &json!({})).is_err());
    });
    assert_eq!(graceful, Some(true));
}
