//! An annotative block survives a save: its BLOCK_RECORD keeps the `AcadAnnotative` xdata and
//! uniform scaling (281), and a reference keeps its annotation scale contexts (the reference's
//! scale at each annotation scale), pointing at the same scales and following the reference when
//! it is moved, turned or scaled. Without them AutoCAD scales the reference by its units, or once
//! more by the current annotation scale.
//!
//! The fixture is synthetic: block SIGN (metres) with one line, a reference at (10, 20) scaled 5,
//! and its contexts for 1:5 (default, scale 5) and 1:1 (scale 1).

use cadcraft_doc::geom::Vec2;
use cadcraft_doc::{Drawing, Handle};
use cadcraft_io::{read_dxf, write_dxf};

const FIXTURE: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1032
9 $INSUNITS
70 6
0 ENDSEC
0 SECTION
2 TABLES
0 TABLE
2 BLOCK_RECORD
5 1
330 0
100 AcDbSymbolTable
70 1
0 BLOCK_RECORD
5 1F
330 1
100 AcDbSymbolTableRecord
100 AcDbBlockTableRecord
2 SIGN
70 6
280 1
281 1
1001 AcadAnnotative
1000 AnnotativeData
1002 {
1070 1
1070 1
1002 }
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
2 SIGN
70 0
10 0
20 0
30 0
3 SIGN
1
0 LINE
5 21
330 1F
100 AcDbEntity
8 0
100 AcDbLine
10 0
20 0
30 0
11 1
21 0
31 0
0 ENDBLK
5 22
330 1F
100 AcDbEntity
8 0
100 AcDbBlockEnd
0 ENDSEC
0 SECTION
2 ENTITIES
0 INSERT
5 40
102 {ACAD_XDICTIONARY
360 41
102 }
330 1F
100 AcDbEntity
8 0
100 AcDbBlockReference
2 SIGN
10 10
20 20
30 0
41 5
42 5
43 5
50 0
66 1
0 ATTRIB
5 70
102 {ACAD_XDICTIONARY
360 71
102 }
330 40
100 AcDbEntity
8 0
100 AcDbText
10 15
20 30
30 0
40 1
1 X
100 AcDbAttribute
2 TAG
70 0
0 SEQEND
5 77
330 40
100 AcDbEntity
8 0
0 ENDSEC
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_SCALELIST
350 50
0 DICTIONARY
5 50
330 C
100 AcDbDictionary
281 1
3 A0
350 51
3 A1
350 52
0 SCALE
5 51
330 50
100 AcDbScale
70 0
300 1:1
140 1
141 1
290 0
0 SCALE
5 52
330 50
100 AcDbScale
70 0
300 1:5
140 1
141 5
290 0
0 DICTIONARY
5 41
330 40
100 AcDbDictionary
280 1
281 1
3 AcDbContextDataManager
360 42
0 DICTIONARY
5 42
330 41
100 AcDbDictionary
281 1
3 ACDB_ANNOTATIONSCALES
350 43
0 DICTIONARY
5 43
330 42
100 AcDbDictionary
281 1
3 *A1
350 44
3 *A2
350 45
0 ACDB_BLKREFOBJECTCONTEXTDATA_CLASS
5 44
330 43
100 AcDbObjectContextData
70 4
290 1
100 AcDbAnnotScaleObjectContextData
340 52
50 0
10 10
20 20
30 0
41 5
42 5
43 5
0 ACDB_BLKREFOBJECTCONTEXTDATA_CLASS
5 45
330 43
100 AcDbObjectContextData
70 4
290 0
100 AcDbAnnotScaleObjectContextData
340 51
50 0
10 10
20 20
30 0
41 1
42 1
43 1
0 DICTIONARY
5 71
330 70
100 AcDbDictionary
280 1
281 1
3 AcDbContextDataManager
360 73
0 DICTIONARY
5 73
330 71
100 AcDbDictionary
281 1
3 ACDB_ANNOTATIONSCALES
350 74
0 DICTIONARY
5 74
330 73
100 AcDbDictionary
281 1
3 *A1
350 75
3 *A2
350 76
0 ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS
5 75
330 74
100 AcDbObjectContextData
70 4
290 1
100 AcDbAnnotScaleObjectContextData
340 52
70 0
50 0
10 15
20 30
11 15
21 30
290 0
0 ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS
5 76
330 74
100 AcDbObjectContextData
70 4
290 0
100 AcDbAnnotScaleObjectContextData
340 51
70 0
50 0
10 11
20 22
11 11
21 22
290 0
0 ENDSEC
0 EOF
";

