//! Plot style tables in plots and exports (issue #410): the PDF content stream carries the
//! table's colours and pen widths, layout SVG exports follow "Display plot styles", and the
//! page setup's table and flag survive a DXF round trip.

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{read_dxf, svg, write_dxf};
use serde_json::json;

fn layout_with_red_line() -> Drawing {
    let mut d = Drawing::new_metric();
    let line = EntityKind::Line(Line { a: Vec3::new(10.0, 10.0, 0.0), b: Vec3::new(100.0, 10.0, 0.0) });
    d.add(&Space::Paper("Layout1".into()), Common { color: Color::Index(1), ..Default::default() }, line).unwrap();
    d
}

/// The page content of Layout1's PDF with extra plot options.
fn pdf_text(d: &Drawing, extra: serde_json::Value) -> String {
    let mut opts = json!({"compress": false});
    if let (Some(o), Some(e)) = (opts.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    let bytes = cadcraft_io::plot(d, &Space::Paper("Layout1".into()), &opts).unwrap();
    crate::pdf_read::read(&bytes).content().to_string()
}

#[test]
fn pdf_uses_the_page_setup_table() {
    let mut d = layout_with_red_line();
    assert!(pdf_text(&d, json!({})).contains("1 0 0 RG"), "no table: red");
    d.layouts[0].page.plot_style_table = "monochrome.ctb".into();
    let mono = pdf_text(&d, json!({}));
    assert!(mono.contains("0 0 0 RG") && !mono.contains("1 0 0 RG"), "{mono}");
    // The plot's own option replaces the page setup's; "None" plots object colours.
    assert!(pdf_text(&d, json!({"plotStyleTable": "None"})).contains("1 0 0 RG"));
    let gray = pdf_text(&d, json!({"plotStyleTable": "grayscale.ctb"}));
    assert!(!gray.contains("1 0 0 RG") && !gray.contains("0 0 0 RG"), "red prints mid grey: {gray}");
}

#[test]
fn layout_svg_follows_display_plot_styles() {
    let mut d = layout_with_red_line();
    d.layouts[0].page.plot_style_table = "monochrome.ctb".into();
    let space = Space::Paper("Layout1".into());
    assert!(svg::export(&d, &space).contains("#ff0000"));
    d.layouts[0].page.show_plot_styles = true;
    let s = svg::export(&d, &space);
    assert!(!s.contains("#ff0000") && s.contains("#000000"), "{s}");
}

#[test]
fn page_setup_table_and_display_flag_round_trip() {
    let mut d = layout_with_red_line();
    d.layouts[0].page.plot_style_table = "grayscale.ctb".into();
    d.layouts[0].page.show_plot_styles = true;
    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    let page = &back.layout("Layout1").unwrap().page;
    assert_eq!(page.plot_style_table, "grayscale.ctb");
    assert!(page.show_plot_styles);
    assert!(!read_dxf(write_dxf(&layout_with_red_line()).as_bytes()).unwrap().layout("Layout1").unwrap().page.show_plot_styles);
}

#[test]
fn named_plot_styles_round_trip_through_the_plot_style_dictionary() {
    use cadcraft_doc::{Layer, PlotStyle, PlotStyleTable};
    let mut d = layout_with_red_line();
    d.layers.push(Layer { plot_style: "Black".into(), ..Layer::new("Walls") });
    let line = |y: f64| EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(1.0, y, 0.0) });
    let named = d.add(&Space::Model, Common { plot_style: "Thick".into(), ..Default::default() }, line(0.0)).unwrap();
    let by_layer = d.add(&Space::Model, Common::default(), line(1.0)).unwrap();
    let mut pens = PlotStyleTable::named("pens.stb", "ours", vec![PlotStyle { name: "Thick".into(), lineweight: Some(70), ..PlotStyle::default() }]);
    pens.description = "office pens".into();
    d.plot_style_tables.push(pens.clone());
    let text = write_dxf(&d);
    assert!(text.contains("ACAD_PLOTSTYLENAME") && text.contains("ACDBPLACEHOLDER") && text.contains("ACDBDICTIONARYWDFLT"));
    assert!(text.contains("CADCRAFT_PLOTSTYLES"));
    let back = read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.layer("Walls").unwrap().plot_style, "Black");
    assert_eq!(back.layer("0").unwrap().plot_style, "Normal");
    assert_eq!(back.entity(named).unwrap().common.plot_style, "Thick");
    assert_eq!(back.entity(by_layer).unwrap().common.plot_style, "ByLayer");
    assert_eq!(back.plot_style_tables, vec![pens]);
    // A drawing without named styles still writes the dictionary with Normal, and no tables.
    let plain = write_dxf(&Drawing::new_metric());
    assert!(plain.contains("ACAD_PLOTSTYLENAME") && !plain.contains("CADCRAFT_PLOTSTYLES"));
}

#[test]
fn pdf_applies_a_named_table_from_the_drawing() {
    use cadcraft_doc::{PlotStyle, PlotStyleTable};
    let mut d = layout_with_red_line();
    let blue = PlotStyleTable::named(
        "blue.stb",
        "",
        vec![PlotStyle { name: "Blue".into(), color: Some(cadcraft_color::Rgb(0, 0, 255)), ..PlotStyle::default() }],
    );
    d.plot_style_tables.push(blue);
    let space = Space::Paper("Layout1".into());
    let h = d.space(&space).unwrap().iter().next().unwrap().handle;
    d.modify_entity(h, |e| e.common.plot_style = "Blue".into()).unwrap();
    d.layouts[0].page.plot_style_table = "blue.stb".into();
    let text = pdf_text(&d, json!({}));
    assert!(text.contains("0 0 1 RG") && !text.contains("1 0 0 RG"), "{text}");
}
