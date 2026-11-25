//! Dynamic blocks from another program: their parameters and actions are read into
//! `cadcraft_doc::dynblock`, and survive a save unchanged.
//!
//! The fixture is synthetic, written for this test: a DOOR block whose BLOCK_RECORD carries an
//! evaluation graph with a visibility parameter (Open, Closed), a linear parameter (Width, a list
//! of values), a stretch action on it, a lookup parameter and its lookup table, and a grip.

use cadcraft_doc::dynblock::{ActionKind, DynamicBlock, ParameterKind, dynamic_block};
use cadcraft_doc::{Handle, RawTag};
use cadcraft_io::{read_dxf, write_dxf};

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

/// The DOOR drawing as `code value` lines (shared with the engine's dynamic grip tests), with
/// LF line ends whatever a checkout gave the file.
fn fixture() -> String {
    include_str!("../data/dynblock_door.txt").replace("\r\n", "\n")
}

fn h(hex: &str) -> Handle {
    Handle::parse_hex(hex).unwrap()
}

#[test]
fn a_dynamic_block_reads_its_parameters_and_actions() {
    let d = read_dxf(dxf(&fixture()).as_bytes()).unwrap();
    assert_eq!(d.block("DOOR").unwrap().entities.len(), 3);
    let m = dynamic_block(&d, "door").expect("Door is a dynamic block");
    assert!(dynamic_block(&d, "*Model_Space").is_none(), "model space has no graph here");

    let labels: Vec<_> = m.parameters.iter().map(|p| p.kind.label().unwrap_or("-")).collect();
    assert_eq!(labels, ["Visibility1", "Width", "Size"]);
    let ParameterKind::Visibility { states, controlled, description, .. } = &m.parameters[0].kind else { panic!("{:?}", m.parameters[0]) };
    assert_eq!(description, "Door swing shown or not");
    assert_eq!(controlled, &[h("31"), h("32")]);
    let names: Vec<_> = states.iter().map(|s| (s.name.as_str(), s.visible.len(), s.active.len())).collect();
    assert_eq!(names, [("Open", 2, 1), ("Closed", 0, 0)]);

    let p = &m.parameters[1];
    assert_eq!((p.node, p.element.as_str(), p.show_properties), (2, "Linear", true));
    let ParameterKind::Linear { base, end, values, description, .. } = &p.kind else { panic!("{p:?}") };
    assert_eq!((base.x, base.y, end.x, end.y), (0.0, -4.0, 36.0, -4.0));
    assert_eq!(description, "Door leaf width");
    assert!(values.is_list() && !values.has_minimum());
    assert_eq!(values.list, [30.0, 32.0, 36.0]);

    let names: Vec<_> = m.actions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Stretch", "Lookup1"]);
    let a = &m.actions[0];
    assert_eq!(a.selection, [h("30")]);
    assert_eq!(a.connections, [(2, "EndXDelta".to_string()), (2, "EndYDelta".to_string())]);
    let ActionKind::Stretch { frame, vertices, multiplier, .. } = &a.kind else { panic!("{a:?}") };
    assert_eq!(frame.len(), 2);
    assert_eq!((frame[0].x, frame[1].y), (34.0, 2.0));
    assert_eq!(vertices, &[(h("30"), vec![1])]);
    assert_eq!(*multiplier, 1.0);

    let ActionKind::Lookup { columns, rows } = &m.actions[1].kind else { panic!("{:?}", m.actions[1]) };
    assert_eq!(rows, &[vec!["30".to_string(), "Narrow".to_string()], vec!["36".to_string(), "Wide".to_string()]]);
    let cols: Vec<_> = columns.iter().map(|c| (c.node, c.property.as_str(), c.lookup, c.unmatched.as_str())).collect();
    assert_eq!(cols, [(2, "UpdatedDistance", false, "Custom"), (4, "lookupString", true, "Custom")]);

    assert_eq!(m.other, ["BLOCKFLIPGRIP"]);
}

#[test]
fn a_saved_drawing_keeps_the_dynamic_block_as_read() {
    cadcraft_io::dwg_worker::allow_in_process();
    let d = read_dxf(dxf(&fixture()).as_bytes()).unwrap();
    let before = dynamic_block(&d, "Door").unwrap();
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    // The objects may get new handles on save; the block's objects keep theirs, and the graph's
    // references between its members follow the new ones.
    assert_eq!(by_node(dynamic_block(&again, "Door").unwrap()), by_node(before.clone()));
    // And through DWG: the lookup parameter (AutoCAD writes its marker AcDbBlockLookUpParameter)
    // keeps its name and lookup action.
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();
    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(by_node(dynamic_block(&back, "Door").unwrap()), by_node(before));
}

/// The Door fixture with a scale action on the Width parameter: graph node 6, object 68.
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
fn with_scale_action() -> String {
    let node = "91 6\n93 32\n95 7\n360 68\n92 -1\n92 -1\n92 -1\n92 -1\n";
    let action = "0 BLOCKSCALEACTION\n5 68\n330 61\n100 AcDbEvalExpr\n90 7\n98 33\n99 232\n\
        100 AcDbBlockElement\n300 Scale\n98 33\n99 232\n1071 0\n\
        100 AcDbBlockAction\n70 0\n71 1\n330 30\n1010 0\n1020 0\n1030 0\n\
        100 AcDbBlockActionWithBasePt\n92 2\n93 2\n301 UpdatedBaseX\n302 UpdatedBaseY\n\
        1011 0\n1021 0\n1031 0\n280 1\n1012 0\n1022 0\n1032 0\n\
        100 AcDbBlockScaleAction\n94 2\n95 2\n96 2\n303 Scale\n304 XScale\n305 YScale\n";
    fixture()
        .replacen("96 6\n97 6\n", "96 7\n97 7\n", 1)
        .replacen("0 BLOCKVISIBILITYPARAMETER\n", &format!("{node}0 BLOCKVISIBILITYPARAMETER\n"), 1)
        .replacen("0 BLOCKLOOKUPPARAMETER\n", &format!("{action}0 BLOCKLOOKUPPARAMETER\n"), 1)
}

/// A scale action goes through CADCraft's own DWG save (the bridge DXF, then acadrust) whole:
/// the DWG holds it with its connections, and it reads back as the same action. AutoCAD refused
/// such DWGs while acadrust left out the byte after its connections (craft-dwg's
/// `scale_actions_keep_the_byte_autocad_reads_after_their_connections` covers the byte).
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
#[test]
fn a_scale_action_survives_a_dwg_save() {
    use acadrust::objects::{DynamicBlockData, DynamicBlockObject, ObjectType};
    cadcraft_io::dwg_worker::allow_in_process();
    let d = read_dxf(dxf(&with_scale_action()).as_bytes()).unwrap();
    let before = dynamic_block(&d, "Door").unwrap();
    assert!(before.actions.iter().any(|a| a.node == 7 && matches!(a.kind, ActionKind::Scale { .. })), "{:?}", before.actions);
    let dwg = cadcraft_io::write(&d, "x.dwg").unwrap();

    let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&dwg)).read().unwrap();
    let scale = doc.objects.values().find_map(|o| match o {
        ObjectType::DynamicBlock(DynamicBlockObject { data: DynamicBlockData::ScaleAction(s), .. }) => Some(s.clone()),
        _ => None,
    });
    let scale = scale.expect("the DWG has no scale action");
    let names = |c: &[acadrust::objects::BlockConnection]| c.iter().map(|c| (c.code, c.name.clone())).collect::<Vec<_>>();
    assert_eq!(names(&scale.connections), [(2, "Scale".to_string()), (2, "XScale".into()), (2, "YScale".into())]);
    assert_eq!(names(&scale.action.connections), [(2, "UpdatedBaseX".to_string()), (2, "UpdatedBaseY".into())]);
    assert_eq!(scale.dwg_tail_byte, 0);

    let back = cadcraft_io::read(&dwg, "x.dwg").unwrap();
    assert_eq!(by_node(dynamic_block(&back, "Door").unwrap()), by_node(before));
}

/// The model with each parameter and action object named by its graph id instead of its handle.
fn by_node(mut m: DynamicBlock) -> DynamicBlock {
    let ids: std::collections::HashMap<Handle, i32> = m
        .parameters
        .iter()
        .map(|p| (p.handle, p.node))
        .chain(m.actions.iter().map(|a| (a.handle, a.node)))
        .chain(m.grip_objects.iter().map(|(n, h)| (*h, *n)))
        .collect();
    let id = |h: &Handle| Handle(ids.get(h).copied().unwrap_or(-1) as u64);
    for p in &mut m.parameters {
        p.handle = id(&p.handle);
        if let ParameterKind::Visibility { states, .. } = &mut p.kind {
            for s in states {
                s.active = s.active.iter().map(id).collect();
            }
        }
    }
    for a in &mut m.actions {
        a.handle = id(&a.handle);
    }
    m.grip_objects = m.grip_objects.iter().map(|(n, h)| (*n, id(h))).collect();
    m
}

