//! Plug-in runs against a real session, with modules assembled from WebAssembly text.

use serde_json::json;

use crate::Session;
use crate::plugin::craft_plugin::bundle::Bundle;
use crate::temp_dir::TempDir;

/// A module that calls `calls` (command, params JSON) in order. A refused or failed call fails the
/// run when `strict`; otherwise its message becomes the output and the run goes on. `tail` is WAT
/// run after the calls (a trap, an endless loop), before returning 0.
fn module(calls: &[(&str, &str)], strict: bool, tail: &str) -> Vec<u8> {
    let mut data = String::new();
    let mut body = String::new();
    let mut at = 0usize;
    for (command, params) in calls {
        let (c, p) = (at, at + command.len());
        at = p + params.len();
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        data.push_str(&format!(r#"(data (i32.const {c}) "{}") (data (i32.const {p}) "{}")"#, esc(command), esc(params)));
        let on_error = if strict {
            "(return (i32.const 1))".to_string()
        } else {
            "(call $set_output (i32.const 8192) (call $reply_read (i32.const 8192) (call $reply_len)))".to_string()
        };
        body.push_str(&format!(
            "(if (i32.ne (call $call (i32.const {c}) (i32.const {}) (i32.const {p}) (i32.const {})) (i32.const 0)) (then {on_error}))\n",
            command.len(),
            params.len()
        ));
    }
    let wat = format!(
        r#"(module
          (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "craft" "reply_len" (func $reply_len (result i32)))
          (import "craft" "reply_read" (func $reply_read (param i32 i32) (result i32)))
          (import "craft" "set_output" (func $set_output (param i32 i32)))
          (memory (export "memory") 1)
          {data}
          (func (export "craft_run") (param $n i32) (result i32) {body} {tail} (i32.const 0)))"#
    );
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("{e}\n{wat}"))
}

/// The `.craftplugin` of plug-in `id` for CADCraft, asking for `access`.
fn bundle(id: &str, access: &str, wasm: Vec<u8>) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": "Test Plug-in", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"cadcraft": {"api": 6}},
        "permissions": {"access": access},
        "commands": [{"id": "go", "label": "Go"}]
    });
    Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode()
}

fn base64(bytes: &[u8]) -> String {
    crate::cmd::file::base64_encode(bytes)
}

/// A drawing with plug-in `id` installed for the session.
fn with_plugin(id: &str, access: &str, wasm: Vec<u8>) -> Session {
    let mut s = Session::new();
    let r = s.execute("plugin.install", &json!({"data": base64(&bundle(id, access, wasm)), "name": "test.craftplugin"})).unwrap();
    assert_eq!(r["installed"], true);
    assert_eq!(r["stored"], false, "without a store, a plug-in is kept for the session");
    s
}

fn objects(s: &Session) -> usize {
    s.doc().unwrap().entity_count()
}

fn undo_labels(s: &Session) -> Vec<String> {
    s.state().unwrap().undo.iter().map(|u| u.label.clone()).collect()
}

