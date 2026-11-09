//! Saved layer states in AutoCAD's form: the LAYER table's extension dictionary holds an
//! `ACAD_LAYERSTATES` dictionary of one XRECORD per state. The groups below are copied from
//! AutoCAD 2024's own DXFOUT of states saved with -LAYER stAte on a self-built drawing.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Drawing, Layer, LayerState, Lineweight};
use cadcraft_io::{read, write_dxf};

/// `(code, value)` pairs of a DXF, trimmed.
fn pairs(dxf: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    lines.chunks(2).filter_map(|c| Some((c.first()?.trim().to_string(), c.get(1)?.trim().to_string()))).collect()
}

fn join(p: &[(String, String)]) -> String {
    p.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn g(c: &str, v: &str) -> (String, String) {
    (c.to_string(), v.to_string())
}

/// The records of a DXF: (type, groups after the `0`).
fn records(p: &[(String, String)]) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for (c, v) in p {
        if c == "0" {
            out.push((v.clone(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push((c.clone(), v.clone()));
        }
    }
    out
}

fn value<'a>(groups: &'a [(String, String)], code: &str) -> Option<&'a str> {
    groups.iter().find(|(c, _)| c == code).map(|(_, v)| v.as_str())
}

/// The handle of table record `kind` named `name`.
fn record_handle(p: &[(String, String)], kind: &str, name: &str) -> String {
    let recs = records(p);
    let (_, groups) = recs.iter().find(|(k, gs)| k == kind && value(gs, "2").is_some_and(|n| n.eq_ignore_ascii_case(name))).unwrap();
    value(groups, "5").unwrap().to_string()
}

/// A handle above every handle of the file, plus `k`.
fn fresh(p: &[(String, String)], k: u64) -> String {
    let top = p.iter().filter(|(c, _)| c == "5" || c == "105").filter_map(|(_, v)| u64::from_str_radix(v, 16).ok()).max().unwrap();
    format!("{:X}", top + 0x100 + k)
}

/// `dxf` with the LAYER table given extension dictionary `xdict` and `objects` added at the end
/// of OBJECTS.
fn inject(dxf: &str, xdict: &str, objects: Vec<(String, String)>) -> String {
    let mut p = pairs(dxf);
    let table = p.iter().enumerate().position(|(i, (c, v))| c == "0" && v == "TABLE" && p.get(i + 1).is_some_and(|n| n.1 == "LAYER")).unwrap();
    let at = table + p[table..].iter().position(|(c, _)| c == "5").unwrap() + 1;
    p.splice(at..at, [g("102", "{ACAD_XDICTIONARY"), g("360", xdict), g("102", "}")]);
    let objs = p.iter().position(|(c, v)| c == "2" && v == "OBJECTS").unwrap();
    let end = objs + p[objs..].iter().position(|(c, v)| c == "0" && v == "ENDSEC").unwrap();
    p.splice(end..end, objects);
    join(&p)
}

/// Layers A (red) and B, and the DASHED linetype.
fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { color: Color::Index(1), ..Layer::new("A") });
    d.layers.push(Layer::new("B"));
    d.linetypes.push(cadcraft_doc::library::standard_linetypes().into_iter().find(|l| l.name == "DASHED").unwrap());
    d
}

/// State S2 as AutoCAD 2024 wrote it after -LAYER: A true colour 10,20,30 (nearest index 168),
/// 50% transparent, 0.35 mm, DASHED, not plotted; B locked and frozen in new viewports. Then the
/// entry of a layer purged since (AutoCAD keeps it, pointing at handles that are gone).
fn s2(p: &[(String, String)], rec: &str, dict: &str) -> Vec<(String, String)> {
    let (a, b, zero) = (record_handle(p, "LAYER", "A"), record_handle(p, "LAYER", "B"), record_handle(p, "LAYER", "0"));
    let (cont, dashed) = (record_handle(p, "LTYPE", "Continuous"), record_handle(p, "LTYPE", "DASHED"));
    let gone = fresh(p, 0x50);
    let mut v =
        vec![g("0", "XRECORD"), g("5", rec), g("102", "{ACAD_REACTORS"), g("330", dict), g("102", "}"), g("330", dict), g("100", "AcDbXrecord")];
    v.extend([g("280", "1"), g("91", "2047"), g("301", ""), g("290", "0"), g("302", "0")]);
    for (h, flags, aci, lw, lt, ps, tr) in [
        (b.as_str(), "28", "7", "-3", cont.as_str(), "Color_7", "0"),
        (a.as_str(), "0", "168", "35", dashed.as_str(), "Color_168", "33554559"),
        (zero.as_str(), "8", "7", "-3", cont.as_str(), "Color_7", "0"),
        (gone.as_str(), "8", "7", "-3", gone.as_str(), "Color_7", "0"),
    ] {
        v.extend([g("330", h), g("90", flags), g("62", aci), g("370", lw), g("331", lt), g("1", ps), g("440", tr)]);
        if h == a {
            v.push(g("92", "-1039526882"));
        }
    }
    v
}

