//! 打开 DWG，验证天正实体被解析成 Tianzheng（含字段名和值）。
//! 用法：cargo run -p cadcraft-io --example tz_open -- <dwg路径>

use std::fs;

fn main() {
    let path = std::env::args().nth(1).expect("用法: tz_open <dwg路径>");
    let bytes = fs::read(&path).expect("读失败");
    let d = cadcraft_io::read(&bytes, &path).expect("打开失败");
    let mut count = 0;
    for e in d.model.iter() {
        let cadcraft_doc::EntityKind::Tianzheng(t) = &e.kind else { continue };
        count += 1;
        println!("== {} (handle {:x}) ==", t.class_name, e.handle.0);
        for (name, value) in &t.fields {
            println!("  {name} = {value:?}");
        }
    }
    println!("\n共 {count} 个天正实体（已从 Unknown 解析为 Tianzheng）");
}
