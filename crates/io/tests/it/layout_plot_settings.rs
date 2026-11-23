//! Original synthetic plot subclasses covering the Autodesk DXF Reference's PLOTSETTINGS fields.

use cadcraft_doc::{Drawing, NamedView};
use cadcraft_io::{read, write, write_dxf};
use std::collections::BTreeMap;

fn normalized(text: &str) -> String {
    let lines = text.lines().enumerate().map(|(index, line)| if index % 2 == 0 { line.trim() } else { line }).collect::<Vec<_>>();
    lines.join("\n") + "\n"
}

fn settings(seed: usize, plot_type: i64, flags: i64) -> String {
    format!(
        "1\nSetup {seed}\n2\nDraft-{seed}.pc3\n4\nSheet_{seed}_(11.00_x_17.00_Inches)\n6\nPlot view {seed}\n\
         40\n2.1\n41\n3.2\n42\n4.3\n43\n5.4\n44\n279.4\n45\n431.8\n46\n7.5\n47\n8.5\n\
         48\n-10\n49\n-20\n140\n300\n141\n400\n142\n1\n143\n3.56\n70\n{flags}\n72\n0\n\
         73\n2\n74\n{plot_type}\n7\nDraft-{seed}.ctb\n75\n0\n76\n2\n77\n5\n78\n450\n\
         147\n0.37\n148\n1.25\n149\n2.5\n333\nF00\n"
    )
}

fn source(plot_type: i64, flags: i64) -> String {
    let mut drawing = Drawing::new_imperial();
    for index in 1..=3 {
        drawing.views.push(NamedView {
            name: format!("Plot view {index}"),
            height: 100.0,
            width: 150.0,
            layout: (index > 1).then(|| format!("Layout{}", index - 1)),
            ..Default::default()
        });
    }
    let text = normalized(&write_dxf(&drawing)).replacen("AC1015", "AC1032", 1);
    let mut result = String::new();
    for (index, part) in text.split("100\nAcDbPlotSettings\n").enumerate() {
        if index == 0 {
            result.push_str(part);
        } else {
            result.push_str("100\nAcDbPlotSettings\n");
            result.push_str(&settings(index, plot_type, flags));
            let (_, tail) = part.split_once("100\nAcDbLayout\n").unwrap();
            result.push_str("100\nAcDbLayout\n");
            result.push_str(tail);
        }
    }
    result
        .replacen("2\nCLASSES\n", "2\nCLASSES\n0\nCLASS\n1\nRENDERSETTINGS\n2\nAcDbRenderSettings\n3\nSCENEOE\n90\n1024\n91\n1\n280\n0\n281\n0\n", 1)
        .replacen(
            "2\nOBJECTS\n",
            "2\nOBJECTS\n0\nRENDERSETTINGS\n5\nF00\n330\n0\n100\nAcDbRenderSettings\n90\n2\n1\nDraft render\n290\n1\n290\n1\n290\n1\n290\n1\n1\n\n1\n\n90\n3\n",
            1,
        )
}

fn plots(text: &str) -> BTreeMap<String, BTreeMap<i32, String>> {
    normalized(text)
        .split("100\nAcDbPlotSettings\n")
        .skip(1)
        .map(|part| {
            let (plot, layout) = part.split_once("100\nAcDbLayout\n").unwrap();
            let name = layout.lines().nth(1).unwrap().to_string();
            let lines: Vec<_> = plot.lines().collect();
            let tags = lines.as_chunks::<2>().0.iter().map(|pair| (pair[0].trim().parse().unwrap(), pair[1].to_string())).collect();
            (name, tags)
        })
        .collect()
}

fn assert_plot_eq(expected: &BTreeMap<i32, String>, actual: &BTreeMap<i32, String>, context: &str) {
    let mut differences = Vec::new();
    for (code, value) in expected {
        let Some(back) = actual.get(code) else {
            differences.push(format!("missing group {code}"));
            continue;
        };
        let equal = if (40..=49).contains(code) || (140..=149).contains(code) {
            (value.parse::<f64>().unwrap() - back.parse::<f64>().unwrap()).abs() < 1e-10
        } else if (70..=78).contains(code) {
            value.trim().parse::<i64>().unwrap() == back.trim().parse::<i64>().unwrap()
        } else {
            value == back.trim()
        };
        if !equal {
            differences.push(format!("group {code}: {value:?} != {back:?}"));
        }
    }
    assert!(differences.is_empty(), "{context}: {differences:?}");
}

