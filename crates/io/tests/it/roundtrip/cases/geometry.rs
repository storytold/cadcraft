//! Additional entity families with authored geometry and optional fields. No serializer output
//! is used to build an input. New failures are isolated by the format-specific test registrations.

use cadcraft_doc::{
    Arc, Common, Ellipse, EntityKind, Face3d, Hatch, HatchLoop, Image, Leader, LwPolyline, MText, MTextColumnType, MTextColumns, Point, Polyline3d,
    RayLine, Solid, Table, TableCell, Wipeout,
};
use cadcraft_geom::{PolyVertex, Spline, Vec2, Vec3};

use super::{Case, entity_case};

fn case(name: &str, kind: EntityKind) -> Case {
    entity_case(name, Common::default(), kind)
}

pub(crate) fn arcs() -> Vec<Case> {
    let mut cases = Vec::new();
    for radius in [1e-9, 0.125, 3.25, 1e8] {
        for (start, end) in [(0.0, std::f64::consts::FRAC_PI_2), (0.7, 5.9), (5.9, 0.7)] {
            cases.push(case(
                &format!("arc/r={radius}/{start}..{end}"),
                EntityKind::Arc(Arc { center: Vec3::new(1.25, -2.5, 3.75), radius, start, end }),
            ));
        }
    }
    cases
}

pub(crate) fn ellipses() -> Vec<Case> {
    [0.125, 0.5, 1.0]
        .into_iter()
        .flat_map(|ratio| {
            [(0.0, std::f64::consts::TAU), (0.7, 5.9), (5.9, 0.7)].map(|(start, end)| {
                case(
                    &format!("ellipse/{ratio}/{start}..{end}"),
                    EntityKind::Ellipse(Ellipse { center: Vec3::new(1.25, -2.5, 3.75), major: Vec3::new(3.0, 4.0, 0.0), ratio, start, end }),
                )
            })
        })
        .collect()
}

pub(crate) fn polylines(variable_width: bool) -> Vec<Case> {
    let mut cases = Vec::new();
    for closed in [false, true] {
        for plinegen in [false, true] {
            for bulge in [-1.0, 0.0, 0.5, 1.0] {
                cases.push(case(
                    &format!("lwpolyline/closed={closed}/plinegen={plinegen}/bulge={bulge}/widths={variable_width}"),
                    EntityKind::LwPolyline(LwPolyline {
                        vertices: vec![
                            PolyVertex {
                                p: Vec2::new(1.25, -2.5),
                                bulge,
                                start_width: if variable_width { 0.125 } else { 0.0 },
                                end_width: if variable_width { 0.75 } else { 0.0 },
                            },
                            PolyVertex::with_bulge(Vec2::new(5.0, 3.0), -bulge),
                            PolyVertex::new(Vec2::new(-2.0, 4.0)),
                        ],
                        closed,
                        plinegen,
                        const_width: if variable_width { 0.0 } else { 0.25 },
                        elevation: -3.75,
                    }),
                ));
            }
        }
    }
    cases
}

pub(crate) fn polylines_3d() -> Vec<Case> {
    [false, true]
        .map(|closed| {
            case(
                &format!("polyline3d/closed={closed}"),
                EntityKind::Polyline3d(Polyline3d {
                    points: vec![Vec3::new(1.25, -2.5, 3.75), Vec3::new(5.0, 6.0, -7.0), Vec3::new(-3.0, 4.0, 9.0)],
                    closed,
                }),
            )
        })
        .into()
}

pub(crate) fn splines(rational: bool) -> Vec<Case> {
    [1, 2, 3]
        .map(|degree| {
            let mut spline = Spline::from_control(vec![Vec2::ZERO, Vec2::new(2.0, 3.0), Vec2::new(5.0, -1.0), Vec2::new(8.0, 2.0)], degree);
            if rational {
                spline.weights = vec![1.0, 0.5, 2.0, 1.0];
            }
            case(&format!("spline/degree={degree}/rational={rational}"), EntityKind::Spline(spline))
        })
        .into()
}

