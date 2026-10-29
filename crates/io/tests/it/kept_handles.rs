//! Tables, blocks, layouts and dictionaries keep the handles they were read with, through DXF. The fixture is synthetic, from the public DXF Reference.

use std::collections::HashMap;

use cadcraft_doc::{Handle, Layer};
use cadcraft_dxf::{parse, records, sections};
use cadcraft_io::{read_dxf, write_dxf};

/// DXF text from lines of `code value`.
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

const SRC: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1032
9 $HANDSEED
5 300
0 ENDSEC
0 SECTION
2 TABLES
0 TABLE
2 LTYPE
5 5
330 0
100 AcDbSymbolTable
70 1
0 LTYPE
5 14
330 5
100 AcDbSymbolTableRecord
100 AcDbLinetypeTableRecord
2 Continuous
70 0
3 Solid line
72 65
73 0
40 0.0
0 ENDTAB
0 TABLE
2 LAYER
5 2
330 0
100 AcDbSymbolTable
70 2
0 LAYER
5 10
330 2
100 AcDbSymbolTableRecord
100 AcDbLayerTableRecord
2 0
70 0
62 7
6 Continuous
0 LAYER
5 11
330 2
100 AcDbSymbolTableRecord
100 AcDbLayerTableRecord
2 WALLS
70 0
62 1
6 Continuous
0 ENDTAB
0 TABLE
2 STYLE
5 3
330 0
100 AcDbSymbolTable
70 1
0 STYLE
5 15
330 3
100 AcDbSymbolTableRecord
100 AcDbTextStyleTableRecord
2 Standard
70 0
40 0.0
41 1.0
3 txt
0 ENDTAB
0 TABLE
2 BLOCK_RECORD
5 1
330 0
100 AcDbSymbolTable
70 3
0 BLOCK_RECORD
5 1F
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *Model_Space
340 22
0 BLOCK_RECORD
5 1B
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *Paper_Space
340 1E
0 BLOCK_RECORD
5 40
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 DOOR
0 ENDTAB
0 ENDSEC
0 SECTION
2 BLOCKS
0 BLOCK
5 20
330 1F
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 *Model_Space
70 0
10 0.0
20 0.0
30 0.0
3 *Model_Space
0 ENDBLK
5 21
330 1F
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 BLOCK
5 1C
330 1B
100 AcDbEntity
67 1
8 0
100 AcDbBlockBegin
2 *Paper_Space
70 0
10 0.0
20 0.0
30 0.0
3 *Paper_Space
0 ENDBLK
5 1D
330 1B
100 AcDbEntity
67 1
8 0
100 AcDbBlockEnd
0 BLOCK
5 41
330 40
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 DOOR
70 0
10 0.0
20 0.0
30 0.0
3 DOOR
0 LINE
5 43
330 40
100 AcDbEntity
8 0
100 AcDbLine
10 0.0
20 0.0
30 0.0
11 1.0
21 0.0
31 0.0
0 ENDBLK
5 42
330 40
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 ENDSEC
0 SECTION
2 ENTITIES
0 LINE
5 50
330 1F
100 AcDbEntity
8 WALLS
100 AcDbLine
10 0.0
20 0.0
30 0.0
11 10.0
21 0.0
31 0.0
0 ENDSEC
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_LAYOUT
350 1A
0 DICTIONARY
5 1A
330 C
100 AcDbDictionary
281 1
3 Layout1
350 1E
3 Model
350 22
0 LAYOUT
5 1E
330 1A
100 AcDbPlotSettings
1
2 none_device
4 ISO_A4_(210.00_x_297.00_MM)
6
40 7.5
41 20.0
42 7.5
43 20.0
44 210.0
45 297.0
100 AcDbLayout
1 Layout1
70 1
71 1
10 0.0
20 0.0
11 420.0
21 297.0
330 1B
0 LAYOUT
5 22
330 1A
100 AcDbPlotSettings
1
2 none_device
4 ISO_A4_(210.00_x_297.00_MM)
6
40 7.5
41 20.0
42 7.5
43 20.0
44 210.0
45 297.0
100 AcDbLayout
1 Model
70 1
71 0
10 0.0
20 0.0
11 12.0
21 9.0
330 1F
0 ENDSEC
0 EOF
";

/// Handle of each record of the written file by (type, name); entities by (type, handle).
fn handles(text: &str) -> HashMap<(String, String), String> {
    let tags = parse(text.as_bytes()).unwrap();
    let mut out = HashMap::new();
    for s in sections(&tags) {
        let mut block = String::new();
        let mut table = String::new();
        for (kind, g) in records(&s.tags) {
            let h = g.iter().find(|t| t.code == 5 || (kind == "DIMSTYLE" && t.code == 105)).map(|t| t.str()).unwrap_or_default();
            let name = match kind.as_str() {
                "TABLE" => {
                    table = g.iter().find(|t| t.code == 2).map(|t| t.str()).unwrap_or_default();
                    table.clone()
                }
                "BLOCK" => {
                    block = g.iter().find(|t| t.code == 2).map(|t| t.str()).unwrap_or_default();
                    block.clone()
                }
                "ENDBLK" => block.clone(),
                "LAYOUT" => g.iter().rev().find(|t| t.code == 1).map(|t| t.str()).unwrap_or_default(),
                "DICTIONARY" => h.clone(),
                _ => g.iter().find(|t| t.code == 2).map(|t| t.str()).unwrap_or_else(|| h.clone()),
            };
            let _ = &table;
            out.insert((kind, name.to_ascii_uppercase()), h.to_ascii_uppercase());
        }
    }
    out
}

