//! What survives DXF open → save → reopen of data CADCraft does not model (#528): unknown
//! entities and objects, other applications' xdata, extension dictionaries, layer states, colour
//! books, hyperlinks, DICTIONARYVAR objects, XRECORDs and persistent reactors. One test per item.
//!
//! The fixture is synthetic, written from the public DXF Reference: a drawing as another program
//! might save it, with an application `ACME` that defines its own entity and object types.

use std::collections::HashMap;

use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_dxf::{Tag, records, sections};
use cadcraft_io::{SaveOptions, Version, read_dxf, write_dxf, write_with};

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

const HEADER: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1018
9 $HANDSEED
5 100
0 ENDSEC
0 SECTION
2 CLASSES
0 CLASS
1 ACME_WIDGET
2 AcmeWidget
3 AcmeApp
90 1025
280 0
281 1
0 CLASS
1 ACME_OBJECT
2 AcmeObject
3 AcmeApp
90 0
280 0
281 0
0 CLASS
1 DICTIONARYVAR
2 AcDbDictionaryVar
3 ObjectDBX Classes
90 0
280 0
281 0
0 ENDSEC
";

const TABLES: &str = "
0 SECTION
2 TABLES
0 TABLE
2 LAYER
5 2
102 {ACAD_XDICTIONARY
360 60
102 }
330 0
100 AcDbSymbolTable
70 2
0 LAYER
5 10
102 {ACAD_XDICTIONARY
360 63
102 }
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
420 13382451
430 RAL$RAL 3020
6 Continuous
370 -3
1001 ACME
1000 layer-data
1070 7
0 ENDTAB
0 TABLE
2 UCS
5 3
330 0
100 AcDbSymbolTable
70 1
0 UCS
5 12
330 3
100 AcDbSymbolTableRecord
100 AcDbUCSTableRecord
2 TILTED
70 0
10 1
20 2
30 3
11 0
21 1
31 0
12 -1
22 0
32 0
79 0
146 5.5
0 ENDTAB
0 TABLE
2 APPID
5 4
330 0
100 AcDbSymbolTable
70 3
0 APPID
5 13
330 4
100 AcDbSymbolTableRecord
100 AcDbRegAppTableRecord
2 ACAD
70 0
0 APPID
5 14
330 4
100 AcDbSymbolTableRecord
100 AcDbRegAppTableRecord
2 ACME
70 0
0 APPID
5 15
330 4
100 AcDbSymbolTableRecord
100 AcDbRegAppTableRecord
2 PE_URL
70 0
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
340 0
0 BLOCK_RECORD
5 1E
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 *Paper_Space
340 0
0 BLOCK_RECORD
5 1D
102 {ACAD_XDICTIONARY
360 65
102 }
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 DOOR
340 0
70 0
280 1
1001 ACME
1000 block-record-data
0 ENDTAB
0 ENDSEC
";

const BLOCKS: &str = "
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
10 0
20 0
30 0
3 *Model_Space
1
0 ENDBLK
5 21
330 1F
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 BLOCK
5 22
330 1E
100 AcDbEntity
67 1
8 0
100 AcDbBlockBegin
2 *Paper_Space
70 0
10 0
20 0
30 0
3 *Paper_Space
1
0 ENDBLK
5 23
330 1E
100 AcDbEntity
67 1
8 0
100 AcDbBlockEnd
0 BLOCK
5 24
330 1D
100 AcDbEntity
8 0
100 AcDbBlockBegin
2 DOOR
70 0
10 0
20 0
30 0
3 DOOR
1
4 A door leaf
1001 ACME
1000 block-entity-data
0 LINE
5 26
330 1D
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 0
30 0
11 1
21 0
31 0
1001 ACME
1000 in-block
0 ACME_WIDGET
5 32
330 1D
100 AcDbEntity
8 0
100 AcDbAcmeWidget
10 0.5
20 0.5
30 0
1 widget-in-block
0 ENDBLK
5 25
330 1D
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 ENDSEC
";

