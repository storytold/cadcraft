//! A damaged DWG opens partially (#859); `info` lists the reader's notes on what it left out
//! (`readNotes`) and `convert` warns with them before writing the copy, so a script or agent sees
//! the loss. The exit code stays 0.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A DWG of 200 lines, and a copy damaged so that the reader skips some of its records (found the
/// way the reader's own test finds one: flipping a few bytes at a time across the objects area).
fn dwg_and_damaged_copy() -> (Vec<u8>, Vec<u8>) {
    let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..200 {
        dxf += &format!("0\nLINE\n8\n0\n10\n{i}\n20\n0\n11\n{i}\n21\n10\n");
    }
    dxf += "0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read(dxf.as_bytes(), "lines.dxf").unwrap();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2000), binary: false };
    let dwg = cadcraft_io::write_with(&d, "lines.dwg", &opts).unwrap();
    let damaged = (dwg.len() / 4..dwg.len() * 3 / 5)
        .step_by(97)
        .map(|at| {
            let mut b = dwg.clone();
            b[at..at + 6].iter_mut().for_each(|x| *x ^= 0xA5);
            b
        })
        .find(|b| cadcraft_io::read(b, "damaged.dwg").is_ok_and(|d| !d.read_notes.is_empty()))
        .expect("no damaged copy read partially");
    (dwg, damaged)
}

#[test]
fn info_lists_and_convert_warns_about_read_notes() {
    let dir = std::env::temp_dir().join(format!("cc-cli-read-notes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (whole, damaged) = dwg_and_damaged_copy();
    let path = dir.join("damaged.dwg");
    std::fs::write(&path, &damaged).unwrap();
    let path = path.to_str().unwrap();

    let out = cli(&["info", path]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let notes = v["readNotes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{v}");
    assert!(notes[0].as_str().unwrap().contains(" couldn't be read and "), "{v}");

    let copy = dir.join("copy.dxf");
    let out = cli(&["convert", path, copy.to_str().unwrap()]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    let warnings: Vec<&str> = err.lines().filter(|l| l.contains(" couldn't be read and ")).collect();
    assert_eq!(warnings.len(), 1, "{err}");
    assert!(warnings[0].starts_with(&format!("{path}: DWG: ")), "{err}");
    assert!(copy.exists());

    // A complete read: no notes, and no warning.
    let intact = dir.join("whole.dwg");
    std::fs::write(&intact, &whole).unwrap();
    let out = cli(&["info", intact.to_str().unwrap()]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["readNotes"], serde_json::json!([]));
    let out = cli(&["convert", intact.to_str().unwrap(), copy.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("couldn't be read"), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