/// State S7, saved with only On and restored once (AutoCAD then sets bit 65536), current layer A.
fn s7(p: &[(String, String)], rec: &str, dict: &str) -> Vec<(String, String)> {
    let (a, b) = (record_handle(p, "LAYER", "A"), record_handle(p, "LAYER", "B"));
    let cont = record_handle(p, "LTYPE", "Continuous");
    let mut v =
        vec![g("0", "XRECORD"), g("5", rec), g("102", "{ACAD_REACTORS"), g("330", dict), g("102", "}"), g("330", dict), g("100", "AcDbXrecord")];
    v.extend([g("280", "1"), g("91", "66049"), g("301", "only on"), g("290", "0"), g("302", "A")]);
    for (h, aci) in [(b.as_str(), "7"), (a.as_str(), "1")] {
        v.extend([g("330", h), g("90", "8"), g("62", aci), g("370", "-3"), g("331", &cont), g("1", &format!("Color_{aci}")), g("440", "0")]);
    }
    v
}

/// The extension dictionary of the LAYER table and the states dictionary, with `extra` entries in
/// the extension dictionary.
fn dictionaries(p: &[(String, String)], x: &str, y: &str, states: &[(&str, &str)], extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let table = {
        let recs = records(p);
        let (_, gs) = recs.iter().find(|(k, gs)| k == "TABLE" && value(gs, "2") == Some("LAYER")).unwrap();
        value(gs, "5").unwrap().to_string()
    };
    let mut v = vec![g("0", "DICTIONARY"), g("5", x), g("330", &table), g("100", "AcDbDictionary"), g("280", "1"), g("281", "1")];
    for (k, h) in extra.iter().copied().chain([("ACAD_LAYERSTATES", y)]) {
        v.extend([g("3", k), g("360", h)]);
    }
    v.extend([
        g("0", "DICTIONARY"),
        g("5", y),
        g("102", "{ACAD_REACTORS"),
        g("330", x),
        g("102", "}"),
        g("330", x),
        g("100", "AcDbDictionary"),
        g("281", "1"),
    ]);
    for (name, h) in states {
        v.extend([g("3", name), g("350", h)]);
    }
    v
}

fn autocad_file() -> String {
    let base = write_dxf(&drawing());
    let p = pairs(&base);
    let h = |k| fresh(&p, k);
    let (x, y, r2, r7) = (h(1), h(2), h(3), h(4));
    let mut objects = dictionaries(&p, &x, &y, &[("S2", &r2), ("S7", &r7)], &[]);
    objects.extend(s2(&p, &r2, &y));
    objects.extend(s7(&p, &r7, &y));
    inject(&base, &x, objects)
}