fn dxf(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (code, value) = l.split_once(' ').unwrap_or((l, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// A written file's records: (type, groups).
fn records(text: &str) -> Vec<(String, Vec<(i32, String)>)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for pair in lines.chunks(2) {
        let (Some(code), Some(value)) = (pair.first().and_then(|c| c.parse::<i32>().ok()), pair.get(1)) else { continue };
        if code == 0 {
            out.push((value.to_string(), Vec::new()));
        } else if let Some((_, tags)) = out.last_mut() {
            tags.push((code, value.to_string()));
        }
    }
    out
}

fn get(tags: &[(i32, String)], code: i32) -> Option<&str> {
    tags.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

/// The written block reference contexts: (scale name, default, insertion, rotation, x scale).
fn contexts(text: &str) -> Vec<(String, bool, Vec2, f64, f64)> {
    let recs = records(text);
    let scales: std::collections::HashMap<&str, &str> =
        recs.iter().filter(|(k, _)| k == "SCALE").filter_map(|(_, t)| Some((get(t, 5)?, get(t, 300)?))).collect();
    let num = |t: &[(i32, String)], c: i32| get(t, c).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
    recs.iter()
        .filter(|(k, _)| k == "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")
        .map(|(_, t)| {
            let scale = get(t, 340).and_then(|h| scales.get(h)).map_or("?", |s| s).to_string();
            (scale, get(t, 290) == Some("1"), Vec2::new(num(t, 10), num(t, 20)), num(t, 50), num(t, 41))
        })
        .collect()
}

/// The written attribute contexts: (scale name, insertion point), and the ATTRIB they hang on
/// (its tag, through its extension dictionary).
fn attribute_contexts(text: &str) -> Vec<(String, Vec2)> {
    let recs = records(text);
    let scales: std::collections::HashMap<&str, &str> =
        recs.iter().filter(|(k, _)| k == "SCALE").filter_map(|(_, t)| Some((get(t, 5)?, get(t, 300)?))).collect();
    let num = |t: &[(i32, String)], c: i32| get(t, c).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
    recs.iter()
        .filter(|(k, _)| k == "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS")
        .map(|(_, t)| (get(t, 340).and_then(|h| scales.get(h)).map_or("?", |s| s).to_string(), Vec2::new(num(t, 10), num(t, 20))))
        .collect()
}

fn read() -> Drawing {
    read_dxf(dxf(FIXTURE).as_bytes()).unwrap()
}

#[test]
fn an_annotative_block_keeps_its_record_and_its_reference_s_contexts() {
    let d = read();
    let b = d.block("SIGN").unwrap();
    assert!(b.annotative && b.scale_uniformly && b.units == 6);
    let text = write_dxf(&d);
    // The record: units, uniform scaling and the annotative flag.
    let recs = records(&text);
    let (_, rec) = recs.iter().find(|(k, t)| k == "BLOCK_RECORD" && get(t, 2) == Some("SIGN")).unwrap();
    assert_eq!((get(rec, 70), get(rec, 281)), (Some("6"), Some("1")));
    assert!(rec.iter().any(|(c, v)| *c == 1001 && v == "AcadAnnotative"), "{rec:?}");
    // The contexts, as read, at the same scales.
    let mut ctx = contexts(&text);
    ctx.sort_by(|a, b| a.0.cmp(&b.0));
    let got: Vec<(String, bool, f64)> = ctx.iter().map(|c| (c.0.clone(), c.1, c.4)).collect();
    assert_eq!(got, [("1:1".to_string(), false, 1.0), ("1:5".to_string(), true, 5.0)]);
    assert!(ctx.iter().all(|c| c.2 == Vec2::new(10.0, 20.0) && c.3 == 0.0));
    // Read back: still annotative.
    assert!(read_dxf(text.as_bytes()).unwrap().block("SIGN").unwrap().annotative);
    // The attribute's contexts, on the written attribute (its handle is new), as read.
    let mut actx = attribute_contexts(&text);
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(actx, [("1:1".to_string(), Vec2::new(11.0, 22.0)), ("1:5".to_string(), Vec2::new(15.0, 30.0))]);
    let recs = records(&text);
    let (_, attrib) = recs.iter().find(|(k, t)| k == "ATTRIB" && get(t, 2) == Some("TAG")).unwrap();
    assert!(attrib.iter().any(|(c, v)| *c == 102 && v == "{ACAD_XDICTIONARY"), "{attrib:?}");
}

#[test]
fn an_attribute_s_contexts_follow_its_reference() {
    // Moved 5 along x and scaled 2 (the attribute with it, as MOVE and SCALE move it): at 1:5
    // the attribute goes to (15 + 10, 20 + 20); at 1:1, through the reference's 1:1 context (now
    // at (15, 20), scale 2), to (15 + 2, 20 + 4).
    let mut d = read();
    let r = Handle::parse_hex("40").unwrap();
    d.modify_entity(r, |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            i.insert = i.insert + cadcraft_doc::geom::Vec3::new(5.0, 0.0, 0.0);
            i.scale = i.scale * 2.0;
            for a in &mut i.attribs {
                a.text.insert = cadcraft_doc::geom::Vec3::new(25.0, 40.0, 0.0);
                a.text.align_pt = Some(a.text.insert);
            }
        }
    })
    .unwrap();
    let mut actx = attribute_contexts(&write_dxf(&d));
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    let near = |p: Vec2, x: f64, y: f64| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
    assert_eq!(actx.len(), 2);
    assert!(near(actx[0].1, 17.0, 24.0), "1:1 {:?}", actx[0]);
    assert!(near(actx[1].1, 25.0, 40.0), "1:5 {:?}", actx[1]);
}

#[test]
fn a_reference_s_contexts_follow_it_when_it_is_moved_turned_and_scaled() {
    let mut d = read();
    let r = Handle::parse_hex("40").unwrap();
    d.modify_entity(r, |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            i.insert = i.insert + cadcraft_doc::geom::Vec3::new(5.0, 0.0, 0.0);
            i.rotation = 30f64.to_radians();
            i.scale = i.scale * 2.0;
        }
    })
    .unwrap();
    let mut ctx = contexts(&write_dxf(&d));
    ctx.sort_by(|a, b| a.0.cmp(&b.0));
    for (scale, _, at, rot, sx) in &ctx {
        assert!((at.x - 15.0).abs() < 1e-9 && (at.y - 20.0).abs() < 1e-9, "{scale}: {at:?}");
        assert!((rot - 30.0).abs() < 1e-9, "{scale}: {rot}");
        // Each context doubles: 1 → 2 at 1:1, 5 → 10 at 1:5.
        let want = if scale == "1:1" { 2.0 } else { 10.0 };
        assert!((sx - want).abs() < 1e-9, "{scale}: {sx}");
    }
    assert_eq!(ctx.len(), 2);
}

