//! PDF export with layers, several sheets and hyperlinks (issue #560): drawing layers become
//! optional content groups, `layouts` plots several sheets into one file with page labels, and
//! linked entities get link annotations. The tests parse the written objects.

use cadcraft_color::Color;
use cadcraft_doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, Space, Viewport};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::pdf::{MAX_PAGES, pdf_sheets};
use cadcraft_io::{PdfOptions, plot};
use serde_json::json;

/// The objects of a PDF by number, after checking the cross-reference table points at them.
fn objects(bytes: &[u8]) -> Vec<(usize, String)> {
    assert!(bytes.starts_with(b"%PDF-1."));
    // Byte offsets stay valid: non-ASCII bytes (the binary comment) become `?`.
    let text: String = bytes.iter().map(|b| if b.is_ascii() { char::from(*b) } else { '?' }).collect();
    let sx = text.rfind("startxref\n").expect("startxref");
    let xref: usize = text[sx + 10..].lines().next().unwrap().trim().parse().unwrap();
    assert!(text[xref..].starts_with("xref\n"));
    let mut lines = text[xref..].lines().skip(1);
    let count: usize = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let entries: Vec<&str> = lines.take(count).collect();
    assert_eq!(entries.len(), count);
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate().skip(1) {
        let off: usize = e[..10].parse().unwrap();
        let head = format!("{i} 0 obj\n");
        assert!(bytes[off..].starts_with(head.as_bytes()), "object {i} offset");
        let body = &text[off + head.len()..];
        let end = body.find("\nendobj\n").expect("endobj");
        out.push((i, body[..end].to_string()));
    }
    assert!(text.contains(&format!("/Size {count}")));
    out
}

fn object(objs: &[(usize, String)], n: usize) -> &str {
    &objs.iter().find(|(i, _)| *i == n).unwrap().1
}

/// The object a `/Key n 0 R` entry of `dict` refers to.
fn reference(dict: &str, key: &str) -> usize {
    let i = dict.find(&format!("{key} ")).unwrap_or_else(|| panic!("{key} in {dict}")) + key.len() + 1;
    dict[i..].split_whitespace().next().unwrap().parse().unwrap()
}

/// Uncompressed content streams in page order.
fn contents(objs: &[(usize, String)]) -> Vec<String> {
    pages(objs).iter().map(|p| object(objs, reference(p, "/Contents")).to_string()).collect()
}

fn pages(objs: &[(usize, String)]) -> Vec<String> {
    objs.iter().filter(|(_, b)| b.starts_with("<< /Type /Page ")).map(|(_, b)| b.clone()).collect()
}

fn catalog(objs: &[(usize, String)]) -> String {
    objs.iter().find(|(_, b)| b.starts_with("<< /Type /Catalog")).unwrap().1.clone()
}

/// The names of the optional content groups.
fn ocg_names(objs: &[(usize, String)]) -> Vec<String> {
    objs.iter().filter_map(|(_, b)| b.strip_prefix("<< /Type /OCG /Name ").map(|r| r.trim_end_matches(" >>").to_string())).collect()
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
    let objs = objects(&plot(&d, &Space::Model, &raw(json!({}))).unwrap());
    let names = ocg_names(&objs);
    // Plotted layers only, in drawing order of first use; block contents on layer 0 are on
    // the block reference's layer.
    assert_eq!(names, ["(0)", "(Walls)", "(Doors)"]);
    let cat = catalog(&objs);
    assert!(cat.contains("/Version /1.5") && cat.contains("/OCProperties") && cat.contains("/OFF []"), "{cat}");
    // Every OCG is listed and switchable.
    let ocgs: Vec<usize> = objs.iter().filter(|(_, b)| b.starts_with("<< /Type /OCG")).map(|(i, _)| *i).collect();
    for id in &ocgs {
        assert!(cat.contains(&format!("{id} 0 R")));
    }
    // The page maps its marks to the groups, and the marked sections are balanced.
    // Marks are named after the layer's place in the layer table (0, Defpoints, Walls, …).
    let page = &pages(&objs)[0];
    for i in [0, 2, 5] {
        let id = reference(page, &format!("/L{i}"));
        assert!(ocgs.contains(&id));
    }
    let c = &contents(&objs)[0];
    assert_eq!(c.matches(" BDC\n").count(), c.matches("EMC\n").count());
    assert!(c.contains("/OC /L2 BDC\n1 0 0 RG"), "the red wall line is on Walls: {c}");
    let (q, bdc, emc) = (c.find("re W n").unwrap(), c.find("BDC").unwrap(), c.rfind("EMC").unwrap());
    assert!(q < bdc && emc < c.rfind("Q\n").unwrap(), "marked content inside the clip's q/Q");
}

