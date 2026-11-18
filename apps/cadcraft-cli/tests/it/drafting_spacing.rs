//! Drawing snap/grid spacing survives saving and opening in a fresh CLI process (#607).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct DxfFile(PathBuf);

impl DxfFile {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("cadcraft-drafting-spacing-{}-{}.dxf", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
        std::fs::File::options().write(true).create_new(true).open(&path).expect("reserve test DXF");
        Self(path)
    }
}

impl Drop for DxfFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn cli(args: &[&str]) -> Vec<Value> {
    let out = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().expect("run cadcraft-cli");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().map(|line| serde_json::from_str(line).expect("CLI JSON reply")).collect()
}

#[test]
fn quarter_unit_spacing_survives_save_and_reopen() {
    let file = DxfFile::new();
    let path = file.0.to_str().expect("test file path");
    cli(&[
        "run",
        "--sample",
        "--cmd",
        r#"setvar {"name":"SNAPUNIT","value":[0.25,0.25]}"#,
        "--cmd",
        r#"setvar {"name":"GRIDUNIT","value":[0.25,0.25]}"#,
        "--save",
        path,
    ]);
    let replies = cli(&["run", path, "--cmd", r#"getvar {"name":"SNAPUNIT"}"#, "--cmd", r#"getvar {"name":"GRIDUNIT"}"#]);
    for name in ["SNAPUNIT", "GRIDUNIT"] {
        let reply = replies.iter().find(|r| r["name"] == name).expect("getvar reply");
        assert_eq!(reply["value"], json!([0.25, 0.25]), "{name} reset on reopen");
    }
}

#[test]
fn drafting_settings_spacing_roundtrips_without_persisting_osmode() {
    let file = DxfFile::new();
    let path = file.0.to_str().expect("test file path");
    cli(&[
        "run",
        "--sample",
        "--cmd",
        r#"dsettings {"snapunit":[0.25,0.125],"gridunit":[0.75,1.5]}"#,
        "--cmd",
        r#"setvar {"name":"OSMODE","value":0}"#,
        "--save",
        path,
    ]);
    let replies = cli(&[
        "run",
        path,
        "--cmd",
        r#"getvar {"name":"SNAPUNIT"}"#,
        "--cmd",
        r#"getvar {"name":"GRIDUNIT"}"#,
        "--cmd",
        r#"getvar {"name":"OSMODE"}"#,
    ]);
    for (name, expected) in [("SNAPUNIT", json!([0.25, 0.125])), ("GRIDUNIT", json!([0.75, 1.5])), ("OSMODE", json!(4133))] {
        let reply = replies.iter().find(|r| r["name"] == name).expect("getvar reply");
        assert_eq!(reply["value"], expected, "{name} after reopening");
    }
}
