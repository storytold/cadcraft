//! The reading channels downstream identification leans on (the cadbot requirements):
//! TEXTS with `scope: "nested"` reading what placed references draw (once per copy), the raw
//! string besides the drawn one, BLOCKDEF.ENTITIES reaching anonymous definitions the Block
//! Editor refuses, ATTRIBS telling a declared-but-absent attribute from an empty one, the proxy
//! frame histogram, DRAWING.READABILITY's whole-drawing self-check, and the Block Editor's state
//! around panics and drawing switches.

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Block, EntityKind, Handle, RawTag, Unknown};
use cadcraft_engine::geom::Vec3;
use serde_json::{Value, json};

fn text(s: &mut Session, at: [f64; 2], value: &str) -> Value {
    s.execute("text", &json!({ "at": at, "text": value, "height": 2.5 })).unwrap()["handle"].clone()
}

/// A drawing with block "Door" (a text "M1022" at (10, 10), base point (10, 10), so block
/// coordinates put the text at the origin) and three references to it: at (0, 0), at (50, 0) and
/// one rotated 90° at (100, 0). Returns the references' handles.
fn doors(s: &mut Session) -> Vec<Value> {
    let t = text(s, [10.0, 10.0], "M1022");
    let r = s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [t] })).unwrap()["insert"].clone();
    let b = s.execute("insert", &json!({ "name": "Door", "at": [50, 0] })).unwrap()["handle"].clone();
    let c = s.execute("insert", &json!({ "name": "Door", "at": [100, 0], "rotation": 90 })).unwrap()["handle"].clone();
    vec![r, b, c]
}

fn texts(s: &mut Session, params: Value) -> Vec<Value> {
    let r = s.execute("texts", &params).unwrap();
    assert_eq!(r["truncated"], json!(false), "{r}");
    r["texts"].as_array().unwrap().clone()
}

#[test]
fn nested_texts_count_one_per_placed_reference() {
    let mut s = Session::new();
    doors(&mut s);

    // The space itself has no texts: the block command converted the one drawn.
    assert_eq!(s.execute("texts", &json!({})).unwrap()["count"], json!(0));

    // Nested scope reads what the three references draw: the definition's text three times.
    let hits = texts(&mut s, json!({ "scope": "nested" }));
    assert_eq!(hits.len(), 3, "{hits:?}");
    for h in &hits {
        assert_eq!(h["source"], json!("block:Door"), "{h}");
        assert_eq!(h["depth"], json!(1), "{h}");
        assert_eq!(h["type"], json!("Text"));
        assert_eq!(h["raw"], json!("M1022"));
        assert_eq!(h["text"], json!("M1022"));
    }
    // Where they are: the first is the reference BLOCK's Convert made, which holds the text where
    // it was drawn; the second is 50 along; the third, rotated 90° at (100, 0), holds its origin.
    let at: Vec<(f64, f64)> = hits.iter().map(|h| (h["at"][0].as_f64().unwrap(), h["at"][1].as_f64().unwrap())).collect();
    assert!((at[0].0 - 10.0).abs() < 1e-9 && (at[0].1 - 10.0).abs() < 1e-9, "{at:?}");
    assert!((at[1].0 - 50.0).abs() < 1e-9 && at[1].1.abs() < 1e-9, "{at:?}");
    assert!((at[2].0 - 100.0).abs() < 1e-9 && at[2].1.abs() < 1e-9, "{at:?}");
    // A rotated reference turns the text with it.
    assert!((hits[2]["rotation"].as_f64().unwrap() - 90.0).abs() < 1e-9, "{}", hits[2]);

    // `contains` filters nested hits too, and `count` is every match while the page is limited.
    let r = s.execute("texts", &json!({ "scope": "nested", "contains": "m10", "limit": 2 })).unwrap();
    assert_eq!(r["count"], json!(3));
    assert_eq!(r["returned"], json!(2));
}

