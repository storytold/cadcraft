//! 探针：提取天正实体 + text 流，搜索 UTF-16 字段名，验证序列化格式。
//! 用法：cargo run -p cadcraft-dwg --example tianzheng_probe -- <dwg路径>

use std::fs;

fn main() {
    let path = std::env::args().nth(1).expect("用法: tianzheng_probe <dwg路径>");
    let bytes = fs::read(&path).expect("读 DWG 失败");
    let entities = cadcraft_dwg::tianzheng::read_tianzheng(&bytes).expect("提取天正实体失败");
    println!("天正实体总数: {}", entities.len());

    for e in entities.iter().take(3) {
        println!("\n== {} (raw {}B, handle_bits {}) ==", e.dxf_class, e.raw_data.len(), e.handle_bits);
        let Some(text) = cadcraft_dwg::tianzheng::extract_text_stream(&e.raw_data, e.handle_bits) else {
            println!("  无 text 流");
            continue;
        };
        println!("  text 流 {}B", text.len());
        // 搜索字节对齐的 UTF-16 字符串
        let mut seg = String::new();
        let mut found = 0;
        for i in (0..text.len().saturating_sub(1)).step_by(2) {
            let ch = text[i];
            let hi = text[i + 1];
            if hi == 0 && (0x20..0x7f).contains(&ch) {
                seg.push(ch as char);
            } else {
                if seg.len() >= 3 {
                    println!("    UTF16@{}: {seg:?}", i.saturating_sub(seg.len() * 2));
                    found += 1;
                }
                seg.clear();
            }
        }
        println!("  共 {found} 个 UTF-16 字符串");
    }
}