const ENTITIES: &str = "
0 SECTION
2 ENTITIES
0 LINE
5 2A
102 {ACAD_REACTORS
330 42
102 }
102 {ACAD_XDICTIONARY
360 2B
102 }
330 1F
100 AcDbEntity
8 WALLS
62 1
420 16711680
430 RAL$RAL 3000
440 33554559
100 AcDbLine
10 0
20 0
30 0
11 10
21 0
31 0
1001 ACME
1000 hello
1070 42
1005 42
1001 PE_URL
1000 https://example.com/spec
1002 {
1000 Specification
1002 {
1071 0
1002 }
1002 }
0 ACME_WIDGET
5 30
330 1F
100 AcDbEntity
8 WALLS
100 AcDbAcmeWidget
10 1
20 2
30 0
1 widget-in-model
340 42
341 11
0 ACME_WIDGET
5 31
330 1E
100 AcDbEntity
67 1
8 0
100 AcDbAcmeWidget
10 3
20 4
30 0
1 widget-in-paper
0 INSERT
5 33
330 1F
100 AcDbEntity
8 0
100 AcDbBlockReference
2 DOOR
10 5
20 5
30 0
0 IMAGE
5 34
102 {ACAD_REACTORS
330 52
102 }
330 1F
100 AcDbEntity
8 0
100 AcDbRasterImage
90 0
10 5
20 5
30 0
11 0.01
21 0
31 0
12 0
22 0.01
32 0
13 640
23 480
340 51
70 7
280 0
281 50
282 50
283 0
360 52
71 1
91 2
14 -0.5
24 -0.5
14 639.5
24 479.5
0 ENDSEC
";

const OBJECTS: &str = "
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_GROUP
350 D
3 ACAD_IMAGE_DICT
350 50
3 ACME_DATA
350 40
3 ACME_NESTED
350 41
3 AcDbVariableDictionary
350 43
0 DICTIONARY
5 D
102 {ACAD_REACTORS
330 C
102 }
330 C
100 AcDbDictionary
281 1
0 XRECORD
5 40
102 {ACAD_REACTORS
330 C
102 }
330 C
100 AcDbXrecord
280 1
1 top-level-xrecord
70 7
40 2.5
0 DICTIONARY
5 41
330 C
100 AcDbDictionary
281 1
3 INNER
350 42
0 ACME_OBJECT
5 42
102 {ACAD_REACTORS
330 41
330 2A
102 }
330 41
100 AcDbAcmeObject
1 nested-object
340 2A
341 11
0 DICTIONARY
5 43
330 C
100 AcDbDictionary
281 1
3 ACMEVAR
350 44
0 DICTIONARYVAR
5 44
102 {ACAD_REACTORS
330 43
102 }
330 43
100 DictionaryVariables
280 0
1 var-value
0 DICTIONARY
5 2B
330 2A
100 AcDbDictionary
280 1
281 1
3 ACME_XREC
360 2D
0 XRECORD
5 2D
330 2B
100 AcDbXrecord
280 1
1 entity-xdict-data
0 DICTIONARY
5 60
330 2
100 AcDbDictionary
281 1
3 ACAD_LAYERSTATES
360 61
0 DICTIONARY
5 61
330 60
100 AcDbDictionary
281 1
3 Plan
350 62
0 XRECORD
5 62
102 {ACAD_REACTORS
330 61
102 }
330 61
100 AcDbXrecord
280 1
91 0
301 Plan
8 WALLS
90 0
62 1
0 DICTIONARY
5 63
330 10
100 AcDbDictionary
281 1
3 ACME_LAYER
360 64
0 XRECORD
5 64
330 63
100 AcDbXrecord
1 layer-xdict-data
0 DICTIONARY
5 65
330 1D
100 AcDbDictionary
281 1
3 ACME_BLOCK
360 66
0 XRECORD
5 66
330 65
100 AcDbXrecord
1 block-xdict-data
0 DICTIONARY
5 50
330 C
100 AcDbDictionary
281 1
3 logo
350 51
0 IMAGEDEF
5 51
102 {ACAD_REACTORS
330 50
330 52
102 }
330 50
100 AcDbRasterImageDef
90 0
1 images/logo.png
10 640
20 480
11 0.25
21 0.25
280 1
281 5
0 IMAGEDEF_REACTOR
5 52
330 34
100 AcDbRasterImageDefReactor
90 2
330 34
0 ENDSEC
0 EOF
";

