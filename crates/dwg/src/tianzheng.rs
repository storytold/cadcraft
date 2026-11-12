//! 天正 T20 自定义实体支持。
//!
//! 天正（Tangent）在 DWG 里用 ObjectARX 自定义实体（proxy entity，DXF 类名 `TCH_*`）
//! 存储建筑 / 结构 / 给排水 / 暖通 / 电气对象。acadrust 把它们解析为 `Proxy` 实体，其中
//! `graphics`（[`acadrust::objects::ProxyPayload`]）保存了天正实体的序列化数据。
//!
//! 序列化格式见 `docs/tianzheng-format.md`。要点：
//! - 天正对实体固定字段做混淆（XOR `0x05` + Base-26），仅 DWG 2004+（保存版本号 ≥ 16）启用。
//! - 命名字段容器（`TEntityFields`）是明文。
//!
//! 本模块提供：加解密、类名识别、以及从 DWG 提取天正实体的入口。

/// 天正加密密钥（硬编码常量 `0x528` 的第二字节，与保存版本无关）。
const KEY: u8 = 0x05;

/// 天正加密：每个字节 `b ^ 0x05` 后，编码为 `('A' + b/26)('A' + b%26)` 两个大写字母。
///
/// 只取每个字符的低字节（Base-26 只编码 0–255），与天正对数值/布尔先转 ASCII 字符串再加密
/// 的做法一致。
pub fn encrypt(plaintext: &str) -> String {
    let mut out = String::with_capacity(plaintext.len() * 2);
    for ch in plaintext.chars() {
        let b = (ch as u32 & 0xFF) as u8 ^ KEY;
        out.push(char::from(b'A' + b / 26));
        out.push(char::from(b'A' + b % 26));
    }
    out
}

/// 天正解密：每两个字母 `(c0-'A')*26 + (c1-'A')`，再 `^ 0x05`。
pub fn decrypt(ciphertext: &str) -> Result<String, String> {
    let bytes = ciphertext.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(format!("天正密文长度必须是偶数，实际 {}", bytes.len()));
    }
    let mut out = String::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let (c0, c1) = (pair[0], pair[1]);
        if !(b'A'..=b'Z').contains(&c0) || !(b'A'..=b'Z').contains(&c1) {
            return Err(format!("非法 Base-26 字符: {:?}", String::from_utf8_lossy(pair)));
        }
        let b = (c0 - b'A') * 26 + (c1 - b'A');
        out.push(char::from(b ^ KEY));
    }
    Ok(out)
}

/// 是否为天正实体的 DXF 类名（proxy 的 `dxf_subclass`）。
pub fn is_tianzheng_class(dxf_class: &str) -> bool {
    dxf_class.starts_with("TCH_") || dxf_class.starts_with("_TCH_") || dxf_class == "TCH_KERNAL"
}

/// 从一个天正 DWG 提取的实体（native 构建，经 acadrust 读取）。
#[cfg(not(no_dwg_bridge))]
mod native {
    use std::io::Cursor;

    use acadrust::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
    use acadrust::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
    use acadrust::io::dwg::DwgVersion;
    use acadrust::types::DxfVersion;

    /// 一个从天正 DWG 提取的实体。
    #[derive(Debug, Clone, PartialEq)]
    pub struct TianzhengEntity {
        /// 天正 DXF 类名，如 `TCH_WALL`。
        pub dxf_class: String,
        /// 实体句柄（用于与文档模型里的实体对应）。
        pub handle: u64,
        /// 原始 merged-stream record 数据（`raw_dwg_data`，含天正序列化的位流）。
        pub raw_data: Vec<u8>,
        /// proxy 的 graphic_data（通常为 8 字节头）。
        pub graphic_data: Vec<u8>,
        /// handle 流的 bit 数（text 流定位需要）。
        pub handle_bits: i64,
    }

