//! The user's command alias file (#552): ALIASEDIT loads a `.pgp`-style file (`ALIAS, *COMMAND`)
//! whose aliases add to or override the built-in ones; REINIT reads it again. The aliases are
//! process-wide, so these tests run one at a time.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::MAX_ALIASES;
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

fn alias_prefix(count: usize) -> String {
    (0..count).map(|i| format!("ZZP{i:05},*LINE\n")).collect()
}

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
    let file = dir.file("mine.pgp", "ZZR, *LINE\nzzr, *RECTANG\n");
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let r = s.execute("aliasedit", &json!({ "file": file })).unwrap();
    assert_eq!(r["loaded"], json!(1));
    assert_eq!(r["file"], json!(file));
    assert_eq!(r["warnings"], json!([]));
    assert_eq!(resolve_alias("zzr"), "rectang");

    std::fs::write(&file, "ZZR, *MOVE\nzzr, *CIRCLE\n").unwrap();
    s.cmdline("reinit").unwrap();
    assert_eq!(resolve_alias("zzr"), "circle");
    assert!(s.log.iter().any(|l| l.contains("1 command alias")), "{:?}", s.log);
    let r = s.execute("aliasedit", &json!({})).unwrap();
    assert_eq!(r["file"], json!(file));
    assert_eq!(r["aliases"], json!({ "zzr": "circle" }));
    let r = s.execute("reinit", &json!({ "text": "ZZS, *LINE\nzzs, *SCALE\n" })).unwrap();
    assert_eq!(r["aliases"], json!({ "zzs": "scale" }));
    assert_eq!(r["loaded"], json!(1));
    assert_eq!(r["warnings"], json!([]));
    assert_eq!(r["file"], json!(file), "reloading text keeps the remembered file");
    let r = s.execute("reinit", &json!({})).unwrap();
    assert_eq!(r["aliases"], json!({ "zzr": "circle" }));
    assert_eq!(r["loaded"], json!(1));
    assert_eq!(r["file"], json!(file));
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}

#[test]
fn valid_redefinitions_win_and_the_listing_stays_sorted() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let text = "ZZZ,*LINE\nZza,*LINE\nzzz,*CIRCLE\nZZA,*NO_SUCH_1933\nLINE,*CIRCLE\n";
    let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
    let aliases = json!({ "zza": "line", "zzz": "circle" });
    assert_eq!(r["aliases"], aliases);
    assert_eq!(r["loaded"], json!(2), "loaded counts unique aliases, not definitions");
    assert_eq!(r["file"], json!(null));
    assert_eq!(
        r["warnings"],
        json!(["line 4: ZZA: unknown command *NO_SUCH_1933; skipped", "line 5: LINE is a command name; it stays the command; skipped"])
    );
    assert_eq!(resolve_alias("ZZZ"), "circle");
    assert_eq!(resolve_alias("zza"), "line", "an invalid redefinition keeps the previous target");
    assert_eq!(resolve_alias("line"), "line");
    assert_eq!(serde_json::to_string(&r["aliases"]).unwrap(), r#"{"zza":"line","zzz":"circle"}"#);
    let listing = s.execute("aliasedit", &json!({})).unwrap();
    assert_eq!(listing, json!({ "aliases": aliases, "file": null, "loaded": 0, "warnings": [] }));

    let empty = s.execute("aliasedit", &json!({ "text": "" })).unwrap();
    assert_eq!(empty, json!({ "aliases": {}, "file": null, "loaded": 0, "warnings": [] }));
    assert_eq!(resolve_alias("zza"), "zza", "an empty import replaces the old set");
    assert_eq!(resolve_alias("zzz"), "zzz");
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}