fn fixture() -> String {
    dxf(&[HEADER, TABLES, BLOCKS, ENTITIES, OBJECTS].concat())
}

/// A written file, record by record: (section, record type, groups after the `0`).
struct Out {
    recs: Vec<(String, String, Vec<Tag>)>,
}

impl Out {
    fn new(text: &str) -> Out {
        let tags = cadcraft_dxf::parse(text.as_bytes()).expect("parse written DXF");
        let mut recs = Vec::new();
        for s in sections(&tags) {
            for (k, t) in records(&s.tags) {
                recs.push((s.name.clone(), k, t));
            }
        }
        Out { recs }
    }
    fn of(&self, kind: &str) -> Vec<&[Tag]> {
        self.recs.iter().filter(|(_, k, _)| k == kind).map(|(_, _, t)| t.as_slice()).collect()
    }
    fn in_section(&self, section: &str, kind: &str) -> Vec<&[Tag]> {
        self.recs.iter().filter(|(s, k, _)| s == section && k == kind).map(|(_, _, t)| t.as_slice()).collect()
    }
    /// The record with handle `h` (group 5, or 105 for DIMSTYLE): (type, groups).
    fn handle(&self, h: &str) -> Option<(&str, &[Tag])> {
        self.recs
            .iter()
            .find(|(_, _, t)| t.iter().any(|x| (x.code == 5 || x.code == 105) && x.str().eq_ignore_ascii_case(h)))
            .map(|(_, k, t)| (k.as_str(), t.as_slice()))
    }
    /// The table record or block record of type `kind` named `name`.
    fn named(&self, kind: &str, name: &str) -> &[Tag] {
        self.of(kind).into_iter().find(|t| s(t, 2).eq_ignore_ascii_case(name)).unwrap_or_else(|| panic!("no {kind} {name}"))
    }
    fn root(&self) -> &[Tag] {
        self.in_section("OBJECTS", "DICTIONARY").into_iter().next().expect("named object dictionary")
    }
    /// The object a dictionary maps `key` to.
    fn entry(&self, dict: &[Tag], key: &str) -> Option<(&str, &[Tag])> {
        let h = entries(dict).into_iter().find(|(k, _)| k == key)?.1;
        self.handle(&h)
    }
    /// Every handle reference (330/340/350/360 groups, 102 groups and 1005 xdata) that is not
    /// null and names no record of the file.
    fn dangling(&self) -> Vec<String> {
        let handles: std::collections::HashSet<String> =
            self.recs.iter().flat_map(|(_, _, t)| t.iter().filter(|x| x.code == 5 || x.code == 105).map(|x| x.str().to_ascii_uppercase())).collect();
        let mut out = Vec::new();
        for (_, k, t) in &self.recs {
            for x in t.iter().filter(|x| matches!(x.code, 330..=369 | 1005)) {
                let v = x.str().to_ascii_uppercase();
                if v != "0" && !v.is_empty() && !handles.contains(&v) {
                    out.push(format!("{k} {} -> {v}", x.code));
                }
            }
        }
        out
    }
    fn counts(&self) -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for (_, k, _) in &self.recs {
            *m.entry(k.clone()).or_insert(0) += 1;
        }
        m
    }
}

fn s(tags: &[Tag], code: i32) -> String {
    tags.iter().find(|t| t.code == code).map(Tag::str).unwrap_or_default()
}

fn h(tags: &[Tag]) -> String {
    tags.iter().find(|t| t.code == 5 || t.code == 105).map(Tag::str).unwrap_or_default()
}

/// The owner: the first 330 outside `102` groups.
fn owner(tags: &[Tag]) -> String {
    let mut depth = 0;
    for t in tags {
        match (t.code, t.str().as_str()) {
            (102, v) if v.starts_with('{') => depth += 1,
            (102, "}") => depth -= 1,
            (330, v) if depth == 0 => return v.to_string(),
            _ => {}
        }
    }
    String::new()
}

/// The handles in a `102 {NAME` group.
fn group(tags: &[Tag], name: &str) -> Vec<String> {
    let start = tags.iter().position(|t| t.code == 102 && t.str() == format!("{{{name}"));
    let Some(start) = start else { return Vec::new() };
    tags[start + 1..].iter().take_while(|t| t.code != 102).map(Tag::str).collect()
}