#[test]
fn attribute_definitions_are_not_drawn_but_reference_attributes_are() {
    let mut s = Session::new();
    // A definition with a text and an attribute definition; the block converts both.
    let t = text(&mut s, [10.0, 10.0], "M1022");
    let a = s.execute("attdef", &json!({ "tag": "A", "at": [10, 12], "height": 2.5, "default": "" })).unwrap()["handle"].clone();
    s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [t, a], "keep": "delete" })).unwrap();
    s.execute("insert", &json!({ "name": "Door", "at": [0, 0], "attribs": { "A": "FM1" } })).unwrap();

    let hits = texts(&mut s, json!({ "scope": "nested" }));
    // The definition's own text once, and the reference's attribute — not the attribute
    // definition, which a reference does not draw.
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert_eq!(hits.iter().filter(|h| h["type"] == "Text" && h["source"] == "block:Door").count(), 1);
    let attr = hits.iter().find(|h| h["type"] == "Attribute").unwrap();
    assert_eq!(attr["tag"], json!("A"));
    assert_eq!(attr["text"], json!("FM1"));
    assert_eq!(attr["source"], json!("space"));
    assert_eq!(attr["depth"], json!(0));
}

#[test]
fn attribs_tell_declared_absent_from_empty_and_from_value() {
    let mut s = Session::new();
    let a = s.execute("attdef", &json!({ "tag": "A", "at": [0, 0], "height": 2.5, "default": "" })).unwrap()["handle"].clone();
    s.execute("block", &json!({ "name": "Door", "base": [0, 0], "handles": [a], "keep": "delete" })).unwrap();
    let with_value = s.execute("insert", &json!({ "name": "Door", "at": [0, 0], "attribs": { "A": "FM甲1222" } })).unwrap()["handle"].clone();
    let empty = s.execute("insert", &json!({ "name": "Door", "at": [10, 0] })).unwrap()["handle"].clone();
    let absent = s.execute("insert", &json!({ "name": "Door", "at": [20, 0] })).unwrap()["handle"].clone();
    // The third reference's attribute is gone, as one past ATT SYNC is: declared, not carried.
    {
        let h = absent.as_str().and_then(Handle::parse_hex).unwrap();
        let d = s.doc_mut().unwrap();
        let sp = d.space_of(h).unwrap();
        d.space_mut(&sp).unwrap().modify(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.attribs.clear();
            }
        });
    }

    let r = s.execute("attribs", &json!({ "block": "Door" })).unwrap();
    let refs = r["references"].as_array().unwrap();
    assert_eq!(r["count"], json!(3));
    let value_of = |h: &Value| -> Value { refs.iter().find(|r| r["handle"] == *h).unwrap()["attribs"][0]["value"].clone() };
    assert_eq!(value_of(&with_value), json!("FM甲1222"));
    assert_eq!(value_of(&empty), json!(""));
    assert_eq!(value_of(&absent), json!(null), "declared but not carried reads as null, not \"\"");
    let entry = refs.iter().find(|r| r["handle"] == absent).unwrap()["attribs"][0].clone();
    assert_eq!(entry["defined"], json!(true));
    assert_eq!(entry["present"], json!(false));
    assert_eq!(entry["source"], json!("none"));

    // A tag filter for one the definition never declares says so.
    let r = s.execute("attribs", &json!({ "handle": with_value, "tag": "B" })).unwrap();
    assert_eq!(r["references"].as_array().unwrap().len(), 0, "{r}");
}

/// An unmodelled entity of `dxf_type` carrying `tags`, placed in model space.
fn unknown(handle: Handle, dxf_type: &str, tags: Vec<RawTag>) -> cadcraft_engine::doc::Entity {
    cadcraft_engine::doc::Entity::new(handle, EntityKind::Unknown(Unknown { dxf_type: dxf_type.to_string(), tags }))
}

fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.to_string() }
}

