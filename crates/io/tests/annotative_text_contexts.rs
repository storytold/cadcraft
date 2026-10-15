//! Annotative text, MText and dimensions survive a save with their annotation scales: each is
//! written at its default scale with AutoCAD's `AcadAnnotative` xdata and one annotation scale
//! context per scale (a dimension's naming its geometry block at that scale), and read back from
//! the contexts at paper size. Without them AutoCAD shows the objects at one size at every scale,
//! and CADCraft read them as plain objects at the default scale's size.
//!
//! The fixture is synthetic: a TEXT (right-justified), an MTEXT and an aligned DIMENSION, each
//! annotative at 1:2 (default) and 1:10, in AutoCAD's layout of the records.

use cadcraft_doc::{AnnoScale, Common, DimKind, Dimension, Drawing, EntityKind, HAlign, Handle, Space, Text, VAlign};
use cadcraft_geom::Vec3;

const FIXTURE: &str = "
0 SECTION
2 HEADER
9 $ACADVER
1 AC1032
9 $CANNOSCALE
1 1:2
0 ENDSEC
0 SECTION
2 ENTITIES
0 TEXT
5 30
102 {ACAD_XDICTIONARY
360 31
102 }
330 1F
100 AcDbEntity
8 0
100 AcDbText
10 16
20 0
30 0
40 2.5
1 RIGHT
72 2
11 30
21 0
31 0
100 AcDbText
1001 AcadAnnotative
1000 AnnotativeData
1002 {
1070 1
1070 1
1002 }
0 MTEXT
5 40
102 {ACAD_XDICTIONARY
360 41
102 }
330 1F
100 AcDbEntity
8 0
100 AcDbMText
10 0
20 10
30 0
40 2.5
41 20
71 1
72 5
1 Multi
1001 AcadAnnotative
1000 AnnotativeData
1002 {
1070 1
1070 1
1002 }
0 DIMENSION
5 50
102 {ACAD_XDICTIONARY
360 51
102 }
330 1F
100 AcDbEntity
8 0
100 AcDbDimension
2 *D1
10 10
20 25
30 0
11 5
21 25
31 0
70 33
3 Standard
100 AcDbAlignedDimension
13 0
23 20
33 0
14 10
24 20
34 0
1001 AcadAnnotative
1000 AnnotativeData
1002 {
1070 1
1070 1
1002 }
0 ENDSEC
0 SECTION
2 OBJECTS
0 DICTIONARY
5 C
330 0
100 AcDbDictionary
281 1
3 ACAD_SCALELIST
350 10
0 DICTIONARY
5 10
330 C
100 AcDbDictionary
281 1
3 A0
350 11
3 A1
350 12
3 A2
350 13
0 SCALE
5 11
330 10
100 AcDbScale
70 0
300 1:1
140 1
141 1
290 1
0 SCALE
5 12
330 10
100 AcDbScale
70 0
300 1:2
140 1
141 2
290 0
0 SCALE
5 13
330 10
100 AcDbScale
70 0
300 1:10
140 1
141 10
290 0
0 DICTIONARY
5 31
330 30
100 AcDbDictionary
280 1
281 1
3 AcDbContextDataManager
360 32
0 DICTIONARY
5 32
330 31
100 AcDbDictionary
281 1
3 ACDB_ANNOTATIONSCALES
350 33
0 DICTIONARY
5 33
330 32
100 AcDbDictionary
281 1
3 *A1
350 34
3 *A2
350 35
0 ACDB_TEXTOBJECTCONTEXTDATA_CLASS
5 34
330 33
100 AcDbObjectContextData
70 4
290 1
100 AcDbAnnotScaleObjectContextData
340 12
70 2
50 0
10 16
20 0
11 30
21 0
0 ACDB_TEXTOBJECTCONTEXTDATA_CLASS
5 35
330 33
100 AcDbObjectContextData
70 4
290 0
100 AcDbAnnotScaleObjectContextData
340 13
70 2
50 0
10 -40
20 0
11 30
21 0
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
0 ACDB_MTEXTOBJECTCONTEXTDATA_CLASS
5 44
330 43
100 AcDbObjectContextData
70 4
290 1
100 AcDbAnnotScaleObjectContextData
340 12
70 1
10 1
20 0
30 0
11 0
21 10
31 0
40 20
41 0
42 8.75
43 2.5
71 0
0 ACDB_MTEXTOBJECTCONTEXTDATA_CLASS
5 45
330 43
100 AcDbObjectContextData
70 4
290 0
100 AcDbAnnotScaleObjectContextData
340 13
70 1
10 1
20 0
30 0
11 0
21 10
31 0
40 100
41 0
42 43.75
43 12.5
71 0
0 DICTIONARY
5 51
330 50
100 AcDbDictionary
280 1
281 1
3 AcDbContextDataManager
360 52
0 DICTIONARY
5 52
330 51
100 AcDbDictionary
281 1
3 ACDB_ANNOTATIONSCALES
350 53
0 DICTIONARY
5 53
330 52
100 AcDbDictionary
281 1
3 *A1
350 54
3 *A2
350 55
0 ACDB_ALDIMOBJECTCONTEXTDATA_CLASS
5 54
330 53
100 AcDbObjectContextData
70 4
290 1
100 AcDbAnnotScaleObjectContextData
340 12
100 AcDbDimensionObjectContextData
2 *D1
293 0
10 5
20 25
294 1
140 0
100 AcDbAlignedDimensionObjectContextData
11 10
21 25
31 0
0 ACDB_ALDIMOBJECTCONTEXTDATA_CLASS
5 55
330 53
100 AcDbObjectContextData
70 4
290 0
100 AcDbAnnotScaleObjectContextData
340 13
100 AcDbDimensionObjectContextData
2 *D2
293 0
10 5
20 25
294 1
140 0
100 AcDbAlignedDimensionObjectContextData
11 10
21 25
31 0
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

