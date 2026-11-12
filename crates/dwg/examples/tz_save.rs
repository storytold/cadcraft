//! 端到端保存：读天正 DWG → 修改中文字段值 → 写回新 DWG。
//! 用法：cargo run -p cadcraft-dwg --example tz_save -- <输入dwg> <输出dwg>

use std::fs;
use std::io::Cursor;

use acadrust::entities::EntityType;

fn main() {
    let input = std::env::args().nth(1).expect("用法: tz_save <输入dwg> <输出dwg>");
    let output = std::env::args().nth(2).expect("用法: tz_save <输入dwg> <输出dwg>");
    let bytes = fs::read(&input).expect("读 DWG 失败");

    let outcome = acadrust::DwgReader::from_stream_with_options(Cursor::new(&bytes), acadrust::DwgReadOptions::failsafe())
        .read_with_stats()
        .expect("读 DWG 失败");
    let mut doc = outcome.document;

    let mut changed = 0;
    for e in doc.entities_mut() {
        let EntityType::Unknown(u) = e else { continue };
        if !cadcraft_dwg::tianzheng::is_tianzheng_class(&u.dxf_name) {
            continue;
        }
        let Some(raw) = &u.raw_dwg_data else { continue };
        let Some(text) = cadcraft_dwg::tianzheng::extract_text_stream(raw, u.dwg_handle_bits) else { continue };
        let strings = cadcraft_dwg::tianzheng::parse_text_strings(&text);
        let modified: Vec<String> = strings
            .iter()
            .map(|s| match s.as_str() {
                "填充墙" => "砖墙".to_string(),
                "多孔材料" => "实心材料".to_string(),
                "保温层" => "隔热层".to_string(),
                other => other.to_string(),
            })
            .collect();
        if modified == strings {
            continue;
        }
        let new_text = cadcraft_dwg::tianzheng::encode_text_strings(&modified);
        if let Some(new_raw) = cadcraft_dwg::tianzheng::replace_text_stream(raw, u.dwg_handle_bits, &new_text) {
            u.raw_dwg_data = Some(new_raw);
            changed += 1;
        }
    }

    let new_dwg = acadrust::DwgWriter::write_to_vec(&doc).expect("写 DWG 失败");
    fs::write(&output, &new_dwg).expect("保存失败");
    let n = new_dwg.len();
    println!("✓ 已修改 {changed} 个天正实体，保存到 {output}（{n} 字节）");
}
