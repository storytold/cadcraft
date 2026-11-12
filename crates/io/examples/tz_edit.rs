//! 打开 DWG → 改天正字段值 → 验证写回 raw_data（再读能读到新值）。
//! 用法：cargo run -p cadcraft-io --example tz_edit -- <dwg路径>

use std::fs;

fn main() {
    let path = std::env::args().nth(1).expect("用法: tz_edit <dwg路径>");
    let bytes = fs::read(&path).expect("读失败");
    let mut d = cadcraft_io::read(&bytes, &path).expect("打开失败");

    // 找第一个含「填充墙」字段的天正实体
    let target = d
        .model
        .iter()
        .find_map(|e| {
            if let cadcraft_doc::EntityKind::Tianzheng(t) = &e.kind {
                if t.fields.iter().any(|(_, v)| v == "填充墙") {
                    return Some(e.handle);
                }
            }
            None
        })
        .expect("无含填充墙的天正实体");

    // 改字段值并写回 raw_data
    let ok = cadcraft_io::set_tianzheng_field(&mut d, target, "T20s14", "砖墙");
    println!("改字段成功: {ok}");

    // 验证 doc 里的字段已更新
    if let Some(cadcraft_doc::EntityKind::Tianzheng(t)) = d.model.get(target).map(|e| &e.kind) {
        let wall = t.fields.iter().find(|(n, _)| n == "T20s14");
        println!("T20s14 现为: {wall:?}");
        assert_eq!(wall.map(|(_, v)| v.as_str()), Some("砖墙"));
    }

    // 验证 raw_data 已更新（从新 raw_data 再解析能读到「砖墙」）
    if let Some(cadcraft_doc::EntityKind::Tianzheng(t)) = d.model.get(target).map(|e| &e.kind) {
        if let Some(raw) = &t.raw_data {
            let text = cadcraft_dwg::tianzheng::extract_text_stream(raw, t.handle_bits).expect("提取 text 流");
            let strings = cadcraft_dwg::tianzheng::parse_text_strings(&text);
            assert!(strings.iter().any(|s| s == "砖墙"), "砖墙 未写回 raw_data");
            println!("✓ raw_data 已更新，再读可读到「砖墙」");
        }
    }
}