#[test]
fn damaged_dynamic_block_data_is_skipped_not_trusted() {
    // Counts far beyond the groups present, values that aren't numbers, a graph member that
    // doesn't exist and a dictionary entry with no graph: reading must not panic or hang.
    let src = fixture()
        .replace("93 2\n331 31", "93 999999999\n331 31")
        .replace("92 2\n303 Open", "92 -7\n303 Open")
        .replace("175 3", "175 2147483647")
        .replace("144 32", "144 nan")
        .replace("72 2\n1011 34", "72 99999\n1011 34")
        .replace("92 2\n93 2\n301\n", "92 1000000\n93 1000000\n301\n")
        .replace("360 67", "360 FFFF");
    let d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let m = dynamic_block(&d, "Door").unwrap();
    assert_eq!(m.parameters.len(), 3);
    let ParameterKind::Linear { values, .. } = &m.parameters[1].kind else { panic!() };
    assert!(values.list.len() <= 3 && values.list.iter().all(|v| v.is_finite()), "{:?}", values.list);
    assert!(m.other.is_empty(), "a member that doesn't exist is skipped");

    let tags = |v: &[(i32, &str)]| v.iter().map(|(c, s)| RawTag { code: *c, value: (*s).into() }).collect::<Vec<_>>();
    let mut d = read_dxf(dxf(&fixture()).as_bytes()).unwrap();
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    if let Some(dict) = f.objects.iter_mut().find(|o| o.handle == h("60")) {
        dict.tags = tags(&[(100, "AcDbDictionary"), (3, "ACAD_ENHANCEDBLOCK")]);
    }
    assert!(dynamic_block(&d, "Door").is_none(), "no graph handle");
}

/// The fixture with references to Door: one with Door's own values (handle 80), and two to
/// anonymous blocks with values of their own, as another program leaves them: 81 (scale 2,
/// Closed, Width 30, so Size is Narrow) and 82 (Width 33, which no Size row has).
fn with_references() -> String {
    let record =
        |h: &str, name: &str| format!("0 BLOCK_RECORD\n5 {h}\n330 1\n100 AcDbSymbolTableRecord\n100 AcDbBlockTableRecord\n2 {name}\n340 0\n70 0\n");
    let block = |h: &str, rec: &str, name: &str| {
        format!(
            "0 BLOCK\n5 {h}\n330 {rec}\n100 AcDbEntity\n8 0\n100 AcDbBlockBegin\n2 {name}\n70 1\n10 0\n20 0\n30 0\n3 {name}\n1\n0 LINE\n5 {h}1\n330 {rec}\n100 AcDbEntity\n8 0\n100 AcDbLine\n10 0\n20 0\n30 0\n11 30\n21 0\n31 0\n0 ENDBLK\n5 {h}2\n330 {rec}\n100 AcDbEntity\n8 0\n100 AcDbBlockEnd\n"
        )
    };
    let insert = |h: &str, block: &str, xdict: Option<&str>, scale: f64| {
        let x = xdict.map(|x| format!("102 {{ACAD_XDICTIONARY\n360 {x}\n102 }}\n")).unwrap_or_default();
        format!(
            "0 INSERT\n5 {h}\n{x}330 1F\n100 AcDbEntity\n8 0\n100 AcDbBlockReference\n2 {block}\n10 100\n20 0\n30 0\n41 {scale}\n42 {scale}\n43 {scale}\n"
        )
    };
    let dict = |h: &str, owner: &str, entries: &[(&str, &str)]| {
        let e: String = entries.iter().map(|(k, v)| format!("3 {k}\n360 {v}\n")).collect();
        format!("0 DICTIONARY\n5 {h}\n330 {owner}\n100 AcDbDictionary\n280 1\n281 1\n{e}")
    };
    let xrecord =
        |h: &str, owner: &str, body: &str| format!("0 XRECORD\n5 {h}\n330 {owner}\n100 AcDbXrecord\n280 1\n1071 1\n1071 2\n70 25\n70 104\n{body}");
    // The values of a reference: representation data pointing at Door's record (1D) and one
    // XRECORD per parameter graph id.
    let values = |p: &str, owner: &str, vis: &str, width: f64, size: &str| {
        [
            dict(&format!("{p}0"), owner, &[("AcDbBlockRepresentation", &format!("{p}1"))]),
            dict(&format!("{p}1"), &format!("{p}0"), &[("AcDbRepData", &format!("{p}2")), ("AppDataCache", &format!("{p}3"))]),
            format!("0 ACDB_BLOCKREPRESENTATION_DATA\n5 {p}2\n330 {p}1\n100 AcDbBlockRepresentationData\n70 1\n340 1D\n"),
            dict(&format!("{p}3"), &format!("{p}1"), &[("ACAD_ENHANCEDBLOCKDATA", &format!("{p}4"))]),
            dict(
                &format!("{p}4"),
                &format!("{p}3"),
                &[("1", &format!("{p}5")), ("2", &format!("{p}6")), ("3", &format!("{p}7")), ("4", &format!("{p}8"))],
            ),
            xrecord(&format!("{p}5"), &format!("{p}4"), &format!("10 18\n20 40\n30 0\n1 {vis}\n")),
            xrecord(&format!("{p}6"), &format!("{p}4"), &format!("10 0\n20 -4\n30 0\n10 {width}\n20 -4\n30 0\n10 0\n20 0\n30 -1\n")),
            xrecord(&format!("{p}7"), &format!("{p}4"), "40 0\n"),
            xrecord(&format!("{p}8"), &format!("{p}4"), &format!("10 -6\n20 40\n30 0\n1 {size}\n")),
        ]
        .concat()
    };
    let src = dxf(&fixture());
    let lines = |s: String| dxf(&s);
    src.replace("0\nENDTAB\n", &format!("{}0\nENDTAB\n", lines(record("A1", "*U1") + &record("B1", "*U2"))))
        .replace(
            "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n",
            &lines(
                block("A2", "A1", "*U1")
                    + &block("B2", "B1", "*U2")
                    + "0 ENDSEC\n0 SECTION\n2 ENTITIES\n"
                    + &insert("80", "Door", None, 1.0)
                    + &insert("81", "*U1", Some("C0"), 2.0)
                    + &insert("82", "*U2", Some("D0"), 1.0)
                    + "0 ENDSEC\n",
            ),
        )
        .replace(
            "0\nENDSEC\n0\nEOF\n",
            &format!("{}0\nENDSEC\n0\nEOF\n", lines(values("C", "81", "Closed", 30.0, "Narrow") + &values("D", "82", "Open", 33.0, "Narrow"))),
        )
}

fn props(d: &cadcraft_doc::Drawing, h: &str) -> (String, Vec<(String, String, Vec<String>)>) {
    let (name, ps) = cadcraft_doc::dynblock::custom_properties(d, Handle::parse_hex(h).unwrap()).unwrap();
    (name, ps.into_iter().map(|p| (p.name, p.value, p.choices)).collect())
}

#[test]
fn dynamic_block_references_show_their_values() {
    let d = read_dxf(with_references().as_bytes()).unwrap();
    assert_eq!(d.model.len(), 3, "the three references were read");
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();

    // Door's own values.
    let (name, p) = props(&d, "80");
    assert_eq!(name, "Door");
    assert_eq!(
        p,
        [
            ("Visibility1".into(), "Open".into(), s(&["Open", "Closed"])),
            ("Width".into(), "36.0000".into(), s(&["30.0000", "32.0000", "36.0000"])),
            ("Size".into(), "Wide".into(), s(&["Narrow", "Wide"])),
        ]
    );

    // An anonymous block with values of its own, scaled 2: lengths in drawing units.
    let r = cadcraft_doc::dynblock::reference(&d, h("81")).unwrap();
    assert_eq!(r.definition, "Door");
    assert_eq!(r.values.len(), 3, "the action's record has no value");
    let (name, p) = props(&d, "81");
    assert_eq!(name, "Door");
    assert_eq!(
        p,
        [
            ("Visibility1".into(), "Closed".into(), s(&["Open", "Closed"])),
            ("Width".into(), "60.0000".into(), s(&["60.0000", "64.0000", "72.0000"])),
            ("Size".into(), "Narrow".into(), s(&["Narrow", "Wide"])),
        ]
    );

    // A width no Size row has: the lookup shows its "no match" name, whatever was stored.
    let (_, p) = props(&d, "82");
    assert_eq!(p[2], ("Size".into(), "Custom".into(), s(&["Narrow", "Wide"])));

    // Other references and objects aren't dynamic block references.
    assert!(cadcraft_doc::dynblock::reference(&d, h("30")).is_none(), "a line in the block");
    assert!(cadcraft_doc::dynblock::reference(&d, h("FFFF")).is_none());
}

#[test]
fn dynamic_block_references_survive_a_save() {
    let d = read_dxf(with_references().as_bytes()).unwrap();
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    for hx in ["80", "81", "82"] {
        assert_eq!(props(&again, hx), props(&d, hx), "{hx}");
    }
}

fn block_of(d: &cadcraft_doc::Drawing, h: &str) -> String {
    match d.entity(Handle::parse_hex(h).unwrap()).map(|e| &e.kind) {
        Some(cadcraft_doc::EntityKind::Insert(i)) => i.block.clone(),
        _ => panic!("no reference {h}"),
    }
}

#[test]
fn changing_the_visibility_state_makes_a_new_anonymous_block() {
    let mut d = read_dxf(with_references().as_bytes()).unwrap();
    // Door's own reference to Closed: the swing (lines 31, 32) hidden, the leaf (30) shown.
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "visibility1", "closed").unwrap();
    let name = block_of(&d, "80");
    assert!(name.starts_with("*U") && name != "*U1" && name != "*U2", "{name}");
    let shown: Vec<bool> = d.block(&name).unwrap().entities.iter().map(|e| e.common.visible).collect();
    assert_eq!(shown, [true, false, false]);
    assert!(d.block("Door").unwrap().entities.iter().all(|e| e.common.visible), "the dynamic block is unchanged");
    let (def, p) = props(&d, "80");
    assert_eq!((def.as_str(), p[0].1.as_str()), ("Door", "Closed"));

    // A reference with values of its own (an anonymous block) back to Open: its value record is
    // updated, and its old anonymous block, used by nothing else, goes.
    cadcraft_doc::dynblock::set_property(&mut d, h("81"), "Visibility1", "Open").unwrap();
    assert_eq!(props(&d, "81").1[0].1, "Open");
    assert!(d.block("*U1").is_none(), "the old anonymous block is dropped");
    assert_eq!(props(&d, "81").1[1].1, "60.0000", "its other values stay");

    // Both survive a save.
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&again, "80").1[0].1, "Closed");
    assert_eq!(props(&again, "81").1[0].1, "Open");
    let shown: Vec<bool> = again.block(&block_of(&again, "80")).unwrap().entities.iter().map(|e| e.common.visible).collect();
    assert_eq!(shown, [true, false, false]);
}