#[test]
fn layers_that_are_off_are_included_hidden_on_request() {
    let d = layered();
    let objs = objects(&plot(&d, &Space::Model, &raw(json!({"offLayers": true}))).unwrap());
    let names = ocg_names(&objs);
    assert!(names.contains(&"(Notes)".to_string()) && !names.contains(&"(Hidden)".to_string()), "{names:?}");
    let notes = objs.iter().find(|(_, b)| b.contains("/Name (Notes)")).unwrap().0;
    let cat = catalog(&objs);
    assert!(cat.contains(&format!("/OFF [{notes} 0 R]")), "{cat}");
    // Off: neither the layer nor its objects; without layers, no optional content at all.
    let flat = objects(&plot(&d, &Space::Model, &raw(json!({"layers": false}))).unwrap());
    assert!(ocg_names(&flat).is_empty() && !catalog(&flat).contains("/OCProperties"));
    assert!(!contents(&flat)[0].contains("BDC"));
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
        view3d: None,
    };
    d.add(&paper, Common { layer: "Viewports".into(), ..Default::default() }, EntityKind::Viewport(vp)).unwrap();
    add_line(&mut d, &paper, "Title", (10.0, 10.0), (50.0, 10.0));
    let objs = objects(&plot(&d, &paper, &raw(json!({}))).unwrap());
    let names = ocg_names(&objs);
    // The viewport's frozen layer is left out; the model layers seen through it are kept.
    for n in ["(Viewports)", "(0)", "(Doors)", "(Title)"] {
        assert!(names.contains(&n.to_string()), "{n} in {names:?}");
    }
    assert!(!names.contains(&"(Walls)".to_string()));
}

#[test]
fn several_sheets_in_one_file_with_page_labels() {
    let mut d = layered();
    add_line(&mut d, &Space::Paper("Layout1".into()), "Walls", (10.0, 10.0), (50.0, 10.0));
    let bytes = plot(&d, &Space::Model, &raw(json!({"layouts": ["model", "LAYOUT1"]}))).unwrap();
    // "all": model (it has objects), then every layout in tab order.
    let all = objects(&plot(&d, &Space::Model, &raw(json!({"layouts": "All"}))).unwrap());
    assert!(catalog(&all).contains("[0 << /P (Model) >> 1 << /P (Layout1) >> 2 << /P (Layout2) >>]"), "{}", catalog(&all));
    let objs = objects(&bytes);
    let tree = objs.iter().find(|(_, b)| b.starts_with("<< /Type /Pages")).unwrap().1.clone();
    assert!(tree.contains("/Count 2"), "{tree}");
    assert_eq!(pages(&objs).len(), 2);
    let cat = catalog(&objs);
    assert!(cat.contains("/PageLabels << /Nums [0 << /P (Model) >> 1 << /P (Layout1) >>] >>"), "{cat}");
    // One OCG per layer for the whole file, shared by the pages.
    let names = ocg_names(&objs);
    assert_eq!(names.iter().filter(|n| *n == "(Walls)").count(), 1);
    let walls = objs.iter().find(|(_, b)| b.contains("/Name (Walls)")).unwrap().0;
    for p in pages(&objs) {
        assert!(p.contains(&format!(" {walls} 0 R")), "{p}");
    }
    // Each page has its own sheet: model on A4 landscape, the layout on its page setup.
    let c = contents(&objs);
    assert!(c[0].contains("1 0 0 RG") && c[1].contains("1 0 0 RG"));
    // The library call does the same.
    let o = PdfOptions { compress: false, ..PdfOptions::default() };
    let objs2 = objects(&pdf_sheets(&d, &[Space::Paper("Layout1".into()), Space::Model], &o).unwrap());
    assert!(catalog(&objs2).contains("0 << /P (Layout1) >> 1 << /P (Model) >>"));
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
    let objs = objects(&plot(&d, &Space::Model, &raw(json!({ "links": links }))).unwrap());
    let annots: Vec<&String> = objs.iter().map(|(_, b)| b).filter(|b| b.contains("/Subtype /Link")).collect();
    assert_eq!(annots.len(), 2, "{annots:?}");
    assert!(annots[0].contains("/A << /S /URI /URI (https://example.com/a\\(1\\)) >>"), "{}", annots[0]);
    assert!(annots[1].contains("(https://example.com/%C3%BC)"), "{}", annots[1]);
    let page = &pages(&objs)[0];
    assert!(page.contains("/Annots ["));
    // The rectangle is inside the A4 landscape sheet and wide like the line.
    let r = annots[0].split("/Rect [").nth(1).unwrap().split(']').next().unwrap();
    let v: Vec<f64> = r.split_whitespace().map(|x| x.parse().unwrap()).collect();
    assert!(v[0] >= 0.0 && v[2] <= 841.9 && v[1] >= 0.0 && v[3] <= 595.3 && v[2] - v[0] > 100.0, "{v:?}");
}

#[test]
fn unusual_layer_names_and_titles_are_escaped() {
    let mut d = Drawing::new_metric();
    for name in ["a(b)\\c", "Wände", "日本"] {
        d.layers.push(Layer { name: name.into(), ..Layer::default() });
        add_line(&mut d, &Space::Model, name, (0.0, 0.0), (10.0, 10.0));
    }
    let objs = objects(&plot(&d, &Space::Model, &raw(json!({"title": "Plan ü"}))).unwrap());
    let names = ocg_names(&objs);
    assert_eq!(names, ["(a\\(b\\)\\\\c)", "<FEFF005700E4006E00640065>", "<FEFF65E5672C>"]);
    assert!(objs.iter().any(|(_, b)| b.contains("/Title <FEFF0050006C0061006E002000FC>")));
}
