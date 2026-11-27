//! What an add-on built into an app registers with the engine at startup: commands that join the
//! catalog beside the built-in ones, and passes that run after every command inside that
//! command's undo step. A pass that panics is undone and reported; the others still run.
//!
//! The registries are process-wide, so the whole scenario is one test.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Common, Drawing, EntityKind, ExtData, Line, Space};
use cadcraft_engine::{CommandSpec, Session, find_command, register_after_command, register_specs};
use serde_json::{Value, json};

/// What the add-on keeps on the drawing: the number of model-space objects it saw last.
#[derive(Clone, Debug, PartialEq)]
struct Count(usize);

impl ExtData for Count {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn dyn_eq(&self, other: &dyn ExtData) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
}

const COUNT: &str = "test.count";
const BROKEN: &str = "test.broken";
const AFTER: &str = "test.after";
const SWITCHED: &str = "test.switched";

/// Arms [`switching_pass`] for the next command only.
static SWITCH_ARMED: AtomicBool = AtomicBool::new(false);

fn add_line(s: &mut Session, _: &Value) -> cadcraft_engine::Result<Value> {
    let line = EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) });
    s.doc_mut()?.add(&Space::Model, Common::default(), line)?;
    Ok(Value::Null)
}

fn model_len(d: &Drawing) -> usize {
    d.space(&Space::Model).map_or(0, |m| m.len())
}

/// Keeps [`Count`] up to date after every command.
fn count_pass(s: &mut Session, _before: Option<&Arc<Drawing>>, _ok: bool) {
    let Ok(n) = s.doc().map(model_len) else { return };
    if s.doc().ok().and_then(|d| d.ext.get::<Count>(COUNT)) != Some(&Count(n))
        && let Ok(d) = s.doc_mut()
    {
        d.ext.set(COUNT, Count(n));
    }
}

/// Changes the drawing and the selection, then panics: its changes must not survive.
fn broken_pass(s: &mut Session, _before: Option<&Arc<Drawing>>, _ok: bool) {
    if let Ok(d) = s.doc_mut() {
        d.ext.set(BROKEN, Count(0));
    }
    if let Ok(st) = s.state_mut() {
        st.selection = st.doc.space(&Space::Model).map(|m| m.iter().map(|e| e.handle).collect()).unwrap_or_default();
    }
    panic!("an add-on bug");
}

/// Registered after the broken pass: it still runs.
fn after_pass(s: &mut Session, _before: Option<&Arc<Drawing>>, _ok: bool) {
    let Ok(n) = s.doc().map(model_len) else { return };
    if let Ok(d) = s.doc_mut() {
        d.ext.set(AFTER, Count(n));
    }
}

/// When armed: changes the drawing, makes another drawing the active one, then panics.
fn switching_pass(s: &mut Session, _before: Option<&Arc<Drawing>>, _ok: bool) {
    if !SWITCH_ARMED.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Ok(d) = s.doc_mut() {
        d.ext.set(SWITCHED, Count(0));
    }
    s.active = usize::from(s.active == 0);
    panic!("an add-on bug after a document switch");
}

fn count(s: &Session) -> Option<usize> {
    s.doc().ok().and_then(|d| d.ext.get::<Count>(COUNT)).map(|c| c.0)
}

#[test]
fn add_on_commands_and_after_command_passes() {
    // A command registered after the catalog was first read is found and runs.
    let before = cadcraft_engine::command_specs().len();
    assert!(find_command("test.addline").is_none());
    let refused = register_specs(vec![CommandSpec::new("test.addline", "Add a line", add_line)]);
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(cadcraft_engine::command_specs().len(), before + 1);
    assert!(find_command("test.addline").is_some());

    // An add-on never replaces a built-in command or one registered before, and a call that adds
    // nothing keeps the catalog as it is (no rebuild).
    let catalog = cadcraft_engine::command_specs().as_ptr();
    let refused = register_specs(vec![
        CommandSpec::new("line", "Not LINE", add_line),
        CommandSpec::new("test.addline", "Again", add_line),
        CommandSpec::new("LINE", "Not LINE either", add_line),
        CommandSpec::new("test.Upper", "Never found", add_line),
    ]);
    assert_eq!(refused, vec!["line", "test.addline", "LINE", "test.Upper"]);
    assert_eq!(find_command("line").map(|c| c.label), Some("Line"));
    assert_eq!(cadcraft_engine::command_specs().len(), before + 1);
    assert_eq!(cadcraft_engine::command_specs().as_ptr(), catalog);

    // The pass runs after the add-on's command and after built-in ones, in the same undo step.
    register_after_command(count_pass);
    register_after_command(count_pass); // the same pass twice runs once
    let mut s = Session::new();
    s.execute("new", &json!({})).unwrap();
    s.execute("test.addline", &json!({})).unwrap();
    assert_eq!(count(&s), Some(1));
    s.execute("line", &json!({"points": [[0, 0], [5, 5]]})).unwrap();
    assert_eq!(count(&s), Some(2));
    s.execute("u", &json!({})).unwrap();
    assert_eq!(count(&s), Some(1), "undo takes the pass's change back with the command's");

    // A pass that panics is undone and reported; the passes after it still run.
    register_after_command(broken_pass);
    register_after_command(after_pass);
    assert!(s.selection().is_empty());
    s.execute("test.addline", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(count(&s), Some(2));
    assert!(d.ext.get::<Count>(BROKEN).is_none(), "the broken pass's change was skipped");
    assert!(s.selection().is_empty(), "the broken pass's selection was skipped");
    assert_eq!(d.ext.get::<Count>(AFTER), Some(&Count(2)));

    // A pass that switches drawings and then panics: the drawing the command ran in gets its
    // saved state back and is active again, and the other drawing is not overwritten.
    register_after_command(switching_pass);
    s.execute("new", &json!({})).unwrap();
    assert!(s.docs.len() >= 2);
    let active = s.active;
    let other = usize::from(active == 0);
    let other_doc = Arc::clone(&s.docs[other].doc);
    SWITCH_ARMED.store(true, Ordering::SeqCst);
    s.execute("test.addline", &json!({})).unwrap();
    assert_eq!(s.active, active, "the drawing the command ran in is active again");
    assert!(s.doc().unwrap().ext.get::<Count>(SWITCHED).is_none(), "the switching pass's change was skipped");
    assert_eq!(model_len(s.doc().unwrap()), 1, "the command's own line stays");
    assert!(Arc::ptr_eq(&s.docs[other].doc, &other_doc), "the other drawing was not overwritten");
}
