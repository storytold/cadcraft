//! Literal MTEXT carets and their adjacent whitespace survive DXF and DWG saves (#2250).

use cadcraft_doc::{Common, Drawing, EntityKind, Handle, MText, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version, read, write_with};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn drawing() -> Result<(Drawing, Vec<(Handle, String)>), Box<dyn std::error::Error>> {
    let mut d = Drawing::new_metric();
    let mut expected = Vec::new();
    let mut cases: Vec<String> = [
        "ordinary text",
        "^ C",
        "A ^ C B",
        "^  C",
        "A ^   C  B",
        "^ ",
        "^^ C ^ ^  D^",
        "^I ^J ^ C",
        "\t^ C\nD\tE",
        "^\t ^\n C",
        r"A\P^ C\P^I ^J",
        r"\S+0.1^-0.1; ^ C",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    // Exercise multiple group-3 chunks and a caret at the output chunk boundary.
    cases.push(format!("{}^ C", "A".repeat(124)));
    cases.push(format!("{}^ C B", "A".repeat(260)));
    for contents in cases {
        let kind = EntityKind::MText(MText {
            insert: Vec3::new(1.0, 2.0, 0.0),
            height: 0.5,
            width: 12.5,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: contents.clone(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        });
        let handle = d.add(&Space::Model, Common::default(), kind)?;
        expected.push((handle, contents));
    }
    Ok((d, expected))
}

fn assert_contents(d: &Drawing, expected: &[(Handle, String)], context: &str) -> TestResult {
    assert_eq!(d.model.len(), expected.len(), "{context}: entity count");
    for (handle, contents) in expected {
        let e = d.entity(*handle).ok_or("MTEXT handle lost")?;
        let EntityKind::MText(t) = &e.kind else { return Err("MTEXT entity type changed".into()) };
        assert_eq!(&t.contents, contents, "{context}: {handle:?}");
    }
    Ok(())
}

#[test]
fn dxf_mtext_caret_spaces_round_trip() -> TestResult {
    let (original, expected) = drawing()?;
    for version in Version::ALL {
        for binary in [false, true] {
            let mut d = original.clone();
            for pass in 1..=2 {
                let bytes = write_with(&d, "carets.dxf", &SaveOptions { version: Some(version), binary })?;
                d = read(&bytes, "carets.dxf")?;
                assert_contents(&d, &expected, &format!("{version:?}, binary={binary}, pass={pass}"))?;
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_mtext_caret_spaces_round_trip() -> TestResult {
    let (original, expected) = drawing()?;
    for version in Version::ALL {
        let mut d = original.clone();
        for pass in 1..=2 {
            let bytes = write_with(&d, "carets.dwg", &SaveOptions { version: Some(version), binary: false })?;
            assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some(version.code()));
            // Inspect the stored DWG text before its DXF rendition: a compensating reader
            // must not hide corruption on save. This uses the same codec, not an external oracle.
            let native = acadrust::DwgReader::from_stream(std::io::Cursor::new(&bytes)).read()?;
            for (handle, contents) in &expected {
                let text = native.entities().find_map(|e| match e {
                    acadrust::EntityType::MText(t) if t.common.handle.value() == handle.0 => Some(&t.value),
                    _ => None,
                });
                assert_eq!(text, Some(contents), "native DWG {version:?}, pass={pass}, {handle:?}");
            }
            let dxf = cadcraft_dwg::dwg_to_dxf(&bytes)?;
            let converted = read(&dxf, "bridge.dxf")?;
            assert_contents(&converted, &expected, &format!("DWG bridge DXF {version:?}, pass={pass}"))?;
            d = read(&bytes, "carets.dwg")?;
            assert_contents(&d, &expected, &format!("DWG {version:?}, pass={pass}"))?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bridge_caret_space_is_escaped_once_in_each_direction() -> TestResult {
    let (d, _) = drawing()?;
    let dxf = cadcraft_io::write_dxf(&d);
    assert!(dxf.contains("  1\r\n^  C\r\n"), "literal caret plus authored space needs two wire spaces");
    let native = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf.into_bytes()))?.read()?;
    assert!(native.entities().any(|e| matches!(e, acadrust::EntityType::MText(t) if t.value == "^ C")));
    let dwg = write_with(&d, "carets.dwg", &SaveOptions { version: Some(Version::R2018), binary: false })?;
    let returned = String::from_utf8(cadcraft_dwg::dwg_to_dxf(&dwg)?)?;
    assert!(returned.contains("  1\r\n^  C\r\n"), "the return bridge must encode the separator independently of authored whitespace");
    Ok(())
}