#[test]
fn bad_values_are_refused_and_change_nothing() {
    let mut d = read_dxf(with_references().as_bytes()).unwrap();
    let before = d.clone();
    let ps = cadcraft_doc::dynblock::custom_properties(&d, h("80")).unwrap().1;
    let editable: Vec<(&str, bool)> = ps.iter().map(|p| (p.name.as_str(), p.editable)).collect();
    assert_eq!(editable, [("Visibility1", true), ("Width", true), ("Size", true)]);
    for (name, value) in [("Visibility1", "Ajar"), ("Size", "Huge"), ("Width", "wide"), ("Width", "-3"), ("Width", "inf"), ("Nope", "x"), ("", "")] {
        assert!(cadcraft_doc::dynblock::set_property(&mut d, h("80"), name, value).is_err(), "{name}={value}");
    }
    assert!(cadcraft_doc::dynblock::set_property(&mut d, h("30"), "Visibility1", "Open").is_err(), "not a reference");
    assert!(cadcraft_doc::dynblock::set_property(&mut d, h("FFFF"), "Visibility1", "Open").is_err());
    assert_eq!(d, before);
}

/// The ends of the lines of the block reference `insert` shows, in block coordinates.
fn line_ends(d: &cadcraft_doc::Drawing, insert: &str) -> Vec<(f64, f64)> {
    let b = d.block(&block_of(d, insert)).unwrap();
    b.entities.iter().filter_map(|e| if let cadcraft_doc::EntityKind::Line(l) = &e.kind { Some((l.b.x, l.b.y)) } else { None }).collect()
}

#[test]
fn a_distance_stretches_the_block_and_snaps_to_its_values() {
    let mut d = read_dxf(with_references().as_bytes()).unwrap();
    // Width 36 → 30: the stretch moves the leaf's end (line 30, vertex 1) by -6 in x.
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Width", "30").unwrap();
    assert_eq!(line_ends(&d, "80")[0], (30.0, 0.0));
    let p = props(&d, "80").1;
    assert_eq!((p[1].1.as_str(), p[2].1.as_str()), ("30.0000", "Narrow"), "the lookup follows");
    // 33 is not in Width's list (30, 32, 36): the nearest is taken.
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "width", "33").unwrap();
    assert_eq!(line_ends(&d, "80")[0], (32.0, 0.0));
    assert_eq!(props(&d, "80").1[2].1, "Custom", "no Size row has 32");
    // Lengths are in drawing units: reference 81 is scaled 2, so 72 is Width 36.
    cadcraft_doc::dynblock::set_property(&mut d, h("81"), "Width", "72").unwrap();
    assert_eq!(props(&d, "81").1[1].1, "72.0000");
    // A save keeps the stretched geometry and the values.
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(line_ends(&again, "80")[0], (32.0, 0.0));
    assert_eq!(props(&again, "80").1[1].1, "32.0000");
}

#[test]
fn a_lookup_value_sets_its_distance() {
    let mut d = read_dxf(with_references().as_bytes()).unwrap();
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Size", "narrow").unwrap();
    assert_eq!(line_ends(&d, "80")[0], (30.0, 0.0));
    let p = props(&d, "80").1;
    assert_eq!((p[1].1.as_str(), p[2].1.as_str()), ("30.0000", "Narrow"));
}

#[test]
fn connections_are_read_in_either_group_order() {
    // AutoCAD writes 92 301 93 302; the DWG bridge 92 93 301 302.
    let src = fixture().replace("92 2\n93 2\n301 EndXDelta\n302 EndYDelta", "92 2\n301 EndXDelta\n93 2\n302 EndYDelta");
    assert_ne!(src, fixture());
    let d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let m = dynamic_block(&d, "Door").unwrap();
    assert_eq!(m.actions[0].connections, [(2, "EndXDelta".to_string()), (2, "EndYDelta".to_string())]);
}

#[test]
fn parameters_in_a_stretch_selection_are_carried_along() {
    // A point parameter (Position1 at (35, 0), inside the stretch frame) in the stretch's selection.
    let point = "0 BLOCKPOINTPARAMETER\n5 68\n330 61\n100 AcDbEvalExpr\n90 7\n98 33\n99 232\n100 AcDbBlockElement\n300 Point\n98 33\n99 232\n1071 0\n100 AcDbBlockParameter\n280 1\n281 0\n100 AcDbBlock1PtParameter\n1010 35\n1020 0\n1030 0\n93 12\n170 0\n171 0\n100 AcDbBlockPointParameter\n303 Position1\n304\n1011 35\n1021 4\n1031 0\n";
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 2\n330 30\n330 68\n1010 40")
        .replace("91 5\n93 32\n95 6\n360 67", "91 5\n93 32\n95 6\n360 67\n92 -1\n92 -1\n92 -1\n92 -1\n91 6\n93 32\n95 7\n360 68")
        .replace("0 BLOCKFLIPGRIP", &format!("{point}0 BLOCKFLIPGRIP"));
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    assert!(dynamic_block(&d, "Door").unwrap().parameters.iter().any(|p| p.kind.label() == Some("Position1")));
    // A reference to Door with Door's own values.
    let at = d.model.iter().count();
    assert_eq!(at, 0);
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: cadcraft_doc::geom::Vec3::new(0.0, 0.0, 0.0),
        scale: cadcraft_doc::geom::Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Insert(ins)).unwrap();
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let ps = cadcraft_doc::dynblock::custom_properties(&d, r).unwrap().1;
    let pos: Vec<(&str, &str)> = ps.iter().filter(|p| p.name.starts_with("Position1")).map(|p| (p.name.as_str(), p.value.as_str())).collect();
    assert_eq!(pos, [("Position1 X", "29.0000"), ("Position1 Y", "0.0000")]);
    // Whether a parameter's point is in the stretch frame (34,-2)-(38,2) is decided where Door
    // has it, as AutoCAD evaluates from the block: moved out to x = 60 first, it still follows.
    {
        let mut d = d.clone();
        cadcraft_doc::dynblock::set_property(&mut d, r, "Position1 X", "60").unwrap();
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "36").unwrap();
        let ps = cadcraft_doc::dynblock::custom_properties(&d, r).unwrap().1;
        assert_eq!(ps.iter().find(|p| p.name == "Position1 X").map(|p| p.value.as_str()), Some("66.0000"));
    }
    // A position set directly is recorded as the drag of its grip (12), from where Door has it.
    cadcraft_doc::dynblock::set_property(&mut d, r, "Position1 X", "20").unwrap();
    let x = d.foreign.entities.get(&r).and_then(|x| x.xdict).unwrap();
    let cache = dict_entry(&d, x, "AcDbBlockRepresentation").and_then(|rep| dict_entry(&d, rep, "AppDataCache")).unwrap();
    let history = dict_entry(&d, cache, "ACAD_ENHANCEDBLOCKHISTORY").unwrap();
    let tags: Vec<(i32, String)> =
        d.foreign.objects.iter().find(|o| o.handle == history).unwrap().tags.iter().map(|t| (t.code, t.value.clone())).collect();
    let grip = tags.iter().position(|t| *t == (1071, "12".to_string())).expect("grip entry");
    assert_eq!(tags[grip + 1..grip + 3], [(300, "GRIPLOC".to_string()), (11, "-15".to_string())]);
}

/// A reference to Door at the origin with Door's own values, in a drawing read from `src`.
fn door_reference(src: &str) -> (cadcraft_doc::Drawing, Handle) {
    let mut d = read_dxf(dxf(src).as_bytes()).unwrap();
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: cadcraft_doc::geom::Vec3::new(0.0, 0.0, 0.0),
        scale: cadcraft_doc::geom::Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Insert(ins)).unwrap();
    (d, r)
}

/// The points of the lines and polylines of reference `r`'s block, in its order.
fn shapes(d: &cadcraft_doc::Drawing, r: Handle) -> Vec<Vec<(f64, f64)>> {
    let b = d.block(&block_of(d, &r.hex())).unwrap();
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Line(l) => Some(vec![(l.a.x, l.a.y), (l.b.x, l.b.y)]),
            cadcraft_doc::EntityKind::LwPolyline(p) => Some(p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stretch_moves_the_points_inside_its_frame_not_the_listed_ones() {
    // The stretch (frame (34,-2)-(38,2)) also acts on line 32, which is all outside the frame, and
    // on a polyline whose last two vertices are inside it; and the vertex lists it stores are
    // wrong for all three. AutoCAD 2024 moves the points inside the frame, measured on a dynamic
    // block from another program whose lists didn't match its objects: every point of its lines and
    // polylines moved exactly when it was inside the frame, whatever the lists said. A selected
    // line or polyline all outside the frame moves whole, as in AutoCAD's own stretched copies.
    let polyline = "0 LWPOLYLINE\n5 33\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbPolyline\n90 3\n70 0\n10 20\n20 1\n10 36\n20 1\n10 36\n20 -1\n";
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 3\n330 30\n330 32\n330 33\n1010 40")
        .replace("73 1\n331 30\n74 1\n94 1\n75 0", "73 3\n331 30\n74 1\n94 0\n331 32\n74 1\n94 1\n331 33\n74 1\n94 0\n75 0")
        .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{polyline}0 ENDBLK\n"));
    let (mut d, r) = door_reference(&src);
    let ActionKind::Stretch { vertices, .. } = &dynamic_block(&d, "Door").unwrap().actions[0].kind else { panic!("a stretch action") };
    assert_eq!(vertices, &[(h("30"), vec![0]), (h("32"), vec![1]), (h("33"), vec![0])]);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let want: Vec<Vec<(f64, f64)>> = vec![
        vec![(0.0, 0.0), (30.0, 0.0)],
        vec![(0.0, 0.0), (0.0, 36.0)],
        vec![(-6.0, 0.0), (19.0, 25.0)],
        vec![(20.0, 1.0), (30.0, 1.0), (30.0, -1.0)],
    ];
    assert_eq!(shapes(&d, r), want);
}

/// The outlines of the wipeouts of reference `r`'s block, in its order.
fn wipeouts(d: &cadcraft_doc::Drawing, r: Handle) -> Vec<Vec<(f64, f64)>> {
    let b = d.block(&block_of(d, &r.hex())).unwrap();
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Wipeout(w) => Some(w.boundary.iter().map(|p| (p.x, p.y)).collect()),
            _ => None,
        })
        .collect()
}

