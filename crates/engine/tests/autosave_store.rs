//! Autosave into a key-value store (the web build's `localStorage`, docs/gaps.md F4): records are
//! written SAVETIME after a change and removed when the drawing is saved or closed, DRAWINGRECOVERY
//! lists, opens and discards what a stopped tab left, and a store that is missing, full or holds
//! damaged records is reported, never a panic. Its own test binary: SAVETIME is process-wide (every
//! test here sets it to 1 minute), the store is per thread (each test installs its own).

use std::cell::RefCell;
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_engine::autosave::store::{self, MemoryStore, Store, StoreError};
use cadcraft_engine::autosave::{self, Autosaver};
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use serde_json::json;

const DXF: &[u8] = b"0\nSECTION\n0\nENDSEC\n0\nEOF\n";

/// A [`MemoryStore`] the test keeps a handle on while the engine owns the installed box.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<MemoryStore>>);

impl Store for Shared {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.0.borrow().get(key)
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        self.0.borrow_mut().set(key, value)
    }
    fn remove(&mut self, key: &str) -> Result<(), StoreError> {
        self.0.borrow_mut().remove(key)
    }
    fn keys(&self) -> Result<Vec<String>, StoreError> {
        self.0.borrow().keys()
    }
    fn now(&self) -> f64 {
        self.0.borrow().now()
    }
}

impl Shared {
    fn at(&self, secs: f64) {
        self.0.borrow_mut().clock = secs;
    }
    fn map(&self) -> std::collections::BTreeMap<String, String> {
        self.0.borrow().map.clone()
    }
}

/// Install a fresh store at time 1000 s and the DXF hooks (a "DXF" is anything starting `0\n`).
fn setup() -> Shared {
    set_io(IoHooks {
        read: |b, _| if b.starts_with(b"0\n") { Ok(Drawing::new_metric()) } else { Err("not a DXF file".into()) },
        write: |_, _| Ok(DXF.to_vec()),
        plot: None,
    });
    autosave::set_savetime(1);
    let shared = Shared::default();
    shared.at(1000.0);
    store::install(Box::new(shared.clone()));
    shared
}

fn draw(s: &mut Session) {
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
}

fn records(shared: &Shared) -> Vec<String> {
    shared.map().into_keys().filter(|k| k.starts_with(store::KEY_PREFIX)).collect()
}

#[test]
fn a_changed_drawing_is_kept_in_the_store_until_it_is_saved() {
    let shared = setup();
    let mut s = Session::new();
    let uid = s.state().unwrap().uid;
    let mut a = Autosaver::for_store("12-ab");
    assert!(a.uses_store());
    assert_eq!(a.tick_store(&s), autosave::StoreTick::default(), "nothing to save in a clean drawing");

    draw(&mut s);
    assert_eq!(a.tick_store(&s).next_due, Some(60.0), "the change starts the clock");
    shared.at(1059.0);
    assert_eq!(a.tick_store(&s).written, 0);
    shared.at(1060.0);
    let tick = a.tick_store(&s);
    assert_eq!((tick.written, tick.errors.len()), (1, 0));
    let key = store::key(&autosave::file_name("Drawing1", "12-ab", uid));
    assert_eq!(records(&shared), vec![key.clone()]);
    assert_eq!(a.files(), vec![key.clone()]);
    let record = shared.map()[&key].clone();
    assert!(record.starts_with("CCAS1 1060 t "), "{record}");
    assert_eq!(store::decode(&record).unwrap(), DXF);
    assert_eq!(shared.map().get(&store::lock_key("12-ab")).map(String::as_str), Some("1060"), "a running app checks in");

    // While it runs, its record is not offered; another app sees nothing to recover.
    assert!(s.execute("drawingrecovery", &json!({})).unwrap()["files"].as_array().unwrap().is_empty());

    // Saving (nothing unsaved left) deletes the record and, with nothing left, the stale lock.
    let st = s.state_mut().unwrap();
    st.saved = st.doc.clone();
    shared.at(2000.0);
    a.tick_store(&s);
    assert!(records(&shared).is_empty());
    assert!(a.files().is_empty());
}