/// Dictionary entries: (key, handle).
fn entries(tags: &[Tag]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key = None;
    for t in tags {
        match t.code {
            3 => key = Some(t.str()),
            350 | 360 => {
                if let Some(k) = key.take() {
                    out.push((k, t.str()));
                }
            }
            _ => {}
        }
    }
    out
}

/// The xdata of application `app`: the groups after its `1001` up to the next application.
fn xdata(tags: &[Tag], app: &str) -> Vec<(i32, String)> {
    let Some(start) = tags.iter().position(|t| t.code == 1001 && t.str() == app) else { return Vec::new() };
    tags[start + 1..].iter().take_while(|t| t.code != 1001).map(|t| (t.code, t.str())).collect()
}

/// Read the fixture, save, reopen and save again: both saves.
fn saves(src: &str) -> (Drawing, Out, Out) {
    let d = read_dxf(src.as_bytes()).expect("read fixture");
    let first = write_dxf(&d);
    let d2 = read_dxf(first.as_bytes()).expect("reopen");
    let second = write_dxf(&d2);
    let d3 = read_dxf(second.as_bytes()).expect("reopen again");
    (d3, Out::new(&first), Out::new(&second))
}

fn entity_by_text<'a>(o: &'a Out, kind: &str, text: &str) -> &'a [Tag] {
    o.of(kind).into_iter().find(|t| s(t, 1) == text).unwrap_or_else(|| panic!("no {kind} `{text}`"))
}

#[test]
fn unknown_entities_in_model_paper_and_blocks_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        assert_eq!(o.of("ACME_WIDGET").len(), 3);
        let model = entity_by_text(o, "ACME_WIDGET", "widget-in-model");
        assert_eq!(owner(model), h(o.named("BLOCK_RECORD", "*Model_Space")));
        assert_eq!(s(model, 8), "WALLS");
        // Its pointers follow the objects they name.
        assert_eq!(o.handle(&s(model, 340)).map(|r| r.0), Some("ACME_OBJECT"));
        assert_eq!(o.handle(&s(model, 341)).map(|r| s(r.1, 2)), Some("WALLS".into()));
        let paper = entity_by_text(o, "ACME_WIDGET", "widget-in-paper");
        assert_eq!(s(paper, 67), "1");
        assert_eq!(owner(paper), h(o.named("BLOCK_RECORD", "*Paper_Space")));
        let in_block = entity_by_text(o, "ACME_WIDGET", "widget-in-block");
        assert_eq!(owner(in_block), h(o.named("BLOCK_RECORD", "DOOR")));
        // It keeps its place in the drawing order: after the line, before the block reference.
        let order: Vec<String> = o.in_section("ENTITIES", "LINE").into_iter().chain(o.in_section("ENTITIES", "ACME_WIDGET")).map(h).collect();
        let pos = |x: &str| o.recs.iter().position(|(_, _, t)| h(t) == x);
        assert!(order.contains(&"30".to_string()) && pos("2A") < pos("30") && pos("30") < pos("33"), "order");
        // Its class is declared.
        assert!(o.of("CLASS").iter().any(|c| s(c, 1) == "ACME_WIDGET"), "ACME_WIDGET class");
    }
}

#[test]
fn unknown_objects_in_named_and_nested_dictionaries_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let root = o.root();
        let (kind, x) = o.entry(root, "ACME_DATA").expect("ACME_DATA entry");
        assert_eq!(kind, "XRECORD");
        assert_eq!((s(x, 1), s(x, 70), s(x, 40)), ("top-level-xrecord".into(), "7".into(), "2.5".into()));
        assert_eq!(owner(x), h(root));
        let (kind, nested) = o.entry(root, "ACME_NESTED").expect("ACME_NESTED entry");
        assert_eq!(kind, "DICTIONARY");
        let (kind, obj) = o.entry(nested, "INNER").expect("INNER entry");
        assert_eq!(kind, "ACME_OBJECT");
        assert_eq!(s(obj, 1), "nested-object");
        assert_eq!(owner(obj), h(nested));
        assert!(o.of("CLASS").iter().any(|c| s(c, 1) == "ACME_OBJECT"), "ACME_OBJECT class");
        // CADCraft's own dictionaries are not doubled.
        let keys: Vec<String> = entries(root).into_iter().map(|e| e.0).collect();
        for k in ["ACAD_GROUP", "ACAD_IMAGE_DICT"] {
            assert_eq!(keys.iter().filter(|x| *x == k).count(), 1, "{k} in {keys:?}");
        }
    }
}

