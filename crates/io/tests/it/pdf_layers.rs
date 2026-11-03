//! PDF export with layers, several sheets and hyperlinks (issue #560): drawing layers become
//! optional content groups, `layouts` plots several sheets into one file with page labels, and
//! linked entities get link annotations. The tests read the written file back.

use cadcraft_color::Color;
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, Space, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::pdf::{MAX_PAGES, pdf_sheets};
use cadcraft_io::{PdfOptions, plot};
use serde_json::json;

use crate::pdf_read::{self, Doc};

fn read(bytes: &[u8]) -> Doc {
    pdf_read::read(bytes)
}

fn ocg_names(doc: &Doc) -> Vec<String> {
    doc.ocgs.iter().map(|g| g.name.clone()).collect()
}

/// The `BDC` marks of a content: the property resource each one names.
fn marks(content: &str) -> Vec<String> {
    pdf_read::ops(content).into_iter().filter(|o| o.op == "BDC").filter_map(|o| o.names.get(1).cloned()).collect()
}

fn add_line(d: &mut Drawing, space: &Space, layer: &str, a: (f64, f64), b: (f64, f64)) -> Handle {
    let line = EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) });
    d.add(space, Common { layer: layer.into(), ..Default::default() }, line).unwrap()
}

/// Lines on layers "0", "Walls" (red), "Notes" (off) and "Hidden" (frozen), and a block
/// reference on "Doors" whose contents are on layer 0.
fn layered() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), ..Layer::default() });
    d.layers.push(Layer { name: "Notes".into(), on: false, ..Layer::default() });
    d.layers.push(Layer { name: "Hidden".into(), frozen: true, ..Layer::default() });
    d.layers.push(Layer { name: "Doors".into(), ..Layer::default() });
    add_line(&mut d, &Space::Model, "0", (0.0, 0.0), (100.0, 0.0));
    add_line(&mut d, &Space::Model, "Walls", (0.0, 10.0), (100.0, 10.0));
    add_line(&mut d, &Space::Model, "Notes", (0.0, 20.0), (100.0, 20.0));
    add_line(&mut d, &Space::Model, "Hidden", (0.0, 30.0), (100.0, 30.0));
    let leaf = EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(5.0, 5.0, 0.0) });
    let mut blk = Block::new("DOOR");
    blk.entities.push(Entity::new(Handle(0x900), leaf));
    d.blocks.insert(blk.name.clone(), std::sync::Arc::new(blk));
    let ins = Insert {
        block: "DOOR".into(),
        insert: Vec3::new(50.0, 40.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    d.add(&Space::Model, Common { layer: "Doors".into(), ..Default::default() }, EntityKind::Insert(ins)).unwrap();
    d
}

fn raw(extra: serde_json::Value) -> serde_json::Value {
    let mut o = json!({"compress": false});
    if let (Some(m), Some(e)) = (o.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    o
}

#[test]
fn drawing_layers_become_pdf_layers() {
    let d = layered();
    let doc = read(&plot(&d, &Space::Model, &raw(json!({}))).unwrap());
    // Plotted layers only, in drawing order of first use; block contents on layer 0 are on
    // the block reference's layer. None starts hidden.
    assert_eq!(ocg_names(&doc), ["0", "Walls", "Doors"]);
    assert!(doc.ocgs.iter().all(|g| !g.off));
    // One sheet: the viewer opens with its layer panel.
    assert_eq!(doc.page_mode.as_deref(), Some("UseOC"));
    // The page maps its marks to the groups, and the marked sections are balanced.
    let page = &doc.pages[0];
    let groups: Vec<&str> = page.properties.iter().map(|(_, g)| g.as_str()).collect();
    for n in ["0", "Walls", "Doors"] {
        assert!(groups.contains(&n), "{n} in {groups:?}");
    }
    let c = &page.content;
    assert_eq!(pdf_read::count(c, "BDC"), pdf_read::count(c, "EMC"));
    for m in marks(c) {
        assert!(page.properties.iter().any(|(k, _)| *k == m), "mark {m} is a page property");
    }
    // The red wall line is on Walls.
    let walls = page.properties.iter().find(|(_, g)| g == "Walls").map(|(k, _)| k.clone()).unwrap();
    let ops = pdf_read::ops(c);
    let at = ops.iter().position(|o| o.op == "BDC" && o.names.get(1) == Some(&walls)).unwrap();
    let next = ops[at..].iter().find(|o| o.op == "RG").unwrap();
    assert_eq!(next.nums, [1.0, 0.0, 0.0], "{c}");
    // Marked content inside the clip's q/Q.
    let first_bdc = ops.iter().position(|o| o.op == "BDC").unwrap();
    let last_emc = ops.iter().rposition(|o| o.op == "EMC").unwrap();
    let clip = ops.iter().position(|o| o.op == "W").unwrap();
    let last_q = ops.iter().rposition(|o| o.op == "Q").unwrap();
    assert!(clip < first_bdc && last_emc < last_q, "marked content inside the clip's q/Q");
}

#[test]
fn layers_that_are_off_are_included_hidden_on_request() {
    let d = layered();
    let doc = read(&plot(&d, &Space::Model, &raw(json!({"offLayers": true}))).unwrap());
    let names = ocg_names(&doc);
    assert!(names.contains(&"Notes".to_string()) && !names.contains(&"Hidden".to_string()), "{names:?}");
    let off: Vec<&str> = doc.ocgs.iter().filter(|g| g.off).map(|g| g.name.as_str()).collect();
    assert_eq!(off, ["Notes"]);
    // Without layers, no optional content at all.
    let flat = read(&plot(&d, &Space::Model, &raw(json!({"layers": false}))).unwrap());
    assert!(flat.ocgs.is_empty() && flat.page_mode.is_none());
    assert_eq!(pdf_read::count(flat.content(), "BDC"), 0);
}

#[test]
fn layers_that_are_off_are_plotted_only_on_request() {
    let d = layered();
    let strokes = |o: serde_json::Value| {
        let doc = read(&plot(&d, &Space::Model, &raw(o)).unwrap());
        pdf_read::drawn(doc.content()).iter().filter(|o| o.op == "m").count()
    };
    // By default Notes (off) and Hidden (frozen) aren't drawn: the four lines less those two,
    // plus the door.
    let plain = strokes(json!({}));
    assert_eq!(plain, 3, "lines on 0 and Walls, and the door leaf");
    // `offLayers` adds Notes (on a hidden PDF layer), never the frozen layer; it needs PDF layers.
    assert_eq!(strokes(json!({"offLayers": true})), 4);
    assert_eq!(strokes(json!({"layers": false, "offLayers": true})), plain);
}

#[test]
fn viewport_contents_keep_their_layers() {
    let mut d = layered();
    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        center: Vec3::new(150.0, 100.0, 0.0),
        width: 200.0,
        height: 150.0,
        view_center: Vec2::new(50.0, 20.0),
        view_height: 80.0,
        id: 2,
        locked: false,
        frozen_layers: vec!["Walls".into()],
        layer_colors: Vec::new(),
        clip: None,
        on: true,
    };
    d.add(&paper, Common { layer: "Viewports".into(), ..Default::default() }, EntityKind::Viewport(vp)).unwrap();
    add_line(&mut d, &paper, "Title", (10.0, 10.0), (50.0, 10.0));
    let names = ocg_names(&read(&plot(&d, &paper, &raw(json!({}))).unwrap()));
    // The viewport's frozen layer is left out; the model layers seen through it are kept.
    for n in ["Viewports", "0", "Doors", "Title"] {
        assert!(names.contains(&n.to_string()), "{n} in {names:?}");
    }
    assert!(!names.contains(&"Walls".to_string()));
}

#[test]
fn several_sheets_in_one_file_with_page_labels() {
    let mut d = layered();
    add_line(&mut d, &Space::Paper("Layout1".into()), "Walls", (10.0, 10.0), (50.0, 10.0));
    let doc = read(&plot(&d, &Space::Model, &raw(json!({"layouts": ["model", "LAYOUT1"]}))).unwrap());
    // "all": model (it has objects), then every layout in tab order.
    let all = read(&plot(&d, &Space::Model, &raw(json!({"layouts": "All"}))).unwrap());
    assert_eq!(all.labels, ["Model", "Layout1", "Layout2"]);
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.labels, ["Model", "Layout1"]);
    // Each page has its own sheet: model on A4 landscape, the layout on its page setup.
    for p in &doc.pages {
        assert!(pdf_read::ops(&p.content).iter().any(|o| o.op == "RG" && o.nums == [1.0, 0.0, 0.0]), "the red wall line: {}", p.content);
    }
    let size = |p: &pdf_read::Page| (p.media[2] - p.media[0], p.media[3] - p.media[1]);
    assert!((size(&doc.pages[0]).0 - 841.89).abs() < 0.01 && (size(&doc.pages[0]).1 - 595.276).abs() < 0.01);
    // The library call does the same.
    let o = PdfOptions { compress: false, ..PdfOptions::default() };
    let doc2 = read(&pdf_sheets(&d, &[Space::Paper("Layout1".into()), Space::Model], &o).unwrap());
    assert_eq!(doc2.labels, ["Layout1", "Model"]);
    // Title: the first page's label unless given.
    assert_eq!(doc2.title.as_deref(), Some("Layout1"));
    assert_eq!((doc2.creator.as_deref(), doc2.producer.as_deref()), (Some("CADCraft"), Some("CADCraft")));
}

