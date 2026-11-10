//! Data an add-on keeps on a drawing (`Drawing::ext`) is saved in the DXF as one XRECORD per key
//! in the named object dictionary (`CADCRAFT_EXT_<KEY>`), and read back through the loader the
//! add-on registered for that key.

use std::sync::Arc;

use cadcraft_doc::{Drawing, ExtData};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
struct Note {
    text: String,
}

impl ExtData for Note {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn dyn_eq(&self, other: &dyn ExtData) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
    fn to_json(&self) -> Option<Value> {
        Some(json!({ "text": self.text }))
    }
}

fn load_note(v: Value) -> Option<Arc<dyn ExtData>> {
    let text = v.get("text")?.as_str()?.to_string();
    Some(Arc::new(Note { text }))
}

/// Kept in memory only: `to_json` is `None`.
#[derive(Debug, PartialEq)]
struct Scratch;

impl ExtData for Scratch {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn dyn_eq(&self, other: &dyn ExtData) -> bool {
        other.as_any().is::<Self>()
    }
}

fn occurrences(text: &str, name: &str) -> usize {
    text.lines().filter(|l| l.trim() == name).count()
}

#[test]
fn extension_data_round_trips_through_dxf() {
    cadcraft_doc::ext::register_loader("test.note_data", load_note);
    let mut d = Drawing::new_metric();
    d.ext.set("test.note_data", Note { text: "kept \"quoted\" ünïcode".into() });
    d.ext.set("test.scratch", Scratch);

    let text = cadcraft_io::write_dxf(&d);
    assert_eq!(occurrences(&text, "CADCRAFT_EXT_TEST_NOTE_DATA"), 1, "one XRECORD for the key");
    assert_eq!(occurrences(&text, "CADCRAFT_EXT_TEST_SCRATCH"), 0, "values without JSON stay in memory");

    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.ext.get::<Note>("test.note_data"), d.ext.get::<Note>("test.note_data"));
    assert!(back.ext.get::<Scratch>("test.scratch").is_none());

    // Saving the reopened drawing writes the value once, not once more for the copy read.
    let again = cadcraft_io::write_dxf(&back);
    assert_eq!(occurrences(&again, "CADCRAFT_EXT_TEST_NOTE_DATA"), 1);
}

/// A loader that refuses what it reads (a value from a newer, incompatible version).
fn refuse(_: Value) -> Option<Arc<dyn ExtData>> {
    None
}

/// A loader with a bug.
fn panics(_: Value) -> Option<Arc<dyn ExtData>> {
    panic!("a loader bug")
}

#[test]
fn extension_data_without_a_loader_refused_or_whose_loader_panics_is_not_loaded() {
    cadcraft_doc::ext::register_loader("test.strict", refuse);
    cadcraft_doc::ext::register_loader("test.buggy", panics);
    let mut d = Drawing::new_metric();
    d.ext.set("test.unclaimed", Note { text: "nobody loads me".into() });
    d.ext.set("test.strict", Note { text: "refused".into() });
    d.ext.set("test.buggy", Note { text: "the drawing still opens".into() });
    let text = cadcraft_io::write_dxf(&d);
    assert_eq!(occurrences(&text, "CADCRAFT_EXT_TEST_UNCLAIMED"), 1);
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(back.ext.is_empty(), "{:?}", back.ext.keys().collect::<Vec<_>>());
}