/// A WIPEOUT `handle` of the door block whose outline is `points`: a 1 by 1 image at the origin,
/// whose clip point (x - 0.5, 0.5 - y) the reader maps back to (x, y).
fn wipeout(handle: &str, points: &[(f64, f64)]) -> String {
    let mut s = format!(
        "0 WIPEOUT\n5 {handle}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbWipeout\n10 0\n20 0\n30 0\n11 1\n21 0\n31 0\n12 0\n22 1\n32 0\n13 1\n23 1\n71 2\n91 {}\n",
        points.len()
    );
    for (x, y) in points {
        s.push_str(&format!("14 {}\n24 {}\n", x - 0.5, 0.5 - y));
    }
    s
}

#[test]
fn a_stretch_moves_the_points_of_a_wipeout_inside_its_frame() {
    // The stretch (frame (34,-2)-(38,2)) also acts on wipeout 34, whose right side is inside the
    // frame and whose outline repeats its first point to close, and on wipeout 35, all outside it.
    // The list stored for 34 names its left side. AutoCAD 2024 moves the points of a wipeout that
    // are inside the frame and leaves the others, whatever the list says: measured on its own
    // stretched copies of a dynamic block from another program, where each of three copies moved
    // the 324 points of the block's 521-point wipeout inside the frame by the distance's change and
    // left the other 197. A wipeout all outside the frame moves whole, as a line or polyline does
    // (not measured for a wipeout).
    let crossing = [(36.0, -1.0), (36.0, 1.0), (20.0, 1.0), (20.0, -1.0), (36.0, -1.0)];
    let outside = [(0.0, 5.0), (4.0, 5.0), (4.0, 8.0)];
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 3\n330 30\n330 34\n330 35\n1010 40")
        .replace("73 1\n331 30\n74 1\n94 1\n75 0", "73 2\n331 30\n74 1\n94 1\n331 34\n74 2\n94 2\n94 3\n75 0")
        .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{}{}0 ENDBLK\n", wipeout("34", &crossing), wipeout("35", &outside)));
    let (mut d, r) = door_reference(&src);
    assert_eq!(wipeouts(&d, r), [crossing.to_vec(), outside.to_vec()]);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let want = [vec![(30.0, -1.0), (30.0, 1.0), (20.0, 1.0), (20.0, -1.0), (30.0, -1.0)], vec![(-6.0, 5.0), (-2.0, 5.0), (-2.0, 8.0)]];
    assert_eq!(wipeouts(&d, r), want);
    assert_eq!(shapes(&d, r)[0], [(0.0, 0.0), (30.0, 0.0)]);
}

#[test]
fn wipeouts_and_arcs_move_whole_when_the_frame_cant_be_tested() {
    // The frame as a polygon of `n` points, as in the test of the stored lists. Up to 1000 points
    // the points of wipeout 34 and the end of arc 35 inside it move; past that both move whole, as
    // before: the lists stored for them (their first point alone) aren't used, as a wipeout's or an
    // arc's list isn't measured.
    let frame = |n: usize| {
        let mut f = format!("72 {n}\n1011 34\n1021 -2\n1011 38\n1021 -2\n1011 38\n1021 2\n");
        f.push_str(&"1011 34\n1021 2\n".repeat(n - 3));
        f
    };
    let crossing = [(36.0, -1.0), (36.0, 1.0), (20.0, 1.0), (20.0, -1.0)];
    let angle = |x: f64, y: f64| y.atan2(x).to_degrees();
    let cases = [
        (1000, [(30.0, -1.0), (30.0, 1.0), (20.0, 1.0), (20.0, -1.0)], ((30.0, 0.0), (20.0, 0.0), 5.125)),
        (1001, [(30.0, -1.0), (30.0, 1.0), (14.0, 1.0), (14.0, -1.0)], ((30.0, 0.0), (14.0, 0.0), 10.0)),
    ];
    for (n, want, want_arc) in cases {
        let src = fixture()
            .replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", &frame(n))
            .replace("71 1\n330 30\n1010 40", "71 3\n330 30\n330 34\n330 35\n1010 40")
            .replace("73 1\n331 30\n74 1\n94 1\n75 0", "73 3\n331 30\n74 1\n94 1\n331 34\n74 1\n94 0\n331 35\n74 1\n94 0\n75 0")
            .replace(
                "21 25\n31 0\n0 ENDBLK\n",
                &format!("21 25\n31 0\n{}{}0 ENDBLK\n", wipeout("34", &crossing), arc("35", 28.0, -6.0, 10.0, angle(8.0, 6.0), angle(-8.0, 6.0))),
            );
        let (mut d, r) = door_reference(&src);
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
        assert_eq!(wipeouts(&d, r), [want.to_vec()], "{n} frame points");
        let got = arcs(&d, r);
        assert!(got.len() == 1 && near(got[0], want_arc), "{n} frame points: {got:?}");
    }
}

#[test]
fn a_tilted_arc_moves_whole() {
    // An arc in the plane whose normal is x: its centre, at (28, -6) seen from above, and its
    // angles are in that plane, so its ends can't be tested against the frame. It moves whole, as
    // before (not measured in AutoCAD).
    let tilted = "0 ARC\n5 34\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbCircle\n10 -6\n20 0\n30 28\n40 10\n210 1\n220 0\n230 0\n100 AcDbArc\n50 36.86989764584402\n51 143.13010235415598\n";
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 2\n330 30\n330 34\n1010 40")
        .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{tilted}0 ENDBLK\n"));
    let (mut d, r) = door_reference(&src);
    let centre = |d: &cadcraft_doc::Drawing| {
        let b = d.block(&block_of(d, &r.hex())).unwrap();
        b.entities.iter().find_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Arc(a) => Some((a.center, a.radius)),
            _ => None,
        })
    };
    let (before, radius) = centre(&d).expect("the tilted arc is read as an arc");
    assert!((before.x - 28.0).abs() < 1e-9 && (before.y + 6.0).abs() < 1e-9, "{before:?}");
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let (after, r2) = centre(&d).unwrap();
    assert_eq!((after.x - before.x, after.y - before.y, after.z - before.z, r2), (-6.0, 0.0, 0.0, radius));
}

/// An ARC `handle` of the door block: centre (`x`, `y`), radius `r`, from `start` to `end` degrees.
fn arc(handle: &str, x: f64, y: f64, r: f64, start: f64, end: f64) -> String {
    format!("0 ARC\n5 {handle}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbCircle\n10 {x}\n20 {y}\n30 0\n40 {r}\n100 AcDbArc\n50 {start}\n51 {end}\n")
}

/// The arcs of reference `r`'s block, in its order: their start and end points and radius.
fn arcs(d: &cadcraft_doc::Drawing, r: Handle) -> Vec<((f64, f64), (f64, f64), f64)> {
    let b = d.block(&block_of(d, &r.hex())).unwrap();
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Arc(a) => {
                let c = a.center.xy();
                let (s, e) = (cadcraft_doc::geom::Vec2::polar(c, a.radius, a.start), cadcraft_doc::geom::Vec2::polar(c, a.radius, a.end));
                Some(((s.x, s.y), (e.x, e.y), a.radius))
            }
            _ => None,
        })
        .collect()
}

fn near(a: ((f64, f64), (f64, f64), f64), b: ((f64, f64), (f64, f64), f64)) -> bool {
    let d = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).hypot(p.1 - q.1);
    d(a.0, b.0) < 1e-9 && d(a.1, b.1) < 1e-9 && (a.2 - b.2).abs() < 1e-9
}

