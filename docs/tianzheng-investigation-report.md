# 天正图解析改造 — 调查报告快照

> 生成时间：2026-11-12。本文件是调查成果的持久化快照，供重启后完整恢复上下文。
> 详细格式见同目录 `tianzheng-format.md`；代码见 `crates/dwg/src/tianzheng.rs` 和 `reference/tianzheng_crypt.py`。

## 一、调查目标

把 CADCraft（Rust CAD）改造成能**读取保存天正自定义实体**的通用 CAD，为后续 AI 识别和绘制天正图纸做准备。

天正（Tangent）在 DWG 里用 ObjectARX 自定义实体（proxy entity，DXF 类名 `TCH_*`）存储建筑/结构/给排水/暖通/电气对象。不经天正运行时，第三方 CAD 只能看到 `ACAD_PROXY_ENTITY`。

## 二、核心发现（逆向自 tch_kernal.arx）

逆向目标：`C:\Tangent\T20-PlugInV8.0\sys24x64\tch_kernal.arx`（天正 T20 V8.0 全平台内核，9.6MB，stripped x86_64，sha256 `e30522f3…b8eb4be`）。

### 1. 天正数据混淆算法（TgDataEncrypt）

- **密钥**：`0x05`（硬编码常量 `0x528` 第二字节，与版本无关）。
- **加密**：每字节 `b = (byte ^ 0x05)`，再编码为两个大写字母 `('A'+b/26)('A'+b%26)`。
- **解密**：每两个字母 `b = (c0-'A')*26 + (c1-'A')`，再 `byte = b ^ 0x05`。
- **启用条件**：仅 DWG 2004+（保存版本号 ≥16）；更早版本明文。
- 只加密可单字节表示的内容（数值/布尔先 swprintf 成 ASCII 字符串再加密）。
- 函数：TDEncrypt=`sub_1c49b4b0`、TDUnEncrypt=`sub_1c49b320`、上下文=`sub_1c49b310`。

### 2. 序列化结构

基类 `TEntityBase`，入口 `TEntityBase::DwgOutFields`(0x1c410b00) / `DwgInFields`(0x1c410700)。

内存布局：`+0x08`/`+0x10` 两个 double、`+0x20`/`+0x60` 两个对象列表、`+0xa0` 字段容器、`+0xd0` 子对象、`+0x110` GUID 字符串。

### 3. 加密边界（关键）

- **实体固定字段**（double、对象列表等）→ **加密**
- **命名字段容器**（TEntityFields，字段名 wchar 明文 + 类型 int16 明文 + 值明文）→ **明文**

### 4. 实体类清单（proxy DXF 类名，字符串提取）

建筑 `TCH_WALL/COLUMN/SPACE/OPENING/STAIR/CURTAIN_WALL/BALCONY/CAMERA` 等；结构 `TCH_STRUBEAM/STRUREINBAR/STRU_FLOORSLAB/STRUHANGER` 等；给排水 `TCH_PIPE/PIPEVALVE/PIPEFITTING/PUMP/FLANGE`；暖通 `TCH_DBHVACDUCT/RADIATOR`；电气 `TCH_CABLETRY/EEQUIPMENT/EFITTING`。proxy 应用名 `TCH_KERNAL`。

## 三、CADCraft 改造进度

### 已完成
1. **源码更新 v0.12.0 → v0.20.1**（落后 903 提交已补齐）。
2. **天正模块** `crates/dwg/src/tianzheng.rs`：`encrypt`/`decrypt`（XOR 0x05 + Base-26）、`is_tianzheng_class`、`read_tianzheng`（从 acadrust `ProxyEntityData` 提取天正实体）。4 测试通过。
3. **acadrust 已解析 proxy**：`ProxyEntityData.dxf_subclass`（天正类名）+ `graphics`/`payload`（`ProxyPayload` 原始字节）+ `object_ids`。CADCraft 已能 round-trip 保存天正 DWG（新版 `proxy_transport.rs` 处理 proxy class 恢复）。

### 未完成
- **天正 graphics 字段级解析**（几何/属性语义）：需真实 `.dwg` 样本验证。格式已逆向，但没样本坐实。
- 字段数据类型枚举（0..210）未展开。
- 各实体派生类（墙/柱/管段）的固定字段语义未解析。

## 四、编译环境配置（重要，重启后需保留）

网络：`github.com` 主域名被墙，但 `api.github.com`/`codeload.github.com`/`ssh.github.com:443` 通。

- git 全局：`url."ssh://git@ssh.github.com:443/".insteadOf "https://github.com/"`（SSH key `quwang2000`）。
- 23 个 git 依赖（egui/winit fork + craft-libs 4 rev）已改成 **path 依赖**。
- vendor 在 `E:\my soft\cadcraft-vendor`（**必须在 CADCraft 目录外**，否则嵌套 workspace 报继承错误）。
- 项目 `.cargo/config.toml` 有 `[net] git-fetch-with-cli = true`。
- `craft-libs-92d392c` 修了 `craft-keymap`（删 menu 里 `default-features = false`，cargo 1.95 禁止 override）。

## 五、样本验证结果（TIANZHENTEST.dwg）

