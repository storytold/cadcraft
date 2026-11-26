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