#[test]
fn reads_the_layer_states_autocad_writes() {
    let d = read(autocad_file().as_bytes(), "acad.dxf").unwrap();
    let names: Vec<&str> = d.layer_states.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["S2", "S7"]);
    let s2 = &d.layer_states[0];
    assert_eq!((s2.mask, s2.current.as_str(), s2.description.as_str()), (2047, "0", ""));
    // The purged layer's entry is skipped.
    let layers: Vec<&str> = s2.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(layers, ["B", "A", "0"]);
    let a = &s2.layers[1];
    assert_eq!(a.color, Color::True(Rgb(10, 20, 30)));
    assert_eq!((a.transparency, a.lineweight, a.linetype.as_str()), (50, Lineweight::from_dxf(35), "DASHED"));
    assert!(a.on && !a.frozen && !a.locked && !a.plot && !a.vp_freeze_new);
    // A colour-dependent drawing: the plot style stays the layer's ("Color_168" is its colour's).
    assert_eq!(a.plot_style, "Normal");
    let b = &s2.layers[0];
    assert!(b.on && !b.frozen && b.locked && b.plot && b.vp_freeze_new, "{b:?}");
    assert_eq!((b.color, b.linetype.as_str(), b.transparency), (Color::Index(7), "Continuous", 0));
    let s7 = &d.layer_states[1];
    assert_eq!((s7.mask, s7.current.as_str(), s7.description.as_str()), (66049, "A", "only on"));
    assert_eq!(s7.layers.iter().find(|l| l.name == "A").unwrap().color, Color::Index(1));
}

#[test]
fn autocads_layer_states_are_written_back_in_its_form() {
    let d = read(autocad_file().as_bytes(), "acad.dxf").unwrap();
    let text = write_dxf(&d);
    let p = pairs(&text);
    // One states dictionary, in the LAYER table's extension dictionary, and no CADCraft copy.
    assert_eq!(p.iter().filter(|(c, v)| c == "3" && v == "ACAD_LAYERSTATES").count(), 1);
    assert!(!text.contains("CADCRAFT_LAYERSTATES"));
    let recs = records(&p);
    let (_, table) = recs.iter().find(|(k, gs)| k == "TABLE" && value(gs, "2") == Some("LAYER")).unwrap();
    let x = table.windows(2).find(|w| w[0] == g("102", "{ACAD_XDICTIONARY")).map(|w| w[1].1.clone()).unwrap();
    let (_, xd) = recs.iter().find(|(k, gs)| k == "DICTIONARY" && value(gs, "5") == Some(x.as_str())).unwrap();
    assert_eq!(value(xd, "330"), value(table, "5"));
    let back = read(text.as_bytes(), "back.dxf").unwrap();
    assert_eq!(back.layer_states, d.layer_states);
    // The XRECORD of S2 as AutoCAD writes it: B, then A.
    let (a_h, b_h) = (record_handle(&p, "LAYER", "A"), record_handle(&p, "LAYER", "B"));
    let (_, rec) = recs.iter().find(|(k, gs)| k == "XRECORD" && gs.contains(&g("91", "2047"))).unwrap();
    let head = rec.iter().position(|(c, _)| c == "280").unwrap();
    assert_eq!(rec[head..head + 5], [("280", "1"), ("91", "2047"), ("301", ""), ("290", "0"), ("302", "0")].map(|(c, v)| g(c, v)));
    let cont = record_handle(&p, "LTYPE", "Continuous");
    let b = [("330", b_h.as_str()), ("90", "28"), ("62", "7"), ("370", "-3"), ("331", &cont), ("1", "Color_7"), ("440", "0")];
    assert_eq!(rec[head + 5..head + 12], b.map(|(c, v)| g(c, v)));
    let from = rec.iter().position(|(c, v)| c == "330" && *v == a_h).unwrap();
    let a: Vec<(String, String)> = rec[from..from + 9].to_vec();
    let dashed = record_handle(&p, "LTYPE", "DASHED");
    let want = [
        ("330", a_h.as_str()),
        ("90", "0"),
        ("62", "168"),
        ("370", "35"),
        ("331", &dashed),
        ("1", "Color_168"),
        ("440", "33554559"),
        ("92", "-1039526882"),
    ];
    assert_eq!(a[..8], want.map(|(c, v)| g(c, v)), "{a:?}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
        assert_eq!(read(&dwg, "x.dwg").unwrap().layer_states, d.layer_states);
    }
}