type Rec = (String, Vec<(i32, String)>);

/// A written file's records: (type, groups).
fn records(text: &str) -> Vec<Rec> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out: Vec<Rec> = Vec::new();
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

fn num(tags: &[(i32, String)], code: i32) -> f64 {
    get(tags, code).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(f64::NAN)
}

/// A written context: (class, scale name, default, groups after the scale).
struct Ctx {
    class: String,
    scale: String,
    default: bool,
    tags: Vec<(i32, String)>,
}

/// The contexts written for the object with handle `owner` (through its extension dictionary,
/// context manager and dictionary of contexts).
fn contexts_of(text: &str, owner: &str) -> Vec<Ctx> {
    let recs = records(text);
    let by_handle: std::collections::HashMap<&str, &Rec> = recs.iter().filter_map(|r| Some((get(&r.1, 5)?, r))).collect();
    let scales: std::collections::HashMap<&str, &str> =
        recs.iter().filter(|(k, _)| k == "SCALE").filter_map(|(_, t)| Some((get(t, 5)?, get(t, 300)?))).collect();
    let entry = |h: &str, key: &str| -> Option<String> {
        let t = &by_handle.get(h)?.1;
        let i = t.iter().position(|(c, v)| *c == 3 && v.eq_ignore_ascii_case(key))?;
        t.get(i + 1).map(|(_, v)| v.clone())
    };
    let Some(ent) = by_handle.get(owner) else { return Vec::new() };
    let xd = ent.1.iter().skip_while(|(c, v)| !(*c == 102 && v == "{ACAD_XDICTIONARY")).nth(1).map(|(_, v)| v.clone());
    let Some(dict) = xd.and_then(|x| entry(&x, "AcDbContextDataManager")).and_then(|m| entry(&m, "ACDB_ANNOTATIONSCALES")) else {
        return Vec::new();
    };
    let Some(d) = by_handle.get(dict.as_str()) else { return Vec::new() };
    d.1.iter()
        .filter(|(c, _)| *c == 350)
        .filter_map(|(_, h)| by_handle.get(h.as_str()))
        .map(|(class, t)| {
            let at = t.iter().position(|(c, _)| *c == 340).unwrap_or(0);
            Ctx {
                class: class.clone(),
                scale: get(t, 340).and_then(|h| scales.get(h)).map_or("?", |s| s).to_string(),
                default: get(t, 290) == Some("1"),
                tags: t.get(at + 1..).unwrap_or(&[]).to_vec(),
            }
        })
        .collect()
}

fn names(d: &Drawing, h: &str) -> Vec<String> {
    d.entity(Handle::parse_hex(h).unwrap()).unwrap().common.anno_scales.iter().map(|s| s.name.clone()).collect()
}

fn read() -> Drawing {
    cadcraft_io::read_dxf(dxf(FIXTURE).as_bytes()).unwrap()
}

fn save(d: &Drawing) -> String {
    String::from_utf8(cadcraft_io::write(d, "x.dxf").unwrap()).unwrap()
}