pub(crate) fn spline_fit(closed: bool) -> Vec<Case> {
    vec![case(
        &format!("spline/fit/closed={closed}"),
        EntityKind::Spline(Spline::from_fit(&[Vec2::ZERO, Vec2::new(2.0, 3.0), Vec2::new(5.0, -1.0), Vec2::new(8.0, 2.0)], closed)),
    )]
}

pub(crate) fn points_and_rays() -> Vec<Case> {
    let base = Vec3::new(1.25, -2.5, 3.75);
    vec![
        case("point", EntityKind::Point(Point { p: base, angle: 0.0 })),
        case("ray", EntityKind::Ray(RayLine { base, dir: Vec3::new(0.0, 0.0, 1.0) })),
        case("xline", EntityKind::XLine(RayLine { base, dir: Vec3::new(0.0, 1.0, 0.0) })),
    ]
}

pub(crate) fn faces() -> Vec<Case> {
    let corners = [Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 2.0, 3.0), Vec3::new(1.0, 5.0, 3.0), Vec3::new(4.0, 5.0, 3.0)];
    let mut cases = vec![case("solid", EntityKind::Solid(Solid { corners })), case("trace", EntityKind::Trace(Solid { corners }))];
    for hidden_edges in 0..16 {
        cases.push(case(&format!("face3d/hidden={hidden_edges}"), EntityKind::Face3d(Face3d { corners, hidden_edges })));
    }
    cases
}

pub(crate) fn mtext() -> MText {
    MText {
        insert: Vec3::new(1.25, -2.5, 3.75),
        height: 2.25,
        width: 12.5,
        attach: 1,
        rotation: 0.0,
        style: "Notes".into(),
        contents: "first\\Psecond Ø 图 😀".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    }
}

pub(crate) fn mtext_placement() -> Vec<Case> {
    (1..=9)
        .flat_map(|attach| {
            [0.0, 0.7, 5.9]
                .map(|rotation| case(&format!("mtext/attach={attach}/rotation={rotation}"), EntityKind::MText(MText { attach, rotation, ..mtext() })))
        })
        .collect()
}

pub(crate) fn mtext_spacing() -> Vec<Case> {
    [0.25, 1.0, 1.75, 4.0]
        .into_iter()
        .flat_map(|line_spacing| {
            [false, true].map(|line_spacing_exact| {
                case(
                    &format!("mtext/spacing={line_spacing}/exact={line_spacing_exact}"),
                    EntityKind::MText(MText { line_spacing, line_spacing_exact, ..mtext() }),
                )
            })
        })
        .collect()
}

pub(crate) fn mtext_contents() -> Vec<Case> {
    ["", "  a\tb  ", "{\\H0.7x;\\S1/2;}\\Pnext", "Unicode Ø 图 😀 e\u{301}"]
        .map(|contents| case(&format!("mtext/contents={contents:?}"), EntityKind::MText(MText { contents: contents.into(), ..mtext() })))
        .into()
}

pub(crate) fn mtext_carets() -> Vec<Case> {
    vec![case("mtext/caret-space", EntityKind::MText(MText { contents: "\\pxi-3,l3,t3;1.\tA\\P2.\tB ^ C".into(), ..mtext() }))]
}

pub(crate) fn mtext_long() -> Vec<Case> {
    [249, 250, 251, 255, 256, 1024]
        .map(|n| case(&format!("mtext/long={n}"), EntityKind::MText(MText { contents: "Ø😀".repeat(n), ..mtext() })))
        .into()
}

