//! `convert` to an image says how many model-space entities it can't draw, by type, instead of
//! leaving them out silently (#1208). The exit code stays 0.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A fresh scratch directory that is removed on drop, also when an assertion fails.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cc-cli-{tag}-{}", std::process::id()));
        // A directory left behind by an earlier run with the same process id must not leak in.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).expect("create scratch dir");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let scratch = Scratch::new("undrawn");
    let dir = scratch.0.clone();
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
}

/// An OLE2FRAME's frame outline is drawn, so an image export doesn't call it an undrawn entity (#2422); it
/// says once that the OLE content inside the frame is not drawn (review on #2587).
#[test]
fn an_ole_frame_is_noted_as_frame_only() {
    let scratch = Scratch::new("ole");
    let dir = scratch.0.clone();
    let input = dir.join("ole.dxf");
    let ole = "0\nSECTION\n2\nENTITIES\n0\nOLE2FRAME\n8\n0\n100\nAcDbOle2Frame\n10\n0\n20\n0\n30\n0\n11\n5\n21\n-3\n31\n0\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&input, ole).unwrap();
    let out = cli(&["convert", input.to_str().unwrap(), dir.join("out.png").to_str().unwrap()]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("entity not drawn") && !err.contains("entities not drawn"), "{err}");
    let notes: Vec<&str> = err.lines().filter(|l| l.contains("OLE")).collect();
    assert_eq!(notes, [format!("{}: 1 OLE object shown as the frame only (the OLE content is not drawn)", input.display())], "{err}");
}

/// An OLE2FRAME without usable corners (here group 11/21 missing) has no outline to draw, so it is
/// an undrawn entity, not "shown as the frame only" (review on #2587).
#[test]
fn an_ole_frame_without_corners_is_counted_as_not_drawn() {
    let scratch = Scratch::new("ole-nocorner");
    let dir = scratch.0.clone();
    let input = dir.join("ole.dxf");
    let ole = "0\nSECTION\n2\nENTITIES\n0\nOLE2FRAME\n8\n0\n100\nAcDbOle2Frame\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&input, ole).unwrap();
    let out = cli(&["convert", input.to_str().unwrap(), dir.join("out.png").to_str().unwrap()]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("{}: 1 entity not drawn (OLE2FRAME: 1)", input.display())), "{err}");
    assert!(!err.contains("frame only"), "{err}");
}
