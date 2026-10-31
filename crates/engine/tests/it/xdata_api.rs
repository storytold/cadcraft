//! Extended data (xdata) through the API: `entities` shows it when asked (`xdata: true`, or an
//! application name that also filters), `properties` lists it, and `properties.set` replaces or
//! removes one application's data, checked as hostile input; what is set is written to DXF with
//! its APPID record and read back the same.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use serde_json::{Value, json};

/// A LINE with a hyperlink and another application's data, a circle without any, and an
/// LWPOLYLINE with the second application's data.
const DXF: &str = "0\nSECTION\n2\nENTITIES\n\
0\nLINE\n5\n2A\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n\
1001\nPE_URL\n1000\nhttps://example.com/door-7\n1002\n{\n1000\nDoor 7\n1002\n}\n\
1001\nACME_TAGS\n1000\nD-07\n1070\n3\n1040\n2.5\n1071\n70000\n1010\n1.5\n1020\n2.5\n1030\n0\n1005\n2B\n1004\n0AFF\n\
0\nCIRCLE\n5\n2C\n8\n0\n10\n50\n20\n50\n30\n0\n40\n5\n\
0\nLWPOLYLINE\n5\n2B\n8\n0\n90\n3\n70\n0\n10\n0\n20\n5\n10\n5\n20\n5\n10\n5\n20\n10\n\
1001\nACME_TAGS\n1000\nP-01\n\
0\nENDSEC\n0\nEOF\n";

fn session_from(dxf: &str) -> Session {
    let d = cadcraft_io::read_dxf(dxf.as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(d, "xdata", None);
    s
}

fn entities(s: &mut Session, p: Value) -> Vec<Value> {
    s.execute("entities", &p).unwrap()["entities"].as_array().unwrap().clone()
}

fn handle_of(s: &Session, line: bool) -> String {
    s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Line(_)) == line).map(|e| e.handle.hex()).unwrap()
}

#[test]
fn entities_show_xdata_only_when_asked() {
    let mut s = session_from(DXF);
    let plain = entities(&mut s, json!({}));
    assert_eq!(plain.len(), 3);
    assert!(plain.iter().all(|e| e.get("xdata").is_none()), "{plain:?}");

    let all = entities(&mut s, json!({"xdata": true}));
    let line = all.iter().find(|e| e["type"] == "Line").unwrap();
    assert_eq!(line["xdata"]["PE_URL"], json!([[1000, "https://example.com/door-7"], [1002, "{"], [1000, "Door 7"], [1002, "}"]]));
    assert_eq!(
        line["xdata"]["ACME_TAGS"],
        json!([[1000, "D-07"], [1070, 3], [1040, 2.5], [1071, 70000], [1010, [1.5, 2.5, 0.0]], [1005, "2B"], [1004, "0AFF"]])
    );
    assert!(all.iter().find(|e| e["type"] == "Circle").unwrap().get("xdata").is_none(), "no xdata, no key");
    assert_eq!(s.execute("entities", &json!({"xdata": false})).unwrap()["entities"], json!(plain));

    // An application name keeps only the entities carrying its data and shows just that.
    let acme = entities(&mut s, json!({"xdata": "acme_tags"}));
    assert_eq!(acme.len(), 2);
    assert!(acme.iter().all(|e| e["xdata"].as_object().unwrap().keys().eq(["ACME_TAGS"])), "{acme:?}");
    let r = s.execute("entities", &json!({"xdata": ["PE_URL"], "type": "Line"})).unwrap();
    assert_eq!((r["count"].as_u64(), r["returned"].as_u64()), (Some(1), Some(1)));
    assert_eq!(s.execute("entities", &json!({"xdata": ["NOPE"]})).unwrap()["count"], 0);
    for bad in [json!({"xdata": 1}), json!({"xdata": []}), json!({"xdata": [1]}), json!({"xdata": {"app": true}})] {
        assert!(s.execute("entities", &bad).is_err(), "{bad}");
    }
}

