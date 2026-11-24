//! Original synthetic plot subclasses covering the Autodesk DXF Reference's PLOTSETTINGS fields.

use cadcraft_doc::{Drawing, NamedView};
use cadcraft_io::{SaveOptions, Version, read, write, write_dxf, write_with};
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
            "0\nENDSEC\n0\nEOF\n",
            "0\nRENDERSETTINGS\n5\nF00\n330\n0\n100\nAcDbRenderSettings\n90\n2\n1\nDraft render\n290\n1\n290\n1\n290\n1\n290\n1\n1\n\n1\n\n90\n3\n0\nENDSEC\n0\nEOF\n",
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

fn expected_plots(text: &str) -> BTreeMap<String, BTreeMap<i32, String>> {
    plots(text)
        .into_iter()
        .map(|(layout, mut tags)| {
            let flags = tags[&70].parse::<i64>().unwrap();
            tags.insert(70, (if layout == "Model" { flags | 1024 } else { flags & !1024 }).to_string());
            tags.remove(&333);
            (layout, tags)
        })
        .collect()
}

fn saved_plots(drawing: &Drawing) -> BTreeMap<String, BTreeMap<i32, String>> {
    plots(&String::from_utf8(write(drawing, "saved.dxf").unwrap()).unwrap())
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
            let expected = expected_plots(&source);
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
                let again = saved_plots(&reopened);
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
    let expected = expected_plots(&source);
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
        let actual = saved_plots(&reopened);
        for (layout, original) in &expected {
            let mut tags = original.clone();
            for (code, value) in [(74, "1"), (142, "0.5"), (143, "1"), (147, "0.5"), (70, "11936"), (75, "16"), (7, "Changed.ctb"), (72, "0")] {
                tags.insert(code, value.into());
            }
            if layout != "Model" {
                tags.insert(70, (11936 & !1024).to_string());
            }
            assert_plot_eq(&tags, &actual[layout], &format!("{name}: {layout}"));
        }
    }
}

