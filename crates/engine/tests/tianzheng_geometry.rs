//! `properties.set` 改天正墙几何：走 `tianzheng_geometry_setter` hook（安装后）。

use cadcraft_engine::cmd::file::set_tianzheng_geometry_setter;
use cadcraft_engine::doc::{Common, EntityKind, Handle, Space, Tianzheng};
use cadcraft_engine::Session;
use serde_json::{Value, json};

/// 模拟真实的 `cadcraft_io::set_tianzheng_geometry`：只认 `moveWall`，平移 segments 里的每段。
fn setter(d: &mut cadcraft_engine::doc::Drawing, h: Handle, op: &Value) -> bool {
    let (dx, dy, dz) = (op["dx"].as_f64().unwrap(), op["dy"].as_f64().unwrap(), op["dz"].as_f64().unwrap());
    d.model.modify(h, |e| {
        if let EntityKind::Tianzheng(t) = &mut e.kind {
            for s in &mut t.segments {
                *s = (s.0 + dx, s.1 + dy, s.2 + dz, s.3 + dx, s.4 + dy, s.5 + dz);
            }
        }
    })
}

#[test]
fn properties_set_moves_tianzheng_wall_through_hook() {
    set_tianzheng_geometry_setter(setter);

    let mut s = Session::new();
    let seg0 = (100.0, 200.0, 0.0, 100.0, 400.0, 0.0);
    let h = {
        let d = s.doc_mut().unwrap();
        d.add(
            &Space::Model,
            Common::default(),
            EntityKind::Tianzheng(Tianzheng {
                class_name: "TCH_WALL".into(),
                fields: vec![],
                raw_data: None,
                handle_bits: 0,
                tags: vec![],
                dxf_type: "ACAD_PROXY_ENTITY".into(),
                segments: vec![seg0, (240.0, 200.0, 0.0, 240.0, 400.0, 0.0)],
                graphic_data: None,
            }),
        )
        .unwrap()
    };

    let r = s
        .execute("properties.set", &json!({ "handles": [h.hex()], "tianzhengGeometry": { "op": "moveWall", "dx": 10.0, "dy": 20.0, "dz": 0.0 } }))
        .unwrap();
    assert_eq!(r["changed"], 1);

    if let EntityKind::Tianzheng(t) = &s.doc().unwrap().model.get(h).unwrap().kind {
        assert_eq!(t.segments[0], (110.0, 220.0, 0.0, 110.0, 420.0, 0.0));
    } else {
        panic!("实体不是 Tianzheng");
    }
}