/// A reference's annotation contexts as CADCraft's model keeps them: (scale's SCALE handle,
/// rotation, x scale) of each, by the order kept.
fn kept_contexts(d: &Drawing) -> Vec<(f64, f64)> {
    let num = |o: &cadcraft_doc::foreign::ForeignObject, c: i32| {
        o.tags.iter().find(|t| t.code == c).and_then(|t| t.value.trim().parse::<f64>().ok()).unwrap_or(f64::NAN)
    };
    d.foreign.objects.iter().filter(|o| o.kind == "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS").map(|o| (num(o, 50), num(o, 41))).collect()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_reference_s_contexts_survive_dwg() {
    // Turned 30° first, so the rotation's unit shows (DXF degrees, DWG radians).
    let mut d = read();
    let r = Handle::parse_hex("40").unwrap();
    d.modify_entity(r, |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            i.rotation = 30f64.to_radians();
        }
    })
    .unwrap();
    let back =
        cadcraft_io::read(&cadcraft_io::read_dxf(write_dxf(&d).as_bytes()).map(|d| cadcraft_io::write(&d, "x.dwg").unwrap()).unwrap(), "x.dwg")
            .unwrap();
    let mut ctx = kept_contexts(&back);
    ctx.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(ctx.len(), 2, "{ctx:?}");
    for ((rot, sx), want) in ctx.iter().zip([1.0, 5.0]) {
        assert!((rot - 30.0).abs() < 1e-9 && (sx - want).abs() < 1e-9, "{ctx:?}");
    }
}