#[test]
fn properties_set_replaces_removes_and_round_trips_through_dxf() {
    let mut s = Session::new();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let h = handle_of(&s, true);
    // A 1005 handle refers to an object of the drawing (here the line itself): saving remaps it
    // with the other references, so one that refers to nothing would come back as 0.
    let acme = json!([
        [1000, "D-07"],
        [1002, "{"],
        [1003, "0"],
        [1004, "0aff"],
        [1005, h.to_ascii_lowercase()],
        [1010, [1.5, 2.5]],
        [1011, [1, 2, 3]],
        [1040, 2.5],
        [1041, -1.0],
        [1042, 1e3],
        [1070, -3],
        [1071, 70000],
        [1002, "}"]
    ]);
    let want = json!([
        [1000, "D-07"],
        [1002, "{"],
        [1003, "0"],
        [1004, "0AFF"],
        [1005, h],
        [1010, [1.5, 2.5, 0.0]],
        [1011, [1.0, 2.0, 3.0]],
        [1040, 2.5],
        [1041, -1.0],
        [1042, 1000.0],
        [1070, -3],
        [1071, 70000],
        [1002, "}"]
    ]);
    let r = s.execute("properties.set", &json!({"handles": [h], "xdata": {"ACME": acme, "PE_URL": [[1000, "https://example.com"]]}})).unwrap();
    assert_eq!(r["changed"], 1);
    let got = &entities(&mut s, json!({"xdata": true}))[0]["xdata"];
    assert_eq!(got["ACME"], want);
    assert_eq!(got["PE_URL"], json!([[1000, "https://example.com"]]));
    // `properties` lists it too.
    let props = s.execute("properties", &json!({"handles": [h]})).unwrap();
    assert_eq!(props["objects"][0]["xdata"]["ACME"], want);

    // Saved and read back the same, with an APPID record for each application.
    let text = cadcraft_io::write_dxf(s.doc().unwrap());
    let tags = cadcraft_dxf::parse_ascii(&text).unwrap();
    let appids: Vec<String> = cadcraft_dxf::records(&tags)
        .iter()
        .filter(|(k, _)| k == "APPID")
        .filter_map(|(_, t)| t.iter().find(|t| t.code == 2).map(|t| t.str()))
        .collect();
    assert!(appids.contains(&"ACME".to_string()) && appids.contains(&"PE_URL".to_string()), "{appids:?}");
    let mut back = session_from(&text);
    assert_eq!(handle_of(&back, true), h, "the line keeps its handle");
    let again = &entities(&mut back, json!({"xdata": true}))[0]["xdata"];
    assert_eq!(again, got, "{text}");

    // Replacing one application leaves the other; null removes; undo brings it back.
    s.execute("properties.set", &json!({"handles": [h], "xdata": {"ACME": [[1070, 9]]}})).unwrap();
    let got = &entities(&mut s, json!({"xdata": true}))[0]["xdata"];
    assert_eq!(got, &json!({"ACME": [[1070, 9]], "PE_URL": [[1000, "https://example.com"]]}));
    s.execute("properties.set", &json!({"handles": [h], "xdata": {"PE_URL": null, "ACME": []}})).unwrap();
    assert!(entities(&mut s, json!({"xdata": true}))[0].get("xdata").is_none());
    s.undo().unwrap();
    assert_eq!(entities(&mut s, json!({"xdata": true}))[0]["xdata"]["ACME"], json!([[1070, 9]]));
    // The xdata of a drawing's own file comes with it and can be edited too.
    let mut s = session_from(DXF);
    let h = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::LwPolyline(_))).map(|e| e.handle.hex()).unwrap();
    s.execute("properties.set", &json!({"handles": [h], "xdata": {"acme_tags": [[1000, "P-02"]]}})).unwrap();
    assert_eq!(entities(&mut s, json!({"xdata": "ACME_TAGS", "type": "LwPolyline"}))[0]["xdata"], json!({"acme_tags": [[1000, "P-02"]]}));
}

#[test]
fn bad_xdata_is_refused_and_changes_nothing() {
    let mut s = session_from(DXF);
    let h = handle_of(&s, true);
    let before = entities(&mut s, json!({"xdata": true}));
    let huge: Vec<Value> = std::iter::repeat_n(json!([1000, "x"]), cadcraft_engine::doc::MAX_XDATA_GROUPS + 1).collect();
    for bad in [
        json!([]),
        json!("ACME"),
        json!({"": [[1000, "x"]]}),
        json!({"CADCRAFT": [[1000, "x"]]}),
        json!({"bad*name": [[1000, "x"]]}),
        json!({"ACME": "x"}),
        json!({"ACME": [1000, "x"]}),
        json!({"ACME": [[1000]]}),
        json!({"ACME": [[1000, 5]]}),
        json!({"ACME": [[1001, "B"]]}),
        json!({"ACME": [[8, "0"]]}),
        json!({"ACME": [[999, "x"]]}),
        json!({"ACME": [[1072, 1]]}),
        json!({"ACME": [["1000", "x"]]}),
        json!({"ACME": [[1e300, "x"]]}),
        json!({"ACME": [[1002, "("]]}),
        json!({"ACME": [[1002, "}"]]}),
        json!({"ACME": [[1002, "{"], [1000, "x"]]}),
        json!({"ACME": [[1010, 1]]}),
        json!({"ACME": [[1010, [1]]]}),
        json!({"ACME": [[1010, [1, 2, 3, 4]]]}),
        json!({"ACME": [[1010, [1, "y"]]]}),
        json!({"ACME": [[1020, 1]]}),
        json!({"ACME": [[1040, "1"]]}),
        json!({"ACME": [[1040, null]]}),
        json!({"ACME": [[1070, 1.5]]}),
        json!({"ACME": [[1070, 40000]]}),
        json!({"ACME": [[1071, 3000000000i64]]}),
        json!({"ACME": [[1004, "ABC"]]}),
        json!({"ACME": [[1004, "zz"]]}),
        json!({"ACME": [[1005, ""]]}),
        json!({"ACME": [[1005, "nothex"]]}),
        json!({"ACME": [[1000, "x".repeat(256)]]}),
        json!({"ACME": [[1000, "a\nb"]]}),
        json!({"ACME": huge}),
    ] {
        let err = s.execute("properties.set", &json!({"handles": [h], "xdata": bad})).expect_err(&format!("{bad} must be refused"));
        assert!(!err.to_string().is_empty());
        assert_eq!(entities(&mut s, json!({"xdata": true})), before, "{bad} changed something");
    }
    // Together, two applications may not exceed what one object holds.
    let full: Vec<Value> = std::iter::repeat_n(json!([1070, 1]), cadcraft_engine::doc::MAX_XDATA_GROUPS - 1).collect();
    s.execute("properties.set", &json!({"handles": [h], "xdata": {"PE_URL": null, "ACME_TAGS": null, "FULL": full}})).unwrap();
    let err = s.execute("properties.set", &json!({"handles": [h], "xdata": {"MORE": [[1070, 1]]}})).unwrap_err();
    assert!(err.to_string().contains("exceed"), "{err}");
    assert!(entities(&mut s, json!({"xdata": true}))[0]["xdata"].get("MORE").is_none());
}

