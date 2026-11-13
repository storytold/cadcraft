//! `properties.set` 编辑天正自定义实体字段：走 `tianzheng_setter` hook（安装后）。

use cadcraft_engine::cmd::file::set_tianzheng_setter;
use cadcraft_engine::doc::{Common, EntityKind, Space, Tianzheng};
use cadcraft_engine::Session;
use serde_json::json;

/// 模拟真实的 `cadcraft_io::set_tianzheng_field`：改 Tianzheng.fields 里的字段值。
fn setter(d: &mut cadcraft_engine::doc::Drawing, h: cadcraft_engine::doc::Handle, field: &str, value: &str) -> bool {
    d.model.modify(h, |e| {
        if let EntityKind::Tianzheng(t) = &mut e.kind {
            for (name, v) in &mut t.fields {
                if name == field {
                    *v = value.to_string();
                }
            }
        }
    })
}

#[test]
fn properties_set_edits_tianzheng_field_through_hook() {
    set_tianzheng_setter(setter);

    let mut s = Session::new();
    let h = {
        let d = s.doc_mut().unwrap();
        d.add(
            &Space::Model,
            Common::default(),
            EntityKind::Tianzheng(Tianzheng {
                class_name: "TCH_WALL".into(),
                fields: vec![("T20s14".to_string(), "砖墙".to_string())],
                raw_data: None,
                handle_bits: 0,
                tags: vec![],
                dxf_type: "ACAD_PROXY_ENTITY".into(),
                segments: vec![],
                graphic_data: None,
                texts: vec![],
            }),
        )
        .unwrap()
    };

    let r = s
        .execute("properties.set", &json!({ "handles": [h.hex()], "tianzhengField": "T20s14", "tianzhengValue": "空心砖墙" }))
        .unwrap();
    assert_eq!(r["changed"], 1);

    if let EntityKind::Tianzheng(t) = &s.doc().unwrap().model.get(h).unwrap().kind {
        assert_eq!(t.fields.iter().find(|(n, _)| n == "T20s14").map(|(_, v)| v.as_str()), Some("空心砖墙"));
    } else {
        panic!("实体不是 Tianzheng");
    }
}
