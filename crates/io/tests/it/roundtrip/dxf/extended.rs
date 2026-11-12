//! Extended DXF property families. Pending losses stay separate from passing families;
//! version domains are explicit for Lock position, HELIX/MULTILEADER (2007) and the native
//! multiline-attribute representation (2018).

use super::super::cases::{Case, blocks, complex, geometry, properties};
use super::super::collections;
use cadcraft_io::Version;

fn check(cases: Vec<Case>) {
    assert!(!cases.is_empty());
    for case in cases {
        super::check(&case.name, &case.drawing);
    }
}
fn since_2007(cases: Vec<Case>) {
    assert!(!cases.is_empty());
    for case in cases {
        for version in Version::ALL.into_iter().filter(|v| *v >= Version::R2007) {
            super::check_version(&case.name, &case.drawing, &case.drawing, version);
        }
    }
}

#[test]
fn mixed_case_layers() {
    check(properties::mixed_case_references("layer"));
}
#[test]
fn mixed_case_linetypes() {
    check(properties::mixed_case_references("linetype"));
}
#[test]
fn mixed_case_styles() {
    check(properties::mixed_case_references("style"));
}
#[test]
fn layer_flags() {
    check(properties::layer_flags());
}
#[test]
fn layer_appearance() {
    check(properties::layer_appearance());
}
#[test]
fn layer_plot_styles() {
    check(properties::layer_plot_styles());
}
#[test]
fn text_style_metrics() {
    check(properties::text_style_metrics());
}
#[test]
fn text_style_flags() {
    check(properties::text_style_flags());
}
#[test]
fn text_style_fonts() {
    check(properties::text_style_fonts());
}
#[test]
fn linetype_text() {
    check(properties::complex_linetypes(false));
}
#[test]
fn linetype_shapes() {
    check(properties::complex_linetypes(true));
}
#[test]
fn xdata_lists() {
    check(properties::xdata_lists());
}
#[test]
fn xdata_points() {
    check(properties::xdata_points());
}
#[test]
fn xdata_handles() {
    check(properties::xdata_handles());
}
#[test]
fn handles_and_order() {
    check(properties::handles_and_order());
}
#[test]
fn handles_near_integer_limits() {
    check(properties::handles_near_integer_limits());
}
#[test]
fn degenerate_lines() {
    check(properties::degenerate_geometry());
}
#[test]
fn layer_inheritance_overrides() {
    check(properties::layer_inheritance_overrides());
}
#[test]
fn alignment_style_interactions() {
    check(properties::alignment_style_interactions());
}
#[test]
fn nested_attributes() {
    check(blocks::nested_attributes());
}
#[test]
fn text_interactions() {
    check(properties::text_interactions());
}
#[test]
fn long_text() {
    check(properties::long_text());
}
#[test]
fn arcs() {
    check(geometry::arcs());
}
#[test]
fn ellipses() {
    check(geometry::ellipses());
}
#[test]
fn polyline_constant_widths() {
    check(geometry::polylines(false));
}
#[test]
fn polyline_variable_widths() {
    check(geometry::polylines(true));
}
#[test]
fn polylines_3d() {
    check(geometry::polylines_3d());
}
#[test]
fn splines() {
    check(geometry::splines(false));
}
#[test]
fn rational_splines() {
    check(geometry::splines(true));
}
#[test]
fn fit_splines() {
    check(geometry::spline_fit(false));
    check(geometry::spline_fit(true));
}
#[test]
fn points_and_rays() {
    check(geometry::points_and_rays());
}
#[test]
fn faces() {
    check(geometry::faces());
}
#[test]
fn mtext_placement() {
    check(geometry::mtext_placement());
}
#[test]
fn mtext_spacing() {
    check(geometry::mtext_spacing());
}
#[test]
fn mtext_contents() {
    check(geometry::mtext_contents());
    check(geometry::mtext_carets());
}
#[test]
fn mtext_long() {
    check(geometry::mtext_long());
}
#[test]
#[ignore = "Confirmed DXF loss: MTEXT column group 48 is also read as Common.ltscale"]
fn mtext_columns() {
    check(geometry::mtext_columns());
}
#[test]
fn leaders() {
    check(geometry::leaders(false));
}
#[test]
fn hatches() {
    check(geometry::hatches());
}
#[test]
fn tables() {
    check(geometry::tables());
}
#[test]
fn images() {
    check(geometry::images());
}
#[test]
fn wipeouts() {
    check(geometry::wipeouts(true));
}
#[test]
fn wipeout_implicit_closure() {
    // The format closes the stored boundary explicitly; this is not an extra editable corner.
    // Keep grip behavior separate from this serialization contract (#2256).
    for (case, expected) in geometry::wipeouts(false).into_iter().zip(geometry::wipeouts(true)) {
        for version in Version::ALL {
            super::check_version(&case.name, &case.drawing, &expected.drawing, version);
        }
    }
}
#[test]
#[ignore = "Confirmed DXF loss: LEADER vertex Z coordinates become zero"]
fn leaders_3d() {
    check(geometry::leaders(true));
}
#[test]
#[ignore = "Confirmed DXF loss: ATTRIB constant flag becomes false"]
fn constant_attributes() {
    check(blocks::attributes("constant"));
}
#[test]
fn block_transformations() {
    check(blocks::transformations());
}
#[test]
fn block_arrays() {
    check(blocks::arrays());
}
#[test]
fn block_metadata() {
    check(blocks::metadata());
}
#[test]
fn nested_blocks() {
    check(blocks::nested(false));
}
#[test]
fn cyclic_blocks() {
    check(blocks::nested(true));
}
#[test]
fn unreferenced_blocks() {
    check(blocks::unreferenced());
}
#[test]
fn attribute_flags() {
    check(blocks::attributes("flags"));
}
#[test]
fn attribute_modes() {
    check(blocks::attributes("modes"));
}
#[test]
fn attribute_lock() {
    since_2007(blocks::attributes("lock"));
}
#[test]
#[ignore = "Confirmed DXF read loss (#2255): multiline header changes Left to Center and synthesizes align_pt; existing fix #1942"]
fn multiline_attributes() {
    for case in blocks::multiline_attributes() {
        super::check_version(&case.name, &case.drawing, &case.drawing, Version::R2018);
    }
}
#[test]
fn attribute_overrides() {
    check(blocks::attributes("overrides"));
}
#[test]
fn helices() {
    since_2007(complex::helices());
}
#[test]
fn mlines() {
    check(complex::mlines());
}
#[test]
fn mleaders() {
    since_2007(complex::mleaders());
}
#[test]
fn regions() {
    check(complex::regions());
}
#[test]
fn tolerances() {
    check(complex::tolerances());
}
#[test]
fn dimensions() {
    check(complex::dimensions());
}

