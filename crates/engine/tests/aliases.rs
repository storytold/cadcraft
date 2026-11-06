//! The user's command alias file (#552): ALIASEDIT loads a `.pgp`-style file (`ALIAS, *COMMAND`)
//! whose aliases add to or override the built-in ones; REINIT reads it again. The aliases are
//! process-wide, so these tests run one at a time.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::resolve_alias;
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const PGP: &str = "\
; my own aliases
ZZC,  *CIRCLE
L,    *PLINE
ZZL,  *_.LINE
ZZDL, *-LINETYPE
NOTEPAD, START NOTEPAD, 1,*File to edit: ,
ZZBAD, *NOSUCHCOMMAND
LINE, *CIRCLE
";

#[test]
fn an_alias_file_adds_and_overrides_aliases() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    assert_eq!(resolve_alias("l"), "line");
    let r = s.execute("aliasedit", &json!({ "text": PGP })).unwrap();
    assert_eq!(r["aliases"], json!({ "l": "pline", "zzc": "circle", "zzdl": "-linetype", "zzl": "line" }));
    let warnings = r["warnings"].to_string();
    for what in ["NOTEPAD", "NOSUCHCOMMAND", "LINE is a command name"] {
        assert!(warnings.contains(what), "no warning about {what}: {warnings}");
    }
    assert_eq!(resolve_alias("ZZC"), "circle");
    assert_eq!(resolve_alias("l"), "pline", "the file overrides a built-in alias");
    assert_eq!(resolve_alias("line"), "line", "command names win");
    assert_eq!(resolve_alias("c"), "circle", "built-in aliases the file doesn't mention stay");

    // Typed at the command line like any alias.
    s.cmdline("zzc 0,0 3").unwrap();
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);

    // Single aliases, then forgetting them all.
    s.execute("aliasedit", &json!({ "set": { "zzq": "qsave" }, "remove": ["zzl"] })).unwrap();
    assert_eq!(resolve_alias("zzq"), "qsave");
    assert_eq!(resolve_alias("zzl"), "zzl");
    assert!(s.execute("aliasedit", &json!({ "set": { "zzx": "nosuchcommand" } })).is_err());
    assert!(s.execute("aliasedit", &json!({ "set": { "two words": "line" } })).is_err());
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    assert_eq!(resolve_alias("l"), "line");
    assert_eq!(resolve_alias("zzc"), "zzc");
}

#[test]
fn reinit_reads_the_alias_file_again() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new("aliases");
    let file = dir.file("mine.pgp", "ZZR, *RECTANG\n");
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "file": file })).unwrap();
    assert_eq!(resolve_alias("zzr"), "rectang");

    std::fs::write(&file, "ZZR, *CIRCLE\n").unwrap();
    s.cmdline("reinit").unwrap();
    assert_eq!(resolve_alias("zzr"), "circle");
    assert!(s.log.iter().any(|l| l.contains("1 command alias")), "{:?}", s.log);
    let r = s.execute("reinit", &json!({ "text": "ZZS, *SCALE\n" })).unwrap();
    assert_eq!(r["aliases"], json!({ "zzs": "scale" }));
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}

#[test]
fn a_saved_alias_profile_restores_without_overriding_commands() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    s.execute("aliasedit", &json!({ "set": { "zzp": "qsave", "zzcmd": "line" } })).unwrap();
    let saved = cadcraft_engine::cmd::file::alias_profile();
    assert_eq!(saved["aliases"]["zzp"], json!("qsave"));

    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    assert_eq!(resolve_alias("zzp"), "zzp");
    cadcraft_engine::cmd::file::restore_aliases(&saved);
    assert_eq!(resolve_alias("zzp"), "qsave");
    assert_eq!(resolve_alias("zzcmd"), "line");
    assert_eq!(resolve_alias("line"), "line", "command names win");
    assert_eq!(resolve_alias("c"), "circle", "built-in aliases still resolve");

    // Hostile saved entries are skipped, and an oversized set is capped without panicking.
    let mut hostile = serde_json::Map::new();
    for i in 0..12_000 {
        hostile.insert(format!("ZZ{i}"), json!("line"));
    }
    hostile.insert("LINE".into(), json!("circle"));
    hostile.insert("ZZLONG".into(), json!("x".repeat(5000)));
    hostile.insert("ZZNUM".into(), json!(3));
    cadcraft_engine::cmd::file::restore_aliases(&json!({ "aliases": hostile, "file": "mine.pgp" }));
    assert_eq!(resolve_alias("line"), "line", "a restored alias never overrides a command name");
    assert_eq!(resolve_alias("zzlong"), "zzlong");
    assert_eq!(resolve_alias("zzn"), "zzn");
    let listing = s.execute("aliasedit", &json!({})).unwrap();
    let restored = listing["aliases"].as_object().map_or(0, serde_json::Map::len);
    assert!(restored <= cadcraft_engine::cmd::file::MAX_ALIASES, "the restored set is capped, not {restored}");
    assert!(restored > 9_000, "the valid entries restore, not just {restored}");
    assert_eq!(listing["file"], json!("mine.pgp"));

    // A malformed section is skipped without changing what ALIASEDIT holds.
    cadcraft_engine::cmd::file::restore_aliases(&json!({ "aliases": { "zzchange": "circle" }, "file": 7 }));
    assert_eq!(resolve_alias("zzchange"), "zzchange", "a malformed section restores nothing");
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    assert_eq!(resolve_alias("zz0"), "zz0", "clear still empties the restored set");
}

#[test]
fn hostile_alias_files_never_panic() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    let long = format!("{},*LINE\n", "A".repeat(5000));
    let many: String = (0..12_000).map(|i| format!("ZZ{i},*LINE\n")).collect();
    let huge = "x".repeat(9 << 20);
    for text in [long.as_str(), many.as_str(), ",,,,\n*\n;\n\u{0},*\n\u{feff}", ", *LINE\n'X, *LINE\n{X, *LINE\n"] {
        let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
        assert!(r["aliases"].as_object().unwrap().len() <= 10_000);
    }
    assert!(s.execute("aliasedit", &json!({ "text": huge })).is_err());
    for p in [json!({ "set": [1] }), json!({ "remove": "x" }), json!({ "file": 3 }), json!({ "set": { "zz": 3 } })] {
        assert!(s.execute("aliasedit", &p).is_err());
    }
    assert!(s.execute("reinit", &json!({ "file": "/no/such/file.pgp" })).is_err());
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}
