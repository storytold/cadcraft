//! Fields are written the way AutoCAD writes them (#551): an `ACAD_FIELD` extension dictionary
//! on the MTEXT, a `_text` FIELD with one child FIELD per field code, the shown value in the
//! MTEXT contents and a FIELDLIST of all FIELD objects. `data/autocad_fields.dxf` holds the
//! relevant parts (the three MTEXT, their dictionaries, the 8 FIELD objects and the FIELDLIST)
//! of an AutoCAD 2023 sample ASCII DXF R2010 that a contributor of #551 made and shared for this
//! purpose.

use std::collections::HashMap;

use cadcraft_doc::fields::{self, FieldKind};
use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space};
use cadcraft_dxf::Tag;
use cadcraft_geom::Vec3;

const SAMPLE: &[u8] = include_bytes!("../data/autocad_fields.dxf");

/// The three texts of the sample as AutoCAD shows them.
const DATE: &str = "Date : 03/11/2026";
const TWO: &str = "Fichier champs_551_r2010.dxf du 03/11/2026";
const EXPR: &str = "Double de LTSCALE : 2.0000";

/// The same texts as CADCraft holds them: the field codes with their last values.
const DATE_CODE: &str = "Date : %<\\AcVar Date \\f \"dd/MM/yyyy\" \\CcValue \"03/11/2026\">%";
const TWO_CODE: &str =
    "Fichier %<\\AcVar DWGNAME \\CcValue \"champs_551_r2010.dxf\">% du %<\\AcVar Date \\f \"dd/MM/yyyy\" \\CcValue \"03/11/2026\">%";
const EXPR_CODE: &str = "Double de LTSCALE : %<\\AcExpr (%<\\AcVar LTSCALE \\CcValue \"1.000000\">%*2) \\f \"%lu2\" \\CcValue \"2.0000\">%";

fn contents(d: &Drawing) -> Vec<String> {
    let mut v: Vec<(f64, String)> = d
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::MText(t) => Some((-t.insert.y, t.contents.clone())),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, c)| c).collect()
}

fn drawing_with(texts: &[&str]) -> Drawing {
    let mut d = Drawing::new_metric();
    for (i, c) in texts.iter().enumerate() {
        let t = MText {
            insert: Vec3::new(0.0, -10.0 * i as f64, 0.0),
            height: 2.5,
            width: 120.0,
            attach: 1,
            rotation: 0.0,
            style: "Standard".into(),
            contents: (*c).into(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t)).unwrap();
    }
    d
}

fn write_tags(d: &Drawing) -> Vec<Tag> {
    cadcraft_dxf::parse(&cadcraft_io::write(d, "out.dxf").unwrap()).unwrap()
}

fn reread(d: &Drawing) -> Drawing {
    cadcraft_io::read(&cadcraft_io::write(d, "out.dxf").unwrap(), "out.dxf").unwrap()
}

type Rec = (String, Vec<Tag>);

fn objects(tags: &[Tag]) -> Vec<Rec> {
    cadcraft_dxf::records(tags)
}