#[test]
fn a_missing_handle_changes_nothing() {
    // One handle that names no object fails the whole call before any object or the layer
    // table changes: the other object keeps its data and the new layer is not created.
    let mut s = session_from(DXF);
    let h = handle_of(&s, true);
    let before = entities(&mut s, json!({"xdata": true}));
    let r = s.execute("properties.set", &json!({"handles": [h, "FFFF"], "layer": "NEWLAYER", "xdata": {"ACME": [[1000, "x"]]}}));
    assert!(r.is_err(), "a missing handle is refused");
    assert_eq!(entities(&mut s, json!({"xdata": true})), before);
    assert!(s.doc().unwrap().layer("NEWLAYER").is_none(), "no layer is left behind");
}

#[test]
fn runs_of_one_application_in_any_case_are_one_entry() {
    // `ACME` and `acme` runs on one entity are one application (as `set_xdata_app` treats
    // them): one JSON key with every group, and `properties.set` with that entry keeps them all.
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n2A\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n0\n31\n0\n\
        1001\nACME\n1070\n1\n1001\nacme\n1070\n2\n1001\nOTHER\n1000\nx\n0\nENDSEC\n0\nEOF\n";
    let mut s = session_from(dxf);
    let h = handle_of(&s, true);
    let shown = entities(&mut s, json!({"xdata": true}));
    assert_eq!(shown[0]["xdata"], json!({"ACME": [[1070, 1], [1070, 2]], "OTHER": [[1000, "x"]]}));
    assert_eq!(entities(&mut s, json!({"xdata": "acme"}))[0]["xdata"], json!({"ACME": [[1070, 1], [1070, 2]]}));
    s.execute("properties.set", &json!({"handles": [h], "xdata": shown[0]["xdata"]})).unwrap();
    assert_eq!(entities(&mut s, json!({"xdata": true}))[0]["xdata"], shown[0]["xdata"]);
}

#[test]
fn acad_xdata_of_a_dimension_is_refused_and_kept_elsewhere() {
    // A dimension's `ACAD` run is its DSTYLE overrides, written from the model and read back
    // into them: raw `ACAD` groups on a dimension would be lost on save, so they are refused
    // (nothing changes). On any other object `ACAD` is foreign data and survives a save.
    let mut s = Session::new();
    s.script("dimlinear 0,0 10,0 5,-3\n").unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let dim = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).map(|e| e.handle.hex()).unwrap();
    let line = handle_of(&s, true);
    s.execute("properties.set", &json!({"handles": [&dim], "textHeight": 5})).unwrap();
    let before = entities(&mut s, json!({"xdata": true}));
    let r = s.execute("properties.set", &json!({"handles": [&dim], "xdata": {"ACAD": [[1000, "MINE"], [1070, 42]]}}));
    assert!(r.is_err(), "ACAD on a dimension is refused");
    assert_eq!(entities(&mut s, json!({"xdata": true})), before);
    s.execute("properties.set", &json!({"handles": [&line], "xdata": {"ACAD": [[1000, "MINE"], [1070, 42]]}})).unwrap();
    let bytes = cadcraft_io::write(s.doc().unwrap(), "acad.dxf").unwrap();
    let mut back = Session::empty();
    back.open_drawing(cadcraft_io::read_dxf(&bytes).unwrap(), "acad", None);
    let kept = entities(&mut back, json!({"xdata": "ACAD"}));
    assert_eq!(kept.len(), 1, "only the line carries raw ACAD data");
    assert_eq!(kept[0]["type"], "Line");
    assert_eq!(kept[0]["xdata"], json!({"ACAD": [[1000, "MINE"], [1070, 42]]}));
    // The dimension's own override is still there after the round trip.
    let dm = back.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap();
    assert!(matches!(&dm.kind, EntityKind::Dimension(d) if !d.overrides.is_empty()), "the DSTYLE overrides survive");
}