#[test]
fn duplicates_below_the_limit_leave_room_for_another_alias() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let mut text = alias_prefix(MAX_ALIASES - 1);
    for _ in 0..16 {
        text.push_str("ZZP00000,*MOVE\nzzp00001,*CIRCLE\n");
    }
    let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
    assert_eq!(r["loaded"], json!(MAX_ALIASES - 1));
    assert_eq!(r["aliases"].as_object().unwrap().len(), MAX_ALIASES - 1);
    assert_eq!(r["warnings"], json!([]));
    for i in 0..MAX_ALIASES - 1 {
        let expected = match i {
            0 => "move",
            1 => "circle",
            _ => "line",
        };
        assert_eq!(r["aliases"][format!("zzp{i:05}")], json!(expected));
    }

    text.push_str("ZZNEW,*PLINE\n");
    let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
    assert_eq!(r["loaded"], json!(MAX_ALIASES));
    assert_eq!(r["aliases"].as_object().unwrap().len(), MAX_ALIASES);
    assert_eq!(r["aliases"]["zznew"], json!("pline"));
    assert_eq!(r["aliases"]["zzp00000"], json!("move"));
    assert_eq!(r["aliases"]["zzp00001"], json!("circle"));
    assert_eq!(r["warnings"], json!([]), "duplicate definitions do not use up the alias limit");
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}

#[test]
fn a_full_file_validates_lines_before_stopping_at_the_next_valid_definition() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let mut text = alias_prefix(MAX_ALIASES);
    text.push_str("; comment\nZZP00000,*NO_SUCH_1933\nLINE,*CIRCLE\nZZEXTERNAL, START NOTHING\nmalformed\nZZP00000,*MOVE\nZZAFTER,*CIRCLE\nanother malformed sentinel\n");
    let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
    assert_eq!(r["loaded"], json!(MAX_ALIASES));
    assert_eq!(r["aliases"].as_object().unwrap().len(), MAX_ALIASES);
    assert_eq!(r["aliases"]["zzp00000"], json!("line"), "the cap stops even a valid duplicate replacement");
    assert!(r["aliases"].get("zzafter").is_none(), "the remainder of the file is skipped");
    assert_eq!(
        r["warnings"],
        json!([
            format!("line {}: ZZP00000: unknown command *NO_SUCH_1933; skipped", MAX_ALIASES + 2),
            format!("line {}: LINE is a command name; it stays the command; skipped", MAX_ALIASES + 3),
            format!("line {}: ZZEXTERNAL is an external command; CADCraft doesn't run programs from alias files", MAX_ALIASES + 4),
            format!("line {}: `malformed` is not ALIAS, *COMMAND; skipped", MAX_ALIASES + 5),
            format!("line {}: more than {MAX_ALIASES} aliases; the rest are skipped", MAX_ALIASES + 6)
        ])
    );

    // Setting one existing alias through JSON has a different rule from importing a full file.
    let r = s.execute("aliasedit", &json!({ "set": { "ZZP00000": "move" } })).unwrap();
    assert_eq!(r["aliases"]["zzp00000"], json!("move"));
    assert_eq!(r["loaded"], json!(1));
    assert!(s.execute("aliasedit", &json!({ "set": { "ZZNEW": "line" } })).is_err());
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
}

#[test]
fn the_warning_limit_does_not_stop_a_later_valid_redefinition() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = Session::new();
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
    let mut text = String::from("ZZW,*LINE\n");
    for _ in 0..101 {
        text.push_str("ZZW,*NO_SUCH_1933\n");
    }
    text.push_str("zzw,*CIRCLE\n");
    let r = s.execute("aliasedit", &json!({ "text": text })).unwrap();
    assert_eq!(r["aliases"], json!({ "zzw": "circle" }));
    assert_eq!(r["loaded"], json!(1));
    let warnings: Vec<_> = (2..=101).map(|no| format!("line {no}: ZZW: unknown command *NO_SUCH_1933; skipped")).collect();
    assert_eq!(r["warnings"], json!(warnings), "only the first 100 warnings are returned");
    s.execute("aliasedit", &json!({ "clear": true })).unwrap();
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
