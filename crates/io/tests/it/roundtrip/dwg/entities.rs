//! DWG property families stay separate: a failing escape, plot-style or alignment regression
//! must not disable unrelated TEXT or Common tests. Pending cases assert the original model,
//! never today's lossy output. Only confirmed failures receive an explicit ignore reason.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Common, EntityKind, Transparency};
use cadcraft_io::Version;

use super::super::cases;
use super::{check, check_version};

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
#[ignore = "Confirmed DWG bridge loss: named plot style Review becomes ByLayer, including R2018"]
fn named_plot_style_is_preserved() {
    check(cases::plot_styles());
}
#[test]
fn foreign_xdata() {
    check(cases::foreign_xdata());
}
#[test]
fn common_property_interactions() {
    check(cases::common_interactions("ByLayer"));
}
#[test]
#[ignore = "Confirmed DWG bridge loss: named plot style is lost in combination with other properties"]
fn common_property_interactions_with_named_plot_style() {
    check(cases::common_interactions("Review"));
}
#[test]
fn text_values() {
    check(cases::text_values());
}
#[test]
#[ignore = "Confirmed DWG bridge loss: literal Unicode/MIF escape text is decoded again, including R2018"]
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
}
#[test]
#[ignore = "Confirmed change: missing justified TEXT align_pt becomes (0,0,elevation), moving text"]
fn text_alignment_without_points() {
    check(cases::text_alignment(false));
}
#[test]
#[ignore = "Confirmed loss on the shared DXF path: ByBlock plot style becomes ByLayer"]
fn plot_style_byblock_is_preserved() {
    check(cases::byblock_plot_style());
}
#[test]
#[ignore = "Confirmed loss on the shared DXF path: non-default extrusion becomes (0,0,1)"]
fn nondefault_extrusion_is_preserved() {
    check(cases::extrusions());
}

#[test]
fn text_points_share_one_elevation_in_dwg() {
    // Public ODA DWG specification, TEXT (1): insertion and alignment points contain XY;
    // a single elevation supplies both Z coordinates. This is a representational limit, not
    // a bug or a comparison tolerance. DXF's original XYZ cases are still checked unchanged.
    // https://www.opendesign.com/files/guestdownloads/OpenDesign_Specification_for_.dwg_files.pdf
    let cases = cases::noncoplanar_text_alignment();
    assert!(!cases.is_empty());
    for case in cases {
        let mut expected = case.drawing.clone();
        let handles: Vec<_> = expected.model.iter().map(|e| e.handle).collect();
        for handle in handles {
            expected.model.modify(handle, |e| {
                let EntityKind::Text(t) = &mut e.kind else { panic!("expected TEXT") };
                let point = t.align_pt.as_mut().expect("second point");
                point.z = t.insert.z;
            });
        }
        for version in Version::ALL {
            check_version(&case.name, &case.drawing, &expected, version);
        }
    }
}

#[test]
fn r2000_explicit_color_and_transparency_downgrade() {
    // R2000 has no true-color/alpha fields. An exact red RGB maps to ACI 1; neither a fixed
    // alpha nor transparency ByBlock is representable. Expected values are authored here,
    // not learnt from an initial save, and are checked again after the second save.
    for transparency in [Transparency::ByBlock, Transparency::Percent(0), Transparency::Percent(40), Transparency::Percent(90)] {
        let case = cases::common_case(
            &format!("legacy/true-red/transparency={transparency:?}"),
            Common { color: Color::True(Rgb(255, 0, 0)), transparency, ..Common::default() },
        );
        let mut expected = case.drawing.clone();
        let handles: Vec<_> = expected.model.iter().map(|e| e.handle).collect();
        for handle in handles {
            expected.model.modify(handle, |e| {
                e.common.color = Color::Index(1);
                e.common.transparency = Transparency::ByLayer;
            });
        }
        check_version(&case.name, &case.drawing, &expected, Version::R2000);
    }
}