样本 `C:\Users\qw784\Desktop\TIANZHENTEST.dwg`（AC1032/2018，45KB）实测验证：

1. **天正实体是 `EntityType::Unknown`（不是 Proxy）** —— acadrust 0.6.8 把天正自定义实体（`TCH_*`）解析为 Unknown，`dxf_name` 直接是天正类名。这是 `read_tianzheng` 之前返回 0 的根因，已修正。
2. **成功提取 25 个天正实体**：TCH_WALL ×8、TCH_COLUMN ×8、TCH_OPENING ×4、TCH_DIMENSION2 ×4、TCH_AXIS_LABEL ×1。数据在 `raw_dwg_data`（1429~1920 字节，原始 merged-stream record 位流）。
3. **字段容器明文已验证**：`converted_test.dxf`（tch_dump 输出）的 310 组码里，直接看到 UTF-16 字段名/值（`T20d7`、`T20s9`、`20b3A`、`*_BLOCK_BEGIN_AU`）和产品 GUID `209943AA-4212-4AFA-BB0C-AB88470D5E81`。
4. **加密默认启用**：天正全局版本变量 `0x1c8ba38c` 初始值 = 16（≥16 触发加密分支）。
5. **text 流定位成功**（`extract_text_stream` 已固化到 `tianzheng.rs`）：flag 在 record 末尾（`total_bits-1`，`total_bits = raw.len()*8 - handle_bits`），`str_size` 在 flag 前 2 字节（16 bit），`text_start = length - str_size`。实测 str_size = 13220 bit。
6. **text 流结构实解出**：开头是明文产品 GUID `{61309675-BD2E-44BE-B870-B80B03BB3A76}`，随后是加密字段（乱码，单字节 bit 编码），再后是明文字段名 `TDBXDATAMAP_BLOCK_BEGIN_` 和字段值 `T20u6`、`cic`。

## 六、加密算法验证状态

- **汇编级验证可靠**：XOR 0x05（`0x528` 常量）和 Base-26（`0x4ec4ec4f` ÷26 魔数）在 `tch_kernal.arx` 反汇编里确认。
- **样本级验证受阻**：加密的固定字段是字符串，写在 DWG merged record 的 **text 流**里（布局 `[RL][main][text][modular_short][flag][handles]`）。text 流起始位置需由 flag bit 反向定位，且字符串是 bit 流编码（不字节对齐）。需用 acadrust 的 `DwgMergedReader`（`setup_text_and_handle` + `read_variable_text`）才能解出，而非简单的 `DwgBitReader`。

## 七、完整解析已实现（decrypt_field）

加密字段的 code units 是 Base-26 字母加 **0x18B 偏移**（天正自定义编码防明文）。解密链：
`code unit - 0x18B = Base-26 值(0-25) → Base-26 解码 c0*26+c1 → XOR 0x05`。

`decrypt_field` 已固化到 `tianzheng.rs`，实测完整解出 TCH_WALL 的 45 个字符串：

- **字段容器完全明文**：字段名 `T20b2/T20b3/T20d7/T20s9/T20u6` 等，字段值含**中文**（`A级`、`填充墙`、`多孔材料`、`保温层`），边界 `TDBXDATAMAP_BLOCK_BEGIN_/END_`。
- **固定字段加密**：GUID 之后的加密字段，解密后多为数值（如 `777...7`、`123`、`341` 等）。
- 少数加密字节解出非数字字符（`0x29`/`0x3e`/`0x3f`），疑为 double 的二进制尾数或非字符串类型，待最后确认。

## 八、完整「读-改-写」闭环已实现

`tianzheng.rs` 现在有完整的对称 API 链（7 测试全过）：

| 读 | 写 |
|---|---|
| `read_tianzheng` | （Unknown 实体 round-trip 保存） |
| `extract_text_stream` | — |
| `parse_text_strings` | `encode_text_strings` |
| `decrypt_field` | `encode_field` |
| `decrypt` | `encrypt` |

实测 `tz_decode` example：读 TIANZHENTEST.dwg 的 TCH_WALL → 解析出中文字段值（`A级`/`填充墙`/`多孔材料`/`保温层`）→ 替换为（`砖墙`/`实心材料`/`隔热层`）→ 重新编码 → round-trip 验证成功。

## 九、完整端到端保存已实现 ✅

`replace_text_stream`（写回 merged record）+ `tz_save` example 实现完整「读 → 改 → 写 → 保存」：

实测 `tz_save`：读 TIANZHENTEST.dwg → 8 个 TCH_WALL 的中文字段值 `填充墙/多孔材料/保温层` 替换为 `砖墙/实心材料/隔热层` → `DwgWriter` 保存 → 再读验证成功（42939 字节新 DWG）。

**项目目标「读取保存天正实体」已完整实现**：识别 → 提取 → 解析 → 解密 → 修改 → 加密 → 写回 → 保存，全链路对称可逆，端到端验证通过。

## 十、剩余待办（非阻塞）

1. 加密字段少数非数字字节的语义确认（double 二进制表示 vs 非字符串类型）。
2. 展开字段 DataType 枚举（0..210）与各实体派生类固定字段语义。
3. 完整 workspace 编译验证（只验证了 cadcraft-dwg）。