fn get(m: &HashMap<(String, String), String>, kind: &str, name: &str) -> Option<String> {
    m.get(&(kind.to_string(), name.to_ascii_uppercase())).cloned()
}

fn check(text: &str) {
    let h = handles(text);
    assert_eq!(get(&h, "TABLE", "LAYER").as_deref(), Some("2"));
    assert_eq!(get(&h, "TABLE", "LTYPE").as_deref(), Some("5"));
    assert_eq!(get(&h, "TABLE", "BLOCK_RECORD").as_deref(), Some("1"));
    assert_eq!(get(&h, "LAYER", "WALLS").as_deref(), Some("11"));
    assert_eq!(get(&h, "LAYER", "0").as_deref(), Some("10"));
    assert_eq!(get(&h, "LTYPE", "Continuous").as_deref(), Some("14"));
    assert_eq!(get(&h, "STYLE", "Standard").as_deref(), Some("15"));
    assert_eq!(get(&h, "BLOCK_RECORD", "*Model_Space").as_deref(), Some("1F"));
    assert_eq!(get(&h, "BLOCK_RECORD", "*Paper_Space").as_deref(), Some("1B"));
    assert_eq!(get(&h, "BLOCK_RECORD", "DOOR").as_deref(), Some("40"));
    assert_eq!(get(&h, "BLOCK", "DOOR").as_deref(), Some("41"));
    assert_eq!(get(&h, "ENDBLK", "DOOR").as_deref(), Some("42"));
    assert_eq!(get(&h, "BLOCK", "*Paper_Space").as_deref(), Some("1C"));
    assert_eq!(get(&h, "ENDBLK", "*Paper_Space").as_deref(), Some("1D"));
    assert_eq!(get(&h, "LAYOUT", "Layout1").as_deref(), Some("1E"));
    assert_eq!(get(&h, "LAYOUT", "Model").as_deref(), Some("22"));
    assert_eq!(get(&h, "DICTIONARY", "C").as_deref(), Some("C"), "the named object dictionary");
    assert_eq!(get(&h, "DICTIONARY", "1A").as_deref(), Some("1A"), "ACAD_LAYOUT");
    assert_eq!(get(&h, "LINE", "50").as_deref(), Some("50"));
    // Every handle once.
    let all: Vec<&String> = h.values().filter(|v| !v.is_empty()).collect();
    let mut uniq = all.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), all.len(), "a handle was given twice");
}

#[test]
fn dxf_round_trip_keeps_handles() {
    let d = read_dxf(dxf(SRC).as_bytes()).unwrap();
    assert_eq!(d.layer("WALLS").map(|l| l.handle.get()), Some(Handle(0x11)));
    assert_eq!(d.block("DOOR").map(|b| (b.handle.get(), b.begin_handle.get(), b.end_handle.get())), Some((Handle(0x40), Handle(0x41), Handle(0x42))));
    assert!(d.handseed > 0x50);
    let out = write_dxf(&d);
    check(&out);
    // And again: a drawing CADCraft wrote keeps them too.
    check(&write_dxf(&read_dxf(out.as_bytes()).unwrap()));
}

#[test]
fn new_and_copied_records_get_fresh_handles() {
    let mut d = read_dxf(dxf(SRC).as_bytes()).unwrap();
    // A copy of WALLS (same handle) and a new layer.
    let copy = Layer { name: "WALLS2".into(), ..d.layer("WALLS").cloned().unwrap() };
    d.layers.push(copy);
    d.layers.push(Layer { name: "NEW".into(), ..Layer::default() });
    let h = handles(&write_dxf(&d));
    assert_eq!(get(&h, "LAYER", "WALLS").as_deref(), Some("11"));
    let copy = get(&h, "LAYER", "WALLS2").unwrap();
    let new = get(&h, "LAYER", "NEW").unwrap();
    for x in [&copy, &new] {
        assert!(u64::from_str_radix(x, 16).unwrap() >= 0x300, "{x} is not above the seed");
    }
    assert_ne!(copy, new);
}

#[test]
fn renamed_layer_keeps_its_handle() {
    let mut d = read_dxf(dxf(SRC).as_bytes()).unwrap();
    if let Some(l) = d.layers.iter_mut().find(|l| l.name == "WALLS") {
        l.name = "PARTITIONS".into();
    }
    let h = handles(&write_dxf(&d));
    assert_eq!(get(&h, "LAYER", "PARTITIONS").as_deref(), Some("11"));
}

/// The APPID records, the active viewport and the Model layout keep handles on read, so new objects
/// must not be given them in memory (the handle seed after a read and the writer's reserved set
/// both come from `kept_handles`).
#[test]
fn kept_handles_include_the_appids_the_active_viewport_and_the_model_layout() {
    use cadcraft_doc::{AppId, Drawing, KeptHandle, Layout, VportTile};
    let kept = |h: u64| KeptHandle(Handle(h));
    let mut d = Drawing::default();
    d.appids.push(AppId { handle: kept(0x7001), ..AppId::default() });
    d.active_vport = Some(Box::new(VportTile { handle: kept(0x7002), ..VportTile::default() }));
    let mut model = Layout::new("Model", 0);
    model.handle = kept(0x7003);
    model.block_handle = kept(0x7004);
    model.begin_handle = kept(0x7005);
    model.end_handle = kept(0x7006);
    d.model_layout = Some(Box::new(model));
    let handles = d.kept_handles();
    for h in 0x7001..=0x7006u64 {
        assert!(handles.contains(&h), "handle {h:X} is not kept");
    }
}