/// A graphics metafile of `frames`: (type, payload) each, with the framing the format needs.
fn metafile(frames: &[(u32, &[u8])]) -> Vec<u8> {
    let mut data = vec![0u8; 8];
    for (ty, payload) in frames {
        let size = 8 + payload.len();
        data.extend_from_slice(&(size as u32).to_le_bytes());
        data.extend_from_slice(&ty.to_le_bytes());
        data.extend_from_slice(payload);
    }
    let total = data.len() as u32;
    data[0..4].copy_from_slice(&total.to_le_bytes());
    data[4..8].copy_from_slice(&(frames.len() as u32).to_le_bytes());
    data
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

#[test]
fn proxy_frames_are_counted_and_an_empty_shell_says_so() {
    let mut s = Session::new();
    // One proxy whose graphics is all matrix pushes and pops (the watermark kind), one with a
    // polyline frame, one without any graphics data.
    let matrix_only = metafile(&[(29, [0.0f64; 16].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>().as_slice()), (31, &[])]);
    let with_drawing = metafile(&[(29, [0.0f64; 16].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>().as_slice()), (6, [0u8; 4].as_slice())]);
    let h = {
        let d = s.doc_mut().unwrap();
        d.model.push(unknown(
            Handle(0x2A0),
            "ACAD_PROXY_ENTITY",
            vec![tag(0, "ACAD_PROXY_ENTITY"), tag(92, &matrix_only.len().to_string()), tag(310, &hex(&matrix_only))],
        ));
        d.model.push(unknown(
            Handle(0x2A1),
            "ACAD_PROXY_ENTITY",
            vec![tag(0, "ACAD_PROXY_ENTITY"), tag(160, &with_drawing.len().to_string()), tag(310, &hex(&with_drawing))],
        ));
        d.model.push(unknown(Handle(0x2A2), "ACAD_PROXY_ENTITY", vec![tag(0, "ACAD_PROXY_ENTITY"), tag(91, "500")]));
        d.model.iter().last().unwrap().handle.hex()
    };

    let r = s.execute("entities", &json!({ "type": "ACAD_PROXY_ENTITY" })).unwrap();
    let ents = r["entities"].as_array().unwrap();
    assert_eq!(r["count"], json!(3));
    let empty = ents.iter().find(|e| e["proxy"]["drawable"] == 0).unwrap();
    assert_eq!(empty["proxy"]["drawableChunks"]["PushModelXform"], json!(1));
    assert_eq!(empty["proxy"]["drawableChunks"]["PopModelXform"], json!(1));
    assert_eq!(empty["proxy"]["unparsed"], json!(0));
    let draws = ents.iter().find(|e| e["proxy"]["drawable"] == 1).unwrap();
    assert_eq!(draws["proxy"]["drawableChunks"]["Polyline"], json!(1));
    assert_eq!(draws["proxy"]["originalBytes"], json!(with_drawing.len()));
    assert!(ents.iter().any(|e| e["proxy"].is_null()), "no graphics data, no proxy object");

    // Inspect counts unmodelled entities under their own DXF name, not as one "Proxy" heap.
    let r = s.execute("drawing.inspect", &json!({ "entities": false })).unwrap();
    assert_eq!(r["counts"]["ACAD_PROXY_ENTITY"], json!(3), "{}", r["counts"]);

    // Readability: what draws, what cannot be judged, out of reach and why.
    let r = s.execute("drawing.readability", &json!({})).unwrap();
    assert_eq!(r["proxy"]["acadProxy"], json!(3));
    assert_eq!(r["proxy"]["withDrawable"], json!(1));
    assert_eq!(r["proxy"]["noGraphics"], json!(1));
    let unreachable = r["unreachable"].as_array().unwrap();
    assert_eq!(unreachable.iter().find(|u| u["kind"] == "proxyDrawable").unwrap()["count"], json!(1));
    assert_eq!(unreachable.iter().find(|u| u["kind"] == "proxyNoGraphics").unwrap()["count"], json!(1));
    assert_eq!(r["selfCheck"]["skipped"]["proxyDrawable"]["count"], json!(1));

    // The frame walk lives on one entity's tags: a broken blob counts unparsed, not as empty.
    let _ = h;
}

#[test]
fn blockdef_entities_reach_anonymous_definitions() {
    let mut s = Session::new();
    // An anonymous definition, as a drawing's own object makes one (`*U…`), with a line in it.
    {
        let d = s.doc_mut().unwrap();
        let mut b = Block::new("*U7");
        b.entities.push(cadcraft_engine::doc::Entity::new(
            Handle(0x2B1),
            EntityKind::Line(cadcraft_engine::doc::Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) }),
        ));
        d.blocks.insert(b.name.clone(), Arc::new(b));
    }
    // A reference to it, as the drawing itself would place one (INSERT's `*` prefix means
    // "explode", so the reference is placed as the drawing's own object would).
    let ins = {
        let d = s.doc_mut().unwrap();
        let e = cadcraft_engine::doc::Entity::new(
            Handle(0x2B2),
            EntityKind::Insert(cadcraft_engine::doc::Insert { block: "*U7".into(), insert: Vec3::new(5.0, 5.0, 0.0), ..Default::default() }),
        );
        d.model.push(e);
        d.model.iter().last().unwrap().handle.hex()
    };

    // The listing keeps anonymous definitions hidden unless asked, and counts references.
    let r = s.execute("blocks.list", &json!({})).unwrap();
    assert_eq!(r["count"], json!(0), "anonymous definitions stay hidden by default");
    let r = s.execute("blocks.list", &json!({ "includeAnonymous": true })).unwrap();
    let b = &r["blocks"][0];
    assert_eq!(b["name"], json!("*U7"));
    assert_eq!(b["anonymous"], json!(true));
    assert_eq!(b["refs"], json!(1));

    // The definition's objects, read-only — the Block Editor refuses this name, this does not.
    let err = s.execute("bedit", &json!({ "name": "*U7" })).unwrap_err();
    assert!(err.to_string().contains("anonymous block"), "{err}");
    let r = s.execute("blockdef.entities", &json!({ "name": "*U7" })).unwrap();
    assert_eq!(r["block"]["anonymous"], json!(true));
    assert_eq!(r["count"], json!(1));
    assert_eq!(r["entities"][0]["type"], json!("Line"));

    // Through a reference's handle: the definition it shows, with where it places it.
    let r = s.execute("blockdef.entities", &json!({ "insert": ins })).unwrap();
    assert_eq!(r["block"]["name"], json!("*U7"));
    assert_eq!(r["insert"]["handle"], ins);
    assert_eq!(r["count"], json!(1));
}