#[test]
fn changing_only_paper_or_device_keeps_other_plot_settings() {
    cadcraft_io::dwg_worker::allow_in_process();
    let source = source(3, 11952);
    let expected = expected_plots(&source);
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
            let again = saved_plots(&reopened);
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
fn kept_model_type_is_corrected_with_and_without_page_edits() {
    cadcraft_io::dwg_worker::allow_in_process();
    let source = source(0, 688);
    let source = source.replacen(&settings(2, 0, 688), &settings(2, 0, 1712), 1);
    let drawing = read(source.as_bytes(), "source.dxf").unwrap();
    for edited in [false, true] {
        let mut drawing = drawing.clone();
        if edited {
            for page in drawing.model_page.iter_mut().chain(drawing.layouts.iter_mut().map(|layout| &mut layout.page)) {
                page.center = true;
            }
        }
        for name in ["saved.dxf", "saved.dwg"] {
            let bytes = write(&drawing, name).unwrap();
            let text = if name.ends_with("dwg") { craft_dwg::dwg_to_dxf(&bytes).unwrap() } else { bytes.clone() };
            let actual = plots(&String::from_utf8(text).unwrap());
            let centered = if edited { 4 } else { 0 };
            assert_eq!(actual["Model"][&70].trim().parse::<i64>().unwrap(), 1712 | centered, "{name}, edited {edited}");
            assert_eq!(actual["Layout1"][&70].trim().parse::<i64>().unwrap(), 688 | centered, "{name}, edited {edited}");
            let reopened = read(&bytes, name).unwrap();
            assert_eq!(reopened.model_page.as_ref().unwrap().file_plot_flags, Some(1712 | centered));
            assert_eq!(reopened.layouts.iter().find(|layout| layout.name == "Layout1").unwrap().page.file_plot_flags, Some(688 | centered));
        }
    }
}

#[test]
fn shade_plot_groups_follow_the_target_version_not_the_source() {
    let source = source(0, 688);
    let drawing = read(source.as_bytes(), "source.dxf").unwrap();
    for version in [Version::R2000, Version::R2004, Version::R2007, Version::R2018] {
        let bytes = write_with(&drawing, "saved.dxf", &SaveOptions { version: Some(version), binary: false }).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        for (layout, tags) in plots(&text) {
            for code in [76, 77, 78] {
                assert_eq!(tags.contains_key(&code), version >= Version::R2004, "{version:?}: {layout}, group {code}");
            }
            assert!(!tags.contains_key(&333), "unwritten shade plot object: {version:?}: {layout}");
        }
        for layout in normalized(&text).split("100\nAcDbLayout\n").skip(1) {
            let lines = layout.lines().collect::<Vec<_>>();
            assert!(lines.as_chunks::<2>().0.iter().take_while(|pair| pair[0] != "0").any(|pair| pair[0] == "76"), "layout UCS group: {version:?}");
        }
    }
}

#[test]
fn shade_plot_handle_names_a_written_object_or_is_omitted() {
    cadcraft_io::dwg_worker::allow_in_process();
    let source = source(0, 688);
    let root = source.split("2\nOBJECTS\n").nth(1).unwrap().split("0\nDICTIONARY\n5\n").nth(1).unwrap().lines().next().unwrap();
    let source = source.replacen("3\nACAD_GROUP\n", "3\nDraft render\n350\nF00\n3\nACAD_GROUP\n", 1).replacen(
        "5\nF00\n330\n0\n",
        &format!("5\nF00\n330\n{root}\n"),
        1,
    );
    let drawing = read(source.as_bytes(), "source.dxf").unwrap();
    assert!(drawing.foreign.objects.iter().any(|object| object.kind == "RENDERSETTINGS"));
    for keep_object in [true, false] {
        let mut drawing = drawing.clone();
        if !keep_object {
            std::sync::Arc::make_mut(&mut drawing.foreign).objects.retain(|object| object.kind != "RENDERSETTINGS");
        }
        for version in [Version::R2000, Version::R2004, Version::R2007, Version::R2018] {
            for name in ["saved.dxf", "saved.dwg"] {
                let bytes = write_with(&drawing, name, &SaveOptions { version: Some(version), binary: false }).unwrap();
                let text = if name.ends_with("dwg") { craft_dwg::dwg_to_dxf(&bytes).unwrap() } else { bytes };
                let text = normalized(&String::from_utf8(text).unwrap());
                let written = text.split("0\nRENDERSETTINGS\n5\n").nth(1).and_then(|part| part.lines().next());
                for (layout, tags) in plots(&text) {
                    if keep_object && version >= Version::R2007 {
                        assert_eq!(tags.get(&333).map(String::as_str), written, "{name}, {version:?}: {layout}");
                        assert!(tags.contains_key(&333), "{name}, {version:?}: {layout}");
                    } else {
                        assert!(!tags.contains_key(&333), "{name}, {version:?}: {layout}");
                    }
                }
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

fn pixel_paper_units(edit: Option<bool>) {
    cadcraft_io::dwg_worker::allow_in_process();
    let input = source(3, 11952).replace("72\n0\n", "72\n2\n");
    let mut drawing = read(input.as_bytes(), "source.dxf").unwrap();
    for page in drawing.model_page.iter_mut().chain(drawing.layouts.iter_mut().map(|layout| &mut layout.page)) {
        assert_eq!(page.paper_mm, None);
        match edit {
            Some(false) => page.device = "Changed-device.pc3".into(),
            Some(true) => page.paper_mm = Some(true),
            None => {}
        }
    }
    for name in ["saved.dxf", "saved.dwg"] {
        let bytes = write(&drawing, name).unwrap();
        let text = if name.ends_with("dwg") { craft_dwg::dwg_to_dxf(&bytes).unwrap() } else { bytes };
        for (layout, tags) in plots(&String::from_utf8(text).unwrap()) {
            assert_eq!(tags[&72].trim(), if edit == Some(true) { "1" } else { "2" }, "{name}: {layout}");
        }
    }
}

#[test]
fn pixel_units_survive_unchanged() {
    pixel_paper_units(None);
}

#[test]
fn pixel_units_survive_device_edit() {
    pixel_paper_units(Some(false));
}

#[test]
fn pixel_units_follow_explicit_unit_edit() {
    pixel_paper_units(Some(true));
}
