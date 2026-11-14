# 天正实体读写改造 — 最终交付

> 项目目标：把 CADCraft 改造成能读取保存天正（Tangent）自定义实体的通用 CAD，为 AI 识别绘制天正图纸做准备。
> 交付日期：2026-11-12

## 一、完成度总览

| 能力 | 状态 | 说明 |
|---|---|---|
| 识别天正实体 | ✅ | `read_tianzheng` 匹配 `EntityType::Unknown`，实测提取 5 类 25 实体 |
| 提取 text 流 | ✅ | `extract_text_stream`（flag 反向定位） |
| 解析字段容器 | ✅ | `parse_text_strings`（字段名 + 中文字段值 + 加密字段） |
| 解密/加密字段 | ✅ | `decrypt_field`/`encode_field`（XOR 0x05 + Base-26 + 0x18B 偏移） |
| 修改字段值 | ✅ | 中文字段值替换 + round-trip 实测 |
| 写回 merged record | ✅ | `replace_text_stream`（padding 对齐） |
| 保存 DWG | ✅ | `tz_save` example（DwgWriter），8 实体实测 |
| 完整 workspace 编译 | ✅ | 删除 4 个残留 mod.rs 后通过 |
| 数据模型逆向 | ✅ | 固定字段/基类字段/派生类字段/子对象几何 四层理清 |

**核心目标「读取保存天正实体」已完整实现并端到端验证。**

## 二、交付物清单

| 文件 | 作用 |
|---|---|
| `crates/dwg/src/tianzheng.rs` | 天正模块（14.4KB，7 测试通过） |
| `crates/dwg/examples/tianzheng_probe.rs` | 提取探针（识别 + text 流 + 字段名） |
| `crates/dwg/examples/tz_decode.rs` | 解码探针（字段容器 + 加密字段完整解密） |
| `crates/dwg/examples/tz_save.rs` | 端到端保存（读→改→写→存 DWG） |
| `docs/tianzheng-format.md` | 格式逆向文档（11.3KB） |
| `docs/tianzheng-investigation-report.md` | 调查报告快照（8.4KB） |
| `reference/tianzheng_crypt.py` | 解密参考实现（Python，含测试向量） |

## 三、天正实体格式（逆向结论速查）

### 数据分层

```
TEntityBase（所有 TCH_* 实体基类）
├─ +0x08/+0x10: double（固定字段，加密 → text 流）
├─ +0x20/+0x60: 对象列表（固定字段，加密）
├─ +0xa0: 基类字段容器（通用属性）
├─ +0xd0: 子对象（几何：路径点数/高度/对象引用，虚函数 DwgOut）
├─ +0xe8: 派生类字段容器（实体特有属性：材料/等级/尺寸）
└─ +0x110: GUID（产品标识，明文）
```

### 加密算法（固定字段）

```
密钥 0x05（常量 0x528 第二字节）
加密 = 每字节 XOR 0x05 → Base-26 编码（'A'+b/26, 'A'+b%26）
解密 = Base-26 解码 → XOR 0x05
仅 DWG 2004+（版本 ≥16）启用；数值先 %.16f/%d 格式化再加密
```

### 字段容器（派生类 +0xe8）

```
[字段数 int32][逐字段：字段名 UTF-16 明文 + 类型 RC 1字节 + 值]
类型分类：0=bool、2=char、32=string(text流)、164/202=double、其他=default(不写main流)
字段填充 = AFD(字段容器, 字段名字符串, 值)
```

### text/main 双流

- **text 流**：GUID、字段名、字符串字段值（明文）、加密固定字段（乱码）
- **main 流**：字段数、字段类型、数值字段值、子对象几何数据

## 四、使用方法

### Rust API（`cadcraft_dwg::tianzheng`）

```rust
// 1. 识别天正实体
let entities = read_tianzheng(&dwg_bytes)?;
// entities[i].dxf_class = "TCH_WALL"，raw_data = 原始位流

// 2. 提取 text 流 + 解析字段
let text = extract_text_stream(&e.raw_data, e.handle_bits)?;
let strings = parse_text_strings(&text);  // GUID、字段名、中文字段值、加密字段

// 3. 解密加密字段
let plain = decrypt_field(&units);  // code units → 明文字节

// 4. 修改 + 写回 + 保存
let new_text = encode_text_strings(&modified);
let new_raw = replace_text_stream(&e.raw_data, e.handle_bits, &new_text)?;
// 写回 Unknown 实体的 raw_dwg_data，DwgWriter 保存
```

### 命令行示例

```bash
# 提取天正实体
cargo run -p cadcraft-dwg --example tianzheng_probe -- 输入.dwg

# 完整解码（字段容器 + 加密字段 + 中文字段值）
cargo run -p cadcraft-dwg --example tz_decode -- 输入.dwg

# 端到端修改保存（把"填充墙"改成"砖墙"等）
cargo run -p cadcraft-dwg --example tz_save -- 输入.dwg 输出.dwg
```

## 五、关键结论

1. **天正实体在 acadrust 里是 `EntityType::Unknown`（不是 Proxy）**，`dxf_name` 是天正类名，数据在 `raw_dwg_data`。
2. **字段容器明文**：字段名（`T20xx`）和字符串字段值（中文材料/保温层等）直接可读。
3. **固定字段加密**：版本标记/double/对象列表经 XOR+Base26 混淆（非真加密，可完全还原）。
4. **几何在子对象（+0xd0）**：路径点数/高度/对象引用，虚函数序列化。
5. **「读写」是通用的**：所有 `TCH_*` 实体共享 `TEntityBase` 框架，代码不依赖实体类型。

## 六、剩余工作（非阻塞，递减收益）

| 项目 | 说明 |
|---|---|
| 数值字段精确命名 | `T20d7` = 墙高还是墙厚，需逐个逆向值来源偏移（或在实际图纸对照） |
| 各实体字段语义 | TCH_COLUMN/OPENING/DIMENSION2/AXIS_LABEL 的字段名语义（机械重复） |
| 子对象 8 字段语义 | 墙的路径点/高度/对象引用的精确含义 |
| 编辑器 UI 入口 | 属性面板（egui）显示+修改天正字段（读写已通，只差 UI 壳） |

## 七、环境注意事项

- vendor 在 `E:\my soft\cadcraft-vendor`（必须在 CADCraft 目录外，否则嵌套 workspace 报错）
- 项目 `.cargo/config.toml` 有 `[net] git-fetch-with-cli = true`
- git 全局 `insteadOf` 把 github.com 重定向到 SSH over HTTPS
- builder 后端已切 Claude Code（config.toml `Codex enabled=false`）