#[test]
fn readability_counts_spaces_definitions_and_missing_references() {
    let mut s = Session::new();
    doors(&mut s);
    text(&mut s, [200.0, 200.0], "NOTE %%c8");
    {
        // Two references to a block the drawing does not define, on the title-block layer, and a
        // gridded one: its copies sit on the lattice, inside the envelope with the others.
        let d = s.doc_mut().unwrap();
        let ghost = |h: u64, at: [f64; 2]| {
            let mut e = cadcraft_engine::doc::Entity::new(
                Handle(h),
                EntityKind::Insert(cadcraft_engine::doc::Insert {
                    block: "Ghost".into(),
                    insert: cadcraft_engine::geom::Vec3::new(at[0], at[1], 0.0),
                    ..Default::default()
                }),
            );
            e.common.layer = "frame".into();
            e
        };
        d.model.push(ghost(0x2C0, [100.0, 50.0]));
        d.model.push(ghost(0x2C1, [-20.0, 480.0]));
        let mut gridded = cadcraft_engine::doc::Entity::new(
            Handle(0x2C2),
            EntityKind::Insert(cadcraft_engine::doc::Insert {
                block: "Ghost".into(),
                insert: cadcraft_engine::geom::Vec3::new(0.0, 0.0, 0.0),
                cols: 2,
                col_spacing: 100.0,
                ..Default::default()
            }),
        );
        gridded.common.layer = "frame".into();
        d.model.push(gridded);
    }

    let r = s.execute("drawing.readability", &json!({})).unwrap();
    assert_eq!(r["modelEntities"], json!(7), "{}", r["message"]); // 3 references, 1 text, 3 ghosts (one gridded)
    assert_eq!(r["blockDefs"], json!(1));
    assert_eq!(r["anonymousRefs"], json!(0));
    assert_eq!(r["missingBlockRefs"], json!(4));
    assert_eq!(r["textsInModel"], json!(1));
    assert_eq!(r["textsInBlockDefs"], json!(1));
    assert_eq!(r["textsInBlocks"], json!(3), "one per placed reference");
    let unreachable = r["unreachable"].as_array().unwrap();
    // What the missing references are, so a pipeline can judge them without the drawing: the
    // block, the entities that place one, their layers, and where on the sheet they sit.
    let missing = unreachable.iter().find(|u| u["kind"] == "missingBlock").unwrap();
    assert_eq!(missing["code"], json!("MISSING_BLOCK"));
    assert_eq!(missing["count"], json!(4));
    assert_eq!(missing["blockNames"], json!(["Ghost"]));
    assert_eq!(missing["handles"], json!(["2C0", "2C1", "2C2"]));
    assert_eq!(missing["layers"], json!(["frame"]));
    assert_eq!(missing["insertPointsBBox"], json!({ "min": [-20.0, 0.0], "max": [100.0, 480.0] }));
    // Every entry carries its machine-readable code.
    assert!(unreachable.iter().all(|u| u["code"].is_string()), "{unreachable:?}");

    // The drawn text keeps its raw form: `%%c` intact as stored, decoded as drawn (⌀, so a
    // `contains` filter matches what the sheet shows, not what the file stores).
    let hits = texts(&mut s, json!({ "contains": "⌀8", "scope": "nested" }));
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["raw"], json!("NOTE %%c8"));
    assert_eq!(hits[0]["text"], json!("NOTE ⌀8"), "as drawn: the control code becomes its character");
}