#[test]
fn drawing_recovery_opens_and_discards_what_a_stopped_tab_left() {
    let shared = setup();
    // Tab 1 autosaves two drawings, then its page goes away (reload, close or crash).
    let mut s1 = Session::new();
    draw(&mut s1);
    s1.execute("new", &json!({})).unwrap();
    draw(&mut s1);
    let mut tab1 = Autosaver::for_store("1-a");
    tab1.tick_store(&s1);
    shared.at(1060.0);
    assert_eq!(tab1.tick_store(&s1).written, 2);
    store::with(|st| store::forget_instance(st, "1-a")).unwrap();
    // Keys of other code on the site are never listed or touched.
    shared.0.borrow_mut().map.insert("other.app/settings".into(), "keep".into());

    // Tab 2 starts: the same list the Drawing Recovery window shows.
    let mut s2 = Session::empty();
    let r = s2.execute("drawingrecovery", &json!({})).unwrap();
    let files = r["files"].as_array().unwrap().clone();
    assert_eq!(files.len(), 2, "{files:?}");
    assert_eq!(files[0]["modified"], 1060);
    assert_eq!(files[0]["bytes"], DXF.len());

    let first = files[0]["path"].as_str().unwrap();
    let r = s2.execute("drawingrecovery", &json!({ "action": "open", "path": first })).unwrap();
    assert!(r["uid"].as_u64().is_some());
    let st = s2.state().unwrap();
    assert_eq!(st.title, files[0]["drawing"].as_str().unwrap());
    assert_eq!(st.path, None, "a recovered drawing is an unsaved copy");
    assert!(st.is_dirty());
    assert!(shared.map().contains_key(first), "the record stays until the copy is saved");

    let second = files[1]["path"].as_str().unwrap();
    let r = s2.execute("drawingrecovery", &json!({ "action": "discard", "path": second })).unwrap();
    assert_eq!(r["files"].as_array().map(Vec::len), Some(1));
    assert!(!shared.map().contains_key(second));

    // Only listed records: anything else is refused and left alone.
    for path in ["other.app/settings", "cadcraft.autosave/notes.txt", "/etc/passwd"] {
        assert!(s2.execute("drawingrecovery", &json!({ "action": "discard", "path": path })).is_err(), "{path}");
        assert!(s2.execute("drawingrecovery", &json!({ "action": "open", "path": path })).is_err(), "{path}");
    }
    assert_eq!(shared.map()["other.app/settings"], "keep");
}

#[test]
fn records_of_a_tab_still_running_are_offered_only_once_its_lock_goes_stale() {
    let shared = setup();
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::for_store("7-c");
    a.tick_store(&s);
    shared.at(1060.0);
    a.tick_store(&s);
    let listed = || store::with(|st| store::recoverable(st, "8-d")).unwrap();
    assert!(listed().is_empty(), "fresh lock");
    shared.at(1060.0 + autosave::STALE_LOCK_SECS as f64 - 1.0);
    assert!(listed().is_empty());
    shared.at(1060.0 + autosave::STALE_LOCK_SECS as f64);
    assert_eq!(listed().len(), 1, "stopped checking in");
    assert!(store::with(|st| store::recoverable(st, "7-c")).unwrap().is_empty(), "never an app's own records");

    // A lock in the future (clock changes) counts as fresh; a damaged one as stale.
    shared.0.borrow_mut().map.insert(store::lock_key("7-c"), "1e12".into());
    assert!(listed().is_empty());
    shared.0.borrow_mut().map.insert(store::lock_key("7-c"), "soon".into());
    assert_eq!(listed().len(), 1);
}

#[test]
fn damaged_records_are_listed_reported_and_discardable() {
    let shared = setup();
    let key = store::key(&autosave::file_name("Plan", "3-e", 9));
    let cut = store::key(&autosave::file_name("Cut", "3-e", 10));
    let bad_dxf = store::key(&autosave::file_name("Junk", "3-e", 11));
    {
        let mut m = shared.0.borrow_mut();
        m.map.insert(key.clone(), "garbage".into());
        m.map.insert(cut.clone(), "CCAS1 5 t 999\n0\nSECTION".into());
        m.map.insert(bad_dxf.clone(), store::encode(7, b"not dxf at all"));
    }
    let mut s = Session::empty();
    let files = s.execute("drawingrecovery", &json!({})).unwrap()["files"].as_array().unwrap().clone();
    assert_eq!(files.len(), 3);
    assert!(files.iter().any(|f| f["path"] == key.as_str() && f["modified"] == 0), "{files:?}");

    for k in [&key, &cut] {
        let e = s.execute("drawingrecovery", &json!({ "action": "open", "path": k })).unwrap_err().to_string();
        assert!(e.contains("damaged"), "{e}");
    }
    let e = s.execute("drawingrecovery", &json!({ "action": "open", "path": bad_dxf })).unwrap_err().to_string();
    assert!(e.contains("can't be read"), "{e}");
    assert!(s.state().is_err(), "nothing opened");

    for k in [&key, &cut, &bad_dxf] {
        s.execute("drawingrecovery", &json!({ "action": "discard", "path": k })).unwrap();
    }
    assert!(records(&shared).is_empty());
}

