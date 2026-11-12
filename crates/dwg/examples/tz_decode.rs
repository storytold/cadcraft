//! 解码 main 流：字段数 + 逐字段（类型 RC + 值）。
//! 用法：cargo run -p cadcraft-dwg --example tz_decode -- <dwg路径>

use std::fs;

use acadrust::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
use acadrust::io::dwg::DwgVersion;
use acadrust::types::DxfVersion;

fn main() {
    let path = std::env::args().nth(1).expect("用法");
    let bytes = fs::read(&path).expect("读失败");
    let entities = cadcraft_dwg::tianzheng::read_tianzheng(&bytes).expect("提取失败");
    let e = &entities[0];

    let total_bits = (e.raw_data.len() as i64) * 8 - e.handle_bits;
    let flag_pos = total_bits - 1;
    let mut br = DwgBitReader::new(e.raw_data.clone(), DwgVersion::AC24, DxfVersion::AC1032);
    br.set_position_in_bits(flag_pos);
    let _flag = br.read_bit();
    let mut length = flag_pos - 16;
    br.set_position_in_bits(length);
    let mut str_size = br.read_raw_ushort() as i64;
    if str_size & 0x8000 != 0 {
        length -= 16;
        br.set_position_in_bits(length);
        str_size &= 0x7fff;
        let hi = br.read_raw_ushort() as i64;
        str_size += (hi & 0xffff) << 15;
    }
    let text_start = length - str_size;

    let mut br = DwgBitReader::new(e.raw_data.clone(), DwgVersion::AC24, DxfVersion::AC1032);
    let field_count = br.read_bit_long();
    println!("字段数 = {field_count}，main 流 [0, {text_start})");

    // 先读所有字段类型（RC 1字节）
    let mut types = Vec::new();
    for i in 0..field_count {
        let pos = br.position_in_bits();
        let t = br.read_byte();
        types.push((i, pos, t));
    }
    println!("字段类型读完，pos = {}bit", br.position_in_bits());
    for (i, pos, t) in &types {
        println!("  字段{i:2} @{pos}: 类型={t}");
    }

    // 尝试读字段值：根据类型（bool=1bit、byte=RC、double=BD、string=text流）
    println!("\n字段值（从 {}bit 开始）:", br.position_in_bits());
    for (i, _, t) in &types {
        let pos = br.position_in_bits();
        if pos >= text_start {
            break;
        }
        match t {
            32 => println!("  字段{i} 类型32(string): 值在 text 流"), // string 跳过
            0 => {
                let v = br.read_bit();
                println!("  字段{i} 类型0(bool): {v}");
            }
            2 => {
                let v = br.read_byte();
                println!("  字段{i} 类型2(byte): {v}");
            }
            10 | 164 | 202 => {
                let v = br.read_bit_double();
                println!("  字段{i} 类型{t}(double): {v}");
            }
            _ => {
                // 未知类型，读一个 byte 试探
                let v = br.read_byte();
                println!("  字段{i} 类型{t}: byte={v}");
            }
        }
    }
    println!("字段值读完，pos = {}bit / text_start = {text_start}bit", br.position_in_bits());
}