#[test]
fn contexts_make_text_mtext_and_dimensions_annotative_at_paper_size() {
    let d = read();
    for h in ["30", "40", "50"] {
        assert_eq!(names(&d, h), ["1:2", "1:10"], "{h}");
    }
    let Some(EntityKind::Text(t)) = d.entity(Handle(0x30)).map(|e| &e.kind) else { panic!("text") };
    // 2.5 at 1:2: 1.25 on paper. The start point moves with the size about the alignment point.
    assert!((t.height - 1.25).abs() < 1e-12, "{}", t.height);
    assert!((t.insert.x - 23.0).abs() < 1e-12, "{:?}", t.insert);
    let Some(EntityKind::MText(m)) = d.entity(Handle(0x40)).map(|e| &e.kind) else { panic!("mtext") };
    assert!((m.height - 1.25).abs() < 1e-12 && (m.width - 10.0).abs() < 1e-12, "{} {}", m.height, m.width);
}

#[test]
fn each_object_is_written_at_its_default_scale_with_a_context_per_scale() {
    let text = save(&read());
    let recs = records(&text);
    let ent = |h: &str| recs.iter().find(|(_, t)| get(t, 5) == Some(h)).unwrap();
    // The current scale (1:2) is the default: the groups are at its size.
    assert_eq!(num(&ent("30").1, 40), 2.5);
    assert_eq!(num(&ent("40").1, 40), 2.5);
    assert_eq!(num(&ent("40").1, 41), 20.0);
    for h in ["30", "40", "50"] {
        assert!(ent(h).1.iter().any(|(c, v)| *c == 1001 && v == "AcadAnnotative"), "{h}");
        let ctx = contexts_of(&text, h);
        let got: Vec<(&str, bool)> = ctx.iter().map(|c| (c.scale.as_str(), c.default)).collect();
        assert_eq!(got, [("1:2", true), ("1:10", false)], "{h}");
    }
    let text_ctx = contexts_of(&text, "30");
    assert!(text_ctx.iter().all(|c| c.class == "ACDB_TEXTOBJECTCONTEXTDATA_CLASS"));
    // Right-justified: the alignment point stays, the start point moves with the size.
    let starts: Vec<(f64, f64)> = text_ctx.iter().map(|c| (num(&c.tags, 10), num(&c.tags, 11))).collect();
    assert_eq!(starts, [(16.0, 30.0), (-40.0, 30.0)]);
    let widths: Vec<f64> = contexts_of(&text, "40").iter().map(|c| num(&c.tags, 40)).collect();
    assert_eq!(widths, [20.0, 100.0]);
    // A dimension's contexts name its geometry block at each scale; the default one is its own.
    let dim = contexts_of(&text, "50");
    assert!(dim.iter().all(|c| c.class == "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS"));
    let blocks: Vec<String> = dim.iter().map(|c| get(&c.tags, 2).unwrap().to_string()).collect();
    assert_eq!(get(&ent("50").1, 2), Some(blocks[0].as_str()));
    let text_height = |block: &str| {
        let start = recs.iter().position(|(k, t)| k == "BLOCK" && get(t, 2) == Some(block)).unwrap();
        let end = start + recs[start..].iter().position(|(k, _)| k == "ENDBLK").unwrap();
        recs[start..end].iter().find(|(k, _)| k == "MTEXT").map(|(_, t)| num(t, 40)).unwrap()
    };
    let (h2, h10) = (text_height(&blocks[0]), text_height(&blocks[1]));
    assert!((h10 / h2 - 5.0).abs() < 1e-9, "{h2} {h10}");
    // Only the dimension has blocks per scale: no empty `*D` block for the text or the MText.
    let d_blocks: Vec<&str> = recs.iter().filter(|(k, _)| k == "BLOCK").filter_map(|(_, t)| get(t, 2)).filter(|n| n.starts_with("*D")).collect();
    assert_eq!(d_blocks.len(), 2, "{d_blocks:?}");
    for name in &d_blocks {
        let start = recs.iter().position(|(k, t)| k == "BLOCK" && get(t, 2) == Some(name)).unwrap();
        assert_ne!(recs.get(start + 1).map(|(k, _)| k.as_str()), Some("ENDBLK"), "{name} is empty");
    }
    // Each context class is declared.
    for class in ["ACDB_TEXTOBJECTCONTEXTDATA_CLASS", "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS", "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS"] {
        assert!(recs.iter().any(|(k, t)| k == "CLASS" && get(t, 1) == Some(class)), "{class}");
    }
}