#[test]
fn foreign_xdata_on_entities_tables_and_blocks_is_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let line = o.handle("2A").expect("line").1;
        let acme = xdata(line, "ACME");
        assert_eq!(acme.get(..2), Some(&[(1000, "hello".to_string()), (1070, "42".to_string())][..]));
        let target = acme.get(2).map(|x| x.1.clone()).unwrap_or_default();
        assert_eq!(o.handle(&target).map(|r| r.0), Some("ACME_OBJECT"), "1005 handle follows its object");
        assert_eq!(xdata(o.named("LAYER", "WALLS"), "ACME"), vec![(1000, "layer-data".into()), (1070, "7".into())]);
        assert_eq!(xdata(o.named("BLOCK_RECORD", "DOOR"), "ACME"), vec![(1000, "block-record-data".into())]);
        assert_eq!(xdata(o.named("BLOCK", "DOOR"), "ACME"), vec![(1000, "block-entity-data".into())]);
        let in_block = o.handle("26").expect("line in block").1;
        assert_eq!(xdata(in_block, "ACME"), vec![(1000, "in-block".into())]);
        // Every application used has its APPID record.
        let apps: Vec<String> = o.of("APPID").iter().map(|r| s(r, 2)).collect();
        for app in ["ACME", "PE_URL"] {
            assert_eq!(apps.iter().filter(|a| *a == app).count(), 1, "{app} in {apps:?}");
        }
    }
}

#[test]
fn extension_dictionaries_on_entities_and_table_records_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let check = |rec: &[Tag], key: &str, text: &str| {
            let [xd] = group(rec, "ACAD_XDICTIONARY").try_into().unwrap_or_else(|g| panic!("one extension dictionary, got {g:?}"));
            let (kind, dict) = o.handle(&xd).expect("extension dictionary");
            assert_eq!(kind, "DICTIONARY");
            assert_eq!(owner(dict), h(rec), "the extension dictionary's owner");
            let (_, x) = o.entry(dict, key).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(s(x, 1), text);
        };
        check(o.handle("2A").expect("line").1, "ACME_XREC", "entity-xdict-data");
        check(o.named("LAYER", "0"), "ACME_LAYER", "layer-xdict-data");
        check(o.named("BLOCK_RECORD", "DOOR"), "ACME_BLOCK", "block-xdict-data");
    }
}

#[test]
fn image_imagedef_and_reactor_are_kept() {
    let (d, a, b) = saves(&fixture());
    let img = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Image(i) => Some(i.clone()),
        _ => None,
    });
    assert_eq!(img.map(|i| (i.path, i.name)), Some(("images/logo.png".into(), "logo".into())));
    for o in [&a, &b] {
        assert_eq!(o.of("IMAGEDEF").len(), 1);
        assert_eq!(o.of("IMAGEDEF_REACTOR").len(), 1);
        let reactor = o.of("IMAGEDEF_REACTOR")[0];
        assert_eq!(o.handle(&owner(reactor)).map(|r| r.0), Some("IMAGE"));
    }
}

#[test]
fn ucs_layer_states_and_block_descriptions_are_kept() {
    let (d, a, b) = saves(&fixture());
    let u = d.ucss.iter().find(|u| u.name == "TILTED").expect("UCS TILTED");
    assert_eq!((u.origin.x, u.origin.y, u.origin.z), (1.0, 2.0, 3.0));
    assert_eq!((u.x_axis.y, u.y_axis.x), (1.0, -1.0));
    assert_eq!(d.block("DOOR").map(|b| b.description.as_str()), Some("A door leaf"));
    for o in [&a, &b] {
        // Layer states live in the LAYER table's extension dictionary.
        let table = o.of("TABLE").into_iter().find(|t| s(t, 2) == "LAYER").expect("LAYER table");
        let [xd] = group(table, "ACAD_XDICTIONARY").try_into().expect("LAYER table extension dictionary");
        let (_, dict) = o.handle(&xd).expect("dictionary");
        let (_, states) = o.entry(dict, "ACAD_LAYERSTATES").expect("ACAD_LAYERSTATES");
        let (_, plan) = o.entry(states, "Plan").expect("layer state Plan");
        assert_eq!((s(plan, 301), s(plan, 8)), ("Plan".into(), "WALLS".into()));
    }
}

