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

#[cfg(not(target_arch = "wasm32"))]
mod library {
    use super::*;
    use cadcraft_doc::{PlotStyle, PlotStyleSource, PlotStyleTable};
    use cadcraft_engine::plotstyle_library::{PlotStyleStore, set_plot_style_store};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The library store is process-wide, so library tests run one at a time.
    fn lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }

    struct TempLibrary {
        dir: PathBuf,
    }

    impl TempLibrary {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("cadcraft-plotstyles-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempLibrary { dir }
        }
        fn path(&self) -> &Path {
            &self.dir
        }
        fn write(&self, table: &PlotStyleTable) -> PathBuf {
            let file = self.dir.join(cadcraft_doc::plot_style_file_name(&table.name).unwrap());
            std::fs::write(&file, table.to_file_text()).unwrap();
            file
        }
        fn install(&self) {
            set_plot_style_store(PlotStyleStore::folder(&self.dir));
        }
    }

    impl Drop for TempLibrary {
        fn drop(&mut self) {
            set_plot_style_store(PlotStyleStore::memory());
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn office(description: &str, lineweight: u16) -> PlotStyleTable {
        PlotStyleTable::named(
            "office.stb",
            description,
            vec![PlotStyle { name: "Pen 5".into(), lineweight: Some(lineweight), ..PlotStyle::default() }],
        )
    }

    fn table<'a>(list: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        list["tables"].as_array().and_then(|a| a.iter().find(|t| t["name"] == name)).unwrap_or_else(|| panic!("no table {name}"))
    }

    #[test]
    fn library_tables_are_listed_and_resolve_without_being_embedded() {
        let _lock = lock();
        let lib = TempLibrary::new("listed");
        lib.write(&office("Office pens", 35));
        lib.write(&PlotStyleTable::named("review.stb", "Review", vec![PlotStyle { name: "Red".into(), ..PlotStyle::default() }]));
        lib.install();

        let mut s = Session::new();
        let list = s.execute("stylesmanager", &json!({})).unwrap();
        assert_eq!(table(&list, "office.stb")["source"], "library");
        assert_eq!(table(&list, "office.stb")["inDrawing"], false);
        assert_eq!(table(&list, "review.stb")["source"], "library");
        assert_eq!(table(&list, "monochrome.ctb")["source"], "builtin");
        assert!(list["warnings"].as_array().unwrap().is_empty(), "{list}");

        // A page setup naming a library table resolves it, and the table does not enter the drawing.
        s.execute("pagesetup", &json!({"layout": "Layout1", "plotStyleTable": "office.stb"})).unwrap();
        let d = s.doc().unwrap();
        assert!(d.plot_style_tables.is_empty(), "listing and resolving must not embed the library table");
        let t = cadcraft_doc::plot_style_table(d, "office.stb").expect("a page setup naming a library table resolves it");
        assert_eq!(t.by_name("Pen 5").and_then(|s| s.lineweight), Some(35));
        assert_eq!(cadcraft_doc::plot_style_table_source(d, "office.stb"), Some(PlotStyleSource::Library));
    }

    #[test]
    fn a_drawing_table_with_the_same_name_wins() {
        let _lock = lock();
        let lib = TempLibrary::new("drawing-wins");
        lib.write(&office("From the library", 35));
        lib.install();

        let mut s = Session::new();
        let text = office("From the drawing", 70).to_file_text();
        s.execute("stylesmanager", &json!({"action": "load", "text": text})).unwrap();
        let list = s.execute("stylesmanager", &json!({})).unwrap();
        assert_eq!(table(&list, "office.stb")["source"], "drawing");
        assert_eq!(table(&list, "office.stb")["inDrawing"], true);
        let d = s.doc().unwrap();
        let t = cadcraft_doc::plot_style_table(d, "office.stb").unwrap();
        assert_eq!(t.description, "From the drawing");
        assert_eq!(t.by_name("Pen 5").and_then(|s| s.lineweight), Some(70));
        assert_eq!(d.plot_style_tables.len(), 1);
        assert_eq!(cadcraft_doc::plot_style_table_source(d, "office.stb"), Some(PlotStyleSource::Drawing));
    }

    #[test]
    fn malformed_and_oversized_library_files_are_skipped_with_warnings() {
        let _lock = lock();
        let lib = TempLibrary::new("hostile");
        lib.write(&office("Office pens", 35));
        std::fs::write(lib.path().join("broken.json"), "not a plot style table").unwrap();
        std::fs::write(lib.path().join("huge.json"), "x".repeat(cadcraft_doc::MAX_TABLE_FILE + 1)).unwrap();
        lib.install();

        let mut s = Session::new();
        let list = s.execute("stylesmanager", &json!({})).unwrap();
        assert_eq!(table(&list, "office.stb")["source"], "library");
        let warnings = list["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("broken.json"))), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("huge.json"))), "{warnings:?}");
        assert!(cadcraft_doc::plot_style_table(s.doc().unwrap(), "office.stb").is_some(), "the other tables still load");
    }

    #[test]
    fn stylesmanager_saves_and_loads_tables_through_the_library() {
        let _lock = lock();
        let lib = TempLibrary::new("roundtrip");
        lib.install();

        let mut s = Session::new();
        s.execute("stylesmanager", &json!({"action": "new", "name": "office", "kind": "named"})).unwrap();
        s.execute("stylesmanager", &json!({"action": "set", "name": "office.stb", "style": {"name": "Pen 5", "lineweight": 0.35}})).unwrap();
        let saved = s.execute("stylesmanager", &json!({"action": "save", "name": "office.stb", "library": true})).unwrap();
        assert_eq!(saved["library"], true);
        let file = saved["file"].as_str().expect("the library file name");
        assert!(lib.path().join(file).is_file(), "{saved}");

        // Deleting the drawing's copy leaves the library table shared by every drawing.
        s.execute("stylesmanager", &json!({"action": "delete", "name": "office.stb"})).unwrap();
        assert!(s.doc().unwrap().plot_style_tables.is_empty());
        let got = s.execute("stylesmanager", &json!({"action": "get", "name": "office.stb"})).unwrap();
        assert_eq!((got["source"].as_str(), got["inDrawing"].as_bool()), (Some("library"), Some(false)));
        let pen = got["styles"].as_array().unwrap().iter().find(|v| v["name"] == "Pen 5").unwrap();
        assert_eq!(pen["lineweight"], 35);

        // `load` with `library` stores it there, not in the drawing.
        let review = PlotStyleTable::named("review.stb", "Review", vec![]).to_file_text();
        let loaded = s.execute("stylesmanager", &json!({"action": "load", "text": review, "library": true})).unwrap();
        assert_eq!(loaded["source"], "library");
        assert!(lib.path().join(loaded["file"].as_str().unwrap()).is_file());
        assert!(s.doc().unwrap().plot_style_tables.is_empty(), "loading into the library must not embed it");

        // Without `library`, save still returns the table as text.
        let text = s.execute("stylesmanager", &json!({"action": "save", "name": "review.stb"})).unwrap();
        assert!(text["text"].as_str().is_some_and(|t| t.contains("cadcraftPlotStyleTable")));
    }

    #[test]
    fn a_missing_library_directory_is_handled() {
        let _lock = lock();
        let lib = TempLibrary::new("missing");
        let missing = lib.path().join("not-there");
        set_plot_style_store(PlotStyleStore::folder(&missing));

        let mut s = Session::new();
        let list = s.execute("stylesmanager", &json!({})).unwrap();
        assert_eq!(table(&list, "monochrome.ctb")["source"], "builtin");
        assert!(list["warnings"].as_array().unwrap().is_empty(), "{list}");

        // Saving creates the library folder.
        s.execute("stylesmanager", &json!({"action": "new", "name": "office", "kind": "named"})).unwrap();
        let saved = s.execute("stylesmanager", &json!({"action": "save", "name": "office.stb", "library": true})).unwrap();
        assert!(missing.join(saved["file"].as_str().unwrap()).is_file(), "{saved}");
    }
}