#[test]
fn a_stretch_moves_an_arc_end_inside_its_frame_and_keeps_its_height() {
    // Arc 34 runs from (36, 0), inside the frame (34,-2)-(38,2), over (28, 4) to (20, 0): 4 above
    // its chord. Arc 35 has both ends inside the frame. AutoCAD 2024 moves an arc's ends that are
    // inside the frame and keeps the arc's height over its chord: measured on its own stretched
    // copies of library blocks, where 21 arcs had one end moved and 63 had their ends moved by two
    // stretch actions by different amounts, all with their height unchanged to 1.1e-14 of it.
    let angle = |x: f64, y: f64| y.atan2(x).to_degrees();
    let src = fixture().replace("71 1\n330 30\n1010 40", "71 3\n330 30\n330 34\n330 35\n1010 40").replace(
        "21 25\n31 0\n0 ENDBLK\n",
        &format!(
            "21 25\n31 0\n{}{}0 ENDBLK\n",
            arc("34", 28.0, -6.0, 10.0, angle(8.0, 6.0), angle(-8.0, 6.0)),
            arc("35", 36.0, 0.0, 1.0, 0.0, 180.0)
        ),
    );
    let (mut d, r) = door_reference(&src);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    // The chord is now (30, 0)-(20, 0), 10 long, and the arc still rises 4 over it: radius
    // (5² + 4²) / (2 × 4) = 5.125. Arc 35 moves whole.
    let got = arcs(&d, r);
    let want = [((30.0, 0.0), (20.0, 0.0), 5.125), ((31.0, 0.0), (29.0, 0.0), 1.0)];
    assert_eq!(got.len(), 2);
    for (g, w) in got.iter().zip(want) {
        assert!(near(*g, w), "{g:?} != {w:?}");
    }
    assert_eq!(shapes(&d, r)[0], [(0.0, 0.0), (30.0, 0.0)]);
}

/// The door block with arc 34 (from (36, 0), inside the stretch's frame, over (28, 4) to (20, 0))
/// also held by the stretch, and flip parameter Flip state1 (graph id 7, reflection line from the
/// origin to `end`) with one flip action per entry of `held`, holding those objects. The flip
/// actions are listed in the evaluation graph before the stretch when `first`, else after it.
/// Lookup Size's Narrow row also sets Flip state1 to Flipped (Wide to Not flipped).
fn door_with_flip(end: (f64, f64), held: &[&[&str]], first: bool) -> String {
    let angle = |x: f64, y: f64| y.atan2(x).to_degrees();
    let node = |k: usize, id: usize, handle: &str| format!("91 {k}\n93 32\n95 {id}\n360 {handle}\n92 -1\n92 -1\n92 -1\n92 -1\n");
    let mut nodes = node(6, 7, "6A");
    let mut objects = format!(
        "0 BLOCKFLIPPARAMETER\n5 6A\n330 61\n100 AcDbEvalExpr\n90 7\n98 33\n99 232\n100 AcDbBlockElement\n300 Flip\n98 33\n99 232\n1071 0\n\
         100 AcDbBlockParameter\n280 1\n281 0\n100 AcDbBlock2PtParameter\n1010 0\n1020 0\n1030 0\n1011 {}\n1021 {}\n1031 0\n170 4\n91 0\n91 0\n\
         91 0\n91 0\n171 0\n172 0\n173 0\n174 0\n177 0\n100 AcDbBlockFlipParameter\n305 Flip state1\n306\n307 Not flipped\n308 Flipped\n\
         1012 -12\n1022 0\n1032 0\n309 UpdatedFlip\n96 0\n",
        end.0, end.1
    );
    for (k, objs) in held.iter().enumerate() {
        let handle = format!("6{}", ["B", "C", "D"][k]);
        nodes.push_str(&node(7 + k, 8 + k, &handle));
        objects.push_str(&format!(
            "0 BLOCKFLIPACTION\n5 {handle}\n330 61\n100 AcDbEvalExpr\n90 {}\n98 33\n99 232\n100 AcDbBlockElement\n300 Flip{}\n98 33\n99 232\n\
             1071 0\n100 AcDbBlockAction\n70 2\n91 0\n91 0\n71 {}\n{}1010 -6\n1020 0\n1030 0\n100 AcDbBlockFlipAction\n92 7\n93 7\n94 7\n\
             95 7\n301 Flip\n302 UpdatedFlip\n303 UpdatedBase\n304 UpdatedEnd\n",
            8 + k,
            k + 1,
            objs.len(),
            objs.iter().map(|h| format!("330 {h}\n")).collect::<String>()
        ));
    }
    // The graph's node counts, and its nodes: the new ones first, or after the last.
    let counts = format!("96 {0}\n97 {0}\n", 7 + held.len());
    let last = "360 67\n92 -1\n92 -1\n92 -1\n92 -1\n";
    let src = if first {
        fixture().replacen("96 6\n97 6\n", &format!("{counts}{nodes}"), 1)
    } else {
        fixture().replacen("96 6\n97 6\n", &counts, 1).replacen(last, &format!("{last}{nodes}"), 1)
    };
    src.replace("71 1\n330 30\n1010 40", "71 2\n330 30\n330 34\n1010 40")
        .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{}0 ENDBLK\n", arc("34", 28.0, -6.0, 10.0, angle(8.0, 6.0), angle(-8.0, 6.0))))
        .replace(
            "93 2\n301\n302 30\n302 Narrow\n302 36\n302 Wide\n303\n94 2\n95 1\n96 0\n282 0\n305 Custom\n281 0\n304 UpdatedDistance\n",
            "93 3\n301\n302 30\n302 1\n302 Narrow\n302 36\n302 0\n302 Wide\n303\n94 2\n95 1\n96 0\n282 0\n305 Custom\n281 0\n304 UpdatedDistance\n\
             303\n94 7\n95 1\n96 0\n282 0\n305 Custom\n281 0\n304 UpdatedFlip\n",
        )
        .replace("0 ENDSEC\n0 EOF", &format!("{objects}0 ENDSEC\n0 EOF"))
}

/// The point halfway round each arc of reference `r`'s block, in its order.
fn arc_middles(d: &cadcraft_doc::Drawing, r: Handle) -> Vec<(f64, f64)> {
    let b = d.block(&block_of(d, &r.hex())).unwrap();
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Arc(a) => {
                let sweep = (a.end - a.start).rem_euclid(std::f64::consts::TAU);
                let m = cadcraft_doc::geom::Vec2::polar(a.center.xy(), a.radius, a.start + sweep / 2.0);
                Some((m.x, m.y))
            }
            _ => None,
        })
        .collect()
}

/// Whether reference `r`'s one arc runs from `start` to `end` with radius `radius` through `mid`.
fn arc_is(d: &cadcraft_doc::Drawing, r: Handle, start: (f64, f64), end: (f64, f64), radius: f64, mid: (f64, f64)) -> bool {
    let (got, m) = (arcs(d, r), arc_middles(d, r));
    got.len() == 1 && near(got[0], (start, end, radius)) && (m[0].0 - mid.0).hypot(m[0].1 - mid.1) < 1e-9
}

#[test]
fn a_stretch_moves_the_end_of_a_flipped_arc_that_was_inside_its_frame() {
    // Flip state1 mirrors arc 34 across the x axis, and a mirror swaps an arc's start and end: in
    // the flipped reference the arc runs from (20, 0) under (28, -4) to (36, 0). A stretch then
    // moves the end that was inside its frame in the dynamic block, (36, 0), to (30, 0), and the
    // arc keeps its height of 4 over its chord, now below it; (20, 0) stays. Not flipped, the
    // block's start (36, 0) moves and the arc rises over the chord. A selection that repeats the
    // arc mirrors it once; two flip actions holding it mirror it back; a reflection line of no
    // length mirrors nothing.
    let over = ((30.0, 0.0), (20.0, 0.0), 5.125, (25.0, 4.0));
    let under = ((20.0, 0.0), (30.0, 0.0), 5.125, (25.0, -4.0));
    let cases: [(&str, (f64, f64), &[&[&str]], &str, _); 5] = [
        ("not flipped", (-10.0, 0.0), &[&["34"]], "Not flipped", over),
        ("flipped", (-10.0, 0.0), &[&["34"]], "Flipped", under),
        ("flipped, the arc held twice", (-10.0, 0.0), &[&["34", "34"]], "Flipped", under),
        ("flipped by two actions", (-10.0, 0.0), &[&["34"], &["34"]], "Flipped", over),
        ("flipped across no line", (0.0, 0.0), &[&["34"]], "Flipped", over),
    ];
    for (case, end, held, state, (s, e, radius, mid)) in cases {
        let (mut d, r) = door_reference(&door_with_flip(end, held, false));
        cadcraft_doc::dynblock::set_property(&mut d, r, "Flip state1", state).unwrap();
        let mirrored = s == under.0;
        let (s0, e0, m0) = if mirrored { ((20.0, 0.0), (36.0, 0.0), (28.0, -4.0)) } else { ((36.0, 0.0), (20.0, 0.0), (28.0, 4.0)) };
        assert!(arc_is(&d, r, s0, e0, 10.0, m0), "{case}, before the stretch: {:?} {:?}", arcs(&d, r), arc_middles(&d, r));
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
        assert!(arc_is(&d, r, s, e, radius, mid), "{case}: {:?} {:?}", arcs(&d, r), arc_middles(&d, r));
        assert_eq!(shapes(&d, r)[0], [(0.0, 0.0), (30.0, 0.0)], "{case}");
    }
}