fn handle_of(t: &[Tag]) -> String {
    t.iter().find(|x| x.code == 5).map(|x| x.str().trim().to_ascii_uppercase()).unwrap_or_default()
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// The field objects with every handle replaced by what it points at, so two files compare
/// equal when only their handle numbers differ: an owner (330) is named by its type, a child
/// (350, 360) by its own canonical form.
fn canonical(tags: &[Tag]) -> Vec<String> {
    let recs = objects(tags);
    let by: HashMap<String, usize> = recs.iter().enumerate().map(|(i, (_, t))| (handle_of(t), i)).collect();
    fn render(recs: &[Rec], by: &HashMap<String, usize>, i: usize, depth: usize) -> String {
        let Some((kind, tags)) = recs.get(i) else { return "?".into() };
        let mut out = format!("{kind}[");
        let mut reactors = false;
        for t in tags {
            let v = t.str();
            match t.code {
                5 => {}
                102 => {
                    reactors = v.trim().starts_with('{');
                    out.push_str(&format!("102={v};"));
                }
                330 if reactors => out.push_str("330=reactor;"),
                330 => out.push_str(&format!("330={};", by.get(&norm(&v)).and_then(|j| recs.get(*j)).map_or("none", |r| r.0.as_str()))),
                350 | 360 if depth < 8 => match by.get(&norm(&v)) {
                    Some(&j) => out.push_str(&format!("{}={};", t.code, render(recs, by, j, depth + 1))),
                    None => out.push_str(&format!("{}=gone;", t.code)),
                },
                _ => out.push_str(&format!("{}={};", t.code, v.trim())),
            }
        }
        out.push(']');
        out
    }
    let mut out: Vec<String> = recs
        .iter()
        .enumerate()
        .filter(|(_, (k, t))| {
            matches!(k.as_str(), "FIELD" | "FIELDLIST")
                || (k == "DICTIONARY" && t.iter().any(|x| x.code == 3 && matches!(x.str().as_str(), "ACAD_FIELD" | "TEXT")))
        })
        .map(|(i, _)| render(&recs, &by, i, 0))
        .collect();
    out.sort();
    out
}

/// What a FIELD says, without its data records: (evaluator, code, value shown, children).
fn summary(tags: &[Tag]) -> Vec<(String, String, String, usize)> {
    let mut out: Vec<_> = objects(tags)
        .into_iter()
        .filter(|(k, _)| k == "FIELD")
        .map(|(_, t)| {
            let first = |c: i32| t.iter().find(|x| x.code == c).map(|x| x.str()).unwrap_or_default();
            let last = |c: i32| t.iter().rev().find(|x| x.code == c).map(|x| x.str()).unwrap_or_default();
            let children = t.iter().skip_while(|x| x.code != 90).skip(1).take_while(|x| x.code == 360).count();
            (first(1), first(2), last(301), children)
        })
        .collect();
    out.sort();
    out
}

/// The text of every MTEXT in the file, in file order.
fn mtext_groups(tags: &[Tag]) -> Vec<String> {
    objects(tags)
        .into_iter()
        .filter(|(k, _)| k == "MTEXT")
        .map(|(_, t)| t.iter().find(|x| x.code == 1).map(|x| x.str()).unwrap_or_default())
        .collect()
}

/// A file's FIELD, FIELDLIST and class bookkeeping is consistent.
fn check_structure(tags: &[Tag], fields: usize) {
    let recs = objects(tags);
    let field_handles: Vec<String> = recs.iter().filter(|(k, _)| k == "FIELD").map(|(_, t)| handle_of(t)).filter(|h| !h.is_empty()).collect();
    assert_eq!(field_handles.len(), fields, "FIELD objects");
    let lists: Vec<&Rec> = recs.iter().filter(|(k, _)| k == "FIELDLIST").collect();
    if fields == 0 {
        assert!(lists.is_empty(), "no FIELDLIST without fields");
        return;
    }
    assert_eq!(lists.len(), 1, "one FIELDLIST");
    let (_, list) = lists[0];
    let count = list.iter().find(|t| t.code == 90).map(Tag::i64).unwrap();
    let members: Vec<String> = list.iter().skip_while(|t| t.code != 90).filter(|t| t.code == 330).map(|t| norm(&t.str())).collect();
    assert_eq!(count as usize, fields, "FIELDLIST count");
    let mut a = members.clone();
    let mut b = field_handles.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b, "FIELDLIST members are the FIELD objects");
    // The named object dictionary lists it, with a hard-owner (350) reference.
    let root = recs.iter().find(|(k, t)| k == "DICTIONARY" && t.iter().any(|x| x.code == 3 && x.str() == "ACAD_FIELDLIST")).unwrap();
    let entry = root.1.iter().skip_while(|t| !(t.code == 3 && t.str() == "ACAD_FIELDLIST")).nth(1).unwrap();
    assert_eq!((entry.code, norm(&entry.str())), (350, handle_of(&list_tags(&recs))));
    // CLASS records, once each, with their instance counts.
    for (name, n) in [("FIELD", fields), ("FIELDLIST", 1)] {
        let classes: Vec<&Rec> = recs.iter().filter(|(k, t)| k == "CLASS" && t.iter().any(|x| x.code == 1 && x.str() == name)).collect();
        assert_eq!(classes.len(), 1, "class {name}");
        assert_eq!(classes[0].1.iter().find(|t| t.code == 91).map(Tag::i64), Some(n as i64), "instances of {name}");
    }
}

fn list_tags(recs: &[Rec]) -> Vec<Tag> {
    recs.iter().find(|(k, _)| k == "FIELDLIST").map(|(_, t)| t.clone()).unwrap_or_default()
}

/// The file as AutoCAD saved it, read and saved again: the field objects stay as they were.
#[test]
fn autocad_sample_saves_unchanged() {
    let sample = cadcraft_dxf::parse(SAMPLE).unwrap();
    assert_eq!(canonical(&sample).len(), 8 + 6 + 1);
    let d = cadcraft_io::read(SAMPLE, "sample.dxf").unwrap();
    let saved = write_tags(&d);
    assert_eq!(canonical(&saved), canonical(&sample));
    assert_eq!(mtext_groups(&saved).len(), 3);
    check_structure(&saved, 8);
    // And again: a second save changes nothing either.
    assert_eq!(canonical(&write_tags(&reread(&d))), canonical(&sample));
}

