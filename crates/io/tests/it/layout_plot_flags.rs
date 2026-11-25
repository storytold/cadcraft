//! #3227: saving kept the layout's plot flags only for the bits CADCraft models and forced 512
//! (plot paper space last), 32 (plot with plot styles) and 16 (standard scale) on, so a layout
//! with flags 0 came back as 560, in DXF and in DWG.

use cadcraft_doc::{Drawing, Layout, PageSetup};
use cadcraft_io::{read, write};

/// A drawing with layout SHEET read with plot layout `flags` (lineweights, centring and display
/// plot styles follow them, as the reader sets them).
fn drawing(flags: i64) -> Drawing {
    let mut d = Drawing::new_imperial();
    let mut l = Layout::new("SHEET", 1);
    l.page = PageSetup {
        lineweights: flags & 128 != 0,
        center: flags & 4 != 0,
        show_plot_styles: flags & 2 != 0,
        file_plot_flags: Some(flags),
        ..PageSetup::default()
    };
    d.layouts.push(l);
    d
}

/// The plot layout flags of SHEET after saving `d` as `name` and reading it back.
fn flags_after_save(d: &Drawing, name: &str) -> Option<i64> {
    let bytes = write(d, name).unwrap();
    let back = read(&bytes, name).unwrap();
    back.layouts.iter().find(|l| l.name == "SHEET")?.page.file_plot_flags
}

#[test]
fn plot_layout_flags_are_saved_as_read() {
    cadcraft_io::dwg_worker::allow_in_process();
    for name in ["sheet.dxf", "sheet.dwg"] {
        for flags in [0, 176, 560, 688] {
            assert_eq!(flags_after_save(&drawing(flags), name), Some(flags), "{name}: flags {flags}");
        }
    }
}

#[test]
fn an_unchanged_standard_scale_keeps_its_flag_and_choice() {
    cadcraft_io::dwg_worker::allow_in_process();
    // Flags 176 with standard scale 25 (1:50): the scale is 0.02, not 1:1, and still standard.
    let mut d = drawing(176);
    let page = &mut d.layouts.last_mut().unwrap().page;
    page.scale = 0.02;
    page.file_standard_scale = Some((25, 0.02));
    for name in ["sheet.dxf", "sheet.dwg"] {
        let back = read(&write(&d, name).unwrap(), name).unwrap();
        let page = &back.layouts.iter().find(|l| l.name == "SHEET").unwrap().page;
        assert_eq!(page.file_plot_flags, Some(176), "{name}");
        assert_eq!(page.file_standard_scale, Some((25, 0.02)), "{name}");
    }
    // A scale changed after reading is a custom one: 16 is cleared and the choice dropped.
    d.layouts.last_mut().unwrap().page.scale = 0.05;
    assert_eq!(flags_after_save(&d, "sheet.dxf"), Some(160));
}

#[test]
fn the_page_still_decides_the_bits_cadcraft_models() {
    // Lineweights turned on and a custom scale chosen after reading flags 16: 128 is set and the
    // standard scale bit cleared, the rest kept.
    let mut d = drawing(16 | 512);
    let page = &mut d.layouts.last_mut().unwrap().page;
    page.lineweights = true;
    page.scale = 0.02;
    assert_eq!(flags_after_save(&d, "sheet.dxf"), Some(512 | 128));
    // A page made in CADCraft keeps the defaults (plot paper space last, plot with plot styles,
    // standard scale, lineweights).
    let mut d = drawing(0);
    d.layouts.last_mut().unwrap().page = PageSetup::default();
    assert_eq!(flags_after_save(&d, "sheet.dxf"), Some(512 | 128 | 32 | 16));
}

/// Each LAYOUT object's name and plot layout flags (the AcDbLayout group 1 and the
/// AcDbPlotSettings group 70) in DXF text.
fn layout_plot_flags(text: &str) -> Vec<(String, i64)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let (mut inside, mut subclass, mut name, mut flags) = (false, "", None, None);
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        match *code {
            "0" => {
                if let (true, Some(n), Some(f)) = (inside, name.take(), flags.take()) {
                    out.push((n, f));
                }
                (inside, subclass, name, flags) = (*value == "LAYOUT", "", None, None);
            }
            "100" if inside => subclass = value,
            "70" if inside && subclass == "AcDbPlotSettings" => flags = value.parse().ok(),
            "1" if inside && subclass == "AcDbLayout" => name = Some(value.to_string()),
            _ => {}
        }
    }
    out
}

#[test]
fn only_the_model_layout_carries_the_model_type_flag() {
    // 1024 is "ModelType" (DXF Reference, PLOTSETTINGS group 70): the plot settings of model space.
    // The Model layout carries it; a paper layout does not, even when it was read with it.
    for read in [688, 688 | 1024] {
        let bytes = write(&drawing(read), "sheet.dxf").unwrap();
        let layouts = layout_plot_flags(&String::from_utf8_lossy(&bytes));
        let flags = |name: &str| layouts.iter().find(|(n, _)| n == name).map(|&(_, f)| f);
        let (model, sheet) = (flags("Model").unwrap(), flags("SHEET").unwrap());
        assert_ne!(model & 1024, 0, "Model, SHEET read with {read}: {layouts:?}");
        assert_eq!(sheet & 1024, 0, "SHEET read with {read}: {layouts:?}");
    }
}
