//! DATAEXTRACTION / EATTEXT: block and attribute data as rows, a CSV file and a table.

use cadcraft_doc::EntityKind;
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// Blocks VALVE (tags PART, QTY) and PUMP (tag PART) with references, and an unattributed block.
fn drawing() -> Session {
    let mut s = Session::new();
    block(&mut s, "VALVE", &["PART", "QTY"]);
    block(&mut s, "PUMP", &["Part"]);
    block(&mut s, "PLAIN", &[]);
    insert(&mut s, "VALVE", 0.0, json!({"PART": "V-1", "QTY": "2"}));
    insert(&mut s, "VALVE", 10.0, json!({"part": "V-1", "QTY": "2"}));
    insert(&mut s, "valve", 20.0, json!({"PART": "V-2, large"}));
    insert(&mut s, "PUMP", 30.0, json!({"PART": "P-9"}));
    insert(&mut s, "PLAIN", 40.0, json!({}));
    s
}

fn block(s: &mut Session, name: &str, tags: &[&str]) {
    for (i, t) in tags.iter().enumerate() {
        s.execute("attdef", &json!({"tag": t, "default": "", "at": [0, i as f64], "height": 0.5})).unwrap();
    }
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": name, "base": [0, 0], "handles": hs, "keep": "delete"})).unwrap();
}

fn insert(s: &mut Session, name: &str, x: f64, attribs: Value) -> String {
    let r = s.execute("insert", &json!({"name": name, "at": [x, 5], "rotation": 90, "attribs": attribs})).unwrap();
    r["handle"].as_str().unwrap().to_string()
}

#[test]
fn rows_are_counted_by_block_and_values() {
    let mut s = drawing();
    let r = s.execute("dataextraction", &json!({})).unwrap();
    assert_eq!(r["columns"], json!(["Count", "Name", "PART", "QTY"]));
    assert_eq!(r["rows"], json!([[1, "PLAIN", "", ""], [1, "PUMP", "P-9", ""], [2, "VALVE", "V-1", "2"], [1, "VALVE", "V-2, large", ""]]));
    // EATTEXT is the same command; block and tag filters take wildcards in any case.
    let r = s.execute("eattext", &json!({"blocks": "v*", "tags": "qty"})).unwrap();
    assert_eq!(r["columns"], json!(["Count", "Name", "QTY"]));
    assert_eq!(r["rows"], json!([[1, "VALVE", ""], [2, "VALVE", "2"]]));
    // Extracting changes nothing.
    assert!(!s.state().unwrap().undo.is_empty());
    let undo = s.state().unwrap().undo.len();
    s.execute("dataextraction", &json!({})).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo);
}

#[test]
fn uncombined_rows_list_each_reference() {
    let mut s = drawing();
    let pump = s.doc().unwrap().model.iter().find(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "PUMP")).unwrap().handle;
    let r = s.execute("dataextraction", &json!({"combine": false, "handles": [pump.hex()]})).unwrap();
    assert_eq!(r["columns"], json!(["Name", "Handle", "Layer", "Position X", "Position Y", "Rotation", "PART"]));
    assert_eq!(r["rows"], json!([["PUMP", pump.hex(), "0", 30.0, 5.0, 90.0, "P-9"]]));
}

#[test]
fn constant_attributes_come_from_the_definition() {
    let mut s = drawing();
    // Make PUMP's PART constant by editing the definition directly.
    let d = s.doc_mut().unwrap();
    let b = std::sync::Arc::make_mut(d.blocks.get_mut("PUMP").unwrap());
    let h = b.entities.iter().find(|e| matches!(e.kind, EntityKind::AttDef(_))).unwrap().handle;
    b.entities.modify(h, |e| {
        if let EntityKind::AttDef(a) = &mut e.kind {
            a.constant = true;
            a.text.value = "FIXED".into();
        }
    });
    let pump = s.doc().unwrap().model.iter().find(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "PUMP")).unwrap().handle;
    s.doc_mut()
        .unwrap()
        .modify_entity(pump, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.attribs.clear();
            }
        })
        .unwrap();
    let r = s.execute("dataextraction", &json!({"blocks": "PUMP"})).unwrap();
    assert_eq!(r["rows"], json!([[1, "PUMP", "FIXED"]]));
}

