//! Independent standard LAYER/STYLE flag contracts. Literal wire-to-field mappings cover the
//! small modeled flag domains, including off-vs-frozen and vertical-vs-mirrored distinctions.
//! Reader fixtures are handwritten records; writer checks inspect tags, not our Drawing reader.
//! Annotative STYLE xdata is covered separately by the round-trip tests, not this native-bit test.
//! References: Autodesk DXF Reference, LAYER and STYLE symbol table entries.

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Layer, TextStyle};
use cadcraft_dxf::{Tag, Value, records, sections};
use cadcraft_io::{SaveOptions, Version, read, write_with};

fn layers() -> Vec<(Layer, i64)> {
    let mut cases = Vec::new();
    // (group 70, frozen, frozen in new viewports, locked). Off is the sign of 62,
    // not another flag; plot is group 290 and must not accidentally change any of these.
    for (flags, frozen, vp_freeze_new, locked) in [
        (0, false, false, false),
        (1, true, false, false),
        (2, false, true, false),
        (3, true, true, false),
        (4, false, false, true),
        (5, true, false, true),
        (6, false, true, true),
        (7, true, true, true),
    ] {
        for on in [false, true] {
            for plot in [false, true] {
                cases.push((
                    Layer { name: "Flags".into(), color: Color::Index(3), frozen, vp_freeze_new, locked, on, plot, ..Layer::default() },
                    flags,
                ));
            }
        }
    }
    cases
}

fn styles() -> Vec<(TextStyle, i64, i64)> {
    let mut cases = Vec::new();
    for (generation, backwards, upside_down) in [(0, false, false), (2, true, false), (4, false, true), (6, true, true)] {
        for (flags, vertical) in [(0, false), (4, true)] {
            cases.push((
                TextStyle { name: "Flags".into(), font: "Synthetic.ttf".into(), backwards, upside_down, vertical, ..TextStyle::default() },
                flags,
                generation,
            ));
        }
    }
    cases
}

fn fixture(table: &str, body: &str) -> Drawing {
    let bytes = format!("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\n{table}\n70\n1\n0\n{table}\n2\nFlags\n{body}0\nENDTAB\n0\nENDSEC\n0\nEOF\n");
    let drawing = read(bytes.as_bytes(), "fixture.dxf").unwrap();
    assert!(drawing.read_notes.is_empty(), "{:?}", drawing.read_notes);
    assert!(drawing.skipped_records.is_empty());
    drawing
}

fn output(drawing: &Drawing, table: &str, version: Version, binary: bool) -> Vec<Tag> {
    let bytes = write_with(drawing, "memory.dxf", &SaveOptions { version: Some(version), binary }).unwrap();
    let tags = cadcraft_dxf::parse(&bytes).unwrap();
    let sections = sections(&tags);
    let tables = records(&sections.iter().find(|s| s.name == "TABLES").unwrap().tags);
    let matching: Vec<_> = tables.into_iter().filter(|(kind, groups)| kind == table && groups.contains(&Tag::s(2, "Flags"))).collect();
    assert_eq!(matching.len(), 1, "{version} binary={binary} {table}");
    matching.into_iter().next().unwrap().1
}

fn group(tags: &[Tag], expected: Tag, context: &str) {
    assert_eq!(tags.iter().filter(|t| t.code == expected.code).collect::<Vec<_>>(), [&expected], "{context}");
}

#[test]
fn reads_all_layer_flag_combinations() {
    for (expected, flags) in layers() {
        let aci = if expected.on { 3 } else { -3 };
        let plot = i64::from(expected.plot);
        let drawing = fixture("LAYER", &format!("70\n{flags}\n62\n{aci}\n6\nContinuous\n290\n{plot}\n"));
        assert_eq!(drawing.layer("Flags"), Some(&expected), "70={flags} 62={aci} 290={plot}");
    }
}

#[test]
fn writes_all_layer_flag_combinations() {
    for (layer, flags) in layers() {
        let mut drawing = Drawing::new_metric();
        drawing.layers.push(layer.clone());
        for version in Version::ALL {
            for binary in [false, true] {
                let tags = output(&drawing, "LAYER", version, binary);
                let context = format!("{version} binary={binary} {layer:?}");
                group(&tags, Tag::i(70, flags), &context);
                group(&tags, Tag::i(62, if layer.on { 3 } else { -3 }), &context);
                if layer.plot {
                    // This writer omits the default true; the fixture above tests explicit true.
                    assert!(!tags.iter().any(|t| t.code == 290), "{context}");
                } else {
                    group(&tags, Tag::new(290, Value::Bool(false)), &context);
                }
            }
        }
    }
}

#[test]
fn reads_all_native_text_style_flags() {
    for (expected, flags, generation) in styles() {
        let drawing = fixture("STYLE", &format!("70\n{flags}\n71\n{generation}\n40\n0\n41\n1\n50\n0\n3\nSynthetic.ttf\n4\n\n"));
        assert_eq!(drawing.text_style("Flags"), Some(&expected), "70={flags} 71={generation}");
    }
}

#[test]
fn writes_all_native_text_style_flags() {
    for (style, flags, generation) in styles() {
        let mut drawing = Drawing::new_metric();
        drawing.text_styles.push(style.clone());
        for version in Version::ALL {
            for binary in [false, true] {
                let tags = output(&drawing, "STYLE", version, binary);
                let context = format!("{version} binary={binary} {style:?}");
                group(&tags, Tag::i(70, flags), &context);
                group(&tags, Tag::i(71, generation), &context);
            }
        }
    }
}