#[test]
fn layer_filters_next_to_the_states_are_kept() {
    // The LAYER table's extension dictionary of a drawing with layer filters holds them next to
    // the states (ACAD_LAYERFILTERS, ACLYDICTIONARY): they stay, in the dictionary CADCraft writes.
    let base = write_dxf(&drawing());
    let p = pairs(&base);
    let h = |k| fresh(&p, k);
    let (x, y, r2, f, fr, gone) = (h(1), h(2), h(3), h(4), h(5), h(6));
    // Two entries whose objects the file doesn't have (a handle of nothing, and a null one): they
    // aren't written into the table's dictionary.
    let extra = [("ACAD_LAYERFILTERS", f.as_str()), ("OTHER_GONE", gone.as_str()), ("OTHER_NULL", "0")];
    let mut objects = dictionaries(&p, &x, &y, &[("S2", &r2)], &extra);
    objects.extend(s2(&p, &r2, &y));
    objects.extend([g("0", "DICTIONARY"), g("5", &f), g("102", "{ACAD_REACTORS"), g("330", &x), g("102", "}"), g("330", &x)]);
    objects.extend([g("100", "AcDbDictionary"), g("281", "1"), g("3", "Walls"), g("350", &fr)]);
    objects.extend([g("0", "XRECORD"), g("5", &fr), g("102", "{ACAD_REACTORS"), g("330", &f), g("102", "}"), g("330", &f), g("100", "AcDbXrecord")]);
    objects.extend([g("280", "1"), g("1", "A*")]);
    let file = inject(&base, &x, objects);
    let mut text = file;
    for round in 0..2 {
        let d = read(text.as_bytes(), "f.dxf").unwrap();
        assert_eq!(d.layer_states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["S2"], "round {round}");
        text = write_dxf(&d);
        let p = pairs(&text);
        let recs = records(&p);
        let (_, table) = recs.iter().find(|(k, gs)| k == "TABLE" && value(gs, "2") == Some("LAYER")).unwrap();
        let xdicts: Vec<String> = table.windows(2).filter(|w| w[0] == g("102", "{ACAD_XDICTIONARY")).map(|w| w[1].1.clone()).collect();
        assert_eq!(xdicts.len(), 1, "round {round}");
        let xd = &xdicts[0];
        let (_, dict) = recs.iter().find(|(k, gs)| k == "DICTIONARY" && value(gs, "5") == Some(xd.as_str())).unwrap();
        let keys: Vec<&str> = dict.iter().filter(|(c, _)| c == "3").map(|(_, v)| v.as_str()).collect();
        assert_eq!(keys, ["ACAD_LAYERSTATES", "ACAD_LAYERFILTERS"], "round {round}");
        // The filters dictionary is owned by the written one, and so is its filter by it.
        let fh = dict.windows(2).find(|w| w[0] == g("3", "ACAD_LAYERFILTERS")).map(|w| w[1].1.clone()).unwrap();
        let (_, fd) = recs.iter().find(|(k, gs)| k == "DICTIONARY" && value(gs, "5") == Some(fh.as_str())).unwrap();
        let owner = fd.iter().filter(|(c, _)| c == "330").map(|(_, v)| v.as_str()).collect::<Vec<_>>();
        assert_eq!(owner, [xd.as_str(), xd.as_str()], "round {round}: reactor and owner");
        let walls = value(fd, "350").unwrap();
        let (_, wr) = recs.iter().find(|(k, gs)| k == "XRECORD" && value(gs, "5") == Some(walls)).unwrap();
        assert!(wr.contains(&g("330", &fh)) && wr.contains(&g("1", "A*")), "round {round}");
        // The kept dictionary itself is not written a second time.
        assert_eq!(recs.iter().filter(|(k, gs)| k == "DICTIONARY" && gs.iter().any(|(c, v)| c == "3" && v == "ACAD_LAYERFILTERS")).count(), 1);
    }
    // With every state deleted the filters stay, in the table's own dictionary, and AutoCAD's states
    // don't come back.
    let mut d = read(text.as_bytes(), "f.dxf").unwrap();
    d.layer_states.clear();
    let text = write_dxf(&d);
    assert!(!text.contains("ACAD_LAYERSTATES"));
    let p = pairs(&text);
    let recs = records(&p);
    let (_, table) = recs.iter().find(|(k, gs)| k == "TABLE" && value(gs, "2") == Some("LAYER")).unwrap();
    let xd = table.windows(2).find(|w| w[0] == g("102", "{ACAD_XDICTIONARY")).map(|w| w[1].1.clone()).unwrap();
    let (_, dict) = recs.iter().find(|(k, gs)| k == "DICTIONARY" && value(gs, "5") == Some(xd.as_str())).unwrap();
    assert_eq!(dict.iter().filter(|(c, _)| c == "3").map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["ACAD_LAYERFILTERS"]);
    assert!(read(text.as_bytes(), "g.dxf").unwrap().layer_states.is_empty());
}