#[test]
fn a_flip_and_a_stretch_in_one_change_move_the_arc_alike_in_either_order() {
    // Size Narrow sets Width to 30 and flips arc 34 in one change. Whether the flip action comes
    // before the stretch (the arc is mirrored, so the stretch moves its end) or after it (the
    // stretch moves its start, then it is mirrored), the arc ends from (20, 0) under (25, -4) to
    // (30, 0). Size Wide then sets Width back to 36 and flips the flipped reference back in one
    // change: the flip first mirrors the arc back (the stretch moves its start), the flip after
    // finds it still mirrored (the stretch moves its end); either way the arc is as the block has
    // it again.
    for first in [true, false] {
        let (mut d, r) = door_reference(&door_with_flip((-10.0, 0.0), &[&["34"]], first));
        let m = dynamic_block(&d, "Door").unwrap();
        let order: Vec<i32> = m.actions.iter().filter(|a| matches!(a.kind, ActionKind::Flip | ActionKind::Stretch { .. })).map(|a| a.node).collect();
        assert_eq!(order, if first { [8, 3] } else { [3, 8] });
        cadcraft_doc::dynblock::set_property(&mut d, r, "Size", "Narrow").unwrap();
        let p = props(&d, &r.hex()).1;
        let shown: Vec<(&str, &str)> = p.iter().map(|(n, v, _)| (n.as_str(), v.as_str())).collect();
        assert!(shown.contains(&("Width", "30.0000")) && shown.contains(&("Flip state1", "Flipped")), "{shown:?}");
        assert!(arc_is(&d, r, (20.0, 0.0), (30.0, 0.0), 5.125, (25.0, -4.0)), "flip first {first}: {:?} {:?}", arcs(&d, r), arc_middles(&d, r));
        cadcraft_doc::dynblock::set_property(&mut d, r, "Size", "Wide").unwrap();
        let p = props(&d, &r.hex()).1;
        let shown: Vec<(&str, &str)> = p.iter().map(|(n, v, _)| (n.as_str(), v.as_str())).collect();
        assert!(shown.contains(&("Width", "36.0000")) && shown.contains(&("Flip state1", "Not flipped")), "{shown:?}");
        assert!(arc_is(&d, r, (36.0, 0.0), (20.0, 0.0), 10.0, (28.0, 4.0)), "back, flip first {first}: {:?} {:?}", arcs(&d, r), arc_middles(&d, r));
    }
}

/// A HATCH `handle` of the door block, associative or not, with one straight loop through
/// `points` made from boundary objects `sources`.
fn hatch(handle: &str, associative: bool, points: &[(f64, f64)], sources: &[&str]) -> String {
    let mut s = format!(
        "0 HATCH\n5 {handle}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbHatch\n10 0\n20 0\n30 0\n210 0\n220 0\n230 1\n2 ANSI31\n70 0\n71 {}\n91 1\n92 3\n72 0\n73 1\n93 {}\n",
        u8::from(associative),
        points.len()
    );
    for (x, y) in points {
        s.push_str(&format!("10 {x}\n20 {y}\n"));
    }
    s.push_str(&format!("97 {}\n", sources.len()));
    for h in sources {
        s.push_str(&format!("330 {h}\n"));
    }
    s.push_str("75 0\n76 1\n52 0\n41 1\n77 0\n78 0\n98 0\n");
    s
}

