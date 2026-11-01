//! `convert` to an image says how many model-space entities it can't draw, by type, instead of
//! leaving them out silently (#1208). The exit code stays 0.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A line, two foreign REGIONs and a PDF underlay: the last three are kept as unknown entities.
const PROXIES: &str = "0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\n0\n10\n0\n20\n0\n11\n5\n21\n5\n\
0\nREGION\n8\n0\n70\n1\n1\nabc\n\
0\nREGION\n8\n0\n70\n1\n1\ndef\n\
0\nPDFUNDERLAY\n8\n0\n10\n0\n20\n0\n41\n1\n42\n1\n43\n1\n\
0\nENDSEC\n0\nEOF\n";

#[test]
fn convert_to_an_image_warns_about_entities_it_cannot_draw() {
    let dir = std::env::temp_dir().join(format!("cc-cli-undrawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("proxies.dxf");
    std::fs::write(&input, PROXIES).unwrap();
    let input = input.to_str().unwrap();

    let out = cli(&["info", input]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["counts"]["Proxy"], 3, "{v}");

    for ext in ["svg", "png", "pdf"] {
        let image = dir.join(format!("out.{ext}"));
        let out = cli(&["convert", input, image.to_str().unwrap()]);
        assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
        let err = String::from_utf8_lossy(&out.stderr);
        let warnings: Vec<&str> = err.lines().filter(|l| l.contains("not drawn")).collect();
        assert_eq!(warnings.len(), 1, "{ext}: {err}");
        assert_eq!(warnings[0], format!("{input}: 3 entities not drawn (PDFUNDERLAY: 1, REGION: 2)"), "{ext}");
    }

    // A DXF copy keeps them, so there is nothing to warn about.
    let copy = dir.join("copy.dxf");
    let out = cli(&["convert", input, copy.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("not drawn"), "{}", String::from_utf8_lossy(&out.stderr));

    // A drawing without them: no warning.
    let sample = dir.join("sample.dxf");
    assert!(cli(&["sample", "floorplan", sample.to_str().unwrap()]).status.success());
    let out = cli(&["convert", sample.to_str().unwrap(), dir.join("sample.svg").to_str().unwrap()]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("not drawn"), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}
