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

/// Every written ACDB_ANNOTATIONSCALES dictionary entry names a written context, and every
/// written context is listed once.
fn listed_contexts(text: &str) -> usize {
    let recs = records(text);
    let handles: std::collections::HashSet<&str> =
        recs.iter().filter(|(k, _)| k.ends_with("OBJECTCONTEXTDATA_CLASS")).filter_map(|(_, t)| get(t, 5)).collect();
    let mut listed = 0;
    for (_, t) in recs.iter().filter(|(k, _)| k == "DICTIONARY") {
        for (c, v) in t {
            if *c == 350 && handles.contains(v.as_str()) {
                listed += 1;
            }
        }
    }
    assert_eq!(listed, handles.len(), "contexts listed in their dictionaries");
    listed
}

#[test]
fn a_context_whose_scale_was_deleted_is_not_written() {
    let mut d = read();
    d.scales.retain(|s| s.name != "1:1");
    let text = write_dxf(&d);
    let ctx = contexts(&text);
    assert_eq!(ctx.iter().map(|c| (c.0.as_str(), c.1)).collect::<Vec<_>>(), [("1:5", true)]);
    let actx = attribute_contexts(&text);
    assert_eq!(actx.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["1:5"]);
    assert_eq!(listed_contexts(&text), 2);
}

#[test]
fn an_annotative_reference_s_contexts_follow_its_scales() {
    // CADCraft's model makes the reference annotative at 1:5 and a new 1:2 (1:1 no longer):
    // the 1:1 contexts go, 1:2 ones are made from the default 1:5 ones, scaled 2/5 about the
    // reference's insertion point.
    let mut d = read();
    let half = cadcraft_doc::AnnoScale::new("1:2", 1.0, 2.0);
    d.scales.push(half.clone());
    let five = d.scales.iter().find(|s| s.name == "1:5").cloned().unwrap();
    d.modify_entity(Handle::parse_hex("40").unwrap(), |e| e.common.anno_scales = vec![five, half]).unwrap();
    let text = write_dxf(&d);
    let mut ctx = contexts(&text);
    ctx.sort_by(|a, b| a.0.cmp(&b.0));
    let got: Vec<(String, bool, f64)> = ctx.iter().map(|c| (c.0.clone(), c.1, c.4)).collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!((got[0].0.as_str(), got[0].1), ("1:2", false));
    assert!((got[0].2 - 2.0).abs() < 1e-9, "{got:?}");
    assert_eq!(got[1], ("1:5".to_string(), true, 5.0));
    assert!(ctx.iter().all(|c| c.2 == Vec2::new(10.0, 20.0)));
    let mut actx = attribute_contexts(&text);
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(actx.len(), 2, "{actx:?}");
    assert_eq!(actx[0].0, "1:2");
    assert!((actx[0].1 - Vec2::new(12.0, 24.0)).len() < 1e-9, "{actx:?}");
    assert_eq!(actx[1], ("1:5".to_string(), Vec2::new(15.0, 30.0)));
    assert_eq!(listed_contexts(&text), 4);
    // Read back, the reference keeps them (and nothing points at a missing scale).
    let back = read_dxf(text.as_bytes()).unwrap();
    assert!(contexts(&write_dxf(&back)).iter().all(|c| c.0 != "?"));
}

/// The fixture with every handle moved up to just past 2^48 (AutoCAD writes handles that high):
/// the reference (40) lands on 2^48 + 1.
fn far_handles(src: &str) -> String {
    const SHIFT: u64 = (1 << 48) - 0x3F;
    let is_handle = |code: i32| matches!(code, 5 | 105 | 320..=369 | 390..=399 | 480 | 481 | 1005);
    let mut out = String::new();
    for line in src.lines() {
        let (code, value) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        let moved = match (code.parse::<i32>(), u64::from_str_radix(value, 16)) {
            (Ok(c), Ok(h)) if is_handle(c) && h != 0 => format!("{:X}", h + SHIFT),
            _ => value.to_string(),
        };
        out.push_str(&format!("{code} {moved}\n"));
    }
    out
}