fn collection(case: Case, verify: fn(&cadcraft_doc::Drawing, &cadcraft_doc::Drawing, &str)) {
    for version in Version::ALL {
        super::check_version_with(&case.name, &case.drawing, &case.drawing, version, verify);
    }
}
#[test]
fn header_variables() {
    collection(collections::headers(), collections::assert_headers);
    for key in ["CLAYER", "TEXTSTYLE"] {
        collection(collections::named_header_reference(key), collections::assert_headers);
    }
}
#[test]
fn unreferenced_tables() {
    collection(collections::symbol_tables(), collections::assert_symbol_tables);
}
#[test]
fn foreign_dictionary_records() {
    collection(collections::foreign_records(), collections::assert_foreign_records);
}
#[test]
fn unknown_entity_payload_and_references() {
    for version in Version::ALL {
        for binary in [false, true] {
            super::super::unknown::check("memory.dxf", version, binary);
        }
    }
}
#[test]
fn views_and_ucs() {
    collection(collections::views_and_ucs(), collections::assert_views_and_ucs);
}
#[test]
fn tiled_viewports() {
    collection(collections::vports(), collections::assert_vports);
}
#[test]
fn groups() {
    collection(collections::groups(), collections::assert_groups);
}
#[test]
fn layer_states() {
    collection(collections::layer_states(), collections::assert_layer_states);
}
#[test]
fn layouts_and_paperspace() {
    collection(collections::layouts(), collections::assert_layouts);
}