#[test]
fn view_snapshots_and_names_twice_in_another_case_stay_in_cadcrafts_record() {
    let mut d = drawing();
    for name in ["Plan", "*VIEW Front", "PLAN", "Wände", "WÄNDE", "", " "] {
        d.layer_states.push(LayerState::new(name, d.layers.clone(), "A"));
    }
    let text = write_dxf(&d);
    let p = pairs(&text);
    let recs = records(&p);
    let (_, states) =
        recs.iter().find(|(k, gs)| k == "DICTIONARY" && gs.contains(&g("281", "1")) && gs.iter().any(|(c, v)| c == "3" && v == "Plan")).unwrap();
    // (The DXF writes "ä" as \U+00E4.)
    let keys: Vec<String> = states.iter().filter(|(c, _)| c == "3").map(|(_, v)| v.replace("\\U+00E4", "ä")).collect();
    assert_eq!(keys, ["Plan", "Wände"], "AutoCAD's dictionary holds the first of each name");
    assert!(text.contains("CADCRAFT_LAYERSTATES"));
    let back = read(text.as_bytes(), "v.dxf").unwrap();
    // AutoCAD's states first, then CADCraft's own.
    // (A blank name can't be a key of AutoCAD's dictionary: those stay in CADCraft's record too.)
    let want: Vec<LayerState> = [0, 3, 1, 2, 4, 5, 6].iter().map(|&i| d.layer_states[i].clone()).collect();
    assert_eq!(back.layer_states, want);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&d, "v.dwg").unwrap();
        assert_eq!(read(&dwg, "v.dwg").unwrap().layer_states, want);
    }
}

#[test]
fn a_layer_of_an_xref_gets_group_93() {
    // AutoCAD 2024 writes 93 (0 for a newly attached xref) after the lineweight of an xref's layer
    // only (measured with a self-built xref).
    let mut d = drawing();
    d.layers.push(Layer::new("XR|L1"));
    d.layer_states.push(LayerState::new("X", d.layers.clone(), "0"));
    let p = pairs(&write_dxf(&d));
    let recs = records(&p);
    let (_, rec) = recs.iter().find(|(k, gs)| k == "XRECORD" && gs.contains(&g("91", "2047"))).unwrap();
    let start = rec.iter().position(|(c, _)| c == "302").unwrap() + 1;
    let mut with_93 = Vec::new();
    let mut layer = String::new();
    for (i, (c, v)) in rec.iter().enumerate().skip(start) {
        match c.as_str() {
            "330" => layer.clone_from(v),
            "93" => {
                assert_eq!((v.as_str(), rec[i - 1].0.as_str(), rec[i + 1].0.as_str()), ("0", "370", "331"));
                with_93.push(layer.clone());
            }
            _ => {}
        }
    }
    assert_eq!(with_93, [record_handle(&p, "LAYER", "XR|L1")]);
}

#[test]
fn named_plot_styles_are_written_by_placeholder() {
    // In a drawing of named plot styles AutoCAD 2024 writes a layer's plot style as group 390, the
    // handle of the style's placeholder object (measured); CADCraft reads it back by that handle.
    let mut d = drawing();
    d.header.set_i64("PSTYLEMODE", 0);
    let a = d.layers.iter().position(|l| l.name == "A").unwrap();
    d.layers[a].plot_style = "Thick".into();
    d.layer_states.push(LayerState::new("N", d.layers.clone(), "0"));
    // No layer uses Thick any more: its placeholder is written for the state.
    d.layers[a].plot_style = "Normal".into();
    let text = write_dxf(&d);
    let p = pairs(&text);
    let recs = records(&p);
    let (_, names) = recs.iter().find(|(k, _)| k == "ACDBDICTIONARYWDFLT").unwrap();
    let thick = names.windows(2).find(|w| w[0] == g("3", "Thick")).map(|w| w[1].1.clone()).unwrap();
    let (_, rec) = recs.iter().find(|(k, gs)| k == "XRECORD" && gs.contains(&g("91", "2047"))).unwrap();
    assert!(rec.contains(&g("390", &thick)) && !rec.iter().any(|(c, _)| c == "1"), "{rec:?}");
    let back = read(text.as_bytes(), "n.dxf").unwrap();
    assert_eq!(back.layer_states[0].layers.iter().find(|l| l.name == "A").unwrap().plot_style, "Thick");
    assert_eq!(back.layer_states, d.layer_states);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dwg = cadcraft_io::write(&d, "n.dwg").unwrap();
        assert_eq!(read(&dwg, "n.dwg").unwrap().layer_states, d.layer_states);
    }
}