#[test]
fn several_sheets_share_one_pdf_layer_per_drawing_layer() {
    let mut d = layered();
    add_line(&mut d, &Space::Paper("Layout1".into()), "Walls", (10.0, 10.0), (50.0, 10.0));
    let doc = read(&plot(&d, &Space::Model, &raw(json!({"layouts": ["model", "LAYOUT1"]}))).unwrap());
    assert_eq!(ocg_names(&doc).iter().filter(|n| *n == "Walls").count(), 1);
    for p in &doc.pages {
        assert!(p.properties.iter().any(|(_, g)| g == "Walls"));
    }
    // Several sheets: no layer panel forced open.
    assert!(doc.page_mode.is_none());
}

#[test]
fn bad_sheet_lists_are_errors() {
    let d = layered();
    for o in [json!({"layouts": []}), json!({"layouts": ["Nope"]}), json!({"layouts": "Model"}), json!({"layouts": [1]})] {
        assert!(plot(&d, &Space::Model, &o).is_err(), "{o}");
    }
    let many: Vec<&str> = vec!["Model"; MAX_PAGES + 1];
    assert!(plot(&d, &Space::Model, &json!({ "layouts": many })).is_err());
    assert!(pdf_sheets(&d, &[], &PdfOptions::default()).is_err());
}

