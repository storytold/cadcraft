//! DXF runs every shared input family in both encodings and all writable versions. Keep known
//! failures isolated from other property sweeps; the comparison contract is never weakened.

use super::super::cases;

fn check(cases: Vec<cases::Case>) {
    for case in cases {
        super::check(&case.name, &case.drawing);
    }
}

#[test]
fn empty_model_and_defaults() {
    check(cases::empty_and_defaults());
}
#[test]
fn line_endpoints_and_circle_centers_radii() {
    check(cases::geometry());
}
#[test]
fn layers() {
    check(cases::layers());
}
#[test]
fn layer_inherited_linetypes() {
    check(cases::layer_linetypes());
}
#[test]
fn colors() {
    check(cases::colors());
}
#[test]
fn linetypes() {
    check(cases::linetypes());
}
#[test]
fn lineweights() {
    check(cases::lineweights());
}
#[test]
fn linetype_scales() {
    check(cases::linetype_scales());
}
#[test]
fn transparencies() {
    check(cases::transparencies());
}
#[test]
fn visibility() {
    check(cases::visibility());
}
#[test]
fn thicknesses() {
    check(cases::thicknesses());
}
#[test]
fn plot_styles() {
    check(cases::plot_styles());
}
#[test]
fn foreign_xdata() {
    check(cases::foreign_xdata());
}
#[test]
fn common_property_interactions() {
    check(cases::common_interactions("ByLayer"));
    check(cases::common_interactions("Review"));
}
#[test]
fn text_values() {
    check(cases::text_values());
}
#[test]
fn text_literal_escapes() {
    check(cases::text_literal_escapes());
}
#[test]
fn text_heights() {
    check(cases::text_heights());
}
#[test]
fn text_rotations() {
    check(cases::text_rotations());
}
#[test]
fn text_widths() {
    check(cases::text_widths());
}
#[test]
fn text_obliques() {
    check(cases::text_obliques());
}
#[test]
fn text_styles() {
    check(cases::text_styles());
}
#[test]
fn annotation_scales() {
    check(cases::annotation_scales());
}
#[test]
fn text_alignment_with_points() {
    check(cases::coplanar_text_alignment());
    check(cases::noncoplanar_text_alignment());
}
#[test]
fn text_alignment_without_points() {
    check(cases::text_alignment(false));
}

#[test]
#[ignore = "Known writer loss: ByBlock plot style becomes ByLayer on the first R2000 ASCII save"]
fn plot_style_byblock_is_preserved() {
    check(cases::byblock_plot_style());
}

#[test]
#[ignore = "Known writer loss: groups 210/220/230 are omitted; a LINE's normal becomes (0,0,1)"]
fn nondefault_extrusion_is_preserved() {
    check(cases::extrusions());
}
