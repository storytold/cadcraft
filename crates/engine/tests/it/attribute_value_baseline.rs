//! Small value changes must not export an imported justified attribute's old baseline.
use cadcraft_doc::{Attrib, Drawing, EntityKind, HAlign, Handle, Text, VAlign};
use cadcraft_engine::{Input, Session};
use cadcraft_geom::Vec3;
use serde_json::json;

const BEFORE: &str = "POWER ADAPTER";
const AFTER: &str = "POWER ADAPTER.";

fn fixture() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    for (tag, value, y) in [("T", BEFORE, 15), ("U", "ADPT-01", 0)] {
        s.execute("attdef", &json!({"tag":tag,"default":value,"at":[0,y],"height":3})).unwrap();
    }
    s.execute("line", &json!({"points":[[0,-5],[40,-5]]})).unwrap();
    let handles: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name":"Equipment","handles":handles,"base":[0,0],"keep":"delete"})).unwrap();
    let a = s.execute("insert", &json!({"name":"Equipment","at":[100,200],"scale":2,"rotation":30})).unwrap();
    let b = s.execute("insert", &json!({"name":"Equipment","at":[300,200],"attribs":{"T":"Other"}})).unwrap();
    let a = Handle::parse_hex(a["handle"].as_str().unwrap()).unwrap();
    let b = Handle::parse_hex(b["handle"].as_str().unwrap()).unwrap();
    // Set existing document fields directly: this regression does not require Text Options.
    s.doc_mut()
        .unwrap()
        .modify_entity(a, |e| {
            let EntityKind::Insert(i) = &mut e.kind else { panic!("insert") };
            let t = &mut i.attribs[0].text;
            t.insert.z = 7.0;
            t.align_pt = Some(t.insert);
            t.halign = HAlign::Center;
            t.valign = VAlign::Top;
        })
        .unwrap();
    let imported = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
    let mut s = Session::empty();
    s.open_drawing(imported, "Imported", None);
    (s, a, b)
}

fn attributes(d: &Drawing, h: Handle) -> Vec<Attrib> {
    let EntityKind::Insert(i) = &d.entity(h).unwrap().kind else { panic!("insert") };
    i.attribs.clone()
}

fn baseline(d: &Drawing, t: &Text) -> cadcraft_geom::Vec2 {
    let style = d.text_style(&t.style).unwrap();
    let (h, v) = cadcraft_render::text_align(t.halign, t.valign);
    cadcraft_fonts::baseline_start(
        &cadcraft_fonts::TextFont::resolve(&style.font),
        &t.value,
        &cadcraft_fonts::TextParams {
            insert: t.insert.xy(),
            align_pt: t.align_pt.map(|p| p.xy()),
            height: t.height,
            rotation: t.rotation,
            width_factor: t.width_factor,
            oblique: t.oblique,
            h,
            v,
            backwards: style.backwards,
            upside_down: style.upside_down,
        },
    )
}

#[test]
fn all_attribute_value_paths_refresh_small_justified_export_changes() {
    for path in ["json", "interactive", "global-json", "global-interactive"] {
        let (mut s, a, b) = fixture();
        let before = attributes(s.doc().unwrap(), a);
        let other = attributes(s.doc().unwrap(), b);
        let definition = s.doc().unwrap().block("Equipment").unwrap().clone();
        match path {
            "json" => {
                s.execute("attedit", &json!({"handle":a.hex(),"values":{"t":AFTER}})).unwrap();
            }
            "interactive" => {
                s.start("attedit").unwrap();
                s.input(Input::Pick(vec![a])).unwrap();
                s.input(Input::Text(AFTER.into())).unwrap();
                s.input(Input::Enter).unwrap();
                assert!(s.running.is_none());
            }
            "global-json" => {
                s.execute("-attedit", &json!({"handles":[a.hex()],"tag":"T","find":"ADAPTER","replace":"ADAPTER."})).unwrap();
            }
            "global-interactive" => {
                s.script("-attedit\nn\nn\nEquipment\nT\n\nADAPTER\nADAPTER.\n").unwrap();
                assert!(s.running.is_none(), "{}", s.prompt_text());
            }
            _ => panic!("path"),
        }
        let edited = attributes(s.doc().unwrap(), a);
        assert_eq!(edited[0].text.value, AFTER, "{path}");
        assert_eq!(edited[0].text.align_pt, before[0].text.align_pt);
        assert_eq!(edited[0].text.insert.z, 7.0);
        assert_eq!(edited[1], before[1]);
        assert_eq!(attributes(s.doc().unwrap(), b), other);
        assert_eq!(s.doc().unwrap().block("Equipment").unwrap(), &definition);
        let expected = baseline(s.doc().unwrap(), &edited[0].text);
        assert!(expected.dist(before[0].text.insert.xy()) > 0.01, "fixture must change width");
        for name in ["edited.dxf", "edited.dwg"] {
            let reopened = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), name).unwrap(), name).unwrap();
            let restored = attributes(&reopened, a);
            assert!(
                restored[0].text.insert.xy().dist(expected) < 1e-8,
                "{path} {name}: stale {:?}, expected {expected:?}",
                restored[0].text.insert.xy()
            );
            assert_eq!(restored[0].text.align_pt, before[0].text.align_pt);
            assert_eq!(restored[0].text.value, AFTER);
            assert_eq!(restored[0].text.insert.z, 7.0);
        }
        s.undo().unwrap();
        assert_eq!(attributes(s.doc().unwrap(), a), before, "{path} undo");
        s.redo().unwrap();
        assert_eq!(attributes(s.doc().unwrap(), a), edited, "{path} redo");
    }
}