#[test]
fn the_block_editor_state_does_not_leak_across_drawings() {
    let mut s = Session::new();
    doors(&mut s);
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    assert!(s.state().unwrap().block_editor().is_some());

    // Opening another drawing puts the Block Editor away with the one it belonged to.
    s.execute("new", &json!({})).unwrap();
    assert!(s.state().unwrap().block_editor().is_none());

    // Closing what is not open says so, not an error about the drawing.
    let err = s.execute("bclose", &json!({})).unwrap_err();
    assert!(err.to_string().contains("The Block Editor is not open"), "{err}");
}

fn doc_text(at: [f64; 2], value: &str) -> cadcraft_engine::doc::Text {
    cadcraft_engine::doc::Text {
        insert: Vec3::new(at[0], at[1], 0.0),
        align_pt: None,
        height: 2.0,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: Default::default(),
        valign: Default::default(),
    }
}

fn block(name: &str) -> Block {
    Block {
        name: name.into(),
        annotative: false,
        base: Vec3::ZERO,
        entities: Default::default(),
        description: String::new(),
        xref_overlay: false,
        xref_status: Default::default(),
        anonymous: false,
        xref_path: None,
        explodable: true,
        units: 0,
        array: None,
    }
}

fn attrib(tag: &str, at: [f64; 2], value: &str) -> cadcraft_engine::doc::Attrib {
    cadcraft_engine::doc::Attrib {
        tag: tag.into(),
        text: doc_text(at, value),
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    }
}

#[test]
fn nested_reference_attributes_sit_where_the_reference_sits_once() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        // "Tag" holds one text; "Panel" places "Tag" at (10, 0) and the reference carries the
        // attribute itself, at Panel-local (15, 0) — the reference's own placement already
        // included, as the reader stores it and the renderer draws it.
        let mut tag = block("Tag");
        tag.entities.push(cadcraft_engine::doc::Entity::new(Handle(0x41), EntityKind::Text(doc_text([0.0, 30.0], "TAGTEXT"))));
        let mut panel = block("Panel");
        panel.entities.push(cadcraft_engine::doc::Entity::new(
            Handle(0x42),
            EntityKind::Insert(cadcraft_engine::doc::Insert {
                block: "Tag".into(),
                insert: Vec3::new(10.0, 0.0, 0.0),
                attribs: vec![attrib("ROOM", [15.0, 0.0], "P1")],
                ..Default::default()
            }),
        ));
        d.blocks.insert("Tag".into(), Arc::new(tag));
        d.blocks.insert("Panel".into(), Arc::new(panel));
        // The model places "Panel" on a 2x2 grid at (100, 0).
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x43),
            EntityKind::Insert(cadcraft_engine::doc::Insert {
                block: "Panel".into(),
                insert: Vec3::new(100.0, 0.0, 0.0),
                cols: 2,
                rows: 2,
                col_spacing: 50.0,
                row_spacing: 40.0,
                ..Default::default()
            }),
        ));
    }

    let r = s.execute("texts", &json!({ "scope": "nested" })).unwrap();
    assert_eq!(r["truncated"], json!(false), "{r}");
    let hits = r["texts"].as_array().unwrap();
    // The reference's own attribute set draws once per placed copy of "Panel" — the cell is a
    // whole copy of the reference, and the renderer draws the set once per copy too — where the
    // reference sits: the stored Panel-local point through the cell's placement alone, (100, 0) +
    // (15, 0) and its three grid siblings. The reference's own placement (10, 0) is already inside
    // the stored point: the walk must not apply it a second time.
    let attrs = hits.iter().filter(|h| h["type"] == "Attribute").collect::<Vec<_>>();
    assert_eq!(attrs.len(), 4, "{hits:?}");
    assert_eq!(attrs[0]["at"], json!([115.0, 0.0]), "{}", attrs[0]);
    assert_eq!(attrs[0]["depth"], json!(1));
    assert_eq!(attrs[0]["source"], json!("block:Panel"));
    assert_eq!(attrs[3]["at"], json!([165.0, 40.0]), "{}", attrs[3]);
    // The definition's own text draws once per placed copy, transformed into the space.
    assert_eq!(hits.iter().filter(|h| h["text"] == "TAGTEXT").count(), 4, "{hits:?}");

    // A window filter speaks space coordinates: it sees the placed attribute at (115, 0), not
    // the block-local point it is stored at.
    let r = s.execute("texts", &json!({ "scope": "nested", "window": [[110.0, -5.0], [120.0, 5.0]] })).unwrap();
    let hits = r["texts"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["type"], "Attribute", "{hits:?}");

    // The definition entities `blockdef.entities` reports are resolvable in turn: the nested
    // reference's handle names its definition.
    let r = s.execute("blockdef.entities", &json!({ "name": "Panel" })).unwrap();
    let inner = r["entities"].as_array().unwrap().iter().find(|e| e["handle"] == json!("42")).cloned().unwrap();
    let r = s.execute("blockdef.entities", &json!({ "insert": inner["handle"] })).unwrap();
    assert_eq!(r["block"]["name"], json!("Tag"), "{r}");
}