#[test]
fn a_state_in_both_records_is_read_from_autocads() {
    // A file of an earlier CADCraft keeps its states in CADCraft's record; AutoCAD then saves a state
    // of the same name in its own dictionary: AutoCAD's is the one read.
    let mut d = drawing();
    d.layer_states.push(LayerState::new("S", d.layers.clone(), "A"));
    d.layer_states.push(LayerState::new("*VIEW S", d.layers.clone(), "B"));
    let text = write_dxf(&d);
    assert!(text.contains("*VIEW S"));
    let text = text.replace("*VIEW S", "S");
    let back = read(text.as_bytes(), "both.dxf").unwrap();
    assert_eq!(back.layer_states.iter().map(|s| (s.name.as_str(), s.current.as_str())).collect::<Vec<_>>(), [("S", "A")]);
}

#[test]
fn states_past_the_limit_are_left_out_and_the_rest_come_in_file_order() {
    // A hostile file with one state more than the reader keeps (100 000): the last one in the file
    // is left out, and the others come in file order, here the reverse of their names' order.
    const LIMIT: u64 = 100_000;
    let base = write_dxf(&drawing());
    let p = pairs(&base);
    let first = u64::from_str_radix(&fresh(&p, 0), 16).unwrap();
    let h = |k: u64| format!("{:X}", first + k);
    let (x, y) = (h(1), h(2));
    let names: Vec<String> = (0..=LIMIT).map(|i| format!("S{:06}", LIMIT + 1 - i)).collect();
    let records: Vec<String> = (0..=LIMIT).map(|i| h(3 + i)).collect();
    let entries: Vec<(&str, &str)> = names.iter().map(String::as_str).zip(records.iter().map(String::as_str)).collect();
    let mut objects = dictionaries(&p, &x, &y, &entries, &[]);
    for r in &records {
        objects.extend([g("0", "XRECORD"), g("5", r), g("330", &y), g("100", "AcDbXrecord")]);
        objects.extend([g("280", "1"), g("91", "2047"), g("301", ""), g("290", "0"), g("302", "0")]);
    }
    let d = read(inject(&base, &x, objects).as_bytes(), "many.dxf").unwrap();
    let names: Vec<&str> = d.layer_states.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), 100_000);
    assert_eq!((names.first().copied(), names.get(1).copied(), names.last().copied()), (Some("S100001"), Some("S100000"), Some("S000002")));
}

#[test]
fn a_record_twice_in_a_file_is_one_state() {
    // A damaged file can hold an object twice under one handle: one state.
    let base = write_dxf(&drawing());
    let p = pairs(&base);
    let h = |k| fresh(&p, k);
    let (x, y, r7) = (h(1), h(2), h(4));
    let mut objects = dictionaries(&p, &x, &y, &[("S7", &r7)], &[]);
    objects.extend(s7(&p, &r7, &y));
    objects.extend(s7(&p, &r7, &y));
    let d = read(inject(&base, &x, objects).as_bytes(), "twice.dxf").unwrap();
    assert_eq!(d.layer_states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["S7"]);
}

#[test]
fn group_92_without_the_true_colour_prefix_is_not_a_true_colour() {
    // AutoCAD 2024 writes 92 only for a true colour, 0xC2RRGGBB (measured); a value with another
    // high byte (not seen from AutoCAD) leaves the colour index of group 62.
    let text = autocad_file().replace("92\n-1039526882", "92\n-1023410175");
    let d = read(text.as_bytes(), "c3.dxf").unwrap();
    assert_eq!(d.layer_states[0].layers.iter().find(|l| l.name == "A").unwrap().color, Color::Index(168));
}