/// The sample's texts are field codes inside CADCraft: the kind, format and last value of each
/// field, and the formula's operand with its own value.
#[test]
fn autocad_sample_reads_as_field_codes() {
    let d = cadcraft_io::read(SAMPLE, "sample.dxf").unwrap();
    assert_eq!(contents(&d), [DATE_CODE, TWO_CODE, EXPR_CODE]);
    let date = &fields::find(DATE_CODE)[0].field;
    assert_eq!(
        (date.kind.clone(), date.format.as_deref(), date.value.as_deref()),
        (FieldKind::Var("Date".into()), Some("dd/MM/yyyy"), Some("03/11/2026"))
    );
    let two = fields::find(&contents(&d)[1]);
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].field.kind, FieldKind::Var("DWGNAME".into()));
    let expr = fields::find(&contents(&d)[2]);
    assert_eq!(expr.len(), 1);
    assert_eq!(expr[0].field.value.as_deref(), Some("2.0000"));
    assert_eq!(expr[0].field.format.as_deref(), Some("%lu2"));
    assert_eq!(fields::display(&contents(&d)[0]), DATE);
    assert_eq!(fields::display(&contents(&d)[1]), TWO);
    assert_eq!(fields::display(&contents(&d)[2]), EXPR);
}

/// Fields placed by CADCraft are written as AutoCAD writes the sample's: the same codes in the
/// FIELD objects, the same shape (children in order, the nested field below the formula) and the
/// text as shown in the MTEXT.
#[test]
fn written_fields_have_the_layout_of_the_sample() {
    let d = drawing_with(&[DATE_CODE, TWO_CODE, EXPR_CODE]);
    let saved = write_tags(&d);
    let sample = cadcraft_dxf::parse(SAMPLE).unwrap();
    assert_eq!(summary(&saved), summary(&sample));
    let mut shown = mtext_groups(&saved);
    shown.sort();
    let mut expect = vec![DATE.to_string(), TWO.to_string(), EXPR.to_string()];
    expect.sort();
    assert_eq!(shown, expect);
    check_structure(&saved, 8);
    // Each MTEXT has an extension dictionary naming the ACAD_FIELD dictionary.
    let recs = objects(&saved);
    let mtexts: Vec<&Rec> = recs.iter().filter(|(k, _)| k == "MTEXT").collect();
    assert_eq!(mtexts.len(), 3);
    for (_, t) in mtexts {
        let x = t.iter().find(|x| x.code == 360).map(|x| norm(&x.str())).unwrap();
        let dict = recs.iter().find(|(k, t)| k == "DICTIONARY" && handle_of(t) == x).unwrap();
        assert!(dict.1.iter().any(|t| t.code == 3 && t.str() == "ACAD_FIELD"));
        assert!(dict.1.iter().any(|t| t.code == 330 && norm(&t.str()) == handle_of(&t_of(&recs, "MTEXT", &x))));
    }
}

fn t_of(recs: &[Rec], kind: &str, xdict: &str) -> Vec<Tag> {
    recs.iter().find(|(k, t)| k == kind && t.iter().any(|x| x.code == 360 && norm(&x.str()) == xdict)).map(|(_, t)| t.clone()).unwrap_or_default()
}

/// What CADCraft writes it reads back as the same field codes, and saving that again changes
/// nothing.
#[test]
fn written_fields_read_back_and_resave_unchanged() {
    let d = drawing_with(&[DATE_CODE, TWO_CODE, EXPR_CODE]);
    let back = reread(&d);
    assert_eq!(contents(&back), [DATE_CODE, TWO_CODE, EXPR_CODE]);
    let first = write_tags(&d);
    assert_eq!(canonical(&write_tags(&back)), canonical(&first));
    assert_eq!(canonical(&write_tags(&reread(&back))), canonical(&first));
    check_structure(&write_tags(&back), 8);
}

/// A text without a cached value is written with an empty value and read back without one.
#[test]
fn field_without_value_roundtrips() {
    let code = "Now %<\\AcVar Date \\f \"yyyy\">% here";
    let d = drawing_with(&[code]);
    let saved = write_tags(&d);
    assert_eq!(mtext_groups(&saved), ["Now #### here"]);
    check_structure(&saved, 2);
    assert_eq!(contents(&reread(&d)), [code]);
}

