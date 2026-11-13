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

    /// 从天正 proxy graphics 提取的一个文字（类型 38 图元）。
    #[derive(Debug, Clone, PartialEq)]
    pub struct TianzhengText {
        /// 插入点 x（建筑坐标 mm）。
        pub x: f64,
        /// 插入点 y（建筑坐标 mm）。
        pub y: f64,
        /// 文字内容。
        pub text: String,
        /// 文字高度（mm）。
        pub height: f64,
        /// 宽度因子。
        pub width_factor: f64,
    }

    /// 从 DWG 字节提取所有天正实体（含 failsafe 读取，坏记录跳过而非整文件失败）。
    ///
    /// 天正自定义实体（`TCH_*`）在 acadrust 里可能是两种形态，取决于文件是否经过
    /// CADCraft 的保存往返：
    /// - 首次打开原始 DWG：acadrust 不认识 `TCH_*` 的 type_code，解析为
    ///   `EntityType::Unknown`，完整 merged 流在 `raw_dwg_data`。
    /// - 保存往返后（`dxf_bridge` 把天正当 `ACAD_PROXY_ENTITY` 写出，再编码回 DWG）：
    ///   解析为 `EntityType::Extended(Proxy)`，完整 merged 流在 `common.raw_record`。
    ///
    /// 两种形态的完整 merged-stream record 位流布局一致（`[main][text][modular_short]
    /// [flag][handles]`），字段提取逻辑（[`extract_text_stream`]）对二者都成立。
    pub fn read_tianzheng(bytes: &[u8]) -> Result<Vec<TianzhengEntity>, String> {
        let outcome = acadrust::DwgReader::from_stream_with_options(
            Cursor::new(bytes),
            acadrust::DwgReadOptions::failsafe(),
        )
        .read_with_stats()
        .map_err(|e| format!("DWG: {e}"))?;
        let mut entities = Vec::new();
        for e in outcome.document.entities() {
            let (dxf_class, handle, raw_data, graphic_data, handle_bits) = match e {
                acadrust::entities::EntityType::Unknown(u) => {
                    if !super::is_tianzheng_class(&u.dxf_name) {
                        continue;
                    }
                    (
                        u.dxf_name.clone(),
                        u.common.handle.value(),
                        u.raw_dwg_data.clone().unwrap_or_default(),
                        u.common.graphic_data.clone().unwrap_or_default(),
                        u.dwg_handle_bits,
                    )
                }
                acadrust::entities::EntityType::Extended(x) => {
                    let acadrust::entities::ExtendedEntityData::Proxy(p) = &x.data else { continue };
                    if !super::is_tianzheng_class(&p.dxf_subclass) {
                        continue;
                    }
                    let raw = x.common.raw_record.as_deref();
                    (
                        p.dxf_subclass.clone(),
                        x.common.handle.value(),
                        raw.map(|r| r.data.clone()).unwrap_or_default(),
                        x.common.graphic_data.clone().unwrap_or_default(),
                        raw.map(|r| r.handle_bits).unwrap_or(0),
                    )
                }
                _ => continue,
            };
            entities.push(TianzhengEntity {
                dxf_class,
                handle,
                raw_data,
                graphic_data,
                handle_bits,
            });
        }
        Ok(entities)
    }

    /// 从 proxy graphics 提取墙的几何线段（类型 6 图元）。
    ///
    /// 真实生产图纸（XML 自描述）的墙几何在 proxy graphics 的类型 6 图元里，每个图元是墙的
    /// 一段：payload 52 字节 = u32 顶点数(2) + 起点 (X1,Y1,Z1) + 终点 (X2,Y2,Z2)，6 个
    /// double（LE）。返回 `(x1,y1,z1,x2,y2,z2)` 列表，按图元出现顺序（墙 = 多段线）。
    /// binary 格式（TIANZHENTEST）的 graphic_data 仅 8 字节，几何在 text 流加密固定字段，本函数返回空。
    pub fn extract_wall_segments(graphic_data: &[u8]) -> Vec<(f64, f64, f64, f64, f64, f64)> {
        let Some(g) = acadrust::entities::ProxyGraphics::decode(graphic_data) else { return Vec::new(); };
        let mut segs = Vec::new();
        for r in &g.records {
            if let acadrust::entities::ProxyGraphicRecord::Unknown { record_type: 6, data } = r {
                if data.len() >= 52 {
                    let d = |off: usize| f64::from_le_bytes(data[off..off + 8].try_into().unwrap());
                    segs.push((d(4), d(12), d(20), d(28), d(36), d(44)));
                }
            }
        }
        segs
    }

    /// 把墙的几何线段反向编码回 proxy graphics 的类型 6 图元（[`extract_wall_segments`] 的逆操作）。
    ///
    /// 每个 segment 生成一个 `Unknown { record_type: 6, data }` 图元，payload 52 字节 =
    /// u32 顶点数(2) + 起点 (X1,Y1,Z1) + 终点 (X2,Y2,Z2)，6 个 double（LE），与天正写入格式一致。
    /// 替换原有 graphics 里**所有**类型 6 图元，其余图元原样保留；`segments` 数量必须等于
    /// 类型 6 图元数，否则返回 `None`（拒绝错位替换）。graphics 里无类型 6 图元（几何不在
    /// graphics 里，如 binary 格式）时也返回 `None`。
    pub fn encode_wall_segments(graphic_data: &[u8], segments: &[(f64, f64, f64, f64, f64, f64)]) -> Option<Vec<u8>> {
        let mut g = acadrust::entities::ProxyGraphics::decode(graphic_data)?;
        let mut i = 0;
        for r in &mut g.records {
            if let acadrust::entities::ProxyGraphicRecord::Unknown { record_type: 6, data } = r {
                let seg = *segments.get(i)?;
                let mut payload = Vec::with_capacity(52);
                payload.extend_from_slice(&2u32.to_le_bytes());
                for v in [seg.0, seg.1, seg.2, seg.3, seg.4, seg.5] {
                    payload.extend_from_slice(&v.to_le_bytes());
                }
                *data = payload;
                i += 1;
            }
        }
        (i == segments.len()).then(|| g.encode()).flatten()
    }

    /// 从 proxy graphics 提取天正文字（类型 38 图元）。
    ///
    /// 类型 38 图元的 payload 布局（实测 TCH_TEXT/TCH_DIMENSION）：
    /// `x(f64) y(f64)` 是插入点（建筑坐标 mm），偏移 72 起是 UTF-16LE null 结尾的文字内容，
    /// 其后是 `char_count(u64)`，按 8 字节对齐后是 `height(f64)` 与 `width_factor(f64)`。
    /// 字体名（SIMPLEX/GBCBIG）在更后面，本函数不解析。
    pub fn extract_text_entities(graphic_data: &[u8]) -> Vec<TianzhengText> {
        let Some(g) = acadrust::entities::ProxyGraphics::decode(graphic_data) else { return Vec::new(); };
        let mut out = Vec::new();
        for r in &g.records {
            if let acadrust::entities::ProxyGraphicRecord::Unknown { record_type: 38, data } = r {
                if data.len() < 88 {
                    continue;
                }
                let x = f64::from_le_bytes(data[0..8].try_into().unwrap());
                let y = f64::from_le_bytes(data[8..16].try_into().unwrap());
                // 文字内容：偏移 72 起，UTF-16LE null 结尾。
                let mut text = String::new();
                let mut i = 72;
                while i + 1 < data.len() {
                    let u = u16::from_le_bytes([data[i], data[i + 1]]);
                    i += 2;
                    if u == 0 {
                        break;
                    }
                    if let Some(c) = char::from_u32(u as u32) {
                        text.push(c);
                    }
                }
                // char_count(u32) 对齐到 4 字节、读后对齐到 8 字节，然后向后扫描第一个
                // 「合理高度」的 f64（建筑文字高度 ~50..5000mm）。上标/下标文字在 char_count
                // 后比主文字多一个字段，固定偏移会读错，故用范围扫描。
                let mut pos = (i + 3) & !3;
                pos = (pos + 4 + 7) & !7;
                let mut height = 0.0;
                let mut width_factor = 1.0;
                while pos + 16 <= data.len() && pos < i + 64 {
                    let v = f64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                    if (50.0..5000.0).contains(&v) {
                        height = v;
                        width_factor = f64::from_le_bytes(data[pos + 8..pos + 16].try_into().unwrap());
                        break;
                    }
                    pos += 8;
                }
                out.push(TianzhengText { x, y, text, height, width_factor });
            }
        }
        out
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
    /// 天正有两种 text 流格式：
    /// - **T20 二进制**（老版/测试文件）：字段名以 `T20` 开头，字符串值紧跟其后，数值值在 main 流。
    /// - **XML 自描述**（真实生产图纸，如 `_t5.dwg`）：`<Tag>value</Tag>`，字段名是语义化标签
    ///   （`<LintelHeight>50</LintelHeight>`），根标签不是字段。
    ///
    /// 返回 `(字段名, 值)` 列表，按出现顺序。
    pub fn parse_tianzheng_fields(text: &[u8]) -> Vec<(String, String)> {
        let strings = parse_text_strings(text);
        // XML 自描述格式：任一字符串含 `</`（结束标签）。T20 的字段名/中文值/加密 Base-26
        // 乱码/GUID 都不含 `<`，不会误判。
        if strings.iter().any(|s| s.contains("</")) {
            return parse_xml_fields(&strings);
        }
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

    /// 解析 XML 自描述格式的字段：`<Tag>value</Tag>`。
    ///
    /// 根标签（值含嵌套 `<Tag>`）不是字段，内部扁平标签是字段；空标签/空值跳过。
    fn parse_xml_fields(strings: &[String]) -> Vec<(String, String)> {
        let mut fields = Vec::new();
        for s in strings {
            let mut rest = s.as_str();
            while let Some(lt) = rest.find('<') {
                let after_lt = &rest[lt + 1..];
                // 结束标签 </Tag>：跳过
                if let Some(r) = after_lt.strip_prefix('/') {
                    let Some(gt) = r.find('>') else { break };
                    rest = &r[gt + 1..];
                    continue;
                }
                // 开始标签 <Tag>；带属性的标签（含空格）不做字段名，直接向内。
                let Some(gt) = after_lt.find('>') else { break };
                let tag_name = after_lt[..gt].trim();
                let content = &after_lt[gt + 1..];
                let close_tag = format!("</{}>", tag_name);
                let Some(close_pos) = content.find(&close_tag) else { break };
                let value = &content[..close_pos];
                if !tag_name.is_empty() && !tag_name.contains(' ') && !value.contains('<') && !value.trim().is_empty() {
                    // 叶子字段：值为纯文本
                    fields.push((tag_name.to_string(), value.trim().to_string()));
                    rest = &content[close_pos + close_tag.len()..];
                } else {
                    // 根标签（值含嵌套）或空标签：前进到 `>` 之后，继续向内扫描
                    rest = content;
                }
            }
        }
        fields
    }

    /// 修改 text 流字符串列表里的一个字段值，自动识别 T20 二进制与 XML 自描述两种格式。
    ///
    /// - T20：字段名是独立字符串，值紧跟其后。
    /// - XML：字段是 `<field_name>value</field_name>`，嵌在 XML 字符串里。
    ///
    /// 返回修改后的字符串列表；字段不存在或不是字符串值字段时返回 `None`。
    pub fn update_tianzheng_field_strings(strings: &[String], field_name: &str, new_value: &str) -> Option<Vec<String>> {
        if strings.iter().any(|s| s.contains("</")) {
            return update_xml_field(strings, field_name, new_value);
        }
        let mut new_strings = strings.to_vec();
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
        changed.then_some(new_strings)
    }

    /// 修改 XML 自描述 text 流里的一个字段值（`<field_name>value</field_name>` → 新值）。
    fn update_xml_field(strings: &[String], field_name: &str, new_value: &str) -> Option<Vec<String>> {
        let open = format!("<{field_name}>");
        let close = format!("</{field_name}>");
        let mut new_strings = strings.to_vec();
        let mut changed = false;
        for s in new_strings.iter_mut() {
            if !s.contains("</") {
                continue;
            }
            let Some(start) = s.find(&open) else { continue };
            let value_start = start + open.len();
            let Some(end) = s[value_start..].find(&close) else { continue };
            s.replace_range(value_start..value_start + end, new_value);
            changed = true;
        }
        changed.then_some(new_strings)
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
        encode_text_strings_with_bits(strings).0
    }

    /// [`encode_text_strings`]，另返回精确的字符串 bit 数（不含末尾 byte 对齐 padding）。
    ///
    /// text 流在 DWG 里按 `str_size`（bit）记录，末尾可能 padding 到 byte 边界；而 proxy
    /// DXF 的 310 里 `text_payload.bit_count`（group 162）记录的是**精确**的字符串 bit 数。
    /// 这个 bit 数必须在 `into_bytes` 之前取（[`DwgBitWriter::position_in_bits`]）。
    pub fn encode_text_strings_with_bits(strings: &[String]) -> (Vec<u8>, i64) {
        let mut w = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1032);
        for s in strings {
            w.write_variable_text(s);
        }
        let bits = w.position_in_bits();
        (w.into_bytes(), bits)
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
pub use native::{decrypt_field, encode_field, encode_text_strings, encode_text_strings_with_bits, encode_wall_segments, extract_text_entities, extract_text_stream, extract_wall_segments, parse_text_strings, parse_tianzheng_fields, read_tianzheng, replace_text_stream, update_tianzheng_field_strings, TianzhengEntity, TianzhengText};

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

    /// XML 自描述格式（真实生产图纸 `_t5.dwg`）的字段解析。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn parses_xml_fields() {
        let strings = vec![
            "<SwrOpening>\r\n<StoreyFlag>0</StoreyFlag>\r\n<LintelHeight>50</LintelHeight>\r\n<LintelExt>50</LintelExt>\r\n</SwrOpening>".to_string(),
            "FM乙1122".to_string(),
        ];
        let encoded = encode_text_strings(&strings);
        let fields = parse_tianzheng_fields(&encoded);
        assert_eq!(
            fields,
            vec![
                ("StoreyFlag".to_string(), "0".to_string()),
                ("LintelHeight".to_string(), "50".to_string()),
                ("LintelExt".to_string(), "50".to_string()),
            ]
        );
    }

    /// T20 二进制格式仍照旧解析（不误判为 XML）。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn t20_fields_still_parse() {
        let strings = vec![
            "{61309675-BD2E-44BE-B870-B80B03BB3A76}".to_string(),
            "T20s14".to_string(),
            "砖墙".to_string(),
            "T20s9".to_string(),
            "隔热层".to_string(),
        ];
        let encoded = encode_text_strings(&strings);
        let fields = parse_tianzheng_fields(&encoded);
        assert_eq!(fields, vec![("T20s14".to_string(), "砖墙".to_string()), ("T20s9".to_string(), "隔热层".to_string())]);
    }

    /// extract_wall_segments 与 encode_wall_segments 互为逆操作（类型 6 图元 round-trip）。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn wall_segments_round_trip() {
        use acadrust::entities::{ProxyGraphicRecord, ProxyGraphics};
        let seg: (f64, f64, f64, f64, f64, f64) = (1.5, 2.5, 3.5, 4.5, 5.5, 6.5);
        let mut payload = Vec::with_capacity(52);
        payload.extend_from_slice(&2u32.to_le_bytes());
        for v in [seg.0, seg.1, seg.2, seg.3, seg.4, seg.5] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        let g = ProxyGraphics { records: vec![ProxyGraphicRecord::Unknown { record_type: 6, data: payload }] };
        let graphic_data = g.encode().unwrap();

        assert_eq!(extract_wall_segments(&graphic_data), vec![seg]);

        let seg2: (f64, f64, f64, f64, f64, f64) = (10.0, 20.0, 30.0, 40.0, 50.0, 60.0);
        let new_g = encode_wall_segments(&graphic_data, &[seg2]).unwrap();
        assert_eq!(extract_wall_segments(&new_g), vec![seg2]);
    }

    /// 段数不匹配时 encode_wall_segments 拒绝（返回 None），不会错位替换。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn wall_segments_rejects_mismatch() {
        use acadrust::entities::{ProxyGraphicRecord, ProxyGraphics};
        let seg: (f64, f64, f64, f64, f64, f64) = (1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        let mut payload = Vec::with_capacity(52);
        payload.extend_from_slice(&2u32.to_le_bytes());
        for v in [seg.0, seg.1, seg.2, seg.3, seg.4, seg.5] {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        let g = ProxyGraphics { records: vec![ProxyGraphicRecord::Unknown { record_type: 6, data: payload }] };
        let graphic_data = g.encode().unwrap();

        assert!(encode_wall_segments(&graphic_data, &[]).is_none());
        assert!(encode_wall_segments(&graphic_data, &[seg, seg]).is_none());
    }

    /// extract_text_entities 从类型 38 图元提取文字（位置/内容/高度/宽度因子）。
    #[cfg(not(no_dwg_bridge))]
    #[test]
    fn extracts_text_from_type38() {
        use acadrust::entities::{ProxyGraphicRecord, ProxyGraphics};
        // 类型 38 图元 payload：9×f64（72B，前两个是插入点）→ UTF-16LE 文字 + null →
        // char_count(u32，对齐到 4) → 对齐到 8 → height → width_factor。
        let mut data = Vec::new();
        for v in [100.0_f64, 200.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        for c in "FM".encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        data.extend_from_slice(&0u16.to_le_bytes());
        while data.len() % 4 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&2u32.to_le_bytes());
        while data.len() % 8 != 0 {
            data.push(0);
        }
        data.extend_from_slice(&297.5_f64.to_le_bytes());
        data.extend_from_slice(&0.7058823529411765_f64.to_le_bytes());
        let g = ProxyGraphics { records: vec![ProxyGraphicRecord::Unknown { record_type: 38, data }] };
        let graphic_data = g.encode().unwrap();

        let texts = extract_text_entities(&graphic_data);
        assert_eq!(texts.len(), 1);
        let t = &texts[0];
        assert_eq!((t.x, t.y), (100.0, 200.0));
        assert_eq!(t.text, "FM");
        assert_eq!(t.height, 297.5);
        assert!((t.width_factor - 0.7058823529411765).abs() < 1e-12);
    }
}
