//! ALIASEDIT aliases are user settings (#1899): they are saved in the user profile the desktop
//! and web hosts keep across restarts (`profile_json` → `load_profile`), never in the drawing.
//! The alias store is process-wide, so these tests run one at a time and use their own names.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::MAX_ALIASES;
use cadcraft_engine::cmd::resolve_alias;
use cadcraft_ui_egui::options::MAX_PROFILE_BYTES;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

fn profile(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or(Value::Null)
}

#[test]
fn aliases_survive_a_restart_and_command_names_still_win() {
    let _one = lock();
    let mut a = app();
    a.run("aliasedit", json!({ "set": { "zzpersist": "qsave" } })).unwrap();
    let saved = a.profile_json();
    assert!(profile(&saved)["aliases"]["aliases"].get("zzpersist").is_some(), "the profile holds the alias: {saved}");

    // A fresh app restores the profile: the alias works, built-ins and command names unchanged.
    let mut b = app();
    b.load_profile(&saved);
    assert_eq!(resolve_alias("zzpersist"), "qsave");
    assert_eq!(resolve_alias("ZZPERSIST"), "qsave");
    assert_eq!(resolve_alias("c"), "circle", "built-in aliases still resolve");
    assert_eq!(resolve_alias("line"), "line", "command names still win");
    assert_eq!(resolve_alias("zzunknown"), "zzunknown");

    // A hand-edited profile can't make an alias take over a command name.
    let mut edited = profile(&saved);
    edited["aliases"]["aliases"]["line"] = json!("circle");
    let mut c = app();
    c.load_profile(&edited.to_string());
    assert_eq!(resolve_alias("line"), "line", "a restored alias never overrides a command name");
    a.run("aliasedit", json!({ "clear": true })).unwrap();
}

#[test]
fn removed_and_cleared_aliases_stay_removed() {
    let _one = lock();
    let mut a = app();
    a.run("aliasedit", json!({ "set": { "zzgone": "line", "zzkeep": "circle" } })).unwrap();
    assert_eq!(resolve_alias("zzgone"), "line");

    a.run("aliasedit", json!({ "remove": ["zzgone"] })).unwrap();
    let saved = a.profile_json();
    let mut b = app();
    b.load_profile(&saved);
    assert_eq!(resolve_alias("zzgone"), "zzgone", "a removed alias stays removed");
    assert_eq!(resolve_alias("zzkeep"), "circle", "the other aliases still restore");

    a.run("aliasedit", json!({ "clear": true })).unwrap();
    let cleared = a.profile_json();
    let mut c = app();
    c.load_profile(&cleared);
    assert_eq!(resolve_alias("zzkeep"), "zzkeep", "a cleared alias stays cleared");
    assert_eq!(profile(&cleared)["aliases"]["aliases"], json!({}), "the profile holds no user aliases");
}