#[test]
fn every_layout_keeps_all_plot_settings_in_dxf_and_dwg() {
    cadcraft_io::dwg_worker::allow_in_process();
    for plot_type in [0, 3, 4] {
        for flags in [11952, 11936] {
            let source = source(plot_type, flags);
            let expected = plots(&source);
            assert_eq!(expected.len(), 3);
            assert!(expected.contains_key("Model"));
            let drawing = read(source.as_bytes(), "source.dxf").unwrap();
            for name in ["saved.dxf", "saved.dwg"] {
                let bytes = write(&drawing, name).unwrap();
                let text = if name.ends_with("dwg") {
                    String::from_utf8(craft_dwg::dwg_to_dxf(&bytes).unwrap()).unwrap()
                } else {
                    String::from_utf8(bytes.clone()).unwrap()
                };
                let actual = plots(&text);
                assert_eq!(expected.keys().collect::<Vec<_>>(), actual.keys().collect::<Vec<_>>(), "{name}");
                for (layout, tags) in &expected {
                    assert_plot_eq(tags, &actual[layout], &format!("{name}: {layout}"));
                }
                let reopened = read(&bytes, name).unwrap();
                let again = plots(&write_dxf(&reopened));
                for (layout, tags) in &expected {
                    assert_plot_eq(tags, &again[layout], &format!("reopened {name}: {layout}"));
                }
            }
        }
    }
}

#[test]
fn changed_page_fields_override_only_their_saved_groups() {
    cadcraft_io::dwg_worker::allow_in_process();
    let source = source(3, 11952).replace("72\n0\n", "72\n2\n");
    let expected = plots(&source);
    let mut drawing = read(source.as_bytes(), "source.dxf").unwrap();
    let pages = drawing.model_page.iter_mut().chain(drawing.layouts.iter_mut().map(|layout| &mut layout.page));
    for page in pages {
        page.plot_area = "extents".into();
        page.scale = 0.5;
        page.scale_to_fit = false;
        page.plot_style_table = "Changed.ctb".into();
        page.paper_mm = Some(false);
    }
    for name in ["changed.dxf", "changed.dwg"] {
        let reopened = read(&write(&drawing, name).unwrap(), name).unwrap();
        let actual = plots(&write_dxf(&reopened));
        for (layout, original) in &expected {
            let mut tags = original.clone();
            for (code, value) in [(74, "1"), (142, "0.5"), (143, "1"), (147, "0.5"), (70, "11936"), (75, "16"), (7, "Changed.ctb"), (72, "0")] {
                tags.insert(code, value.into());
            }
            assert_plot_eq(&tags, &actual[layout], &format!("{name}: {layout}"));
        }
    }
}

#[test]
fn changing_only_paper_or_device_keeps_other_plot_settings() {
    cadcraft_io::dwg_worker::allow_in_process();
    let source = source(3, 11952);
    let expected = plots(&source);
    for (code, value) in [(4, "Changed_paper"), (2, "Changed-device.pc3")] {
        let mut drawing = read(source.as_bytes(), "source.dxf").unwrap();
        let pages = drawing.model_page.iter_mut().chain(drawing.layouts.iter_mut().map(|layout| &mut layout.page));
        for page in pages {
            let mut edited = page.clone();
            if code == 4 {
                edited.paper = value.into();
            } else {
                edited.device = value.into();
            }
            *page = edited;
        }
        for name in ["changed.dxf", "changed.dwg"] {
            let bytes = write(&drawing, name).unwrap();
            let text = if name.ends_with("dwg") {
                String::from_utf8(craft_dwg::dwg_to_dxf(&bytes).unwrap()).unwrap()
            } else {
                String::from_utf8(bytes.clone()).unwrap()
            };
            let saved = plots(&text);
            let reopened = read(&bytes, name).unwrap();
            let again = plots(&write_dxf(&reopened));
            for (layout, original) in &expected {
                let mut tags = original.clone();
                tags.insert(code, value.into());
                assert_plot_eq(&tags, &saved[layout], &format!("saved {name}: {layout}, changed group {code}"));
                assert_plot_eq(&tags, &again[layout], &format!("reopened {name}: {layout}, changed group {code}"));
            }
        }
    }
}

#[test]
fn inherited_paper_units_follow_measurement_when_saved_again() {
    for (initial, changed, expected_unit) in [(1, 0, 25.4), (0, 1, 1.0)] {
        let mut drawing = Drawing::new_imperial();
        drawing.header.set_i64("MEASUREMENT", initial);
        let original = write_dxf(&drawing);
        let original_plots = plots(&original);
        assert!(original_plots.values().all(|tags| tags[&72] == initial.to_string()));
        let mut loaded = read(original.as_bytes(), "original.dxf").unwrap();
        assert!(loaded.layouts.iter().all(|layout| layout.page.paper_mm.is_none()));
        loaded.header.set_i64("MEASUREMENT", changed);
        let saved = write_dxf(&loaded);
        assert!(plots(&saved).values().all(|tags| tags[&72] == changed.to_string()), "saved plot units must follow MEASUREMENT");
        let reopened = read(saved.as_bytes(), "saved.dxf").unwrap();
        for layout in &reopened.layouts {
            assert_eq!(layout.page.paper_mm, None);
            assert_eq!(cadcraft_render::paper::page_unit_mm(&reopened, &layout.page), expected_unit);
        }
    }
}