#[test]
fn the_nested_walk_stops_at_its_budget_not_after_its_output() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        // "Busy" holds 1,000 texts; a 1,000 x 1,000 grid places it a billion visits deep. The
        // walk charges every visit, so it stops at its budget instead of walking the product.
        let mut busy = block("Busy");
        for k in 0..1_000u64 {
            busy.entities.push(cadcraft_engine::doc::Entity::new(
                Handle(0x1000 + k),
                EntityKind::Text(doc_text([(k % 50) as f64 * 3.0, (k / 50) as f64 * 3.0], "BUSY")),
            ));
        }
        d.blocks.insert("Busy".into(), Arc::new(busy));
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x2000),
            EntityKind::Insert(cadcraft_engine::doc::Insert {
                block: "Busy".into(),
                insert: Vec3::ZERO,
                cols: 1000,
                rows: 1000,
                col_spacing: 100.0,
                row_spacing: 100.0,
                ..Default::default()
            }),
        ));
    }
    let r = s.execute("texts", &json!({ "scope": "nested" })).unwrap();
    assert_eq!(r["truncated"], json!(true), "{}", r["count"]);
    assert!(r["count"].as_u64().unwrap() <= 250_000, "{}", r["count"]);
}

#[test]
fn a_self_nesting_reference_says_its_texts_are_partial() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        let mut looping = block("Loop");
        looping.entities.push(cadcraft_engine::doc::Entity::new(Handle(0x300), EntityKind::Text(doc_text([1.0, 1.0], "DEEP"))));
        looping
            .entities
            .push(cadcraft_engine::doc::Entity::new(Handle(0x301), EntityKind::Insert(cadcraft_engine::doc::Insert { block: "Loop".into(), ..Default::default() })));
        d.blocks.insert("Loop".into(), Arc::new(looping));
        d.model
            .push(cadcraft_engine::doc::Entity::new(Handle(0x302), EntityKind::Insert(cadcraft_engine::doc::Insert { block: "Loop".into(), ..Default::default() })));
    }
    // The walk stops at the nesting cap and says the list is partial instead of reporting a
    // complete-looking one.
    let r = s.execute("texts", &json!({ "scope": "nested" })).unwrap();
    assert_eq!(r["truncated"], json!(true), "{r}");
}