    /// 从 DWG 字节提取所有天正实体（含 failsafe 读取，坏记录跳过而非整文件失败）。
    ///
    /// 天正自定义实体（`TCH_*`）在 acadrust 里被解析为 `EntityType::Unknown`（不是 Proxy），
    /// 其完整数据在 `raw_dwg_data`（原始 merged-stream record 位流）里。
    pub fn read_tianzheng(bytes: &[u8]) -> Result<Vec<TianzhengEntity>, String> {
        let outcome = acadrust::DwgReader::from_stream_with_options(
            Cursor::new(bytes),
            acadrust::DwgReadOptions::failsafe(),
        )
        .read_with_stats()
        .map_err(|e| format!("DWG: {e}"))?;
        let mut entities = Vec::new();
        for e in outcome.document.entities() {
            let acadrust::entities::EntityType::Unknown(u) = e else { continue };
            if !super::is_tianzheng_class(&u.dxf_name) {
                continue;
            }
            entities.push(TianzhengEntity {
                dxf_class: u.dxf_name.clone(),
                handle: u.common.handle.value(),
                raw_data: u.raw_dwg_data.clone().unwrap_or_default(),
                graphic_data: u.common.graphic_data.clone().unwrap_or_default(),
                handle_bits: u.dwg_handle_bits,
            });
        }
        Ok(entities)
    }