#[test]
fn true_colours_and_colour_books_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let line = o.handle("2A").expect("line").1;
        assert_eq!((s(line, 420), s(line, 430)), ("16711680".into(), "RAL$RAL 3000".into()));
        let walls = o.named("LAYER", "WALLS");
        assert_eq!((s(walls, 420), s(walls, 430)), ("13382451".into(), "RAL$RAL 3020".into()));
    }
}

#[test]
fn transparency_is_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        assert_eq!(s(o.handle("2A").expect("line").1, 440), "33554559");
    }
}

#[test]
fn hyperlinks_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let url = xdata(o.handle("2A").expect("line").1, "PE_URL");
        let want: Vec<(i32, String)> =
            [(1000, "https://example.com/spec"), (1002, "{"), (1000, "Specification"), (1002, "{"), (1071, "0"), (1002, "}"), (1002, "}")]
                .iter()
                .map(|(c, v)| (*c, v.to_string()))
                .collect();
        assert_eq!(url, want);
    }
}

#[test]
fn dictionary_variables_xrecords_and_persistent_reactors_are_kept() {
    let (_, a, b) = saves(&fixture());
    for o in [&a, &b] {
        let (_, vars) = o.entry(o.root(), "AcDbVariableDictionary").expect("variable dictionary");
        let (kind, var) = o.entry(vars, "ACMEVAR").expect("ACMEVAR");
        assert_eq!((kind, s(var, 1)), ("DICTIONARYVAR", "var-value".into()));
        // The line reacts to the ACME object, and the object to its dictionary and the line.
        let line = o.handle("2A").expect("line").1;
        let reactors = group(line, "ACAD_REACTORS");
        assert!(reactors.iter().any(|r| o.handle(r).is_some_and(|x| x.0 == "ACME_OBJECT")), "line reactors {reactors:?}");
        let (_, nested) = o.entry(o.root(), "ACME_NESTED").expect("ACME_NESTED");
        let (_, obj) = o.entry(nested, "INNER").expect("INNER");
        assert_eq!(group(obj, "ACAD_REACTORS"), vec![h(nested), "2A".to_string()]);
        assert_eq!(o.handle(&s(obj, 340)).map(|r| r.0), Some("LINE"));
    }
}

#[test]
fn every_reference_names_a_written_object() {
    let (_, a, b) = saves(&fixture());
    assert_eq!(a.dangling(), Vec::<String>::new());
    assert_eq!(b.dangling(), Vec::<String>::new());
}

#[test]
fn saving_twice_writes_the_same_records() {
    let (_, a, b) = saves(&fixture());
    assert_eq!(a.counts(), b.counts());
}