#[test]
fn readability_counts_the_proxies_a_placed_definition_holds() {
    let mut s = Session::new();
    let with_drawing = metafile(&[(29, [0.0f64; 16].iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>().as_slice()), (6, [0u8; 4].as_slice())]);
    {
        let d = s.doc_mut().unwrap();
        let mut holder = block("Holder");
        holder.entities.push(unknown(
            Handle(0x2B0),
            "ACAD_PROXY_ENTITY",
            vec![tag(0, "ACAD_PROXY_ENTITY"), tag(160, &with_drawing.len().to_string()), tag(310, &hex(&with_drawing))],
        ));
        d.blocks.insert("Holder".into(), Arc::new(holder));
        // The model places the definition on a grid of two.
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x2B1),
            EntityKind::Insert(cadcraft_engine::doc::Insert {
                block: "Holder".into(),
                insert: Vec3::ZERO,
                cols: 2,
                col_spacing: 50.0,
                ..Default::default()
            }),
        ));
    }
    let r = s.execute("drawing.readability", &json!({})).unwrap();
    // The definition's proxy counts per placed copy: two instances, two proxies that draw — the
    // sheet really does hold two unmodelled entities nothing renders.
    assert_eq!(r["proxy"]["acadProxy"], json!(2), "{}", r["proxy"]);
    assert_eq!(r["proxy"]["withDrawable"], json!(2), "{}", r["proxy"]);
}

#[test]
fn an_unparsable_proxy_graphic_says_so() {
    let mut s = Session::new();
    d_model_junk_proxy(&mut s, Handle(0x2D0));
    let r = s.execute("drawing.readability", &json!({})).unwrap();
    assert_eq!(r["proxy"]["unparsed"], json!(1), "{}", r["proxy"]);
    let unreachable = r["unreachable"].as_array().unwrap();
    let u = unreachable.iter().find(|u| u["code"] == "PROXY_UNPARSED").unwrap();
    assert_eq!(u["count"], json!(1), "{unreachable:?}");
}

fn d_model_junk_proxy(s: &mut Session, h: Handle) {
    let junk = [0xDE, 0xAD, 0xBE, 0xEF];
    s.doc_mut().unwrap().model.push(unknown(
        h,
        "ACAD_PROXY_ENTITY",
        vec![tag(0, "ACAD_PROXY_ENTITY"), tag(92, "999999"), tag(310, &hex(&junk))],
    ));
}

#[test]
fn proxy_entity_data_stays_out_of_the_graphics_walk() {
    let mut s = Session::new();
    let with_drawing = metafile(&[(6, [0u8; 4].as_slice())]);
    s.doc_mut().unwrap().model.push(unknown(
        Handle(0x2E0),
        "ACAD_PROXY_ENTITY",
        vec![
            tag(0, "ACAD_PROXY_ENTITY"),
            tag(92, &with_drawing.len().to_string()),
            tag(310, &hex(&with_drawing)),
            // Group 93 declares the entity-data section; its chunks are the entity's data, not
            // graphics frames, and the walk leaves them alone.
            tag(93, "500"),
            tag(310, &hex(&[0xAA, 0xBB, 0xCC, 0xDD])),
        ],
    ));
    let r = s.execute("entities", &json!({ "type": "ACAD_PROXY_ENTITY" })).unwrap();
    let p = &r["entities"].as_array().unwrap()[0]["proxy"];
    assert_eq!(p["records"], json!(1), "{}", p);
    assert_eq!(p["unparsed"], json!(0), "{}", p);
    assert_eq!(p["drawable"], json!(1), "{}", p);
}

#[test]
fn a_reference_chain_past_the_expansion_cap_says_its_counts_are_partial() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        // A chain deeper than the block-expansion cap: each link holds a text and the next
        // reference. The counts the expansion feeds say they are partial.
        let depth = cadcraft_engine::doc::MAX_BLOCK_DEPTH + 2;
        for k in 0..depth {
            let name = format!("L{k:02}");
            let mut b = block(&name);
            b.entities.push(cadcraft_engine::doc::Entity::new(Handle(0x400 + k as u64), EntityKind::Text(doc_text([0.0, 0.0], "CHAIN"))));
            if k + 1 < depth {
                b.entities.push(cadcraft_engine::doc::Entity::new(
                    Handle(0x500 + k as u64),
                    EntityKind::Insert(cadcraft_engine::doc::Insert { block: format!("L{:02}", k + 1), ..Default::default() }),
                ));
            }
            d.blocks.insert(name, Arc::new(b));
        }
        d.model
            .push(cadcraft_engine::doc::Entity::new(Handle(0x600), EntityKind::Insert(cadcraft_engine::doc::Insert { block: "L00".into(), ..Default::default() })));
    }
    let r = s.execute("drawing.readability", &json!({})).unwrap();
    // The expansion stopped at its depth cap: the self-check names the cap the counts hit.
    assert_eq!(r["selfCheck"]["caps"]["blockDepth"], json!(16), "{}", r["selfCheck"]);
}

