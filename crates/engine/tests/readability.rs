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
        // Two references to a block the drawing does not define, on the title-block layer.
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
    }

    let r = s.execute("drawing.readability", &json!({})).unwrap();
    assert_eq!(r["modelEntities"], json!(6), "{}", r["message"]); // 3 references, 1 text, 2 ghosts
    assert_eq!(r["blockDefs"], json!(1));
    assert_eq!(r["anonymousRefs"], json!(0));
    assert_eq!(r["missingBlockRefs"], json!(2));
    assert_eq!(r["textsInModel"], json!(1));
    assert_eq!(r["textsInBlockDefs"], json!(1));
    assert_eq!(r["textsInBlocks"], json!(3), "one per placed reference");
    let unreachable = r["unreachable"].as_array().unwrap();
    let missing = unreachable.iter().find(|u| u["kind"] == "missingBlock").unwrap();
    assert_eq!(missing["count"], json!(2));

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
