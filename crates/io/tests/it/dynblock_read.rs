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
    for (name, value) in
        [("Visibility1", "Ajar"), ("Size", "Huge"), ("Width", "wide"), ("Width", "0"), ("Width", "-3"), ("Width", "inf"), ("Nope", "x"), ("", "")]
    {
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