pub(crate) fn mtext_columns() -> Vec<Case> {
    [
        MTextColumns { kind: MTextColumnType::Static, count: 3, gutter: 0.5, height: 7.5, ..Default::default() },
        MTextColumns {
            kind: MTextColumnType::Dynamic,
            count: 3,
            gutter: 0.5,
            height: 5.0,
            heights: vec![5.0, 3.0, 4.0],
            flow_reversed: true,
            ..Default::default()
        },
        MTextColumns { kind: MTextColumnType::Dynamic, count: 1, gutter: 0.5, height: 4.0, auto_height: true, ..Default::default() },
    ]
    .into_iter()
    .map(|columns| case(&format!("mtext/columns={columns:?}"), EntityKind::MText(MText { columns: Some(columns), ..mtext() })))
    .collect()
}

pub(crate) fn leaders(elevated: bool) -> Vec<Case> {
    [false, true]
        .into_iter()
        .flat_map(|arrow| {
            [false, true].map(|spline| {
                case(
                    &format!("leader/arrow={arrow}/spline={spline}/elevated={elevated}"),
                    EntityKind::Leader(Leader {
                        vertices: vec![
                            Vec3::ZERO,
                            Vec3::new(2.0, 3.0, if elevated { 4.0 } else { 0.0 }),
                            Vec3::new(5.0, 6.0, if elevated { 7.0 } else { 0.0 }),
                        ],
                        arrow,
                        spline,
                        style: "ISO-25".into(),
                    }),
                )
            })
        })
        .collect()
}

pub(crate) fn hatches() -> Vec<Case> {
    let vertices = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 8.0), Vec2::new(0.0, 8.0)].map(PolyVertex::new).to_vec();
    (0..=2)
        .map(|style| {
            case(
                &format!("hatch/solid/style={style}"),
                EntityKind::Hatch(Hatch {
                    pattern: "SOLID".into(),
                    solid: true,
                    loops: vec![HatchLoop { vertices: vertices.clone(), outer: true, sources: Vec::new() }],
                    scale: 1.0,
                    angle: 0.0,
                    associative: false,
                    style,
                    elevation: 3.75,
                    gradient: None,
                    origin: Vec2::ZERO,
                    background: None,
                    pattern_lines: Vec::new(),
                }),
            )
        })
        .collect()
}

pub(crate) fn tables() -> Vec<Case> {
    vec![case(
        "table/cells",
        EntityKind::Table(Table {
            insert: Vec3::new(1.25, -2.5, 0.0),
            col_widths: vec![5.0, 7.5],
            row_heights: vec![2.0, 3.0],
            cells: vec![
                vec![TableCell { text: "Ø".into(), ..Default::default() }, TableCell { text: "图".into(), ..Default::default() }],
                vec![TableCell::default(), TableCell { text: "3.25".into(), ..Default::default() }],
            ],
            style: "Standard".into(),
            text_height: 1.25,
            title: false,
            header: false,
            row_styles: Vec::new(),
            breaks: None,
            data_link: None,
        }),
    )]
}

pub(crate) fn images() -> Vec<Case> {
    vec![case(
        "image/reference-only",
        EntityKind::Image(Image {
            path: "synthetic-not-loaded.png".into(),
            name: "Synthetic".into(),
            insert: Vec3::new(1.0, 2.0, 3.0),
            u: Vec3::new(0.5, 0.0, 0.0),
            v: Vec3::new(0.0, 0.25, 0.0),
            size: Vec2::new(32.0, 16.0),
            clip: vec![Vec2::new(2.0, 3.0), Vec2::new(20.0, 12.0)],
            clipping: true,
            brightness: 37,
            contrast: 61,
            fade: 25,
            ..Image::default()
        }),
    )]
}

pub(crate) fn wipeouts(explicit_closure: bool) -> Vec<Case> {
    [false, true]
        .into_iter()
        .map(|reversed| {
            let mut boundary = vec![Vec2::new(1.0, 2.0), Vec2::new(7.0, 3.0), Vec2::new(4.0, 9.0)];
            if reversed {
                boundary.reverse();
            }
            if explicit_closure {
                boundary.push(boundary[0]);
            }
            case(&format!("wipeout/triangle/explicit-closure={explicit_closure}/reversed={reversed}"), EntityKind::Wipeout(Wipeout { boundary }))
        })
        .collect()
}