    /// 从天正实体的 `raw_data` 提取 text 流（含字段容器明文 + 加密字段）。
    ///
    /// text 流在 DWG merged record 的布局 `[main][text][modular_short][flag][handles]` 中，
    /// 由末尾的 flag bit 反向定位（`flag` 在 total_bits-1，`str_size` 在 flag 前 2 字节）。
    /// 返回 text 流的原始字节（bit 打包，MSB-first）。
    pub fn extract_text_stream(raw_data: &[u8], handle_bits: i64) -> Option<Vec<u8>> {
        let total_bits = (raw_data.len() as i64) * 8 - handle_bits;
        let flag_pos = total_bits - 1;
        let mut br = DwgBitReader::new(raw_data.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        br.set_position_in_bits(flag_pos);
        if !br.read_bit() {
            return None;
        }
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
        let start_pos = length - str_size;
        if start_pos < 0 || str_size <= 0 {
            return None;
        }
        br.set_position_in_bits(start_pos);
        let byte_len = (str_size as usize).div_ceil(8);
        let mut out = Vec::with_capacity(byte_len);
        for _ in 0..byte_len {
            out.push(br.read_byte());
        }
        Some(out)
    }

    /// 从 text 流读出所有字符串（`read_variable_text`：bit 长度前缀 + UTF-16LE）。
    ///
    /// text 流是 bit 流，字符串用 `read_variable_text` 读。返回的字符串按出现顺序排列：
    /// 开头是明文产品 GUID，随后是加密字段（乱码），再后是明文字段名和字段值。
    pub fn parse_text_strings(text: &[u8]) -> Vec<String> {
        let mut br = DwgBitReader::new(text.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        let total_bits = (text.len() as i64) * 8;
        let mut result = Vec::new();
        while br.position_in_bits() < total_bits {
            let before = br.position_in_bits();
            let s = br.read_variable_text();
            if !s.is_empty() {
                result.push(s);
            }
            if br.position_in_bits() <= before {
                break; // 防止死循环
            }
        }
        result
    }

    /// 从天正实体的 text 流解析出字段名 → 值对。
    ///
    /// 字段名以 `T20` 开头；字符串字段值（如中文材料）紧跟在字段名后；数值字段的值在
    /// main 流（此处记空字符串）。返回 `(字段名, 值)` 列表，按出现顺序。
    pub fn parse_tianzheng_fields(text: &[u8]) -> Vec<(String, String)> {
        let strings = parse_text_strings(text);
        let mut fields = Vec::new();
        let mut i = 0;
        while i < strings.len() {
            let s = &strings[i];
            if s.starts_with("T20") {
                let name = s.clone();
                // 下一个字符串若是中文/普通值（非字段名、非边界标记、非 GUID），则是该字段的字符串值
                let value = if i + 1 < strings.len() {
                    let next = &strings[i + 1];
                    if !next.starts_with("T20") && !next.starts_with("TDBX") && !next.starts_with('{') {
                        let v = next.clone();
                        i += 1;
                        v
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                fields.push((name, value));
            }
            i += 1;
        }
        fields
    }

    /// 解密 text 流里的加密字段（UTF-16 code units → 明文字节）。
    ///
    /// 加密字段的 code units 是 Base-26 字母（'A'-'Z'）加了 0x18B 偏移（天正自定义编码，
    /// 防止明文可见）。解密：`(unit - 0x18B)` 得 Base-26 值（0-25）→ Base-26 解码 `c0*26+c1` → XOR 0x05。
    pub fn decrypt_field(code_units: &[u16]) -> Vec<u8> {
        const OFFSET: u16 = 0x18B;
        let mut out = Vec::new();
        for pair in code_units.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            let c0 = pair[0].wrapping_sub(OFFSET);
            let c1 = pair[1].wrapping_sub(OFFSET);
            if c0 >= 26 || c1 >= 26 {
                continue;
            }
            let b = (c0 * 26 + c1) ^ 0x05;
            out.push(b as u8);
        }
        out
    }

    /// 加密字段（明文字节 → UTF-16 code units），[`decrypt_field`] 的反向。
    ///
    /// 每个明文字节：XOR 0x05 → Base-26 编码（两个 0-25 值）→ 加 0x18B 偏移。
    pub fn encode_field(plain: &[u8]) -> Vec<u16> {
        const OFFSET: u16 = 0x18B;
        let mut out = Vec::with_capacity(plain.len() * 2);
        for &b in plain {
            let x = b ^ 0x05;
            out.push(OFFSET + (x / 26) as u16);
            out.push(OFFSET + (x % 26) as u16);
        }
        out
    }

    /// 把字符串列表编码回 text 流 bit 流（[`parse_text_strings`] 的逆操作）。
    ///
    /// 明文字符串用 `write_variable_text` 写；加密字段的乱码字符串（UTF-16 code units 在
    /// 0x18B-0x1A4 范围）也由 `write_variable_text` 按 UTF-16 原样写回。
    pub fn encode_text_strings(strings: &[String]) -> Vec<u8> {
        let mut w = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1032);
        for s in strings {
            w.write_variable_text(s);
        }
        w.into_bytes()
    }

    /// 把新 text 流替换回 `raw_dwg_data` 的 merged record，返回新的 raw_dwg_data。
    ///
    /// merged record 布局 `[main][text][modular_short][flag][handles]`，本函数保留 main 和
    /// handles，只替换中间的 text 流并更新 str_size。调用者通常用
    /// [`encode_text_strings`] 生成新 text 流。
    pub fn replace_text_stream(raw_data: &[u8], handle_bits: i64, new_text: &[u8]) -> Option<Vec<u8>> {
        // 定位 text 流（同 extract_text_stream）
        let total_bits = (raw_data.len() as i64) * 8 - handle_bits;
        let flag_pos = total_bits - 1;
        let mut br = DwgBitReader::new(raw_data.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        br.set_position_in_bits(flag_pos);
        if !br.read_bit() {
            return None;
        }
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

        // 读 main bit（[0, text_start)）
        let mut br = DwgBitReader::new(raw_data.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        let main: Vec<bool> = (0..text_start).map(|_| br.read_bit()).collect();
        // 读 handles bit（[total_bits, total_bits + handle_bits)）
        let mut br = DwgBitReader::new(raw_data.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        br.set_position_in_bits(total_bits);
        let handles: Vec<bool> = (0..handle_bits).map(|_| br.read_bit()).collect();

        // 重建：main + new_text + padding + modular_short(str_size) + flag + handles
        // padding 让 total_bits + handle_bits 恰好 8 的倍数，避免末尾 padding 使 flag 定位偏移。
        let new_size = (new_text.len() as i64) * 8;
        let total_no_handles = text_start + new_size + 17; // main + text + modular_short + flag
        let padding = (8 - (total_no_handles + handle_bits).rem_euclid(8)).rem_euclid(8);
        let str_size = new_size + padding;
        let mut w = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1032);
        for b in &main {
            w.write_bit(*b);
        }
        let mut tr = DwgBitReader::new(new_text.to_vec(), DwgVersion::AC24, DxfVersion::AC1032);
        for _ in 0..new_size {
            w.write_bit(tr.read_bit());
        }
        for _ in 0..padding {
            w.write_bit(false);
        }
        w.write_raw_ushort(str_size as u16);
        w.write_bit(true);
        for b in &handles {
            w.write_bit(*b);
        }
        Some(w.into_bytes())
    }

    /// 修改天正实体 text 流里的一个字段值，返回新的 raw_data。
    ///
    /// 从 raw_data 提取 text 流 → 解析字符串列表 → 改字段值 → 重新编码 → 替换回 raw_data。
    /// 字段不存在或写回失败时返回 None。
    pub fn update_tianzheng_field(raw_data: &[u8], handle_bits: i64, field_name: &str, new_value: &str) -> Option<Vec<u8>> {
        let text = extract_text_stream(raw_data, handle_bits)?;
        let strings = parse_text_strings(&text);
        let mut new_strings = strings;
        let mut changed = false;
        for i in 0..new_strings.len() {
            if new_strings[i] == field_name
                && i + 1 < new_strings.len()
                && !new_strings[i + 1].starts_with("T20")
                && !new_strings[i + 1].starts_with("TDBX")
            {
                new_strings[i + 1] = new_value.to_string();
                changed = true;
            }
        }
        if !changed {
            return None;
        }
        let new_text = encode_text_strings(&new_strings);
        replace_text_stream(raw_data, handle_bits, &new_text)
    }
}

#[cfg(not(no_dwg_bridge))]
pub use native::{decrypt_field, encode_field, encode_text_strings, extract_text_stream, parse_text_strings, parse_tianzheng_fields, read_tianzheng, replace_text_stream, TianzhengEntity};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for t in ["0", "1", "-1", "3.14159", "100.000000", "wall", "WALL", "pipe-123"] {
            assert_eq!(decrypt(&encrypt(t)).unwrap(), t, "round-trip {t:?}");
        }
    }

    /// 与 `reference/tianzheng_crypt.py` 自测一致的向量。
    #[test]
    fn known_vectors() {
        assert_eq!(encrypt("0"), "CB");
        assert_eq!(encrypt("1"), "CA");
        assert_eq!(encrypt("-1"), "BOCA");
        assert_eq!(encrypt("3.14159"), "CCBRCABXCABWCI");
        assert_eq!(decrypt("CB").unwrap(), "0");
        assert_eq!(decrypt("CA").unwrap(), "1");
        assert_eq!(decrypt("BOCA").unwrap(), "-1");
    }

    #[test]
    fn rejects_odd_length_and_non_letters() {
        assert!(decrypt("C").is_err());
        assert!(decrypt("ab").is_err());
    }

    #[test]
    fn class_detection() {
        assert!(is_tianzheng_class("TCH_WALL"));
        assert!(is_tianzheng_class("TCH_PIPE"));
        assert!(is_tianzheng_class("TCH_KERNAL"));
        assert!(!is_tianzheng_class("LINE"));
        assert!(!is_tianzheng_class("ACAD_PROXY_ENTITY"));
    }

    /// 加密字段 code units（Base-26 字母 + 0x18B 偏移）解密。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn decrypts_offset_field() {
        // 实测 [2] 字段：0x18c 0x1a3 重复 → Base-26 值 (1,24) = 50 → 50^0x05 = 55 = '7'
        assert_eq!(decrypt_field(&[0x18c, 0x1a3]), vec![b'7']);
        let units22: Vec<u16> = [0x18c, 0x1a3].into_iter().cycle().take(44).collect();
        assert_eq!(decrypt_field(&units22), vec![b'7'; 22]);
    }

    /// encode_field 与 decrypt_field 互为逆操作（round-trip）。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn field_round_trip() {
        for plain in [vec![b'7'; 22], b"123".to_vec(), b"341".to_vec(), vec![0u8, 255u8, 128u8]] {
            let units = encode_field(&plain);
            assert_eq!(decrypt_field(&units), plain);
        }
    }

    /// parse_text_strings 与 encode_text_strings 互为逆操作。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn text_strings_round_trip() {
        let strings = vec![
            "{61309675-BD2E-44BE-B870-B80B03BB3A76}".to_string(),
            "TDBXDATAMAP_BLOCK_BEGIN_".to_string(),
            "T20b2".to_string(),
            "填充墙".to_string(),
            "TDBXDATAMAP_BLOCK_END_".to_string(),
        ];
        let encoded = encode_text_strings(&strings);
        let decoded = parse_text_strings(&encoded);
        assert_eq!(decoded, strings);
    }
}