const LINE: (&str, &str) = ("line", r#"{"points":[[0,0],[10,0]]}"#);
const CIRCLE: (&str, &str) = ("circle", r#"{"center":[5,5],"radius":2}"#);

#[test]
fn a_write_plugin_runs_real_commands_as_one_undo_step_named_after_it() {
    let mut s = with_plugin("io.example.draw", "write", module(&[LINE, CIRCLE], true, ""));
    s.execute("line", &json!({"points": [[0, 5], [5, 5]]})).unwrap();
    let (count, steps) = (objects(&s), undo_labels(&s).len());

    let r = s.execute("plugin.run", &json!({"id": "io.example.draw"})).unwrap();

    assert_eq!(r["calls"], 2);
    assert_eq!(objects(&s), count + 2);
    let labels = undo_labels(&s);
    assert_eq!(labels.len(), steps + 1, "two commands, one undo step");
    assert_eq!(labels.last().map(String::as_str), Some("Test Plug-in"));
    assert_eq!(s.undo().unwrap().as_deref(), Some("Test Plug-in"));
    assert_eq!(objects(&s), count, "one undo takes the whole run back");
    assert_eq!(undo_labels(&s).len(), steps, "the drawing's own steps are still there");
    assert_eq!(s.redo().unwrap().as_deref(), Some("Test Plug-in"));
    assert_eq!(objects(&s), count + 2);
}

#[test]
fn a_run_that_changes_nothing_records_no_step_and_keeps_redo() {
    let mut s = with_plugin("io.example.lister", "write", module(&[("entities", "{}")], true, ""));
    s.execute("line", &json!({"points": [[0, 5], [5, 5]]})).unwrap();
    s.undo().unwrap();
    let steps = undo_labels(&s).len();

    s.execute("plugin.run", &json!({"id": "io.example.lister"})).unwrap();

    assert_eq!(undo_labels(&s).len(), steps);
    assert!(s.redo().unwrap().is_some(), "redo survives a run that edited nothing");
}

#[test]
fn a_read_plugin_that_edits_is_put_back_and_refused() {
    let mut s = with_plugin("io.example.reader", "read", module(&[("entities", "{}"), LINE, ("ortho", r#"{"on":true}"#)], false, ""));
    let (count, steps, ortho) = (objects(&s), undo_labels(&s).len(), s.settings.orthomode);
    let dirty = s.state().unwrap().is_dirty();

    let r = s.execute("plugin.run", &json!({"id": "io.example.reader"})).unwrap();

    let message = r["output"].as_str().unwrap();
    assert!(message.contains("may only read it"), "{message}");
    assert_eq!(objects(&s), count);
    assert_eq!(undo_labels(&s).len(), steps, "nothing to undo");
    assert_eq!(s.state().unwrap().is_dirty(), dirty, "the drawing isn't marked changed");
    assert_eq!(s.settings.orthomode, ortho, "the drafting settings are put back too");
}

#[test]
fn refused_commands_and_file_paths_are_refused() {
    for (call, why) in [
        (("open", r#"{"path":"/etc/hosts"}"#), "isn't available to plugins"),
        (("OPEN", "{}"), "isn't available to plugins"),
        (("qsave", "{}"), "isn't available to plugins"),
        (("saveas", r#"{"path":"/tmp/x.dxf"}"#), "isn't available to plugins"),
        (("u", "{}"), "isn't available to plugins"),
        (("undo", r#"{"option":"back"}"#), "isn't available to plugins"),
        (("lisp", r#"{"code":"(command \"open\")"}"#), "isn't available to plugins"),
        (("script", "{}"), "isn't available to plugins"),
        (("document.switch", "{}"), "isn't available to plugins"),
        (("compare", "{}"), "isn't available to plugins"),
        (("pasteclip", "{}"), "isn't available to plugins"),
        (("plugin.list", "{}"), "isn't available to plugins"),
        (("insert", r#"{"name":"A","file":"/etc/hosts","at":[0,0]}"#), "can't name files"),
        (("linetype", r#"{"load":"*","file":"/etc/hosts"}"#), "can't name files"),
    ] {
        let mut s = with_plugin("io.example.sneaky", "write", module(&[call], false, ""));
        let (count, steps) = (objects(&s), undo_labels(&s).len());
        let r = s.execute("plugin.run", &json!({"id": "io.example.sneaky"})).unwrap();
        let message = r["output"].as_str().unwrap_or_default();
        assert!(message.contains(why), "{}: {message}", call.0);
        assert_eq!((objects(&s), undo_labels(&s).len()), (count, steps), "{}", call.0);
    }
}

#[test]
fn a_path_array_may_name_its_path_object() {
    let m = craft_plugin_manifest("write");
    assert!(super::refusal(&m, "arraypath", &json!({"path": "2A", "count": 3})).is_none());
    assert!(super::refusal(&m, "arraypath", &json!({"path": {"file": "/etc/hosts"}})).is_some());
    assert!(super::refusal(&m, "line", &json!({"path": "2A"})).is_some());
}

fn craft_plugin_manifest(access: &str) -> crate::plugin::craft_plugin::Manifest {
    let v = json!({"id": "io.example.m", "name": "M", "version": "1.0.0", "main": "p.wasm", "apps": {"cadcraft": {"api": 6}}, "permissions": {"access": access}});
    crate::plugin::craft_plugin::Manifest::from_value(&v).unwrap()
}

#[test]
fn a_command_outside_the_manifest_allowlist_is_refused() {
    let mut s = Session::new();
    let manifest = json!({"id": "io.example.narrow", "name": "Narrow", "version": "1.0.0", "main": "p.wasm",
        "apps": {"cadcraft": {"api": 6}}, "permissions": {"access": "write", "commands": ["circle*"]}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[LINE], false, ""), assets: vec![] }.encode();
    s.execute("plugin.install", &json!({"data": base64(&bytes)})).unwrap();
    let count = objects(&s);

    let r = s.execute("plugin.run", &json!({"id": "io.example.narrow"})).unwrap();

    assert!(r["output"].as_str().unwrap().contains("asked permission for"), "{r}");
    assert_eq!(objects(&s), count);
}

#[test]
fn a_trapping_or_endless_plugin_leaves_the_drawing_unchanged() {
    for (tail, fuel) in [("unreachable", None), ("(loop $l (br $l))", Some(200_000))] {
        let mut s = with_plugin("io.example.crash", "write", module(&[LINE, CIRCLE, ("ortho", r#"{"on":true}"#)], true, tail));
        s.execute("line", &json!({"points": [[0, 5], [5, 5]]})).unwrap();
        s.execute("selectall", &json!({})).unwrap();
        assert!(!s.selection().is_empty());
        let (count, steps, ortho) = (objects(&s), undo_labels(&s), s.settings.orthomode);
        let (selection, dirty) = (s.selection(), s.state().unwrap().is_dirty());
        let mut params = json!({"id": "io.example.crash"});
        if let Some(f) = fuel {
            params["fuel"] = json!(f);
        }

        let e = s.execute("plugin.run", &params).unwrap_err().to_string();

        assert!(e.contains("Test Plug-in"), "{e}");
        assert_eq!(objects(&s), count, "{tail}: the edits before the failure are rolled back");
        assert_eq!(undo_labels(&s), steps);
        assert_eq!(s.selection(), selection, "the selection is put back");
        assert_eq!(s.state().unwrap().is_dirty(), dirty);
        assert_eq!(s.settings.orthomode, ortho, "the drafting settings are put back");
    }
}

#[test]
fn a_plugin_without_a_drawing_or_inside_lisp_does_not_run() {
    let mut s = with_plugin("io.example.draw", "write", module(&[LINE], true, ""));
    assert!(s.execute("plugin.run", &json!({"id": "io.example.nothing"})).unwrap_err().to_string().contains("no plug-in"));
    assert!(s.execute("plugin.run", &json!({"id": "io.example.draw", "command": "other"})).is_err());
    let mut empty = Session::empty();
    empty.plugins = std::mem::take(&mut s.plugins);
    assert!(empty.execute("plugin.run", &json!({"id": "io.example.draw"})).is_err(), "no drawing open");
}

#[test]
fn settings_are_bounded_and_kept_per_plugin() {
    let mut all = Default::default();
    super::set_setting(&mut all, "a", "k", Some("{\"x\": 1}")).unwrap();
    assert_eq!(all["a"]["k"], json!({"x": 1}));
    assert!(super::set_setting(&mut all, "a", &"k".repeat(129), Some("1")).is_err());
    assert!(super::set_setting(&mut all, "a", "big", Some(&format!("\"{}\"", "x".repeat(64 * 1024)))).is_err());
    assert!(super::set_setting(&mut all, "a", "bad", Some("not json")).is_err());
    super::set_setting(&mut all, "a", "k", None).unwrap();
    assert!(!all.contains_key("a"));
}

#[test]
fn install_review_list_and_remove_through_the_store() {
    let dir = TempDir::new("plugins-store");
    let mut s = Session::new();
    s.plugins.open_store(&dir);
    let bytes = bundle("io.example.stored", "write", module(&[], true, ""));
    let data = base64(&bytes);

    // The confirmation: described, nothing written
    let review = s.execute("plugin.install", &json!({"data": data, "review": true})).unwrap();
    assert_eq!(review["installed"], false);
    assert_eq!(review["permissions"]["access"], "write");
    assert_eq!(review["kind"], "command");
    assert_eq!(review["sha256"], crate::plugin::craft_plugin::sha256_hex(&bytes));
    assert!(s.plugins.list().is_empty());
    assert!(std::fs::read_dir(&*dir).unwrap().next().is_none(), "nothing is written before the install");

    // Another bundle than the one reviewed isn't installed
    let e = s.execute("plugin.install", &json!({"data": data, "sha256": "0".repeat(64)})).unwrap_err();
    assert!(e.to_string().contains("reviewed"), "{e}");
    s.execute("plugin.install", &json!({"data": data, "review": true})).unwrap();
    let r = s.execute("plugin.install", &json!({"data": data, "sha256": review["sha256"]})).unwrap();
    assert_eq!((r["installed"].clone(), r["stored"].clone()), (json!(true), json!(true)));

    // A new session reads it back from the folder
    let mut again = Session::new();
    again.plugins.open_store(&dir);
    let list = again.execute("plugin.list", &json!({})).unwrap();
    assert_eq!(list["plugins"][0]["id"], "io.example.stored");
    assert_eq!(list["plugins"][0]["source"]["type"], "file");
    assert_eq!(list["stored"], true);

    again.execute("plugin.remove", &json!({"id": "io.example.stored"})).unwrap();
    assert_eq!(again.execute("plugin.list", &json!({})).unwrap()["plugins"], json!([]));
    let mut third = Session::new();
    third.plugins.open_store(&dir);
    assert!(third.plugins.list().is_empty(), "removed from the folder");
}

#[test]
fn plugins_for_other_apps_or_not_bundles_are_refused() {
    let mut s = Session::new();
    let manifest = json!({"id": "io.example.other", "name": "Other", "version": "1.0.0", "main": "p.wasm", "apps": {"figcraft": {"api": 6}}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[], true, ""), assets: vec![] }.encode();
    let e = s.execute("plugin.install", &json!({"data": base64(&bytes)})).unwrap_err().to_string();
    assert!(e.contains("written for figcraft"), "{e}");
    let e = s.execute("plugin.install", &json!({"data": base64(b"not a bundle")})).unwrap_err().to_string();
    assert!(e.contains("isn't a Craft plugin bundle"), "{e}");
    assert!(s.execute("plugin.install", &json!({})).is_err());
}

#[test]
fn a_local_catalog_installs_its_package() {
    let dir = TempDir::new("plugins-catalog");
    let bytes = bundle("io.example.listed", "write", module(&[], true, ""));
    let file = dir.file("listed.craftplugin", &bytes);
    let url = |p: &str| format!("file://{}", p.replace('\\', "/"));
    let index = json!({"schema": 1, "packages": [{"id": "io.example.listed", "name": "Listed", "repo": "https://github.com/example/listed",
        "versions": [{"version": "1.0.0", "apps": {"cadcraft": {"api": 6}}, "url": url(&file),
            "sha256": crate::plugin::craft_plugin::sha256_hex(&bytes), "size": bytes.len()}]}]});
    let catalog = dir.file("index.json", index.to_string());
    let mut s = Session::new();

    s.execute("plugin.catalog.add", &json!({"url": url(&catalog)})).unwrap();
    let listed = s.execute("plugin.catalog.list", &json!({})).unwrap();
    let r = s.execute("plugin.install", &json!({"catalog": url(&catalog), "id": "io.example.listed"})).unwrap();

    assert_eq!(listed["packages"][0]["id"], "io.example.listed", "{listed}");
    assert_eq!(r["inCatalog"], true);
    assert_eq!(r["source"]["type"], "catalog");
    assert_eq!(s.execute("plugin.update", &json!({})).unwrap()["updates"], json!([]), "up to date");
    assert!(s.execute("plugin.catalog.add", &json!({"url": "https://example.com/index.json"})).is_err(), "GitHub's hosts only");
}

#[test]
fn a_repository_install_without_network_says_so() {
    let mut s = Session::new();
    let e = s.execute("plugin.install", &json!({"repo": "example/tools"})).unwrap_err().to_string();
    assert!(e.contains("no network access"), "{e}");
}