#[test]
fn new_contexts_get_their_own_handles_when_the_file_s_are_above_2_pow_48() {
    // As in the test above, but every file handle is above 2^48: the new 1:2 contexts and their
    // scale take handles no other object has.
    let mut d = read_dxf(dxf(&far_handles(FIXTURE)).as_bytes()).unwrap();
    let half = cadcraft_doc::AnnoScale::new("1:2", 1.0, 2.0);
    d.scales.push(half.clone());
    let five = d.scales.iter().find(|s| s.name == "1:5").cloned().unwrap();
    let reference = Handle::parse_hex("1000000000001").unwrap();
    d.modify_entity(reference, |e| e.common.anno_scales = vec![five, half]).unwrap();
    let text = write_dxf(&d);
    let mut names: Vec<String> = contexts(&text).into_iter().map(|c| c.0).collect();
    names.sort();
    assert_eq!(names, ["1:2", "1:5"]);
    assert_eq!(attribute_contexts(&text).len(), 2);
    let mut handles: Vec<String> = records(&text).iter().filter_map(|(_, t)| get(t, 5).map(str::to_string)).collect();
    let n = handles.len();
    handles.sort();
    handles.dedup();
    assert_eq!(handles.len(), n, "every written handle is unique");
}

#[test]
fn two_attributes_with_one_tag_keep_their_own_contexts() {
    // A second attribute TAG on the reference, with its own extension dictionary (no contexts
    // in it): each written ATTRIB gets its own dictionary, the first keeps the contexts.
    let second = "
0 ATTRIB
5 80
102 {ACAD_XDICTIONARY
360 81
102 }
330 40
100 AcDbEntity
8 0
100 AcDbText
10 1
20 2
30 0
40 1
1 Y
100 AcDbAttribute
2 TAG
70 0
0 SEQEND";
    let objs = "
0 DICTIONARY
5 81
330 80
100 AcDbDictionary
280 1
281 1
3 OTHER
360 82
0 XRECORD
5 82
330 81
100 AcDbXrecord
1 kept
0 ENDSEC
0 EOF";
    let src = FIXTURE.replacen("0 SEQEND", second, 1);
    let at = src.rfind("0 ENDSEC").unwrap();
    let src = format!("{}{objs}", &src[..at]);
    let d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let text = write_dxf(&d);
    let recs = records(&text);
    let xdict = |t: &[(i32, String)]| {
        let i = t.iter().position(|(c, v)| *c == 102 && v == "{ACAD_XDICTIONARY")?;
        t.get(i + 1).map(|(_, v)| v.clone())
    };
    let attribs: Vec<(String, Option<String>)> =
        recs.iter().filter(|(k, _)| k == "ATTRIB").map(|(_, t)| (get(t, 1).unwrap_or_default().to_string(), xdict(t))).collect();
    assert_eq!(attribs.len(), 2, "{attribs:?}");
    let (x, y) = (attribs[0].1.clone().unwrap(), attribs[1].1.clone().unwrap());
    assert_ne!(x, y, "{attribs:?}");
    // The first attribute's dictionary leads to its contexts, the second's to the xrecord.
    let owner_of = |h: &str| recs.iter().find(|(_, t)| get(t, 5) == Some(h)).and_then(|(_, t)| get(t, 330)).map(str::to_string);
    let xrec = recs.iter().find(|(k, t)| k == "XRECORD" && get(t, 1) == Some("kept")).map(|(_, t)| get(t, 5).unwrap().to_string()).unwrap();
    assert_eq!(owner_of(&xrec), Some(y));
    assert_eq!(attribute_contexts(&text).len(), 2);
}