#[test]
fn a_saved_drawing_reads_back_the_same() {
    let d = read();
    let back = cadcraft_io::read_dxf(save(&d).as_bytes()).unwrap();
    for h in ["30", "40", "50"] {
        let (a, b) = (d.entity(Handle::parse_hex(h).unwrap()).unwrap(), back.entity(Handle::parse_hex(h).unwrap()).unwrap());
        assert_eq!(a.common.anno_scales, b.common.anno_scales, "{h}");
        // A justified text's start point is worked out from its alignment point on save, as
        // AutoCAD does; the rest reads back as it was.
        let mut kind = a.kind.clone();
        if let (EntityKind::Text(t), EntityKind::Text(u)) = (&mut kind, &b.kind)
            && t.align_pt.is_some()
        {
            t.insert = u.insert;
        }
        if !matches!(a.kind, EntityKind::Dimension(_)) {
            assert_eq!(kind, b.kind, "{h}");
        }
    }
    // Saved again, it reads back exactly.
    let again = cadcraft_io::read_dxf(save(&back).as_bytes()).unwrap();
    for h in ["30", "40"] {
        let h = Handle::parse_hex(h).unwrap();
        assert_eq!(back.entity(h).unwrap().kind, again.entity(h).unwrap().kind);
    }
    // Saved again, the file is the same: nothing piles up.
    assert_eq!(contexts_of(&save(&back), "30").len(), 2);
}

/// A drawing made in CADCraft: text and a dimension at 1:1 and 1:50, 1:50 current.
fn made() -> Drawing {
    let mut d = Drawing::default();
    d.header.set_str("CANNOSCALE", "1:50");
    let scales = vec![AnnoScale::new("1:1", 1.0, 1.0), AnnoScale::new("1:50", 1.0, 50.0)];
    let common = Common { anno_scales: scales.clone(), ..Common::default() };
    let t = Text {
        insert: Vec3::ZERO,
        align_pt: None,
        height: 2.5,
        value: "NOTE".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    d.add(&Space::Model, common.clone(), EntityKind::Text(t)).unwrap();
    let dm = Dimension {
        kind: DimKind::Aligned,
        defpt: Vec3::new(100.0, 20.0, 0.0),
        text_mid: Vec3::new(50.0, 20.0, 0.0),
        p13: Vec3::ZERO,
        p14: Vec3::new(100.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 100.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    d.add(&Space::Model, common, EntityKind::Dimension(dm)).unwrap();
    d
}

#[test]
fn objects_made_in_cadcraft_get_contexts_and_read_back() {
    let d = made();
    let text = save(&d);
    let recs = records(&text);
    let handles: Vec<String> = d.model.iter().map(|e| e.handle.hex()).collect();
    let t = recs.iter().find(|(_, t)| get(t, 5) == Some(handles[0].as_str())).unwrap();
    // Written at 1:50, the current scale: 2.5 on paper is 125.
    assert_eq!(num(&t.1, 40), 125.0);
    for h in &handles {
        let got: Vec<(String, bool)> = contexts_of(&text, h).into_iter().map(|c| (c.scale, c.default)).collect();
        assert_eq!(got, [("1:1".to_string(), false), ("1:50".to_string(), true)], "{h}");
    }
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    for (a, b) in d.model.iter().zip(back.model.iter()) {
        assert_eq!(a.common.anno_scales, b.common.anno_scales);
    }
    let Some(EntityKind::Text(t)) = back.model.iter().next().map(|e| &e.kind) else { panic!("text") };
    assert!((t.height - 2.5).abs() < 1e-9, "{}", t.height);
}

#[test]
fn a_scale_missing_from_the_list_is_added_to_it() {
    let mut d = made();
    d.scales.retain(|s| s.name != "1:50");
    let text = save(&d);
    assert!(records(&text).iter().any(|(k, t)| k == "SCALE" && get(t, 300) == Some("1:50")));
    let h = d.model.iter().next().unwrap().handle.hex();
    assert_eq!(contexts_of(&text, &h).len(), 2);
}

#[test]
fn files_older_than_annotation_scaling_keep_cadcraft_s_record() {
    let d = made();
    let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2004), binary: false };
    let bytes = cadcraft_io::write_with(&d, "x.dxf", &opts).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("OBJECTCONTEXTDATA"));
    let back = cadcraft_io::read_dxf(&bytes).unwrap();
    for (a, b) in d.model.iter().zip(back.model.iter()) {
        assert_eq!(a.common.anno_scales, b.common.anno_scales);
        if let (EntityKind::Text(x), EntityKind::Text(y)) = (&a.kind, &b.kind) {
            assert_eq!(x.height, y.height);
        }
    }
}

