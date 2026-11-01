//! Compound and reference-bearing entities. Geometry and cached values are authored explicitly;
//! unsupported versions are selected by callers rather than normalized after reading.

use super::{Case, entity_case, geometry};
use cadcraft_doc::{
    Block, Common, DimKind, DimMarks, Dimension, Entity, EntityKind, Helix, HelixConstrain, Line, MLeader, MLine, MLineJust, MLineStyle, Region,
    Tolerance,
};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use std::sync::Arc;

pub(crate) fn helices() -> Vec<Case> {
    [false, true]
        .into_iter()
        .flat_map(|ccw| {
            [HelixConstrain::Height, HelixConstrain::Turns, HelixConstrain::TurnHeight].map(|constrain| {
                entity_case(
                    &format!("helix/{ccw}/{constrain:?}"),
                    Common::default(),
                    EntityKind::Helix(Helix {
                        center: Vec3::new(1.25, -2.5, 3.75),
                        base_radius: 3.0,
                        top_radius: 1.5,
                        height: -7.5,
                        turns: 2.5,
                        start_angle: 0.0,
                        ccw,
                        constrain,
                    }),
                )
            })
        })
        .collect()
}

pub(crate) fn mlines() -> Vec<Case> {
    [false, true]
        .into_iter()
        .flat_map(|closed| {
            [MLineJust::Top, MLineJust::Zero, MLineJust::Bottom].map(|just| {
                entity_case(
                    &format!("mline/{closed}/{just:?}"),
                    Common::default(),
                    EntityKind::MLine(MLine::new(&MLineStyle::default(), &[Vec2::ZERO, Vec2::new(5.0, 0.0), Vec2::new(5.0, 4.0)], closed, just, 2.5)),
                )
            })
        })
        .collect()
}

pub(crate) fn mleaders() -> Vec<Case> {
    [false, true]
        .map(|spline| {
            entity_case(
                &format!("mleader/spline={spline}"),
                Common::default(),
                EntityKind::MLeader(MLeader {
                    leaders: vec![vec![Vec3::ZERO, Vec3::new(2.0, 3.0, 0.0)], vec![Vec3::new(1.0, -2.0, 0.0)]],
                    landing: Vec3::new(5.0, 4.0, 0.0),
                    dogleg: 0.75,
                    text: Some(cadcraft_doc::MText { insert: Vec3::new(5.9, 4.0, 0.0), rotation: 0.0, ..geometry::mtext() }),
                    style: "Standard".into(),
                    arrow_size: 0.3,
                    spline,
                    block: None,
                }),
            )
        })
        .into()
}

pub(crate) fn regions() -> Vec<Case> {
    vec![entity_case(
        "region/bulged-loop",
        Common::default(),
        EntityKind::Region(Region::from_loop(vec![
            PolyVertex::with_bulge(Vec2::ZERO, 0.5),
            PolyVertex::new(Vec2::new(5.0, 0.0)),
            PolyVertex::new(Vec2::new(5.0, 4.0)),
            PolyVertex::new(Vec2::new(0.0, 4.0)),
        ])),
    )]
}

pub(crate) fn tolerances() -> Vec<Case> {
    vec![entity_case(
        "tolerance/frame",
        Common::default(),
        EntityKind::Tolerance(Tolerance {
            insert: Vec3::new(1.25, -2.5, 3.75),
            dir: Vec3::new(0.0, 1.0, 0.0),
            style: "ISO-25".into(),
            text: "0.125%%vA%%vB".into(),
        }),
    )]
}

pub(crate) fn dimensions() -> Vec<Case> {
    [DimKind::Linear { rotation: 0.0 }, DimKind::Aligned]
        .map(|kind| {
            let mut case = entity_case(
                &format!("dimension/{kind:?}"),
                Common::default(),
                EntityKind::Dimension(Dimension {
                    kind,
                    defpt: Vec3::new(0.0, 3.0, 0.0),
                    text_mid: Vec3::new(5.0, 4.0, 0.0),
                    p13: Vec3::ZERO,
                    p14: Vec3::new(10.0, 0.0, 0.0),
                    p15: Vec3::ZERO,
                    p16: Vec3::ZERO,
                    text: "<> mm".into(),
                    style: "ISO-25".into(),
                    measurement: 10.0,
                    text_rotation: 0.0,
                    user_text_pos: true,
                    block: Some("*D42".into()),
                    overrides: Default::default(),
                    assoc: Vec::new(),
                    marks: DimMarks::default(),
                }),
            );
            let mut block = Block::new("*D42");
            block
                .entities
                .push(Entity::new(case.drawing.new_handle(), EntityKind::Line(Line { a: Vec3::new(0.0, 3.0, 0.0), b: Vec3::new(10.0, 3.0, 0.0) })));
            case.drawing.blocks.insert(block.name.clone(), Arc::new(block));
            case
        })
        .into()
}