#[test]
fn new_contexts_come_from_one_whose_scale_remains() {
    // The default context's scale (1:5) is deleted, and the reference is annotative at 1:1 and a
    // new 1:2: the 1:2 contexts are made from the 1:1 ones (scale 1 → 2 about the insertion
    // point), and 1:1 becomes the default.
    let mut d = read();
    let half = cadcraft_doc::AnnoScale::new("1:2", 1.0, 2.0);
    d.scales.retain(|s| s.name != "1:5");
    d.scales.push(half.clone());
    let one = d.scales.iter().find(|s| s.name == "1:1").cloned().unwrap();
    d.modify_entity(Handle::parse_hex("40").unwrap(), |e| e.common.anno_scales = vec![one, half]).unwrap();
    let text = write_dxf(&d);
    let mut ctx = contexts(&text);
    ctx.sort_by(|a, b| a.0.cmp(&b.0));
    let got: Vec<(String, bool, f64)> = ctx.iter().map(|c| (c.0.clone(), c.1, c.4)).collect();
    assert_eq!(got, [("1:1".to_string(), true, 1.0), ("1:2".to_string(), false, 2.0)]);
    let mut actx = attribute_contexts(&text);
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(actx.len(), 2, "{actx:?}");
    assert_eq!(actx[0], ("1:1".to_string(), Vec2::new(11.0, 22.0)));
    assert_eq!(actx[1].0, "1:2");
    assert!((actx[1].1 - Vec2::new(12.0, 24.0)).len() < 1e-9, "{actx:?}");
    assert_eq!(listed_contexts(&text), 4);
}

/// The written context of the block reference at scale `name`: (insertion, rotation).
fn context_at(text: &str, name: &str) -> (Vec2, f64) {
    contexts(text).into_iter().find(|c| c.0 == name).map(|c| (c.2, c.3)).unwrap()
}

#[test]
fn an_unchanged_reference_keeps_its_contexts_when_its_default_one_is_elsewhere() {
    // The reference sits at (11, 20) while its default context says (10, 20): saved unchanged,
    // every context is written as read (what moves them is what the reference did since).
    let d = read_dxf(dxf(&FIXTURE.replace("2 SIGN\n10 10\n20 20\n30 0\n41 5", "2 SIGN\n10 11\n20 20\n30 0\n41 5")).as_bytes()).unwrap();
    let text = write_dxf(&d);
    for scale in ["1:1", "1:5"] {
        assert_eq!(context_at(&text, scale).0, Vec2::new(10.0, 20.0), "{scale}");
    }
    let mut actx = attribute_contexts(&text);
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(actx, [("1:1".to_string(), Vec2::new(11.0, 22.0)), ("1:5".to_string(), Vec2::new(15.0, 30.0))]);
}

#[test]
fn a_context_s_own_offset_scales_turns_and_mirrors_with_the_reference() {
    // The 1:1 context sits 2 to the right of the reference. Scaled 2 and turned 90°, it is 4
    // above; mirrored (x scale -5), 2 to the left.
    let src = FIXTURE.replace("340 51\n50 0\n10 10", "340 51\n50 0\n10 12");
    let edit = |f: &dyn Fn(&mut cadcraft_doc::Insert)| {
        let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
        d.modify_entity(Handle::parse_hex("40").unwrap(), |e| {
            if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                f(i);
            }
        })
        .unwrap();
        write_dxf(&d)
    };
    let near = |p: Vec2, x: f64, y: f64| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
    let text = edit(&|i| {
        i.scale = i.scale * 2.0;
        i.rotation = 90f64.to_radians();
    });
    let (at, rot) = context_at(&text, "1:1");
    assert!(near(at, 10.0, 24.0) && (rot - 90.0).abs() < 1e-9, "{at:?} {rot}");
    let text = edit(&|i| i.scale.x = -i.scale.x);
    let (at, _) = context_at(&text, "1:1");
    assert!(near(at, 8.0, 20.0), "{at:?}");
}