#[test]
fn a_dimension_s_associativity_and_contexts_share_its_dictionary() {
    let mut d = made();
    let line =
        d.add(&Space::Model, Common::default(), EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(100.0, 0.0, 0.0) })).unwrap();
    let dim = d.model.iter().find(|e| matches!(e.kind, EntityKind::Dimension(_))).unwrap().handle;
    d.modify_entity(dim, |e| {
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.assoc = vec![
                cadcraft_doc::DimAssoc { point: "p13".into(), handle: line, snap: cadcraft_doc::AssocSnap::Start },
                cadcraft_doc::DimAssoc { point: "p14".into(), handle: line, snap: cadcraft_doc::AssocSnap::End },
            ];
        }
    })
    .unwrap();
    let text = save(&d);
    let recs = records(&text);
    let ent = recs.iter().find(|(k, t)| k == "DIMENSION" && get(t, 5) == Some(dim.hex().as_str())).unwrap();
    let xdicts = ent.1.iter().filter(|(c, v)| *c == 102 && v == "{ACAD_XDICTIONARY").count();
    assert_eq!(xdicts, 1);
    assert_eq!(contexts_of(&text, &dim.hex()).len(), 2);
    let xd = ent.1.iter().skip_while(|(c, v)| !(*c == 102 && v == "{ACAD_XDICTIONARY")).nth(1).unwrap().1.clone();
    let dict = recs.iter().find(|(_, t)| get(t, 5) == Some(xd.as_str())).unwrap();
    assert!(dict.1.iter().any(|(c, v)| *c == 3 && v == "ACAD_DIMASSOC"));
}

