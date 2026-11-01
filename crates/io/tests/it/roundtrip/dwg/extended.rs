//! Native DWG extensions to the shared preservation matrix. Version limits (native columns,
//! attributes, HELIX/MULTILEADER and table fields) are explicit, as are named-reference
//! resolution and legacy defaults. Other inputs use the full save-version domain.

use super::super::cases::{Case, blocks, complex, geometry, properties};
use super::super::collections;
use super::check;
use cadcraft_io::Version;

// DWG references a table record by handle, not by an independently spelled name. These
// explicit expected spellings test resolution, not a blanket case-folding comparator.
fn canonical_reference(kind: &str, name: &str) {
    for case in properties::mixed_case_references(kind) {
        let mut expected = case.drawing.clone();
        let handles: Vec<_> = expected.model.iter().map(|e| e.handle).collect();
        for handle in handles {
            expected.model.modify(handle, |e| match kind {
                "layer" => e.common.layer = name.into(),
                "linetype" => e.common.linetype = name.into(),
                "style" => {
                    let cadcraft_doc::EntityKind::Text(t) = &mut e.kind else { panic!("expected TEXT") };
                    t.style = name.into();
                }
                _ => panic!("unknown reference family"),
            });
        }
        for version in Version::ALL {
            super::check_version(&case.name, &case.drawing, &expected, version);
        }
    }
}

#[test]
fn legacy_block_metadata() {
    // DWG BLOCK_HEADER gains insertion units and explodability in R2007 (ODA specification).
    for case in blocks::metadata() {
        let mut expected = case.drawing.clone();
        let block = std::sync::Arc::make_mut(expected.blocks.get_mut("Z_Leaf").unwrap());
        block.explodable = true;
        block.units = 0;
        for version in [Version::R2000, Version::R2004] {
            super::check_version(&case.name, &case.drawing, &expected, version);
        }
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
    canonical_reference("layer", "Survey_Ä");
}
#[test]
fn mixed_case_linetypes() {
    canonical_reference("linetype", "Dash_test");
}
#[test]
fn mixed_case_styles() {
    canonical_reference("style", "Notes");
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
#[ignore = "Confirmed DWG loss: named layer plot style Review becomes Normal"]
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
}
#[test]
#[ignore = "Confirmed DWG loss: periodic closed fit spline reopens as an open spline"]
fn closed_fit_splines() {
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
}
#[test]
fn mtext_long() {
    check(geometry::mtext_long());
}
#[test]
#[ignore = "Confirmed DWG loss: MTEXT columns disappear"]
fn mtext_columns() {
    since_2007(geometry::mtext_columns());
}
#[test]
fn leaders() {
    with_dimension_styles(geometry::leaders(false));
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
#[ignore = "Confirmed shared DXF loss: LEADER vertex Z coordinates become zero"]
fn leaders_3d() {
    check(geometry::leaders(true));
}
#[test]
#[ignore = "Confirmed shared DXF loss: ATTRIB constant flag becomes false"]
fn constant_attributes() {
    check(blocks::attributes("constant"));
}
#[test]
#[ignore = "Confirmed DWG loss: a space following an MTEXT caret is consumed"]
fn mtext_carets() {
    check(geometry::mtext_carets());
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
    since_2007(blocks::metadata());
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
    with_dimension_styles(complex::tolerances());
}
#[test]
fn dimensions() {
    super::dimensions::check();
}

fn collection(case: Case, verify: fn(&cadcraft_doc::Drawing, &cadcraft_doc::Drawing, &str)) {
    for version in Version::ALL {
        super::check_version_with(&case.name, &case.drawing, &case.drawing, version, verify);
    }
}
#[test]
fn header_variables() {
    collection(collections::headers(), collections::assert_headers);
}
#[test]
#[ignore = "Confirmed DWG loss: current layer CLAYER becomes 0"]
fn header_current_layer() {
    collection(collections::named_header_reference("CLAYER"), collections::assert_headers);
}
#[test]
fn header_text_style() {
    collection(collections::named_header_reference("TEXTSTYLE"), collections::assert_headers);
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
#[ignore = "Confirmed DWG data loss: saving an unregistered entity succeeds but drops it instead of preserving it or returning an error"]
fn unknown_entities_are_preserved_or_explicitly_refused() {
    for version in Version::ALL {
        super::super::unknown::check("memory.dwg", version, false);
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

fn with_dimension_styles(cases: Vec<Case>) {
    // Fixed-length extension lines (DIMFXL) were added in 2007. Older DWG records have no
    // slot for the inactive default length, and the bridge supplies zero on read.
    for case in cases {
        for version in Version::ALL {
            let mut expected = case.drawing.clone();
            if version < Version::R2007 {
                for style in &mut expected.dim_styles {
                    style.fixed_ext_length = 0.0;
                }
            }
            super::check_version(&case.name, &case.drawing, &expected, version);
        }
    }
}