#[test]
fn a_full_store_is_reported_and_the_autosave_tried_again() {
    let shared = setup();
    shared.0.borrow_mut().quota = Some(40);
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::for_store("4-f");
    a.tick_store(&s);
    shared.at(1060.0);
    let tick = a.tick_store(&s);
    assert_eq!(tick.written, 0);
    assert_eq!(tick.errors.len(), 1);
    assert!(tick.errors[0].contains("Drawing1") && tick.errors[0].contains("full"), "{:?}", tick.errors);
    assert!(records(&shared).is_empty());

    // Not counted as autosaved: SAVETIME later it is written once there is room.
    shared.0.borrow_mut().quota = None;
    assert_eq!(a.tick_store(&s).written, 0, "the clock restarts");
    shared.at(1120.0);
    assert_eq!(a.tick_store(&s).written, 1);
}

#[test]
fn autosaves_together_stay_within_the_budget() {
    let shared = setup();
    let big = vec![b'x'; store::MAX_TOTAL_BYTES / 2];
    let k1 = store::key(&autosave::file_name("A", "5-a", 1));
    let k2 = store::key(&autosave::file_name("B", "5-a", 2));
    store::with(|st| store::write(st, &k1, &big, 1.0)).unwrap().unwrap();
    // Replacing a record doesn't count its old version.
    store::with(|st| store::write(st, &k1, &big, 2.0)).unwrap().unwrap();
    let e = store::with(|st| store::write(st, &k2, &big, 3.0)).unwrap().unwrap_err();
    assert!(matches!(e, StoreError::Quota(_)), "{e}");
    assert_eq!(records(&shared), vec![k1]);
}

#[test]
fn a_missing_store_never_panics() {
    let shared = setup();
    shared.0.borrow_mut().unavailable = true;
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::for_store("6-b");
    a.tick_store(&s);
    shared.at(1060.0);
    let tick = a.tick_store(&s);
    assert_eq!(tick.written, 0);
    assert!(tick.errors[0].contains("not available"), "{:?}", tick.errors);
    assert!(s.execute("drawingrecovery", &json!({})).unwrap()["files"].as_array().unwrap().is_empty());
    assert!(a.save_now_store(&s).errors.len() == 1);

    // No store installed at all: the store autosaver does nothing.
    store::uninstall();
    assert_eq!(a.tick_store(&s), autosave::StoreTick::default());
}

#[test]
fn save_now_writes_at_once() {
    let shared = setup();
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::for_store("2-a");
    assert_eq!(a.save_now_store(&s).written, 1);
    assert_eq!(records(&shared).len(), 1);
    assert!(shared.map().contains_key(&store::lock_key("2-a")));
}

#[test]
fn records_round_trip_and_damage_is_detected() {
    assert_eq!(store::decode(&store::encode(42, DXF)).unwrap(), DXF);
    let binary = [0u8, 159, 146, 150, 255];
    let r = store::encode(1, &binary);
    assert!(r.starts_with("CCAS1 1 b "), "{r}");
    assert_eq!(store::decode(&r).unwrap(), binary);
    assert_eq!(store::header(&store::encode(9, b"abc")), Ok((9, 3)));
    for bad in
        ["", "CCAS1 1 t 3", "XXXX 1 t 3\nabc", "CCAS1 x t 3\nabc", "CCAS1 1 z 3\nabc", "CCAS1 1 t\nabc", "CCAS1 1 t 3 4\nabc", "CCAS1 1 t 4\nabc"]
    {
        assert!(store::decode(bad).is_err(), "{bad:?}");
    }
    assert!(store::decode("CCAS1 1 b 4\n!!!!").is_err());

    let id = store::instance_id(u32::MAX, u64::MAX);
    assert!(autosave::is_instance_id(&id), "{id}");
    assert!(autosave::is_instance_id(&store::instance_id(0, 0)));
    assert_eq!(store::parse_key(&store::key("Plan_1-a_3.sv$")), Some(("Plan", "1-a", 3)));
    assert_eq!(store::parse_key("Plan_1-a_3.sv$"), None, "not one of our keys");
}