#[test]
fn hyperlinks_become_link_annotations() {
    let mut d = layered();
    let h = add_line(&mut d, &Space::Model, "Walls", (0.0, 50.0), (100.0, 50.0));
    let links = json!([
        {"handle": h.hex(), "url": "https://example.com/a(1)"},
        {"handle": h.0, "url": "javascript:alert(1)"},
        {"handle": "FFFFFF", "url": "https://example.com/none"},
        {"handle": h.hex(), "url": "https://example.com/ü"},
        {"url": "https://example.com/no-handle"},
    ]);
    let doc = read(&plot(&d, &Space::Model, &raw(json!({ "links": links }))).unwrap());
    let annots = &doc.pages[0].links;
    assert_eq!(annots.len(), 2);
    assert_eq!(annots[0].uri, "https://example.com/a(1)");
    assert_eq!(annots[1].uri, "https://example.com/%C3%BC");
    // The rectangle is inside the A4 landscape sheet and wide like the line.
    let v = annots[0].rect;
    assert!(v[0] >= 0.0 && v[2] <= 841.9 && v[1] >= 0.0 && v[3] <= 595.3 && v[2] - v[0] > 100.0, "{v:?}");
}

fn unusual_names() -> Drawing {
    let mut d = Drawing::new_metric();
    for name in ["a(b)\\c", "Wände", "日本"] {
        d.layers.push(Layer { name: name.into(), ..Layer::default() });
        add_line(&mut d, &Space::Model, name, (0.0, 0.0), (10.0, 10.0));
    }
    d
}

#[test]
fn unusual_layer_names_are_kept() {
    let doc = read(&plot(&unusual_names(), &Space::Model, &raw(json!({}))).unwrap());
    assert_eq!(ocg_names(&doc), ["a(b)\\c", "Wände", "日本"]);
}

#[test]
fn unusual_titles_are_kept() {
    let doc = read(&plot(&unusual_names(), &Space::Model, &raw(json!({"title": "Plan ü (1)\\"}))).unwrap());
    assert_eq!(doc.title.as_deref(), Some("Plan ü (1)\\"));
}

#[test]
fn layers_past_the_cap_print_without_a_pdf_layer() {
    // One more drawing layer than a file has PDF layers (4096).
    let mut d = Drawing::new_metric();
    for i in 0..4097 {
        let name = format!("L{i}");
        d.layers.push(Layer { name: name.clone(), ..Layer::default() });
        add_line(&mut d, &Space::Model, &name, (0.0, f64::from(i)), (10.0, f64::from(i)));
    }
    let doc = read(&plot(&d, &Space::Model, &raw(json!({}))).unwrap());
    assert_eq!(doc.ocgs.len(), 4096);
    assert!(!ocg_names(&doc).contains(&"L4096".to_string()));
    // Every line is still drawn; only the last is outside marked content.
    let c = doc.content();
    assert_eq!(pdf_read::drawn(c).iter().filter(|o| o.op == "m").count(), 4097);
    assert_eq!((pdf_read::count(c, "BDC"), pdf_read::count(c, "EMC")), (4096, 4096));
}