/// The first loop of each hatch of reference `r`'s block, in its order.
fn hatch_loops(d: &cadcraft_doc::Drawing, r: Handle) -> Vec<Vec<(f64, f64)>> {
    let b = d.block(&block_of(d, &r.hex())).unwrap();
    b.entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Hatch(h) => Some(h.loops.first()?.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_associative_hatch_follows_its_stretched_boundary() {
    // Lines 36 to 39 outline (20,-1)-(36,1). The stretch acts on 36 (bottom), 37 (right) and 38
    // (top), whose ends at x = 36 are inside the frame, and on hatch 3B. Hatches 3A and 3B are
    // associative with that outline (3A's second vertex 2e-10 off the lines' ends, as a file's
    // rounding leaves it); 3C is the same but not associative; 3D's third vertex is 5e-8 off the
    // lines' ends at (36, 1), more than a rounding (a billionth of 36); 3E's second edge is curved
    // (bulge 0.5), which its lines can't give back.
    // AutoCAD 2024 fills an associative hatch again from its stretched boundary: in its own
    // stretched copies of library blocks, such a hatch's vertices are where its lines' ends went,
    // whether the stretch acts on the hatch or not. The others keep their loops.
    let rect = [(20.0, -1.0), (36.0, -1.0), (36.0, 1.0), (20.0, 1.0)];
    let mut lines = String::new();
    for (h, a, b) in [("36", rect[0], rect[1]), ("37", rect[1], rect[2]), ("38", rect[2], rect[3]), ("39", rect[3], rect[0])] {
        lines.push_str(&format!(
            "0 LINE\n5 {h}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbLine\n10 {}\n20 {}\n30 0\n11 {}\n21 {}\n31 0\n",
            a.0, a.1, b.0, b.1
        ));
    }
    let sources = ["36", "37", "38", "39"];
    let rounded = [(20.0, -1.0), (36.000_000_000_2, -1.0), (36.0, 1.0), (20.0, 1.0)];
    let off = [(20.0, -1.0), (36.0, -1.0), (36.0 - 5e-8, 1.0), (20.0, 1.0)];
    let curved = hatch("3E", true, &rect, &sources).replace("72 0\n", "72 1\n").replace("10 36\n20 -1\n", "10 36\n20 -1\n42 0.5\n");
    let hatches = [
        hatch("3A", true, &rounded, &sources),
        hatch("3B", true, &rect, &sources),
        hatch("3C", false, &rect, &sources),
        hatch("3D", true, &off, &sources),
        curved,
    ];
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 5\n330 30\n330 36\n330 37\n330 38\n330 3B\n1010 40")
        .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{lines}{}0 ENDBLK\n", hatches.concat()));
    let (mut d, r) = door_reference(&src);
    assert_eq!(hatch_loops(&d, r), [rounded.to_vec(), rect.to_vec(), rect.to_vec(), off.to_vec(), rect.to_vec()]);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let stretched = vec![(20.0, -1.0), (30.0, -1.0), (30.0, 1.0), (20.0, 1.0)];
    // 3B moved whole with the stretch, then took its boundary's points.
    assert_eq!(hatch_loops(&d, r), [stretched.clone(), stretched, rect.to_vec(), off.to_vec(), rect.to_vec()]);
}

#[test]
fn a_hatch_vertex_among_too_many_boundary_points_keeps_its_loop() {
    // Lines 36 to 39 outline (20,-1)-(35,1), and the stretch acts on 36 (bottom), 37 (right) and 38
    // (top), whose ends at x = 35 are inside the frame. The hatch's boundary also lists `n` lines
    // from 40 on, starting `gap` and then 1e-10 more further right of (35, -1) each, and left where
    // they are. With 14 of them, the vertex at (35, -1) has 16 boundary points within reach (theirs,
    // and the bottom and right lines' ends) and follows its lines. With 15 it has 17: it can't tell
    // which line it sits on, so the loop keeps what it has. A gap of 2e-8 puts their starts in the
    // next cell of the grid, still within a rounding (a billionth of 41) of the vertex.
    let rect = [(20.0, -1.0), (35.0, -1.0), (35.0, 1.0), (20.0, 1.0)];
    let line = |h: &str, a: (f64, f64), b: (f64, f64)| {
        format!("0 LINE\n5 {h}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbLine\n10 {}\n20 {}\n30 0\n11 {}\n21 {}\n31 0\n", a.0, a.1, b.0, b.1)
    };
    let followed = vec![(20.0, -1.0), (29.0, -1.0), (29.0, 1.0), (20.0, 1.0)];
    for (n, gap, want) in [(14, 0.0, followed), (15, 0.0, rect.to_vec()), (15, 2e-8, rect.to_vec())] {
        let mut lines = String::new();
        let mut sources: Vec<String> = Vec::new();
        for k in 1..=n {
            let h = format!("{:X}", 0x3F + k);
            let x = 35.0 + gap + f64::from(k) * 1e-10;
            lines.push_str(&line(&h, (x, -1.0), (x, -41.0)));
            sources.push(h);
        }
        for (h, a, b) in [("36", rect[0], rect[1]), ("37", rect[1], rect[2]), ("38", rect[2], rect[3]), ("39", rect[3], rect[0])] {
            lines.push_str(&line(h, a, b));
            sources.push(h.to_string());
        }
        let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
        let src = fixture()
            .replace("71 1\n330 30\n1010 40", "71 4\n330 30\n330 36\n330 37\n330 38\n1010 40")
            .replace("21 25\n31 0\n0 ENDBLK\n", &format!("21 25\n31 0\n{lines}{}0 ENDBLK\n", hatch("3A", true, &rect, &sources)));
        let (mut d, r) = door_reference(&src);
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
        assert_eq!(hatch_loops(&d, r), [want], "{n} lines {gap} beside the corner");
    }
}

#[test]
fn a_zero_distance_multiplier_moves_as_one_without_its_angle_offset() {
    // A stretch written with multiplier 0 and angle offset 1 (radian), and group 280 = 1. AutoCAD
    // 2024 moved such an action by the parameter's change, straight along it.
    let src = fixture().replace("140 1\n141 0\n280 0", "140 0\n141 1\n280 1");
    assert_ne!(src, fixture());
    let (mut d, r) = door_reference(&src);
    let ActionKind::Stretch { multiplier, angle_offset, .. } = &dynamic_block(&d, "Door").unwrap().actions[0].kind else {
        panic!("a stretch action")
    };
    assert_eq!((*multiplier, *angle_offset), (0.0, 1.0));
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    assert_eq!(shapes(&d, r)[0], [(0.0, 0.0), (30.0, 0.0)]);
}

#[test]
fn a_point_on_the_edge_of_a_polygon_frame_is_inside_it() {
    // Line 30 ends at (36, 0), and its start is outside every frame here. On the edge of the frame
    // (34,-2)-(36,2) given as two corners, the end is inside and moves alone. The same frame as a
    // polygon, triangles whose slanted edge passes through (36, 0), exactly or up to rounding, and a
    // square 5e-10 across with (36, 0) halfway along its right edge, take it the same way.
    let frames = [
        "72 2\n1011 34\n1021 -2\n1011 36\n1021 2\n",
        "72 4\n1011 34\n1021 -2\n1011 36\n1021 -2\n1011 36\n1021 2\n1011 34\n1021 2\n",
        "72 3\n1011 34\n1021 -2\n1011 38\n1021 2\n1011 34\n1021 2\n",
        "72 3\n1011 35.7\n1021 -0.3\n1011 36.7\n1021 0.7\n1011 30\n1021 10\n",
        "72 4\n1011 35.9999999995\n1021 -2.5e-10\n1011 36\n1021 -2.5e-10\n1011 36\n1021 2.5e-10\n1011 35.9999999995\n1021 2.5e-10\n",
    ];
    for frame in frames {
        let src = fixture().replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", frame);
        assert_ne!(src, fixture());
        let (mut d, r) = door_reference(&src);
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
        assert_eq!(shapes(&d, r)[0], [(0.0, 0.0), (30.0, 0.0)], "{frame}");
    }
}

#[test]
fn a_point_just_outside_a_tiny_frame_is_outside_it() {
    // A frame 1e-8 across whose right edge is 5e-10 short of line 30's end (36, 0): the end is
    // outside, so the line, with no point inside, moves whole. An edge's tolerance is a billionth of
    // its length, not of a drawing unit.
    let frame = "72 4\n1011 35.99999999\n1021 -1e-8\n1011 35.9999999995\n1021 -1e-8\n1011 35.9999999995\n1021 1e-8\n1011 35.99999999\n1021 1e-8\n";
    let src = fixture().replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", frame);
    assert_ne!(src, fixture());
    let (mut d, r) = door_reference(&src);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    assert_eq!(shapes(&d, r)[0], [(-6.0, 0.0), (30.0, 0.0)]);
}

#[test]
fn a_point_less_than_a_rounding_off_a_short_edge_is_outside_it() {
    // A triangle frame whose right edge runs from (35.99999999999999, -2e-10) to
    // (36.00000000000001, 3e-10): line 30's end (36, 0) is 1.4e-15 off it, past a billionth of the
    // edge's length (5e-19), and outside the frame, so the line moves whole. Measured from the
    // edge's start, the distance keeps digits that rounding near x = 36 would drop.
    let frame = "72 3\n1011 35.99999999999999\n1021 -2e-10\n1011 36.00000000000001\n1021 3e-10\n1011 35.999999999\n1021 0\n";
    let src = fixture().replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", frame);
    assert_ne!(src, fixture());
    let (mut d, r) = door_reference(&src);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    assert_eq!(shapes(&d, r)[0], [(-6.0, 0.0), (30.0, 0.0)]);
}

#[test]
fn a_point_past_the_end_of_a_frame_edge_is_outside_it() {
    // The frame's right edge runs along x = 36 from y = -2 to -1: line 30's end (36, 0) is on that
    // edge's line but past its end, outside the frame, so the line moves whole.
    let frame = "72 4\n1011 34\n1021 -2\n1011 36\n1021 -2\n1011 36\n1021 -1\n1011 34\n1021 -1\n";
    let src = fixture().replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", frame);
    assert_ne!(src, fixture());
    let (mut d, r) = door_reference(&src);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    assert_eq!(shapes(&d, r)[0], [(-6.0, 0.0), (30.0, 0.0)]);
}

#[test]
fn a_tilted_polyline_keeps_the_stored_vertex_list() {
    // Polyline 34 lies in a plane tilted about the y axis (extrusion (1,0,0)): it is read as seen
    // from above (#1886), (20, 1) as (0, 20), so the frame is tested against those points; none is
    // inside, so it moves whole. Polyline 36 is tilted by only 5e-10 (at an elevation of 2e9 that
    // moves it about a unit, below what reading projects): its vertices stay in its own plane and
    // the stored list (vertex 0) is used, as before. Polyline 35 is mirrored (extrusion (0,0,-1)):
    // it is read flipped into the block's plane, (-20, 1) as (20, 1), so the frame is tested.
    let polyline = |h: &str, [x0, x1]: [i32; 2], [ex, ey, ez]: [f64; 3], elevation: f64| {
        format!(
            "0 LWPOLYLINE\n5 {h}\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbPolyline\n90 2\n70 0\n38 {elevation}\n10 {x0}\n20 1\n10 {x1}\n20 1\n210 {ex}\n220 {ey}\n230 {ez}\n"
        )
    };
    let src = fixture()
        .replace("71 1\n330 30\n1010 40", "71 4\n330 30\n330 34\n330 35\n330 36\n1010 40")
        .replace("73 1\n331 30\n74 1\n94 1\n75 0", "73 4\n331 30\n74 1\n94 1\n331 34\n74 1\n94 0\n331 35\n74 1\n94 0\n331 36\n74 1\n94 0\n75 0")
        .replace(
            "21 25\n31 0\n0 ENDBLK\n",
            &format!(
                "21 25\n31 0\n{}{}{}0 ENDBLK\n",
                polyline("34", [20, 36], [1.0, 0.0, 0.0], 0.0),
                polyline("35", [-20, -36], [0.0, 0.0, -1.0], 0.0),
                polyline("36", [20, 36], [5e-10, 0.0, 1.0], 2e9)
            ),
        );
    let (mut d, r) = door_reference(&src);
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let s = shapes(&d, r);
    assert_eq!(s[3], [(-6.0, 20.0), (-6.0, 36.0)]);
    assert_eq!(s[4], [(20.0, 1.0), (30.0, 1.0)]);
    assert_eq!(s[5], [(14.0, 1.0), (36.0, 1.0)]);
}

#[test]
fn a_frame_with_too_many_points_to_test_one_by_one_keeps_the_stored_lists() {
    // The same frame as a polygon of `n` points (its four corners, the last repeated). Up to 1000
    // points each point of the objects is tested against it; past that the stored list (here the
    // line's start, outside the frame) is used, so a huge frame doesn't make every point test every
    // edge.
    let frame = |n: usize| {
        let mut f = format!("72 {n}\n1011 34\n1021 -2\n1011 38\n1021 -2\n1011 38\n1021 2\n");
        f.push_str(&"1011 34\n1021 2\n".repeat(n - 3));
        f
    };
    for (n, want) in [(1000, [(0.0, 0.0), (30.0, 0.0)]), (1001, [(-6.0, 0.0), (36.0, 0.0)])] {
        let src = fixture()
            .replace("72 2\n1011 34\n1021 -2\n1011 38\n1021 2\n", &frame(n))
            .replace("73 1\n331 30\n74 1\n94 1\n75 0", "73 1\n331 30\n74 1\n94 0\n75 0");
        let (mut d, r) = door_reference(&src);
        let ActionKind::Stretch { frame, .. } = &dynamic_block(&d, "Door").unwrap().actions[0].kind else { panic!("a stretch action") };
        assert_eq!(frame.len(), n);
        cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
        assert_eq!(shapes(&d, r)[0], want, "{n} frame points");
    }
}

/// The object entry `key` of dictionary `dict` refers to.
fn dict_entry(d: &cadcraft_doc::Drawing, dict: Handle, key: &str) -> Option<Handle> {
    let o = d.foreign.objects.iter().find(|o| o.handle == dict)?;
    let at = o.tags.iter().position(|t| t.code == 3 && t.value == key)?;
    o.tags.get(at + 1).and_then(|t| Handle::parse_hex(&t.value))
}

/// The groups of the reference's AppDataCache entry `key` (an object or a dictionary's keys).
fn cache_entry(d: &cadcraft_doc::Drawing, insert: &str, key: &str) -> Vec<(i32, String)> {
    let x = d.foreign.entities.get(&h(insert)).and_then(|x| x.xdict).expect("xdict");
    let cache = dict_entry(d, x, "AcDbBlockRepresentation").and_then(|r| dict_entry(d, r, "AppDataCache")).expect("cache");
    let Some(e) = dict_entry(d, cache, key) else { return Vec::new() };
    let o = d.foreign.objects.iter().find(|o| o.handle == e).expect("object");
    o.tags.iter().filter(|t| !matches!(t.code, 5 | 102 | 330 | 100 | 280)).map(|t| (t.code, t.value.clone())).collect()
}

#[test]
fn a_change_is_recorded_in_the_references_history_and_data() {
    // Width's grips are graph ids 8 (base) and 9 (end).
    let src = with_references().replace("170\n4\n91\n0\n91\n0\n", "170\n4\n91\n8\n91\n9\n");
    let mut d = read_dxf(src.as_bytes()).unwrap();
    assert_eq!(dynamic_block(&d, "Door").unwrap().parameters.iter().find(|p| p.node == 2).unwrap().grips, [8, 9]);
    let pairs = |v: &[(i32, &str)]| v.iter().map(|(c, s)| (*c, s.to_string())).collect::<Vec<_>>();

    // Door's own reference: new data with a record for each one the other references have.
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Visibility1", "Closed").unwrap();
    let keys: Vec<String> = cache_entry(&d, "80", "ACAD_ENHANCEDBLOCKDATA").into_iter().filter(|(c, _)| *c == 3).map(|(_, k)| k).collect();
    assert_eq!(keys, ["1", "2", "3", "4"]);
    assert_eq!(cache_entry(&d, "80", "ACAD_ENHANCEDBLOCKHISTORY"), pairs(&[(1070, "3"), (1071, "1"), (300, "Visibility1"), (1, "Closed")]));
    assert_eq!(d.block(&block_of(&d, "80")).unwrap().units, 0, "anonymous copies have no units");

    // A distance as typed in Properties: by name, in block units; it replaces a drag of its
    // grips. Entries are replaced, not added.
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Width", "30").unwrap();
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Width", "32").unwrap();
    cadcraft_doc::dynblock::set_property(&mut d, h("80"), "Visibility1", "Open").unwrap();
    let want = pairs(&[(1070, "3"), (1071, "2"), (300, "Width"), (40, "32"), (1071, "1"), (300, "Visibility1"), (1, "Open")]);
    assert_eq!(cache_entry(&d, "80", "ACAD_ENHANCEDBLOCKHISTORY"), want);

    // A reference with data and no history gets one (81 is scaled 2: 64 is 32 in block units);
    // both survive a save.
    cadcraft_doc::dynblock::set_property(&mut d, h("81"), "Width", "64").unwrap();
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let saved = pairs(&[(1070, "3"), (1071, "2"), (300, "Width"), (40, "32.0")]);
    assert_eq!(cache_entry(&again, "81", "ACAD_ENHANCEDBLOCKHISTORY"), saved);
    let saved = pairs(&[(1070, "3"), (1071, "2"), (300, "Width"), (40, "32.0"), (1071, "1"), (300, "Visibility1"), (1, "Open")]);
    assert_eq!(cache_entry(&again, "80", "ACAD_ENHANCEDBLOCKHISTORY"), saved);
}

/// Door with grips: Width's end grip (graph id 9, an object shown in the Open state; its base
/// grip 8 has no object) and the visibility parameter's grip (10).
fn with_grips() -> cadcraft_doc::Drawing {
    let grip = "0 BLOCKLINEARGRIP\n5 68\n330 61\n100 AcDbEvalExpr\n90 9\n98 33\n99 232\n100 AcDbBlockElement\n300 End Grip\n98 33\n99 232\n1071 0\n";
    let src = fixture()
        .replace("170 4\n91 0\n91 0\n", "170 4\n91 8\n91 9\n")
        .replace("1030 0\n93 0\n170 0\n171 0\n100 AcDbBlockVisibilityParameter", "1030 0\n93 10\n170 0\n171 0\n100 AcDbBlockVisibilityParameter")
        .replace("91 5\n93 32\n95 6\n360 67", "91 5\n93 32\n95 6\n360 67\n92 -1\n92 -1\n92 -1\n92 -1\n91 6\n93 32\n95 9\n360 68")
        .replace("95 1\n333 63\n", "95 2\n333 63\n333 68\n")
        .replace("0 BLOCKFLIPGRIP", &format!("{grip}0 BLOCKFLIPGRIP"));
    assert_eq!(src.matches("91 9\n").count(), 1, "the fixture changed");
    read_dxf(dxf(&src).as_bytes()).unwrap()
}

/// A reference to Door at (100, 0), scale 1.
fn door_at_100(d: &mut cadcraft_doc::Drawing) -> Handle {
    let ins = cadcraft_doc::Insert {
        block: "Door".into(),
        insert: cadcraft_doc::geom::Vec3::new(100.0, 0.0, 0.0),
        scale: cadcraft_doc::geom::Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    };
    d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), cadcraft_doc::EntityKind::Insert(ins)).unwrap()
}