#[test]
fn malformed_or_oversized_saved_aliases_are_skipped_safely() {
    let _one = lock();
    let mut a = app();
    a.run("aliasedit", json!({ "set": { "zzsafe": "line" } })).unwrap();
    let saved = profile(&a.profile_json());
    assert_eq!(resolve_alias("zzsafe"), "line");

    let mut huge = serde_json::Map::new();
    for i in 0..MAX_ALIASES.saturating_add(2_000) {
        huge.insert(format!("zz{i}"), json!("line"));
    }
    let mut huge_saved = saved.clone();
    huge_saved["aliases"] = json!({ "aliases": huge, "file": "mine.pgp" });
    let mut bad_values = saved.clone();
    bad_values["aliases"] = json!({ "aliases": { "zzbad": 5, "zznames": { "x": 1 }, "zzreal": "line" } });
    let mut not_an_object = saved.clone();
    not_an_object["aliases"] = json!("zzsafe, *LINE");
    let oversized = format!(r#"{{"aliases": {}, "pad": "{}"}}"#, saved["aliases"], "x".repeat(MAX_PROFILE_BYTES));

    let mut b = app();
    b.load_profile(&huge_saved.to_string()); // must not panic
    let listing = b.run("aliasedit", json!({})).unwrap();
    let count = listing["aliases"].as_object().map_or(0, serde_json::Map::len);
    assert!(count <= MAX_ALIASES, "restored {count} aliases");
    for bad in [bad_values.to_string(), not_an_object.to_string(), oversized] {
        b.load_profile(&bad); // must not panic
    }
    // Entries that aren't aliases of real commands are skipped.
    assert_eq!(resolve_alias("zzbad"), "zzbad", "a non-string command is skipped");
    assert_eq!(resolve_alias("zzreal"), "line", "valid entries beside bad ones still restore");
    a.run("aliasedit", json!({ "clear": true })).unwrap();
}

#[test]
fn a_full_alias_set_saved_by_the_profile_restores_after_a_restart() {
    let _one = lock();
    // ALIASEDIT holds up to MAX_ALIASES aliases. Long names make the saved profile far larger
    // than a small one, and the loader must still accept everything `profile_json` writes.
    let mut a = app();
    let mut set = serde_json::Map::new();
    let mut names = Vec::new();
    for i in 0..MAX_ALIASES {
        let name = format!("zz{i:05}alias{}", "x".repeat(40));
        names.push(name.clone());
        set.insert(name, json!("line"));
    }
    a.run("aliasedit", json!({ "set": set })).unwrap();
    let saved = a.profile_json();
    assert!(saved.len() > 64 * 1024, "the saved profile crosses 64 KiB: {} bytes", saved.len());
    assert!(saved.len() <= MAX_PROFILE_BYTES, "the limit covers a full alias set: {} bytes", saved.len());

    let mut b = app();
    b.load_profile(&saved);
    let listing = b.run("aliasedit", json!({})).unwrap();
    let count = listing["aliases"].as_object().map_or(0, serde_json::Map::len);
    assert_eq!(count, MAX_ALIASES, "every saved alias restores");
    for name in [names.first(), names.last()].into_iter().flatten() {
        assert_eq!(resolve_alias(name), "line", "restored {name}");
    }
    a.run("aliasedit", json!({ "clear": true })).unwrap();
}

#[test]
fn a_profile_one_byte_over_the_limit_is_refused_safely() {
    let _one = lock();
    let defaults = app().run("options", json!({})).unwrap();
    // Still valid JSON that would change PICKBOX, so accepting it would show.
    let overhead = r#"{"sysvars":{"PICKBOX":9},"pad":""}"#.len();
    let over = format!(r#"{{"sysvars":{{"PICKBOX":9}},"pad":"{}"}}"#, "x".repeat(MAX_PROFILE_BYTES + 1 - overhead));
    assert_eq!(over.len(), MAX_PROFILE_BYTES + 1);
    let mut b = app();
    b.load_profile(&over); // must not panic
    assert_eq!(b.run("options", json!({})).unwrap(), defaults, "an oversized profile changes nothing");
}

#[test]
fn a_loaded_alias_file_survives_with_its_path_and_reinit_rereads_it() {
    let _one = lock();
    let dir = std::env::temp_dir().join(format!("cadcraft-alias-profile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mine.pgp").to_string_lossy().into_owned();
    std::fs::write(&file, "ZZPROFILE, *RECTANG\n").unwrap();

    let mut a = app();
    a.run("aliasedit", json!({ "file": file.clone() })).unwrap();
    assert_eq!(resolve_alias("zzprofile"), "rectang");
    let saved = a.profile_json();
    assert_eq!(profile(&saved)["aliases"]["file"], json!(file), "the profile remembers the alias file");

    // A restart keeps both the aliases and the file REINIT reads.
    let mut b = app();
    b.load_profile(&saved);
    assert_eq!(resolve_alias("zzprofile"), "rectang");
    let listing = b.run("aliasedit", json!({})).unwrap();
    assert_eq!(listing["file"], json!(file));
    std::fs::write(&file, "ZZPROFILE, *CIRCLE\n").unwrap();
    b.run("reinit", json!({})).unwrap();
    assert_eq!(resolve_alias("zzprofile"), "circle", "REINIT re-reads the restored file");
    a.run("aliasedit", json!({ "clear": true })).unwrap();
}