#[test]
fn hostile_contexts_do_not_panic() {
    // Contexts naming no scale, a scale of zero, a missing dictionary and a cycle.
    let src = dxf(FIXTURE)
        .replace("340\n13\n", "340\n99\n")
        .replace("141\n2\n", "141\n0\n")
        .replace("360\n42\n", "360\n41\n")
        .replace("350\n53\n", "350\n51\n");
    let d = cadcraft_io::read_dxf(src.as_bytes()).unwrap();
    for h in ["30", "40", "50"] {
        let n = names(&d, h);
        assert!(n.len() <= 1, "{h}: {n:?}");
    }
    // Saved and opened again: the same scales, every context naming a written SCALE.
    let text = save(&d);
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    for h in ["30", "40", "50"] {
        assert_eq!(names(&back, h), names(&d, h), "{h}");
        for c in contexts_of(&text, h) {
            assert_ne!(c.scale, "?", "{h}: a context names a missing SCALE");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn text_mtext_and_dimensions_keep_their_scales_through_dwg() {
    let mut d = read();
    // Turned 30° first, so the text context's rotation unit shows (DXF degrees, DWG radians).
    d.modify_entity(Handle(0x30), |e| {
        if let EntityKind::Text(t) = &mut e.kind {
            t.rotation = 30f64.to_radians();
        }
    })
    .unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    for (h, kind) in [(0x30, "text"), (0x40, "mtext"), (0x50, "dimension")] {
        let e = back.model.iter().find(|e| {
            matches!((&e.kind, kind), (EntityKind::Text(_), "text") | (EntityKind::MText(_), "mtext") | (EntityKind::Dimension(_), "dimension"))
        });
        let e = e.unwrap_or_else(|| panic!("{kind} {h:X}"));
        let got: Vec<&str> = e.common.anno_scales.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(got, ["1:2", "1:10"], "{kind}");
        if let EntityKind::Text(t) = &e.kind {
            assert!((t.height - 1.25).abs() < 1e-9 && (t.rotation.to_degrees() - 30.0).abs() < 1e-9, "{t:?}");
        }
    }
    // The DXF the bridge reads back has AutoCAD's layout: rotation in degrees, no marker, and a
    // dimension context naming its block.
    let dxf = cadcraft_dwg::dwg_to_dxf(&dwg).unwrap();
    let text = String::from_utf8_lossy(&dxf);
    let recs = records(&text);
    let text_ctx: Vec<&Rec> = recs.iter().filter(|(k, _)| k == "ACDB_TEXTOBJECTCONTEXTDATA_CLASS").collect();
    assert_eq!(text_ctx.len(), 2);
    for (_, t) in &text_ctx {
        assert!(t.iter().all(|(c, v)| !(*c == 100 && v == "AcDbTextObjectContextData")));
        assert!((num(t.iter().skip_while(|(c, _)| *c != 340).cloned().collect::<Vec<_>>().as_slice(), 50) - 30.0).abs() < 1e-9, "{t:?}");
    }
    for (_, t) in recs.iter().filter(|(k, _)| k == "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS") {
        assert!(get(t, 2).is_some_and(|b| b.starts_with("*D")), "{t:?}");
    }
}

/// A drawing made in CADCraft with an annotative multileader (text 2.5 on paper, 1:1 and 1:10,
/// 1:10 current).
fn with_mleader() -> (Drawing, Handle) {
    let mut d = Drawing::default();
    d.header.set_str("CANNOSCALE", "1:10");
    let common = Common { anno_scales: vec![AnnoScale::new("1:1", 1.0, 1.0), AnnoScale::new("1:10", 1.0, 10.0)], ..Common::default() };
    let text = cadcraft_doc::MText {
        insert: Vec3::new(10.5, 10.1, 0.0),
        height: 2.5,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "Note".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let m = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::ZERO]],
        landing: Vec3::new(10.0, 10.0, 0.0),
        dogleg: 0.36,
        text: Some(text),
        style: "Standard".into(),
        arrow_size: 0.18,
        spline: false,
        block: None,
    };
    let h = d.add(&Space::Model, common, EntityKind::MLeader(m)).unwrap();
    (d, h)
}

#[test]
fn a_multileader_gets_its_context_data_at_each_scale() {
    let (d, h) = with_mleader();
    let text = save(&d);
    let recs = records(&text);
    let ent = recs.iter().find(|(k, t)| k == "MULTILEADER" && get(t, 5) == Some(h.hex().as_str())).unwrap();
    assert!(ent.1.iter().any(|(c, v)| *c == 1001 && v == "AcadAnnotative"));
    // The entity is at 1:10, the current scale: overall scale 10 and text 25.
    assert_eq!(num(&ent.1, 40), 10.0);
    assert_eq!(num(&ent.1, 41), 25.0);
    let ctx = contexts_of(&text, &h.hex());
    let got: Vec<(&str, bool, &str)> = ctx.iter().map(|c| (c.scale.as_str(), c.default, c.class.as_str())).collect();
    assert_eq!(got, [("1:1", false, "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS"), ("1:10", true, "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS")]);
    for (c, f) in ctx.iter().zip([1.0, 10.0]) {
        // AutoCAD's layout: the context groups straight after the scale, no braces of their own.
        assert_eq!(c.tags.first().map(|t| t.0), Some(40), "{:?}", c.tags);
        assert!(c.tags.iter().all(|(code, v)| !(*code == 300 && v.starts_with("CONTEXT_DATA")) && *code != 301));
        assert_eq!(num(&c.tags, 40), f);
        assert!((num(&c.tags, 41) - 2.5 * f).abs() < 1e-9);
        assert!((num(&c.tags, 140) - 0.18 * f).abs() < 1e-9);
        // The text keeps its place about the landing, at the scale's size.
        assert!((num(&c.tags, 12) - (10.0 + 0.5 * f)).abs() < 1e-9 && (num(&c.tags, 22) - (10.0 + 0.1 * f)).abs() < 1e-9);
    }
    // A multileader has no block per scale (only dimensions do).
    assert!(!recs.iter().any(|(k, t)| k == "BLOCK" && get(t, 2).is_some_and(|n| n.starts_with("*D"))));
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    let (a, b) = (d.entity(h).unwrap(), back.entity(h).unwrap());
    assert_eq!(a.common.anno_scales, b.common.anno_scales);
    let (EntityKind::MLeader(x), EntityKind::MLeader(y)) = (&a.kind, &b.kind) else { panic!("multileader") };
    let (tx, ty) = (x.text.as_ref().unwrap(), y.text.as_ref().unwrap());
    assert!((tx.height - ty.height).abs() < 1e-9 && (tx.insert - ty.insert).len() < 1e-9, "{ty:?}");
    assert!((x.arrow_size - y.arrow_size).abs() < 1e-9 && (x.dogleg - y.dogleg).abs() < 1e-9, "{y:?}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_multileader_keeps_its_scales_through_dwg() {
    let (d, _) = with_mleader();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    let e = back.model.iter().find(|e| matches!(e.kind, EntityKind::MLeader(_))).unwrap();
    let got: Vec<&str> = e.common.anno_scales.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(got, ["1:1", "1:10"]);
    let EntityKind::MLeader(m) = &e.kind else { panic!("multileader") };
    assert!((m.text.as_ref().unwrap().height - 2.5).abs() < 1e-9, "{m:?}");
    // The DXF read back from the DWG has AutoCAD's layout and each context at its scale.
    let text = String::from_utf8_lossy(&cadcraft_dwg::dwg_to_dxf(&dwg).unwrap()).into_owned();
    let ctx: Vec<Rec> = records(&text).into_iter().filter(|(k, _)| k == "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS").collect();
    let mut scales: Vec<f64> = ctx
        .iter()
        .map(|(_, t)| {
            assert!(t.iter().all(|(c, v)| !(*c == 300 && v.starts_with("CONTEXT_DATA"))), "{t:?}");
            let at = t.iter().position(|(c, _)| *c == 340).unwrap();
            num(&t[at + 1..], 40)
        })
        .collect();
    scales.sort_by(f64::total_cmp);
    assert_eq!(scales, [1.0, 10.0]);
}

/// The fixture with AutoCAD's variable dictionary naming 1:10 as the current annotation scale.
fn with_current_scale() -> String {
    dxf(FIXTURE)
        .replace("3\nACAD_SCALELIST\n350\n10\n", "3\nACAD_SCALELIST\n350\n10\n3\nAcDbVariableDictionary\n350\n60\n")
        .replace(
            "0\nENDSEC\n0\nEOF\n",
            "0\nDICTIONARY\n5\n60\n330\nC\n100\nAcDbDictionary\n281\n1\n3\nCANNOSCALE\n350\n61\n0\nDICTIONARYVAR\n5\n61\n330\n60\n100\nDictionaryVariables\n280\n0\n1\n1:10\n0\nENDSEC\n0\nEOF\n",
        )
}

#[test]
fn the_current_scale_is_autocad_s_and_saving_keeps_it_current() {
    let mut d = cadcraft_io::read_dxf(with_current_scale().as_bytes()).unwrap();
    assert_eq!(d.header.str("CANNOSCALE", ""), "1:10");
    // Written at the current scale: the 1:10 context is the default one.
    let text = save(&d);
    let got: Vec<(String, bool)> = contexts_of(&text, "30").into_iter().map(|c| (c.scale, c.default)).collect();
    assert_eq!(got, [("1:2".to_string(), false), ("1:10".to_string(), true)]);
    // Changed in CADCraft, AutoCAD's variable says so too.
    d.header.set_str("CANNOSCALE", "1:2");
    let recs = records(&save(&d));
    let var = recs.iter().find(|(k, _)| k == "DICTIONARYVAR").unwrap();
    assert_eq!(get(&var.1, 1), Some("1:2"));
    assert_eq!(recs.iter().filter(|(k, _)| k == "DICTIONARYVAR").count(), 1);
    // Naming no scale of the drawing's, it is the 1:1 the contexts are written at.
    d.header.set_str("CANNOSCALE", "1:999");
    let recs = records(&save(&d));
    let var = recs.iter().find(|(k, _)| k == "DICTIONARYVAR").unwrap();
    assert_eq!(get(&var.1, 1), Some("1:1"));
}

#[test]
fn a_context_placing_the_object_elsewhere_reads_as_its_offset_there() {
    // The text sits 5 further right at 1:10 than its 1:2 position would put it.
    let src = FIXTURE.replacen("10 -40\n20 0\n11 30", "10 -35\n20 0\n11 35", 1);
    assert_ne!(src, FIXTURE);
    let d = cadcraft_io::read_dxf(dxf(&src).as_bytes()).unwrap();
    let e = d.entity(Handle::parse_hex("30").unwrap()).unwrap();
    assert_eq!(e.common.anno_moves.len(), 1, "{:?}", e.common.anno_moves);
    assert_eq!(e.common.anno_moves[0].scale, "1:10");
    assert!(e.common.anno_moves[0].by.dist(cadcraft_geom::Vec2::new(5.0, 0.0)) < 1e-9);
    // An unmoved fixture has none.
    assert!(read().model.iter().all(|e| e.common.anno_moves.is_empty()));
    // Written back, the 1:10 context keeps its own position.
    let text = save(&d);
    let starts: Vec<f64> = contexts_of(&text, "30").iter().map(|c| num(&c.tags, 11)).collect();
    assert_eq!(starts, [30.0, 35.0]);
}

#[test]
fn offsets_made_in_cadcraft_survive_a_save() {
    let mut d = made();
    let hs: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    for h in &hs {
        d.modify_entity(*h, |e| e.common.move_at("1:1", cadcraft_geom::Vec2::new(3.0, 4.0))).unwrap();
    }
    let back = cadcraft_io::read_dxf(save(&d).as_bytes()).unwrap();
    for h in &hs {
        let (a, b) = (d.entity(*h).unwrap(), back.entity(*h).unwrap());
        for f in [1.0, 50.0] {
            let shown = |d: &Drawing, e: &cadcraft_doc::Entity| match cadcraft_doc::anno_draw(e, f, true) {
                cadcraft_doc::AnnoDraw::Scaled { factor, by } => match cadcraft_doc::anno_placed(e, factor, by).kind {
                    EntityKind::Text(t) => t.insert.xy(),
                    EntityKind::Dimension(dm) if dm.user_text_pos => dm.text_mid.xy(),
                    EntityKind::Dimension(dm) => cadcraft_render::dimension_in(d, &dm).text_pos,
                    k => panic!("{k:?}"),
                },
                m => panic!("{m:?}"),
            };
            let (p, q) = (shown(&d, a), shown(&back, b));
            assert!(p.dist(q) < 1e-6, "{h:?} at {f}: {p:?} vs {q:?}");
        }
    }
}

/// The landing and text of multileader `e` as it shows at annotation scale `f`.
fn mleader_shown(e: &cadcraft_doc::Entity, f: f64) -> (cadcraft_geom::Vec2, cadcraft_geom::Vec2) {
    let cadcraft_doc::AnnoDraw::Scaled { factor, by } = cadcraft_doc::anno_draw(e, f, true) else { panic!("hidden") };
    let EntityKind::MLeader(m) = cadcraft_doc::anno_placed(e, factor, by).kind else { panic!("multileader") };
    (m.landing.xy(), m.text.unwrap().insert.xy())
}

#[test]
fn a_multileader_moved_at_one_scale_keeps_it_through_dxf_and_dwg() {
    let (mut d, h) = with_mleader();
    d.modify_entity(h, |e| e.common.move_at("1:1", cadcraft_geom::Vec2::new(2.0, -1.0))).unwrap();
    let dxf_back = cadcraft_io::read_dxf(save(&d).as_bytes()).unwrap();
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let dwg_back = Some(cadcraft_io::read(&dwg, "x.dwg").unwrap());
    #[cfg(target_arch = "wasm32")]
    let dwg_back: Option<Drawing> = None;
    let a = d.entity(h).unwrap();
    for back in std::iter::once(&dxf_back).chain(dwg_back.as_ref()) {
        let b = back.model.iter().find(|e| matches!(e.kind, EntityKind::MLeader(_))).unwrap();
        for f in [1.0, 10.0] {
            let ((la, ta), (lb, tb)) = (mleader_shown(a, f), mleader_shown(b, f));
            assert!(la.dist(lb) < 1e-6 && ta.dist(tb) < 1e-6, "at {f}: {la:?} {ta:?} vs {lb:?} {tb:?}");
        }
    }
}

/// The MULTILEADER entity's own 293 ("enable annotation scale", after its `CONTEXT_DATA{` block).
fn mleader_enable_annotation_scale(text: &str) -> Option<String> {
    let recs = records(text);
    let (_, t) = recs.iter().find(|(k, _)| k == "MULTILEADER")?;
    let close = t.iter().position(|(c, _)| *c == 301)?;
    t.iter().skip(close + 1).find(|(c, _)| *c == 293).map(|(_, v)| v.trim().to_string())
}

#[test]
fn an_annotative_multileader_has_annotation_scale_enabled() {
    // AutoCAD's ANNORESET passes over a multileader whose 293 is off, even with its contexts.
    let (d, h) = with_mleader();
    assert_eq!(mleader_enable_annotation_scale(&save(&d)).as_deref(), Some("1"));
    let mut plain = d.clone();
    plain.modify_entity(h, |e| e.common.anno_scales.clear()).unwrap();
    assert_eq!(mleader_enable_annotation_scale(&save(&plain)).as_deref(), Some("0"));
}

#[test]
fn hostile_context_positions_do_not_panic() {
    for bad in ["10 1e309\n20 nan\n11 inf", "10 x\n20 0\n11 -1e308", "10 -40\n20 0\n11 1e308"] {
        let src = FIXTURE.replacen("10 -40\n20 0\n11 30", bad, 1);
        let Ok(d) = cadcraft_io::read_dxf(dxf(&src).as_bytes()) else { continue };
        for e in d.model.iter() {
            assert!(e.common.anno_moves.iter().all(|m| m.by.is_finite()), "{:?}", e.common.anno_moves);
        }
        let _ = save(&d);
    }
}
