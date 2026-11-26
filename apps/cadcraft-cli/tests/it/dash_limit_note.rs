//! `convert` to an image says how many dashed runs it drew continuous because of a dash limit
//! (#1069): a line whose DASHED pattern repeats past what one run may be laid out over. A line of
//! 50,001 periods, which an export keeps dashed, gets no note.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

/// A DXF of one line `periods` DASHED (0.5, -0.25) periods long.
fn dashed_line(periods: f64) -> String {
    format!(
        "0
SECTION
2
TABLES
0
TABLE
2
LTYPE
70
1
0
LTYPE
2
DASHED
70
0
3
Dashed
72
65
73
2
40
0.75
         49
0.5
74
0
49
-0.25
74
0
0
ENDTAB
0
ENDSEC
0
SECTION
2
ENTITIES
         0
LINE
8
0
6
DASHED
10
0
20
0
11
{}
21
0
0
ENDSEC
0
EOF
",
        periods * 0.75
    )
}

#[test]
fn convert_to_an_image_notes_dashed_runs_drawn_continuous_past_the_dash_limit() {
    let dir = std::env::temp_dir().join(format!("cc-cli-dash-limit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // (1,000,001 + 2) periods × 2 elements: past the two million one run may be laid out over.
    let long = dir.join("long.dxf");
    std::fs::write(&long, dashed_line(1_000_001.0)).unwrap();
    let control = dir.join("control.dxf");
    std::fs::write(&control, dashed_line(50_001.0)).unwrap();
    let notes = |out: &Output| {
        assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stderr).lines().filter(|l| l.contains("drawn continuous")).map(str::to_string).collect::<Vec<_>>()
    };
    for ext in ["png", "svg", "pdf"] {
        let image = dir.join(format!("long.{ext}"));
        let image = image.to_str().unwrap();
        let got = notes(&cli(&["convert", long.to_str().unwrap(), image]));
        assert!(matches!(got.as_slice(), [note] if note.starts_with(&format!("{image}: 1 dashed run "))), "{ext}: {got:?}");
        let got = notes(&cli(&["convert", control.to_str().unwrap(), dir.join(format!("control.{ext}")).to_str().unwrap()]));
        assert!(got.is_empty(), "{ext}: {got:?}");
    }
    // A DXF copy draws nothing, so there is nothing to note.
    assert!(notes(&cli(&["convert", long.to_str().unwrap(), dir.join("copy.dxf").to_str().unwrap()])).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The engine's commands that write an image say it too, on the command line and in the result's
/// `warnings`: EXPORT, PLOT (PDF, PNG, SVG, to a file or as data), QSAVE and SAVEAS to an image and
/// `document.bytes`.
#[test]
fn image_commands_warn_about_dashed_runs_drawn_continuous() {
    let dir = std::env::temp_dir().join(format!("cc-cli-dash-limit-cmds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = |name: &str| serde_json::to_string(dir.join(name).to_str().unwrap()).unwrap();
    let cmds = [
        format!("export {{\"path\": {}}}", out("export.png")),
        format!("plot {{\"path\": {}}}", out("plot.pdf")),
        format!("plot {{\"path\": {}, \"output\": \"png\", \"resolution\": 50}}", out("plot.png")),
        format!("plot {{\"path\": {}, \"output\": \"svg\"}}", out("plot.svg")),
        format!("saveas {{\"path\": {}}}", out("saved.svg")),
        format!("qsave {{\"path\": {}}}", out("saved.pdf")),
        "plot {\"output\": \"svg\"}".to_string(),
        "document.bytes {\"format\": \"pdf\"}".to_string(),
        format!("saveas {{\"path\": {}, \"version\": 2018}}", out("versioned.png")),
    ];
    for (periods, warned) in [(1_000_001.0, true), (50_001.0, false)] {
        let input = dir.join(format!("line-{periods}.dxf"));
        std::fs::write(&input, dashed_line(periods)).unwrap();
        let mut args = vec!["run".to_string(), input.to_str().unwrap().to_string()];
        for c in &cmds {
            args.extend(["--cmd".to_string(), c.clone()]);
        }
        let run = Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(&args).output().unwrap();
        assert!(run.status.success(), "{:?}: {}", run.status, String::from_utf8_lossy(&run.stderr));
        let results: Vec<serde_json::Value> = String::from_utf8_lossy(&run.stdout).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(results.len(), cmds.len(), "{results:?}");
        for (c, r) in cmds.iter().zip(&results) {
            let notes: Vec<&str> = r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
            let noted = matches!(notes.as_slice(), [n] if n.starts_with("1 dashed run ") && n.contains("drawn continuous"));
            assert_eq!(noted, warned, "{periods} periods, {c}: {r}");
            assert_eq!(r.get("warnings").is_some(), warned, "{periods} periods, {c}: {r}");
        }
        // Said on the command line once for each command.
        let echoed = String::from_utf8_lossy(&run.stderr).lines().filter(|l| l.starts_with("1 dashed run ")).count();
        assert_eq!(echoed, if warned { cmds.len() } else { 0 }, "{}", String::from_utf8_lossy(&run.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