/// CLASS group 91, the instance count, came with the 2004 format (DXF Reference, CLASSES). A
/// 2004+ save counts the records of every class, generated and kept; an R2000 save has none,
/// even for a kept class whose record had one (AutoCAD LT 2026 stops responding on an R2000 file
/// whose CLASS records carry a 91).
#[test]
fn class_records_have_instance_counts_from_2004_only() {
    // The fixture's ACME_OBJECT class as a 2004+ writer has it, with its count.
    let src = fixture().replacen("ACME_OBJECT\n2\nAcmeObject\n3\nAcmeApp\n90\n0\n", "ACME_OBJECT\n2\nAcmeObject\n3\nAcmeApp\n90\n0\n91\n1\n", 1);
    assert_ne!(src, fixture());
    let (d, a, b) = saves(&src);
    for out in [&a, &b] {
        for class in out.in_section("CLASSES", "CLASS") {
            let codes: Vec<i32> = class.iter().map(|t| t.code).collect();
            assert_eq!(codes, [1, 2, 3, 90, 280, 281], "R2000 CLASS {}", s(class, 1));
        }
        assert!(out.in_section("CLASSES", "CLASS").iter().any(|c| s(c, 1) == "ACME_OBJECT"));
    }
    // Binary files the same way: none at R2000, the count at 2018.
    let acme = |version: Version| {
        let bytes = write_with(&d, "x.dxf", &SaveOptions { version: Some(version), binary: true }).expect("save binary");
        let tags = cadcraft_dxf::parse(&bytes).expect("parse binary");
        let class = sections(&tags)
            .into_iter()
            .filter(|sec| sec.name == "CLASSES")
            .flat_map(|sec| records(&sec.tags))
            .find(|(kind, class)| kind == "CLASS" && s(class, 1) == "ACME_OBJECT")
            .map(|(_, class)| class)
            .expect("ACME_OBJECT CLASS");
        let codes: Vec<i32> = class.iter().map(|t| t.code).collect();
        (codes, class.iter().find(|t| t.code == 91).map(Tag::i64))
    };
    assert_eq!(acme(Version::R2000), (vec![1, 2, 3, 90, 280, 281], None), "binary R2000 ACME_OBJECT CLASS");
    assert_eq!(acme(Version::R2018), (vec![1, 2, 3, 90, 91, 280, 281], Some(1)), "binary 2018 ACME_OBJECT CLASS");
    let r2018 = write_with(&d, "x.dxf", &SaveOptions { version: Some(Version::R2018), binary: false }).expect("save 2018");
    let out = Out::new(std::str::from_utf8(&r2018).expect("UTF-8"));
    let counts = out.counts();
    for class in out.in_section("CLASSES", "CLASS") {
        let name = s(class, 1);
        let codes: Vec<i32> = class.iter().map(|t| t.code).collect();
        assert_eq!(codes, [1, 2, 3, 90, 91, 280, 281], "2018 CLASS {name}");
        let instance_count = class.iter().find(|t| t.code == 91).map(Tag::i64);
        assert_eq!(instance_count, Some(counts.get(&name).map_or(0, |n| *n as i64)), "CLASS {name}");
    }
    assert_eq!(counts.get("ACME_OBJECT"), Some(&1));
}

#[test]
fn erased_entities_take_their_extension_data_with_them() {
    let mut d = read_dxf(fixture().as_bytes()).expect("read");
    let line = cadcraft_doc::Handle::parse_hex("2A").expect("handle");
    d.remove_entity(line).expect("erase the line");
    let o = Out::new(&write_dxf(&d));
    assert!(o.handle("2A").is_none());
    // Its extension dictionary is gone; the object that reacted to it no longer names it.
    assert!(o.of("XRECORD").iter().all(|x| s(x, 1) != "entity-xdict-data"));
    assert_eq!(o.dangling(), Vec::<String>::new());
    let (_, nested) = o.entry(o.root(), "ACME_NESTED").expect("ACME_NESTED");
    let (_, obj) = o.entry(nested, "INNER").expect("INNER");
    assert_eq!(s(obj, 340), "0", "a pointer to an erased entity becomes null");
}

#[test]
fn a_new_colour_drops_the_colour_book_name() {
    let mut d = read_dxf(fixture().as_bytes()).expect("read");
    let line = cadcraft_doc::Handle::parse_hex("2A").expect("handle");
    d.modify_entity(line, |e| e.common.color = cadcraft_color::Color::Index(3)).expect("recolour");
    if let Some(l) = d.layer_mut("WALLS") {
        l.color = cadcraft_color::Color::Index(5);
    }
    let o = Out::new(&write_dxf(&d));
    assert_eq!(s(o.handle("2A").expect("line").1, 430), "");
    assert_eq!(s(o.named("LAYER", "WALLS"), 430), "");
}