#[test]
fn equal_attribute_values_preserve_an_imported_font_baseline() {
    let (mut s, a, _) = fixture();
    // A small imported-font offset must survive a no-op edit (writer tolerance is deliberate).
    s.doc_mut()
        .unwrap()
        .modify_entity(a, |e| {
            let EntityKind::Insert(i) = &mut e.kind else { panic!("insert") };
            i.attribs[0].text.insert.x += 0.1;
        })
        .unwrap();
    let before = attributes(s.doc().unwrap(), a);
    s.execute("attedit", &json!({"handle":a.hex(),"values":{"T":BEFORE}})).unwrap();
    assert_eq!(attributes(s.doc().unwrap(), a), before);
    s.start("attedit").unwrap();
    s.input(Input::Pick(vec![a])).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(attributes(s.doc().unwrap(), a), before);
    s.execute("-attedit", &json!({"handles":[a.hex()],"tag":"T","find":"ADAPTER","replace":"ADAPTER"})).unwrap();
    assert_eq!(attributes(s.doc().unwrap(), a), before, "global JSON no-op");
    s.script("-attedit\nn\nn\nEquipment\nT\n\nADAPTER\nADAPTER\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(attributes(s.doc().unwrap(), a), before, "global interactive no-op");
}

/// Recomputing a justified baseline must not replace its independent insertion elevation.
#[test]
fn value_edits_preserve_distinct_insertion_and_alignment_elevations_on_export() {
    let (mut s, a, _) = fixture();
    s.doc_mut()
        .unwrap()
        .modify_entity(a, |e| {
            let EntityKind::Insert(i) = &mut e.kind else { panic!("insert") };
            i.attribs[0].text.insert.z = 9.0;
        })
        .unwrap();
    let anchor = attributes(s.doc().unwrap(), a)[0].text.align_pt;
    s.execute("attedit", &json!({"handle":a.hex(),"values":{"T":AFTER}})).unwrap();
    for name in ["elevations.dxf", "elevations.dwg"] {
        let reopened = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), name).unwrap(), name).unwrap();
        let restored = attributes(&reopened, a);
        assert_eq!(restored[0].text.insert.z, 9.0, "{name}: insertion elevation");
        assert_eq!(restored[0].text.align_pt.map(|p| p.xy()), anchor.map(|p| p.xy()), "{name}: alignment XY");
        if name.ends_with(".dxf") {
            assert_eq!(restored[0].text.align_pt, anchor, "DXF retains independent alignment elevation");
        } else {
            // The DWG bridge uses one text elevation for both points.
            assert_eq!(restored[0].text.align_pt.unwrap().z, 9.0);
        }
    }
}

#[test]
fn value_edits_preserve_left_baseline_and_two_point_placements() {
    for horizontal in [HAlign::Left, HAlign::Aligned, HAlign::Fit] {
        let (mut s, a, _) = fixture();
        s.doc_mut()
            .unwrap()
            .modify_entity(a, |e| {
                let EntityKind::Insert(i) = &mut e.kind else { panic!("insert") };
                let t = &mut i.attribs[0].text;
                t.halign = horizontal;
                t.valign = VAlign::Baseline;
                t.insert = Vec3::new(10.0, 20.0, 7.0);
                t.align_pt = (horizontal != HAlign::Left).then_some(Vec3::new(40.0, 30.0, 9.0));
            })
            .unwrap();
        let before = attributes(s.doc().unwrap(), a);
        s.execute("attedit", &json!({"handle":a.hex(),"values":{"T":AFTER}})).unwrap();
        let mut expected = before;
        expected[0].text.value = AFTER.into();
        assert_eq!(attributes(s.doc().unwrap(), a), expected);
        if horizontal != HAlign::Left {
            let reopened = cadcraft_io::read_dxf(cadcraft_io::write_dxf(s.doc().unwrap()).as_bytes()).unwrap();
            let restored = attributes(&reopened, a);
            assert_eq!(restored[0].text.insert, expected[0].text.insert);
            assert_eq!(restored[0].text.align_pt, expected[0].text.align_pt);
            assert_eq!(restored[0].text.halign, horizontal);
            assert_eq!(restored[0].text.value, AFTER);
        }
    }
}
