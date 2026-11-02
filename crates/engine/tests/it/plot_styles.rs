//! PAGESETUP: the plot style table and "Display plot styles" of a layout (issue #410).

use cadcraft_engine::Session;
use serde_json::json;

#[test]
fn pagesetup_sets_table_and_display_flag() {
    let mut s = Session::new();
    let r = s.execute("pagesetup", &json!({"layout": "Layout1", "plotStyleTable": "monochrome.ctb", "displayPlotStyles": true})).unwrap();
    assert_eq!(r["page"]["plotStyleTable"], "monochrome.ctb");
    assert_eq!(r["page"]["showPlotStyles"], true);
    let page = &s.doc().unwrap().layout("Layout1").unwrap().page;
    assert!(page.show_plot_styles && page.plot_style_table == "monochrome.ctb");
    // The report lists the built-in tables.
    let report = s.execute("pagesetup", &json!({})).unwrap();
    let tables: Vec<&str> = report["plotStyleTables"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(tables.contains(&"monochrome.ctb") && tables.contains(&"grayscale.ctb"), "{tables:?}");
    s.execute("pagesetup", &json!({"layout": "Layout1", "displayPlotStyles": false})).unwrap();
    assert!(!s.doc().unwrap().layout("Layout1").unwrap().page.show_plot_styles);
}

#[test]
fn plotstyle_sets_objects_and_the_current_style() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let handle = s.doc().unwrap().model.last().unwrap().handle;
    let h = handle.hex();
    // No name: a report.
    let r = s.execute("plotstyle", &json!({})).unwrap();
    assert_eq!(r["current"], "ByLayer");
    assert!(r["styles"].as_array().unwrap().iter().any(|v| v == "Normal"));
    // An object's style.
    let r = s.execute("plotstyle", &json!({"name": "Black", "handles": [h]})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(s.doc().unwrap().entity(handle).unwrap().common.plot_style, "Black");
    // Nothing selected: the current style for new objects.
    s.execute("select", &json!({"handles": []})).ok();
    s.execute("plotstyle", &json!({"name": "byblock"})).unwrap();
    s.execute("line", &json!({"points": [[0, 1], [10, 1]]})).unwrap();
    let h2 = s.doc().unwrap().model.last().unwrap().handle;
    assert_eq!(s.doc().unwrap().entity(h2).unwrap().common.plot_style, "ByBlock");
    assert!(s.execute("plotstyle", &json!({"name": "  "})).is_err());
    assert!(s.execute("plotstyle", &json!({"name": 5})).is_err());
    // A layer's plot style.
    s.execute("layer.set", &json!({"name": "0", "plotStyle": "Thick"})).unwrap();
    assert_eq!(s.doc().unwrap().layer("0").unwrap().plot_style, "Thick");
    assert!(s.execute("layer.set", &json!({"name": "0", "plotStyle": "ByLayer"})).is_err());
}

#[test]
fn stylesmanager_creates_edits_saves_and_loads_tables() {
    let mut s = Session::new();
    let list = s.execute("stylesmanager", &json!({})).unwrap();
    let names: Vec<&str> = list["tables"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"monochrome.ctb") && names.contains(&"default.stb"), "{names:?}");
    // A new named table gets its extension and Normal.
    let r = s.execute("stylesmanager", &json!({"action": "new", "name": "office", "kind": "named"})).unwrap();
    assert_eq!(r["name"], "office.stb");
    let r = s
        .execute(
            "stylesmanager",
            &json!({"action": "set", "name": "office.stb", "style": {"name": "Pen 5", "color": "#00ff00", "screening": 40, "lineweight": 0.35, "linetype": "HIDDEN", "end": "round"}}),
        )
        .unwrap();
    let pen = r["styles"].as_array().unwrap().iter().find(|v| v["name"] == "Pen 5").unwrap().clone();
    assert_eq!(
        (pen["color"].as_str(), pen["screening"].as_u64(), pen["lineweight"].as_u64(), pen["end"].as_str()),
        (Some("#00ff00"), Some(40), Some(35), Some("round"))
    );
    // Bad edits are refused.
    for bad in [
        json!({"name": "Pen 5", "screening": 101}),
        json!({"name": "Pen 5", "color": "green"}),
        json!({"name": "Pen 5", "lineweight": 9}),
        json!({"name": "Pen 5", "join": "zigzag"}),
    ] {
        assert!(s.execute("stylesmanager", &json!({"action": "set", "name": "office.stb", "style": bad})).is_err(), "{bad}");
    }
    assert!(s.execute("stylesmanager", &json!({"action": "set", "name": "office.stb", "removeStyle": "Normal"})).is_err());
    // Editing a built-in keeps a copy in the drawing; colour-dependent styles are Color_n.
    s.execute("stylesmanager", &json!({"action": "set", "name": "monochrome.ctb", "style": {"name": "Color_1", "color": null}})).unwrap();
    assert_eq!(s.doc().unwrap().plot_style_tables.len(), 2);
    assert!(s.execute("stylesmanager", &json!({"action": "set", "name": "monochrome.ctb", "style": {"name": "Pen"}})).is_err());
    // Save as text, delete, load back.
    let text = s.execute("stylesmanager", &json!({"action": "save", "name": "office.stb"})).unwrap()["text"].as_str().unwrap().to_string();
    s.execute("stylesmanager", &json!({"action": "delete", "name": "office.stb"})).unwrap();
    assert!(s.execute("stylesmanager", &json!({"action": "get", "name": "office.stb"})).is_err());
    s.execute("stylesmanager", &json!({"action": "load", "text": text})).unwrap();
    let got = s.execute("stylesmanager", &json!({"action": "get", "name": "office.stb"})).unwrap();
    assert_eq!(got["inDrawing"], true);
    assert!(got["styles"].as_array().unwrap().iter().any(|v| v["name"] == "Pen 5"));
    assert!(s.execute("stylesmanager", &json!({"action": "load", "text": "{\"x\": 1}"})).is_err());
    assert!(s.execute("stylesmanager", &json!({"action": "delete", "name": "grayscale.ctb"})).is_err(), "built-ins stay");
    assert!(s.execute("stylesmanager", &json!({"action": "explode"})).is_err());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = std::env::temp_dir().join(format!("cadcraft-pstyle-{}.stb.json", std::process::id()));
        let p = path.to_string_lossy().to_string();
        s.execute("stylesmanager", &json!({"action": "save", "name": "office.stb", "path": p})).unwrap();
        s.execute("stylesmanager", &json!({"action": "load", "path": p, "name": "copy.stb"})).unwrap();
        assert!(s.execute("stylesmanager", &json!({"action": "get", "name": "copy.stb"})).is_ok());
        let _ = std::fs::remove_file(&path);
    }
    // Undo takes a table edit back.
    s.execute("undo", &json!({})).unwrap();
}