/// Hostile variants of the fixture: ownership cycles, references to nothing, oversized and
/// duplicated data. Reading and saving them never panics and writes only valid references.
#[test]
fn hostile_raw_data_never_panics() {
    let base = fixture();
    let variants: Vec<String> = vec![
        // An object that owns itself, and two that own each other.
        base.replace("5\n42\n102\n{ACAD_REACTORS\n330\n41\n", "5\n42\n102\n{ACAD_REACTORS\n330\n42\n")
            .replace("5\n2D\n330\n2B\n", "5\n2D\n330\n2D\n")
            .replace("5\n2B\n330\n2A\n", "5\n2B\n330\n2D\n"),
        // References to handles that don't exist.
        base.replace("340\n2A\n341\n11\n", "340\nFFFFF\n341\nZZ\n").replace("360\n2B\n", "360\nDEAD\n"),
        // Duplicate handles: an object with an entity's handle, an entity with an object's.
        base.replace("5\n40\n", "5\n2A\n").replace("5\n30\n", "5\n42\n"),
        // Unterminated 102 groups and stray xdata.
        base.replace("102\n}\n330\n1F\n", "330\n1F\n").replace("1001\nACME\n1000\nhello\n", "1000\nstray\n1001\n\n"),
        // Huge numbers of xdata groups and dictionary entries.
        base.replace("1000\nhello\n", &"1000\nhello\n".repeat(20_000)).replace("3\nINNER\n350\n42\n", &"3\nINNER\n350\n42\n".repeat(5_000)),
        // A handle seed below every handle, non-hex handles and absurdly large ones.
        base.replace("$HANDSEED\n5\n100\n", "$HANDSEED\n5\n1\n").replace("5\n44\n", "5\n-44\n").replace("5\n66\n", "5\nFFFFFFFFFFFFFFFF\n"),
    ];
    for (i, v) in variants.iter().enumerate() {
        assert_ne!(v, &base, "variant {i} changes the fixture");
        let Ok(d) = read_dxf(v.as_bytes()) else { continue };
        let first = write_dxf(&d);
        let o = Out::new(&first);
        assert_eq!(o.dangling(), Vec::<String>::new(), "variant {i}");
        let again = read_dxf(first.as_bytes()).expect("reopen");
        let _ = write_dxf(&again);
    }
}

/// Through the DWG bridge (acadrust): what the DWG writer and reader keep of the same data.
/// Entities and objects of custom classes, xdata on table records and blocks, entities' colour
/// book names and reactors are not kept there (acadrust gaps); the rest is.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dwg_keeps_extension_dictionaries_xrecords_and_entity_xdata() {
    let via_dwg = |d: &Drawing| cadcraft_io::read(&cadcraft_io::write(d, "x.dwg").expect("write DWG"), "x.dwg").expect("read DWG");
    let d = via_dwg(&via_dwg(&read_dxf(fixture().as_bytes()).expect("read fixture")));
    let o = Out::new(&write_dxf(&d));
    assert_eq!(o.dangling(), Vec::<String>::new());
    let line = o.handle("2A").expect("line").1;
    assert_eq!(xdata(line, "ACME").get(..2), Some(&[(1000, "hello".to_string()), (1070, "42".to_string())][..]));
    assert_eq!(xdata(line, "PE_URL").first(), Some(&(1000, "https://example.com/spec".to_string())));
    assert_eq!(s(line, 440), "33554559");
    let [xd] = group(line, "ACAD_XDICTIONARY").try_into().expect("the line's extension dictionary");
    let (_, dict) = o.handle(&xd).expect("dictionary");
    assert_eq!(o.entry(dict, "ACME_XREC").map(|x| s(x.1, 1)), Some("entity-xdict-data".into()));
    assert_eq!(o.entry(o.root(), "ACME_DATA").map(|x| s(x.1, 1)), Some("top-level-xrecord".into()));
    let (_, vars) = o.entry(o.root(), "AcDbVariableDictionary").expect("variable dictionary");
    assert_eq!(o.entry(vars, "ACMEVAR").map(|x| (x.0, s(x.1, 1))), Some(("DICTIONARYVAR", "var-value".into())));
    let table = o.of("TABLE").into_iter().find(|t| s(t, 2) == "LAYER").expect("LAYER table");
    assert_eq!(group(table, "ACAD_XDICTIONARY").len(), 1, "layer states");
    assert_eq!(s(o.named("LAYER", "WALLS"), 430), "RAL$RAL 3020");
    assert_eq!(d.block("DOOR").map(|b| b.description.as_str()), Some("A door leaf"));
    assert!(d.ucss.iter().any(|u| u.name == "TILTED"));
}