#[test]
fn blocks_list_counts_dimension_block_content_as_a_reference() {
    let mut s = Session::new();
    doors(&mut s);
    {
        let d = s.doc_mut().unwrap();
        // A dimension whose geometry block is the Door block: a definition only it names is in
        // use, and blocks.list must not call it unused.
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x610),
            EntityKind::Dimension(cadcraft_engine::doc::Dimension {
                kind: cadcraft_engine::doc::DimKind::Aligned,
                defpt: Vec3::new(0.0, 0.0, 0.0),
                text_mid: Vec3::new(5.0, 5.0, 0.0),
                p13: Vec3::ZERO,
                p14: Vec3::ZERO,
                p15: Vec3::ZERO,
                p16: Vec3::ZERO,
                text: String::new(),
                style: "Standard".into(),
                measurement: 0.0,
                text_rotation: 0.0,
                user_text_pos: false,
                block: Some("Door".into()),
                insertion_point: Vec3::ZERO,
                overrides: Default::default(),
                assoc: Vec::new(),
                marks: Default::default(),
            }),
        ));
    }
    let r = s.execute("blocks.list", &json!({})).unwrap();
    let door = r["blocks"].as_array().unwrap().iter().find(|b| b["name"] == "Door").unwrap();
    // The three references the doors() helper placed, and the dimension's block.
    assert_eq!(door["refs"], json!(4), "{}", door);
}

#[test]
fn attribs_report_instance_visibility_not_the_definitions() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        let mut stamp = block("Stamp");
        // The definition declares NOTE visible.
        stamp.entities.push(cadcraft_engine::doc::Entity::new(Handle(0x700), EntityKind::AttDef(attrib("NOTE", [0.0, 0.0], "declared"))));
        d.blocks.insert("Stamp".into(), Arc::new(stamp));
        // The reference carries it, invisible: its own flag is what the sheet shows.
        let mut a = attrib("NOTE", [1.0, 1.0], "carried");
        a.invisible = true;
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x701),
            EntityKind::Insert(cadcraft_engine::doc::Insert { block: "Stamp".into(), attribs: vec![a], ..Default::default() }),
        ));
    }
    let r = s.execute("attribs", &json!({ "handle": 0x701u64 })).unwrap();
    let a = &r["references"].as_array().unwrap()[0]["attribs"][0];
    assert_eq!(a["value"], json!("carried"), "{}", a);
    assert_eq!(a["invisible"], json!(true), "{}", a);
}

#[test]
fn a_multiline_attribute_reports_its_formatted_contents_as_raw() {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        let mut ml = attrib("NOTE", [0.0, 0.0], "line1 line2");
        ml.multiline = Some(cadcraft_engine::doc::MultilineAttrib {
            mtext: cadcraft_engine::doc::MText {
                insert: Vec3::ZERO,
                height: 2.0,
                width: 0.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "line1\\Pline2".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            },
            read_as: "line1 line2".into(),
        });
        d.model.push(cadcraft_engine::doc::Entity::new(
            Handle(0x720),
            EntityKind::Insert(cadcraft_engine::doc::Insert { block: "Anything".into(), attribs: vec![ml], ..Default::default() }),
        ));
    }
    let r = s.execute("texts", &json!({ "scope": "space" })).unwrap();
    let hits = r["texts"].as_array().unwrap();
    let a = hits.iter().find(|h| h["type"] == "Attribute").unwrap();
    // Raw keeps the stored formatting; the drawn reading is the plain value.
    assert_eq!(a["raw"], json!("line1\\Pline2"), "{}", a);
    assert_eq!(a["text"], json!("line1 line2"), "{}", a);
}
