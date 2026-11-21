//! `info` reports the records a DXF file lost on reading, by section, and `convert` warns about
//! them before writing the copy (#865). The exit code stays 0.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A LINE after the last ENDSEC and a VERTEX with no POLYLINE before it.
const DAMAGED: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n0\nVERTEX\n8\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nLINE\n8\n0\n10\n9\n20\n9\n11\n8\n21\n8\n0\nEOF\n";

#[test]
fn info_counts_and_convert_warns_about_skipped_records() {
    let dir = std::env::temp_dir().join(format!("cc-cli-skipped-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let damaged = dir.join("damaged.dxf");
    std::fs::write(&damaged, DAMAGED).unwrap();
    let damaged = damaged.to_str().unwrap();

    let out = cli(&["info", damaged]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["skipped"], serde_json::json!({ "count": 2, "sections": { "(no section)": 1, "ENTITIES": 1 } }));

    let copy = dir.join("copy.dxf");
    let out = cli(&["convert", damaged, copy.to_str().unwrap()]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    let warnings: Vec<&str> = err.lines().filter(|l| l.contains("could not be read")).collect();
    assert_eq!(warnings.len(), 1, "{err}");
    assert!(warnings[0].contains("2 records") && warnings[0].contains("ENTITIES: 1") && warnings[0].contains("(no section): 1"), "{err}");

    // An intact file: a zero count, and no warning.
    let sample = dir.join("sample.dxf");
    assert!(cli(&["sample", "floorplan", sample.to_str().unwrap()]).status.success());
    let out = cli(&["info", sample.to_str().unwrap()]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["skipped"], serde_json::json!({ "count": 0, "sections": {} }));
    let out = cli(&["convert", sample.to_str().unwrap(), copy.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("could not be read"), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