#[test]
fn dynamic_references_show_the_grips_of_their_state() {
    use cadcraft_doc::dynblock::{GripPart, GripShape, grips};
    let mut d = with_grips();
    let r = door_at_100(&mut d);
    let g = grips(&d, r);
    let summary: Vec<(i32, GripPart, GripShape, (f64, f64), (f64, f64))> =
        g.iter().map(|g| (g.node, g.part, g.shape, (g.at.x, g.at.y), (g.dir.x, g.dir.y))).collect();
    // The visibility grip (always), Width's end arrow pointing along it (its base grip has no
    // object, so it isn't shown).
    assert_eq!(
        summary,
        [(1, GripPart::At, GripShape::List, (118.0, 40.0), (0.0, 0.0)), (2, GripPart::End, GripShape::Linear, (136.0, -4.0), (1.0, 0.0))]
    );
    assert_eq!(g[0].choices, ["Open", "Closed"]);
    assert_eq!(g[1].property, "Width");
    // Closed lists no grips: only the visibility grip is left.
    cadcraft_doc::dynblock::set_property(&mut d, r, "Visibility1", "Closed").unwrap();
    assert_eq!(grips(&d, r).iter().map(|g| g.node).collect::<Vec<_>>(), [1]);
    // Not a dynamic block reference: no grips.
    assert!(grips(&d, h("30")).is_empty());
}

#[test]
fn dragging_a_distance_grip_changes_the_distance() {
    use cadcraft_doc::dynblock::{GripPart, drag_grip};
    use cadcraft_doc::geom::Vec2;
    let mut d = with_grips();
    let r = door_at_100(&mut d);
    // The end arrow dragged to x = 130.4 (off the parameter's line too): Width 30.4 snaps to 30,
    // the stretch follows and so does the lookup.
    drag_grip(&mut d, r, 2, GripPart::End, Vec2::new(130.4, 7.0)).unwrap();
    assert_eq!(line_ends(&d, &r.hex())[0], (30.0, 0.0));
    let p = cadcraft_doc::dynblock::custom_properties(&d, r).unwrap().1;
    assert_eq!((p[1].value.as_str(), p[2].value.as_str()), ("30.0000", "Narrow"));
    // The base grip: the distance from the new base to the end (32 here).
    drag_grip(&mut d, r, 2, GripPart::Base, Vec2::new(98.0, -4.0)).unwrap();
    assert_eq!(cadcraft_doc::dynblock::custom_properties(&d, r).unwrap().1[1].value, "32.0000");
    // Past the other end, a non-finite point, a grip that doesn't drag: refused, nothing changes.
    let before = d.clone();
    for (node, part, to) in [
        (2, GripPart::End, Vec2::new(50.0, 0.0)),
        (2, GripPart::End, Vec2::new(f64::NAN, 0.0)),
        (1, GripPart::At, Vec2::new(120.0, 40.0)),
        (99, GripPart::End, Vec2::new(130.0, 0.0)),
    ] {
        assert!(drag_grip(&mut d, r, node, part, to).is_err(), "{node} {part:?} {to:?}");
    }
    assert!(drag_grip(&mut d, h("30"), 2, GripPart::End, Vec2::new(130.0, 0.0)).is_err(), "not a reference");
    assert_eq!(d, before);
}

/// The DOOR drawing with every handle moved above 2^48: AutoCAD writes such handles (seen in
/// drawings whose handles start at 0xDAD1…), and the dynamic block's objects must still be kept.
fn far_handles(src: &str) -> String {
    const SHIFT: u64 = 0x000D_AD10_0000_0000;
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
fn a_dynamic_block_with_handles_above_2_pow_48_is_kept() {
    let src = far_handles(&fixture());
    let d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let before = dynamic_block(&d, "Door").expect("the dynamic block with far handles");
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(by_node(dynamic_block(&again, "Door").unwrap()), by_node(before));
}

/// A multiline attribute (LABEL, two lines) in the stretch's selection: when Width changes, the
/// reference's attribute moves with its definition, its multiline text too.
#[test]
fn a_stretch_moves_a_multiline_attributes_text() {
    let attdef = "0 ATTDEF\n5 3A\n330 1D\n100 AcDbEntity\n8 0\n100 AcDbText\n10 35\n20 0\n30 0\n40 1\n1 SITE\n7 Standard\n100 AcDbAttributeDefinition\n280 0\n3 Label\n2 LABEL\n70 0\n280 1\n71 4\n72 0\n11 35\n21 0\n31 0\n101 Embedded Object\n10 35\n20 0\n30 0\n40 1\n41 0\n46 0\n71 1\n72 5\n1 SITE\\PNAME\n7 Standard\n73 1\n44 1\n";
    let src = fixture().replace("71 1\n330 30\n1010 40", "71 2\n330 30\n330 3A\n1010 40");
    // Into Door's block, before its end.
    let door = src.find("2 Door\n70 0").unwrap();
    let end = door + src[door..].find("0 ENDBLK").unwrap();
    let src = format!("{}{attdef}{}", &src[..end], &src[end..]);
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let def = d
        .block("Door")
        .unwrap()
        .entities
        .iter()
        .find_map(|e| if let cadcraft_doc::EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None })
        .expect("the attribute definition");
    assert!(def.multiline.is_some(), "a multiline definition");
    // A reference at (100, 0) with its attribute where the definition puts it.
    let r = door_at_100(&mut d);
    d.modify_entity(r, |e| {
        if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
            let mut a = def.clone();
            let m = cadcraft_doc::geom::Mat3::translate(cadcraft_doc::geom::Vec2::new(100.0, 0.0));
            let mut k = cadcraft_doc::EntityKind::Text(a.text.clone());
            k.transform(&m);
            if let cadcraft_doc::EntityKind::Text(t) = k {
                a.text = t;
            }
            a.transform_multiline(&m);
            i.attribs.push(a);
        }
    })
    .unwrap();
    // Width 36 → 30: the attribute moves 6 to the left, its multiline text with it.
    cadcraft_doc::dynblock::set_property(&mut d, r, "Width", "30").unwrap();
    let Some(cadcraft_doc::EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("no reference") };
    let a = &i.attribs[0];
    assert_eq!(a.text.insert.x, 129.0);
    assert_eq!(a.multiline.as_ref().unwrap().mtext.insert.x, 129.0);
    // Saved (in the drawing's version, 2018: R2000 DXF has no multiline attributes) and reopened:
    // the moved attribute and its multiline text stay where they went.
    let again = cadcraft_io::read(&cadcraft_io::write(&d, "door.dxf").unwrap(), "door.dxf").unwrap();
    let Some(cadcraft_doc::EntityKind::Insert(i)) = again.entity(r).map(|e| &e.kind) else { panic!("no reference after reopening") };
    let a = &i.attribs[0];
    assert_eq!((a.text.insert.x, a.text.insert.y), (129.0, 0.0));
    let m = &a.multiline.as_ref().expect("still multiline").mtext;
    assert_eq!((m.insert.x, m.insert.y), (129.0, 0.0));
    assert_eq!(m.contents, r"SITE\PNAME");
}
