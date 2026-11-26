//! `convert` to DWG says, in one warning, how many linetypes the DWG keeps with only their first
//! 255 dash elements (#1923, F-05); converting to DXF keeps them whole and says nothing.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A DXF with a 256-element linetype and a line drawn with it.
fn dxf() -> String {
    let mut s = String::from("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n1\n0\nLTYPE\n2\nLONG\n70\n0\n3\nLong\n72\n65\n73\n256\n40\n192\n");
    for i in 0..256 {
        s.push_str(if i % 2 == 0 { "49\n1\n74\n0\n" } else { "49\n-0.5\n74\n0\n" });
    }
    s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n6\nLONG\n10\n0\n20\n0\n11\n10\n21\n0\n0\nENDSEC\n0\nEOF\n");
    s
}

#[test]
fn convert_to_dwg_notes_the_linetypes_it_cuts() {
    let dir = std::env::temp_dir().join(format!("cc-cli-ltype-dwg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("long.dxf");
    std::fs::write(&source, dxf()).unwrap();
    let source = source.to_str().unwrap();
    let notes =
        |out: &Output| String::from_utf8_lossy(&out.stderr).lines().filter(|l| l.contains("dash elements")).map(str::to_string).collect::<Vec<_>>();

    let out = cli(&["convert", source, dir.join("long.dwg").to_str().unwrap()]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    assert!(
        matches!(notes(&out).as_slice(), [note] if note.contains("1 linetype") && note.contains("255")),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = cli(&["convert", source, dir.join("copy.dxf").to_str().unwrap()]);
    assert!(out.status.success() && notes(&out).is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
