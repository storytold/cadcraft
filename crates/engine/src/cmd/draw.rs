//! Draw commands: LINE, PLINE, CIRCLE, ARC, RECTANG, POLYGON, ELLIPSE, POINT, XLINE, RAY,
//! SPLINE, DONUT, TEXT, MTEXT. Each has a JSON form and an interactive prompt sequence.

use cadcraft_doc::{EntityKind, HAlign, MText, Point, RayLine, Text, VAlign};
use cadcraft_geom::{Arc, Circle, Ellipse, PolyVertex, Polyline, Spline, Vec2, arc_to_bulge, bulge_to_arc};
use serde_json::{Value, json};

use super::helpers::{arc, circle, line, lwpoly, polygon_from_edge, polygon_typed_radius_point, polygon_vertices, v3};

use super::machines::number;
use super::pline_opts::{Outcome, PlineAsk};
use super::{
    CommandSpec, bad, bool_or, distinct_points, f64_or, f64_req, find_command, point_param, point_req, point_value, points_param, size_param,
    str_param,
};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step, snap};

mod circle;
mod ellipse;
mod line_pline;
mod point;
mod polygon;
mod spline;
mod text;

use circle::{CircleM, run_arc, run_circle};
pub(crate) use ellipse::ellipse_machine;
use ellipse::{EllipseM, run_ellipse};
use line_pline::{LineM, PlineM, run_line, run_pline};
use point::{DonutM, PointM, RayM, run_donut, run_point, run_ray};
use polygon::{PolygonM, run_polygon};
use spline::{SplineM, run_spline};
use text::{MTextM, TextM, run_mtext, run_text};

/// Every draw command's spec: JSON parameters, menu path, aliases and the interactive prompt
/// machine each command starts.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("line", "Line", run_line)
            .menu(&["Draw", "Line"])
            .alias(&["l"])
            .params("{points: [[x,y],...], closed?: bool}")
            .interactive(|_| Ok(Box::new(LineM::default()))),
        CommandSpec::new("pline", "Polyline", run_pline)
            .menu(&["Draw", "Polyline"])
            .alias(&["pl"])
            .params("{vertices: [[x,y] | {p:[x,y], bulge, startWidth, endWidth}], closed?, width?}")
            .interactive(|_| Ok(Box::new(PlineM::default()))),
        CommandSpec::new("circle", "Circle", run_circle)
            .menu(&["Draw", "Circle", "Center, Radius"])
            .alias(&["c"])
            .params("{center, radius} | {center, diameter} | {p1, p2} | {p1, p2, p3}")
            .interactive(|_| Ok(Box::new(CircleM::default()))),
        CommandSpec::new("arc", "Arc", run_arc)
            .menu(&["Draw", "Arc", "3 Points"])
            .alias(&["a"])
            .params("{p1, p2, p3} | {center, radius, start, end (degrees)} | {start, center, end}")
            .interactive(|_| Ok(super::draw2::arc_machine())),
        CommandSpec::new("rectang", "Rectangle", super::draw2::run_rectang2)
            .menu(&["Draw", "Rectangle"])
            .alias(&["rec", "rectangle"])
            .params("{p1, p2 | dimensions: [l, w] (p2 picks the quadrant) | area + length|breadth, rotation? (deg), fillet?, chamfer?: [d1, d2], width?, elevation?, thickness?, center?: bool (p1 is the centre, not a corner; the size spans it evenly)}")
            .interactive(|_| Ok(Box::new(super::draw2::RectM2::default()))),
        CommandSpec::new("polygon", "Polygon", run_polygon)
            .menu(&["Draw", "Polygon"])
            .alias(&["pol"])
            .params("{sides, center, radius, inscribed?: bool, angle?: deg to a vertex (inscribed) or edge midpoint (default: bottom edge horizontal)} | {sides, edge: [[x,y],[x,y]]}")
            .interactive(|_| Ok(Box::new(PolygonM::default()))),
        CommandSpec::new("ellipse", "Ellipse", run_ellipse)
            .menu(&["Draw", "Ellipse", "Center"])
            .alias(&["el"])
            .params("{center, major: [dx,dy], ratio, start?, end? (degrees)} | {isocircle: {center, radius | diameter, plane?}}")
            .interactive(|_| Ok(Box::new(EllipseM::default()))),
        CommandSpec::new("point", "Point", run_point)
            .menu(&["Draw", "Point", "Single Point"])
            .alias(&["po"])
            .params("{at: [x,y]} | {points: [...]}")
            .interactive(|_| Ok(Box::new(PointM { multiple: false }))),
        CommandSpec::new("point.multiple", "Multiple Point", run_point)
            .menu(&["Draw", "Point", "Multiple Point"])
            .params("{points: [...]}")
            .interactive(|_| Ok(Box::new(PointM { multiple: true }))),
        CommandSpec::new("xline", "Construction Line", super::draw2::run_xline2)
            .menu(&["Draw", "Construction Line"])
            .alias(&["xl"])
            .params("{base, through} | {base, angle (degrees)} | {base, hor|ver: true} | {vertex, start, end} (bisect) | {handle, distance, side} | {handle, through} (offset)")
            .interactive(|_| Ok(Box::new(super::draw2::XlineM2::default()))),
        CommandSpec::new("ray", "Ray", run_ray)
            .menu(&["Draw", "Ray"])
            .params("{base, through}")
            .interactive(|_| Ok(Box::new(RayM::default()))),
        CommandSpec::new("spline", "Spline", run_spline)
            .menu(&["Draw", "Spline", "Fit Points"])
            .alias(&["spl"])
            .params("{fit: [[x,y],...], closed?, knots?: chord|sqrt|uniform, startTangent?, endTangent? ([dx,dy]), tolerance?} | {control: [...], degree?} | {object: handle|true, handles?} (polyline to spline)")
            .interactive(|_| Ok(Box::new(SplineM::default()))),
        CommandSpec::new("donut", "Donut", run_donut)
            .menu(&["Draw", "Donut"])
            .alias(&["do", "doughnut"])
            .params("{center, inside, outside}")
            .interactive(|_| Ok(Box::new(DonutM::default()))),
        CommandSpec::new("text", "Single Line Text", run_text)
            .menu(&["Draw", "Text", "Single Line Text"])
            .alias(&["dt", "dtext"])
            .params("{at, text, height?, rotation? (degrees), justify?: L|C|R|M|TL|TC|TR|ML|MC|MR|BL|BC|BR}")
            .interactive(|s| Ok(Box::new(TextM::new(s)))),
        CommandSpec::new("mtext", "Multiline Text", run_mtext)
            .menu(&["Draw", "Text", "Multiline Text..."])
            .alias(&["t", "mt"])
            .params(
                "{at, text, height?, width?, attach?: 1..9 | justify?: TL..BR, rotation?, style?, lineSpacing?, lineSpacingExact?, \
                 list?: bullet|number|lower|upper, indent?, firstIndent?, rightIndent?, tabs?: [..], align?: left|center|right|justify|distributed, \
                 columns?: {type: static|dynamic, count?, gutter?, height, heights?, reversed?}} (indents and tabs in text heights; width = one column)",
            )
            .interactive(|_| Ok(Box::new(MTextM::default()))),
    ]
}

fn added(h: cadcraft_doc::Handle) -> Result<Value> {
    Ok(json!({ "handle": h.hex() }))
}

/// Re-export for tests.
pub(crate) fn _unused(_: Polyline) {}