/// Changing the text of a text read from AutoCAD writes new objects, not the old ones.
#[test]
fn edited_text_gets_new_objects() {
    let mut d = cadcraft_io::read(SAMPLE, "sample.dxf").unwrap();
    let h = d.model.iter().find(|e| matches!(&e.kind, EntityKind::MText(t) if t.contents == DATE_CODE)).unwrap().handle;
    let new = "Heure %<\\AcVar Date \\f \"HH:mm\" \\CcValue \"21:13\">%";
    d.model.modify(h, |e| {
        if let EntityKind::MText(t) = &mut e.kind {
            t.contents = new.into();
        }
    });
    let saved = write_tags(&d);
    check_structure(&saved, 8);
    let s = summary(&saved);
    assert!(s.iter().any(|(_, code, value, _)| code == "\\AcVar Date \\f \"HH:mm\"" && value == "21:13"));
    assert!(!s.iter().any(|(_, code, _, _)| code == "Date : %<\\_FldIdx 0>%"));
    assert!(s.iter().any(|(_, code, _, _)| code == "Heure %<\\_FldIdx 0>%"));
    assert_eq!(contents(&reread(&d)).iter().filter(|c| c.starts_with("Heure")).count(), 1);
}

/// Deleting a text takes its objects out of the FIELDLIST; a text turned into plain text loses
/// its objects.
#[test]
fn deleted_or_plain_text_drops_objects() {
    let mut d = cadcraft_io::read(SAMPLE, "sample.dxf").unwrap();
    let hs: Vec<_> = d.model.iter().map(|e| e.handle).collect();
    // The date text has 2 FIELD objects (`_text` and its child).
    let date = d.model.iter().find(|e| matches!(&e.kind, EntityKind::MText(t) if t.contents == DATE_CODE)).unwrap().handle;
    d.remove_entity(date);
    check_structure(&write_tags(&d), 6);
    let two =
        hs.into_iter().find(|h| d.model.iter().any(|e| e.handle == *h && matches!(&e.kind, EntityKind::MText(t) if t.contents == TWO_CODE))).unwrap();
    d.model.modify(two, |e| {
        if let EntityKind::MText(t) = &mut e.kind {
            t.contents = "plain".into();
        }
    });
    let saved = write_tags(&d);
    check_structure(&saved, 3);
    assert!(mtext_groups(&saved).contains(&"plain".to_string()));
    // Last field text gone: no FIELD, no FIELDLIST.
    let all: Vec<_> =
        d.model.iter().filter(|e| matches!(&e.kind, EntityKind::MText(t) if fields::has_fields(&t.contents))).map(|e| e.handle).collect();
    for h in all {
        d.remove_entity(h);
    }
    check_structure(&write_tags(&d), 0);
}

/// A field CADCraft cannot write as objects (an object's property) stays as its code in the text,
/// as before, and writes no FIELD.
#[test]
fn unsupported_fields_stay_code_in_the_text() {
    let code = "A=%<\\AcObjProp Object(%<\\_ObjId 2A>%).Area \\f \"%lu2%pr2\" \\CcValue \"12.00\">%";
    let d = drawing_with(&[code]);
    let saved = write_tags(&d);
    assert_eq!(mtext_groups(&saved), [code]);
    check_structure(&saved, 0);
    assert_eq!(contents(&reread(&d)), [code]);
}

/// A text without fields writes no field objects.
#[test]
fn text_without_fields_is_unchanged() {
    let d = drawing_with(&["plain text"]);
    let saved = write_tags(&d);
    assert_eq!(mtext_groups(&saved), ["plain text"]);
    check_structure(&saved, 0);
}

/// A field whose value is the empty text is still a field after saving and reading.
#[test]
fn field_with_empty_value_roundtrips() {
    let code = "T=%<\\AcVar Title \\CcValue \"\">%";
    let d = drawing_with(&[code]);
    assert_eq!(contents(&reread(&d)), [code]);
}

/// Before AutoCAD 2004 there are no FIELD objects: the text keeps its code.
#[test]
fn r2000_keeps_the_code() {
    let d = drawing_with(&[DATE_CODE]);
    let bytes =
        cadcraft_io::write_with(&d, "out.dxf", &cadcraft_io::SaveOptions { version: Some(cadcraft_dxf::Version::R2000), ..Default::default() })
            .unwrap();
    let tags = cadcraft_dxf::parse(&bytes).unwrap();
    assert_eq!(mtext_groups(&tags), [DATE_CODE]);
    check_structure(&tags, 0);
}