#[test]
fn writes_csv_and_tab_separated_files() {
    let mut s = drawing();
    let dir = std::env::temp_dir().join(format!("cadcraft-dx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("out.csv");
    let r = s.execute("dataextraction", &json!({"blocks": "VALVE", "file": csv.to_str().unwrap()})).unwrap();
    assert_eq!(r["file"], csv.to_str().unwrap());
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "Count,Name,PART,QTY\r\n2,VALVE,V-1,2\r\n1,VALVE,\"V-2, large\",\r\n");
    let txt = dir.join("out.txt");
    s.execute("dataextraction", &json!({"blocks": "VALVE", "file": txt.to_str().unwrap()})).unwrap();
    assert_eq!(std::fs::read_to_string(&txt).unwrap(), "Count\tName\tPART\tQTY\r\n2\tVALVE\tV-1\t2\r\n1\tVALVE\tV-2, large\t\r\n");
    // A file that can't be written is an error.
    assert!(s.execute("dataextraction", &json!({"file": dir.join("missing/x.csv").to_str().unwrap()})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inserts_a_table_with_a_header_row() {
    let mut s = drawing();
    let r = s.execute("dataextraction", &json!({"blocks": "VALVE", "at": [100, 100], "title": "Valves"})).unwrap();
    let h = cadcraft_doc::Handle::parse_hex(r["table"].as_str().unwrap()).unwrap();
    let EntityKind::Table(t) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!("not a table") };
    let text: Vec<Vec<&str>> = t.cells.iter().map(|r| r.iter().map(|c| c.text.as_str()).collect()).collect();
    assert_eq!(
        text,
        [vec!["Valves", "", "", ""], vec!["Count", "Name", "PART", "QTY"], vec!["2", "VALVE", "V-1", "2"], vec!["1", "VALVE", "V-2, large", ""]]
    );
    assert!(t.title && t.header);
    // The column holding "V-2, large" is wider than the Count column.
    assert!(t.col_widths[2] > t.col_widths[0]);
    // One undo step removes it.
    s.undo().unwrap();
    assert!(s.doc().unwrap().entity(h).is_none());
    assert!(s.execute("dataextraction", &json!({"at": "nowhere"})).is_err());
}

#[test]
fn too_many_rows_for_a_table_are_refused_before_anything_changes() {
    let mut s = Session::new();
    block(&mut s, "B", &["N"]);
    for i in 0..2001 {
        insert(&mut s, "B", i as f64, json!({"N": i.to_string()}));
    }
    let n = s.doc().unwrap().model.len();
    assert!(s.execute("dataextraction", &json!({"at": [0, 0]})).is_err());
    assert_eq!(s.doc().unwrap().model.len(), n);
    assert_eq!(s.execute("dataextraction", &json!({})).unwrap()["rows"].as_array().unwrap().len(), 2001);
}

#[test]
fn dataextraction_at_the_command_line_inserts_a_table() {
    let mut s = drawing();
    s.cmdline("dataextraction").unwrap();
    assert!(s.prompt_text().contains("Enter block names to extract <*>:"), "{}", s.prompt_text());
    s.cmdline("pump,plain").unwrap();
    assert!(s.prompt_text().contains("Combine identical rows? [Yes/No] <Yes>:"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("Where should the data go? [Table/File/Both] <Table>:"), "{}", s.prompt_text());
    s.cmdline("t").unwrap();
    s.cmdline("50,50").unwrap();
    assert!(s.running.is_none());
    assert!(s.log.iter().any(|l| l == "2 rows extracted."), "{:?}", s.log);
    let t = s.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t.clone()) } else { None }).unwrap();
    assert_eq!(t.cells.len(), 3);
}

#[test]
fn eattext_at_the_command_line_writes_a_file() {
    let mut s = drawing();
    let dir = std::env::temp_dir().join(format!("cadcraft-dx-cl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("valves.csv");
    for l in ["eattext", "VALVE", "n", "f", csv.to_str().unwrap()] {
        s.cmdline(l).unwrap();
    }
    assert!(s.running.is_none());
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with("Name,Handle,Layer,Position X,Position Y,Rotation,PART,QTY\r\nVALVE,"), "{text}");
    assert_eq!(text.lines().count(), 4);
    let _ = std::fs::remove_dir_all(&dir);
}