#[test]
fn the_default_context_is_the_one_whose_first_290_is_1() {
    // The 1:1 attribute context comes first, turned 10° in its reference, with its own later
    // 290 set: the 1:5 one is still the default, so an unchanged save keeps the 10°.
    let at = FIXTURE.find("0 ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS\n5 76").unwrap();
    let end = FIXTURE.find("0 ENDSEC\n0 EOF").unwrap();
    let second = FIXTURE[at..end].replace("50 0\n10 11", "50 10\n10 11").replace("21 22\n290 0", "21 22\n290 1");
    let first = FIXTURE.find("0 ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS\n5 75").unwrap();
    let src = format!("{}{second}{}{}", &FIXTURE[..first], &FIXTURE[first..at], &FIXTURE[end..]);
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    // Moved, so the attribute contexts are brought up to date.
    d.modify_entity(Handle::parse_hex("40").unwrap(), |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            i.insert = i.insert + cadcraft_doc::geom::Vec3::new(5.0, 0.0, 0.0);
            for a in &mut i.attribs {
                a.text.insert = a.text.insert + cadcraft_doc::geom::Vec3::new(5.0, 0.0, 0.0);
                a.text.align_pt = Some(a.text.insert);
            }
        }
    })
    .unwrap();
    let text = write_dxf(&d);
    let recs = records(&text);
    let scales: std::collections::HashMap<&str, &str> =
        recs.iter().filter(|(k, _)| k == "SCALE").filter_map(|(_, t)| Some((get(t, 5)?, get(t, 300)?))).collect();
    let rot = recs
        .iter()
        .filter(|(k, _)| k == "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS")
        .find(|(_, t)| get(t, 340).and_then(|h| scales.get(h)) == Some(&"1:1"))
        .and_then(|(_, t)| get(t, 50))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap();
    assert!((rot - 10.0).abs() < 1e-9, "{rot}");
}

#[test]
fn an_unchanged_attribute_turned_at_the_current_scale_keeps_its_contexts_rotation() {
    // The ATTRIB is turned 20° (it is the attribute at the file's current scale) while its default
    // context says 0°: saved unchanged, no context turns (what turns them is what the attribute
    // did since it was read).
    let d = read_dxf(dxf(&FIXTURE.replace("1 X\n", "1 X\n50 20\n")).as_bytes()).unwrap();
    let text = write_dxf(&d);
    let rots: Vec<f64> = records(&text)
        .iter()
        .filter(|(k, _)| k == "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS")
        .filter_map(|(_, t)| get(t, 50).and_then(|v| v.parse::<f64>().ok()))
        .collect();
    assert_eq!(rots.len(), 2, "{rots:?}");
    assert!(rots.iter().all(|r| r.abs() < 1e-9), "{rots:?}");
}

#[test]
fn an_attribute_s_contexts_stay_when_the_reference_s_attributes_are_reordered() {
    // A second attribute OTHER after TAG, then the reference's attributes reversed (as a block
    // redefinition's attribute sync can): TAG is found as read by its tag, so saved otherwise
    // unchanged its contexts don't move.
    let other = "1 X\n100 AcDbAttribute\n2 TAG\n70 0\n0 ATTRIB\n5 80\n330 40\n100 AcDbEntity\n8 0\n100 AcDbText\n10 1\n20 2\n30 0\n40 1\n1 Y\n100 AcDbAttribute\n2 OTHER\n70 0\n";
    let src = FIXTURE.replacen("1 X\n100 AcDbAttribute\n2 TAG\n70 0\n", other, 1);
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    d.modify_entity(Handle::parse_hex("40").unwrap(), |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            assert_eq!(i.attribs.len(), 2);
            i.attribs.reverse();
        }
    })
    .unwrap();
    let mut actx = attribute_contexts(&write_dxf(&d));
    actx.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(actx, [("1:1".to_string(), Vec2::new(11.0, 22.0)), ("1:5".to_string(), Vec2::new(15.0, 30.0))]);
}
