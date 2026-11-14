# 天正 T20 自定义实体 DWG 序列化格式（逆向笔记）

> 逆向来源：`C:\Tangent\T20-PlugInV8.0\sys24x64\tch_kernal.arx`
> （天正 T20 V8.0 全平台内核，AutoCAD 2024 对应版本；stripped x86_64，导入 acdb24.dll 等 ObjectARX 库，sha256 `e30522f3…b8eb4be`）
>
> **状态**：格式主体已还原；加密算法经汇编级验证。但 **尚未用真实 .dwg 样本实测**，且各实体派生类的具体字段语义未完全展开。

## 0. 重要修正（2026-11-12，第二次逆向）

首次逆向（本文件其余章节）有几处**关键结论是错的**，已在第二次逆向（逆 `TDbWall`/`TWallData`/`TEntityField` 虚函数链 + 实测 `TIANZHENTEST_modified.dwg` 与 `02 配电装置室建筑施工图_t5.dwg`）中修正：

1. **`+0xd0` 子对象不是几何，是 `TDbXDataMap`（数据映射容器）**。vftable=`0x1c675a60`，`dwgOutFields`@`0x1c40e840` 写「字段名→内存偏移」映射表（11 个 filer 间接调用）。本文件第 4 节把 `+0xd0` 当「子对象几何」、第 9 节把「子对象 DwgOut 0x1c40e840」当几何，均错。

2. **墙几何是 `TWallData`，在 `+0x130` 偏移**（非 `+0xd0`）。继承链：`TDbWall(+0x00)` → `TDbCurveEntity(+0x00)` → `TEntityBase(+0x18)`。`TWallData` 继承 `TGGeCurveSegment`（几何曲线段），构造接收 `TADSGePoint3d×2 + double` = **起点/终点/高度**。另有 `TWallBase @+0x208`。`TWallData` vftable 槽[10] 是析构函数，几何序列化不在其虚函数。

3. **DataType case 映射**（`TEntityField::DwgOut`@`0x1c469880`，索引表 `0x1c469c44` + 跳转表 `0x1c469bf0`）：只有 `0(bool)/1-2(char)/3(byte)/4(uint16)/5(int16)/6(int32)/7(uint)/100(嵌套字段容器)/101(string)/200-210(double BD)` **写 main 流值**；**`8-99` 与 `102-199` 是 default return，不写 main 流值**。实测字段类型 `164/32/173/67/161/162/146/114/10` 全不写值——之前 `tz_decode` 把它们当 double/byte 读，是「double 垃圾值」「main 流 [451,777) 来源不明」的根源（错位 124 bit）。

4. **真实生产图纸的墙几何在 proxy graphics 的类型 6 图元**（acadrust `ProxyGraphicRecord::Unknown{record_type:6}`），payload 52 字节 = `u32 顶点数(=2) + 起点(X1,Y1,Z1) + 终点(X2,Y2,Z2)`，6 个 double LE。墙 = 多个类型 6 图元（每段路径一个；简单墙 2 段、复杂墙 18 段）。坐标是建筑坐标 mm（如 X=137026.85/Y=4505.88），双线墙间距=墙厚 240mm。已实现 `cadcraft_dwg::tianzheng::extract_wall_segments` 提取（commit `aa85032`）。**binary 格式（TIANZHENTEST）graphic_data 仅 8 字节，几何在 text 流加密固定字段**（`decrypt_field` 解密得 ASCII 数字）。

5. **几何回写已实现（`encode_wall_segments` 逆操作）**：`encode_wall_segments(graphic_data, segments)` 把新 segments 反向构造成类型 6 图元（payload 重写为 `u32 2 + 6×f64`，其余图元原样保留；**段数必须等于类型 6 图元数**，否则拒绝）。保存路径：`cadcraft_io::move_tianzheng_wall`/`move_tianzheng_segment`/`set_tianzheng_segment` 改 `Tianzheng.segments` → `encode_wall_segments` 生成新 graphic_data → `sync_tianzheng_graphics` 写回 tags 的 `160`(i64)/`92`(i32) 长度 + 紧跟的 `310` 段（graphics 段在 `162`/`96` text bits 之前，与 data 的 `161`/`93`+`310` 段区分）。命令层经 `properties.set` 的 `tianzhengGeometry` 参数（`{"op":"moveWall|moveSegment|setSegment|setThickness", ...}`）接入，hook 为 `tianzheng_geometry_setter`。端到端已验证（真实图纸 52 墙，平移 (1000,2000) → 保存 → 再读坐标一致）。**`set_thickness` 已实现（仅直墙）**：`set_tianzheng_thickness` 改 2 段平行双线的间距（墙中线不动，双线各沿法线移动 `(new-old)/2`），端到端验证 240→300mm 落盘成功；拐角/开洞墙（3+ 段）返回 false，AI 用 `setSegment`/`moveSegment` 自算坐标。

6. **binary 字段命名方法**：字段名（`T20xx`）是字符串常量，被 getter/setter 函数引用，函数名即语义（`feature_find` 搜字段名 → 找 `Get/Set…@类名`）。`TDbWall`(TCH_WALL) 字段映射：`T20b2`=HatchPattern 类型(byte)、`T20b3`=LineHatch 线填充标志、`T20s4`=HatchPattern 名("实心材料")、`T20u6`=HatchColor 填充颜色、`T20s14`=WallMat 墙材料("砖墙")、`T20s1`=Grade 等级("A级")、`T20u11`/`T20u16`=InsulateLine 保温层线。**纠正**：`T20d7`=OutCircleTextAlignment 属 `TDbSymbDrawingIndex`（图纸索引符号）、`T20d12`=SourceExtentY 属 `TDbSymbIndexPointer`（索引指针符号），**不是墙字段**（旧版误归 TDbWall）。故 binary 的 TCH_WALL 字段全是「填充/保温/材料」**属性**（T20b/s/u）。**墙高/墙厚在 `TWallData` 子对象（`TDbWall+0x130`）的固定 double**：墙高 = `TWallData+0x70`（`TDbWall+0x1A0`，`GetHeight` 汇编 `movsd [rcx+0x70]`），墙厚 = `TWallData+0x60 + 0x68`（`TDbWall+0x190+0x198`，`GetWidth` 汇编 `[rcx+0x68]+[rcx+0x60]` 取绝对值 = 左右半厚之和）；几何（起点/终点/bulge）也在 TWallData。序列化版本>11 走 `TgDataEncrypt` 加密，≤11 明文。

7. **天正文字在 proxy graphics 类型 38 图元**（实测 TCH_TEXT/TCH_DIMENSION/TCH_OPENING/TCH_ELEVATION）。payload 布局：`x(f64) y(f64)` 是插入点（建筑坐标 mm），偏移 16/24/32 是 0，偏移 40 是缩放因子（恒 1.0），**偏移 48/56 是旋转方向的**方向余弦** `(cos, sin)`**（单位向量：水平 `(1,0)`、垂直 `(0,1)`、坡度箭头 `(0.993,-0.118)`），旋转角 = `atan2(sin@56, cos@48)`；偏移 72 起是 UTF-16LE null 结尾的文字内容，其后 `char_count(u32，对齐到 4)`，再对齐到 8 后是 `height(f64)`（如 297.5）与 `width_factor(f64)`（如 0.70588），更后面是字体名 `SIMPLEX`/`GBCBIG`。**上标/下标文字**（`^U2^U` 控制码对应）的 height 字段布局不同（读到 0 = 继承主文字），已用「扫描 50..5000 范围 + 继承主文字高度」启发式处理。已实现 `extract_text_entities` 提取（含 rotation）+ 渲染层按 `Text` 画出（原「offset 48=旋转角」结论作废）。

8. **图元类型全景与几何图元格式**（真实图纸 `_t5.dwg` 实测，已全部解析）：
   - **type 2 圆**（75）：7×f64 = 圆心(x,y,z) + 半径 + 起点角 + 终点角 + 法线 z（1.0=完整圆）。建筑图半径 400mm = 圆形柱。→ `extract_circles`。
   - **type 6 线段**（3210）：`u32 顶点数(2) + 起点(X1,Y1,Z1) + 终点(X2,Y2,Z2)`，6×f64。墙双线/尺寸线。→ `extract_wall_segments`。
   - **type 6 零长度点**（663）：起点=终点 = **尺寸标注箭头尖端**（尺寸线端点外 300mm）。方向 = 从最近线段端点指向尖端（轴对齐单位向量）。→ `extract_arrow_tips` + 渲染实心三角形（固定 250mm）。
   - **type 9 填充多边形**（35，全是 TCH_COLUMN 柱截面 4 顶点矩形）：`u32 顶点数 + N×(x,y,z 各 f64)` + 尾部 36B 填充属性。→ `extract_polys`。
   - **type 38 文字**（2563）：见第 7 点。
   - **控制/属性图元**（type 14/16/18/20/22-26，4-8B 固定值）：颜色/线型/计数/状态，不需渲染。
   - **type 33 箭头/符号**（1019，33-64B 多子类型）：u32[0]=payload 长度、u32[1]=子类型标志。带坐标的箭头子类型（0x00800005）仅 63 个，全在 TCH_MULTILEADER（引线箭头 48）+ TCH_DRAWINGINDEX（15），payload 布局 `u32 长度 + u32 子类型 0x00800005 + u32 0 + u32 常量 0x09049404 + 0x02`，尖端 = offset 17/25 的 f64 LE（建筑坐标 mm）；其余（47B 0x41 等）是属性子类型（无坐标）。引线折线本身是同一实体的 type 6 线段（已由 `extract_wall_segments` 渲染）。**已并入 `extract_arrow_tips`**（方向 = 从最近 type 6 线段端点指向尖端），与尺寸箭头一起渲染实心三角形。

## 1. 概述

天正 T20（Tangent）在 DWG 中通过 ObjectARX 自定义实体（代理实体 proxy entity，DXF 类名 `TCH_*`）存储建筑 / 结构 / 给排水 / 暖通 / 电气对象。不经过天正运行时，第三方 CAD 只能看到 `ACAD_PROXY_ENTITY`，读不到真实几何与属性。

`tch_kernal.arx` 是天正全平台共享内核。所有自定义实体从基类 `TEntityBase` 派生，序列化入口是 `TEntityBase::DwgOutFields` / `TEntityBase::DwgInFields`。

## 2. 实体类清单（proxy DXF 类名）

字符串直接提取自二进制，按专业分组：

| 专业 | DXF 类名 |
|---|---|
| 建筑 | `TCH_WALL`、`TCH_COLUMN`、`TCH_OPENING`、`TCH_SPACE`、`TCH_CURTAIN_WALL`、`TCH_WALL_PATCH`、`TCH_BALCONY`、`TCH_CORNER_WINDOW`、`TCH_BAND_WINDOW`、`TCH_CAMERA`、`TCH_APRON`、`TCH_ASCENT`、`TCH_AXIS_LABEL`、`TCH_BLOCK_INSERT`、`TCH_MVINSERT`、`TCH_DIMENSION`、`TCH_DIMENSION2`、`TCH_RADIUSDIM`、`TCH_FLOORLINE`、`TCH_CURVEMESH`、`TCH_CURVEMESH2`、`TCH_STAIR` 系列 |
| 结构 | `TCH_STRUBEAM`、`TCH_STRU_FLOORSLAB`、`TCH_STRU_HOLE`、`TCH_STRUSELFBASE`、`TCH_STRUBARBASE`、`TCH_STRUSLABHOLE`、`TCH_STRUHANGER`、`TCH_STRUNODEPOINT`、`TCH_STRUPLAINNUMBER`、`TCH_STRUREINBAR`、`TCH_STRUSECTSTAIR`、`TCH_STRUSTIRRUP`、`TCH_STRUSYMBFOCUS`、`TCH_STRUSYMBINSITU`、`TCH_STRU_WCOLUSECT`、`TCH_STRUEXENTSZONETABLE` |
| 给排水 | `TCH_PIPE`、`TCH_PIPEVALVE`、`TCH_PIPEFITTING`、`TCH_PUMP`、`TCH_FLANGE`、`TCH_DOUBLELINEPIPE`、`TCH_VPIPEDIM`、`TCH_TRENCH` |
| 暖通 | `TCH_DBHVACDUCT`、`TCH_DBHEQUIPMENT`、`TCH_DBHFITTING`、`TCH_RADIATOR` |
| 电气 | `TCH_CABLETRY`、`TCH_EEQUIPMENT`、`TCH_EFITTING`、`TCH_EQUIPMENT` |
| 其他 | `TCH_SECTIONPATHSIGN`、`TCH_Section`、`TCH_TESTSECTIONPLANE`、`TCH_TESTSECTIONLINE`、`TCH_KATANA`、`TCH_FIREZONE`、`TCH_FLOORRECT`、`TCH_EVACPATH`、`TCH_PROTECT_ENTITY`、`TCH_AREAREDLINE`、`TCH_REIN_INDEX_DIM`、`TCH_TDBINSIGHT`、`TCH_DRAWINGNAME`、`TCH_SYMB_*` 系列、`TCH_GROUP` / `TCH_HIDE_GROUP`、`TCH_DBCONFIG` / `TCH_DBPSCONFIG` |

另有 `_TCH_` 前缀的内部/隐藏类名（`_TCH_SOLID`、`_TCH_SPACE`、`_TCH_WINDOW`、`_TCH_AXIS`、`_TCH_DIM`、`_TCH_LABEL` 等）。proxy 应用名（app name）为 `TCH_KERNAL`。

## 3. 数据混淆算法（`TgDataEncrypt`）

天正对写入 DWG 的**实体固定字段**做混淆（不是真密码学加密）。

- **启用条件**：仅 DWG 2004+（保存版本号 ≥ 16，即 R2004 起）启用；更早版本明文直写。
- **密钥**：`0x05`，硬编码（来自常量 `0x528` 的第二字节），与保存版本无关。

算法（对称）：

```
加密 = 每个字节 b → ('A' + b/26)('A' + b%26)，其中 b = 原字节 ^ 0x05
解密 = 每两个字母 ((c0-'A')*26 + (c1-'A')) → 字节 = b ^ 0x05
```

即：先 XOR `0x05`，再做 Base-26（两个大写字母表示一个 0–255 字节）。

对应函数（stripped 内部名）：
- `TDEncrypt` = `sub_1c49b4b0`
- `TDUnEncrypt` = `sub_1c49b320`
- 上下文构造 = `sub_1c49b310`（写死 `0x528`）

因为 Base-26 只编码低字节（0–255），天正**只加密可单字节表示的内容**——数值/布尔先 `swprintf` 成 ASCII 字符串，再加密。因此密文是原字符串两倍长度的大写字母串。

## 4. 实体序列化结构

`TEntityBase` 内存布局（偏移，x86_64）：

| 偏移 | 内容 |
|---|---|
| +0x08 | double |
| +0x10 | double |
| +0x20 | 对象列表（`TDPtrList`，软指针 id） |
| +0x60 | 对象列表（`TDPtrList`） |
| +0xa0 | `TEntityFields` 字段容器 |
| +0xd0 | 子对象（有独立虚表） |
| +0x110 | `TWCString`（产品标识 GUID） |

`TEntityBase::DwgOutFields` 写序：

1. 版本相关前置（据 DWG 保存版本分支）
2. 固定字段（两个 double、两个对象列表）→ **经 `TgDataEncrypt` 加密**
3. `TEntityFields` 字段容器 → **明文**
4. 子对象（+0xd0）
5. 产品标识 GUID 字符串（6 选 1，明文）

## 5. 字段容器格式（明文）

`TEntityFields::DwgOut`（字段数 + 逐字段）：

```
[字段数 int32]                       # 明文
  每个字段 TEntityField::DwgOut:
    [字段名 wchar]                   # 明文
    [数据类型 int16]                 # 明文，枚举 0..210
    [字段值]                         # 明文，按类型经 filer 直写
```

`TEntityField` 布局：+0x10 存数据类型（int16），+0x18 起存值数据。字段值通过 `GetData` 取出后由 `AcDbDwgFiler` 虚函数直写，不经加密。

## 6. 加密边界（关键结论）

- **实体固定字段**（`TEntityBase` 及各派生实体自己的成员：坐标、尺寸、对象 id 列表等）→ **加密**
- **命名字段容器**（`TEntityFields` 内的字段名 / 类型 / 值）→ **明文**

不要误以为天正实体数据全加密——命名字段容器是明文的，可直接解析。

## 7. 产品标识 GUID

`TEntityBase::DwgOutFields` 末尾按全局枚举写 6 个 GUID 之一（疑似区分建筑/结构/给排水/暖通/电气/其他产品系列）：

```
{B0713E6B-AFF6-4871-A273-7896834D6676}
{CE0D96A4-364C-46C2-B793-D33FD4CE9C92}
{209943AA-4212-4AFA-BB0C-AB88470D5E81}
{2143F26C-50DE-40D1-8DC1-33A8F24F5E74}
{FB4404A7-2317-4C60-90E7-DEE93987E6F6}
{20DE790C-46D3-4D54-89D6-B09837DBBD7E}
```

## 8. 参考实现

解密/加密的独立参考实现见 `reference/tianzheng_crypt.py`。

## 9. 样本实测（TIANZHENTEST.dwg，已坐实）

用 `crates/dwg/src/tianzheng.rs`（`read_tianzheng` + `extract_text_stream` + `parse_text_strings`）实测解出 TCH_WALL 的 text 流 45 个字符串，结构：

```
[0]  {61309675-BD2E-44BE-B870-B80B03BB3A76}   产品 GUID（明文）
[1-6] 乱码 ×6                                 加密的固定字段
[7]  TDBXDATAMAP_BLOCK_BEGIN_                  字段容器开始标记（明文）
[8-23] T20b2 / T20b3 / T20d12 / T20d7 / T20s9 / T20u6 ...  字段名（明文，与加密字段值交替）
[24] TDBXDATAMAP_BLOCK_END_                   字段容器结束标记（明文）
[25] {209943AA-4212-4AFA-BB0C-AB88470D5E81}   关联对象 GUID（6 个产品 GUID 之一）
[26+] 乱码                                    更多加密字段
```

**坐实的结论**：
- 字段容器是明文的，边界是 `TDBXDATAMAP_BLOCK_BEGIN_` / `TDBXDATAMAP_BLOCK_END_`。
- 字段名格式 `T20` + 类型字母 + 序号（如 `T20b2`、`T20d7`、`T20s9`、`T20u6`）。
- 加密字段（固定字段 + 字段值）以乱码出现在 text 流，需进一步解密（见下）。

### 字段名类型字母（DataType 分类，逆向确认）

字段名 `T20` + 类型字母 + 序号，类型字母决定字段值类型和存储位置：

| 前缀 | 类型字母 | 类型 | 值存储 | 样本字段值示例 |
|---|---|---|---|---|
| `T20b*` | b | byte/bool | main 流（bit 编码） | 数值 |
| `T20d*` | d | double | main 流（bit 编码） | 数值（坐标/尺寸） |
| `T20s*` | s | string | text 流（明文） | `A级`、`填充墙`、`多孔材料`、`保温层` |
| `T20u*` | u | uint | main 流（bit 编码） | 数值 |

统计（tch_kernal.arx 内）：`T20b`×3、`T20d`×9、`T20s`×7、`T20u`×2。

### main 流结构（DataType 位编码，实测解出）

main 流（`raw_dwg_data[0..text_start]`）= `[字段数 int32(BL)][逐字段：类型 RC 1字节 + 值]`。

实测 TCH_WALL：字段数 = 27，字段类型用 **RC（raw char，1 字节）** 编码，读出的 DataType 值：

```
字段 0-1: 类型 2   = byte/bool（对应 T20b2/T20b3）
字段 2-3: 类型 202/164 = double 子类型（**非** T20d12/T20d7——那两字段属符号实体，见第 6 点；此处 double 疑为墙几何/尺寸，未精确命名）
字段 4-5: 类型 32  = string（对应 T20s1/T20s14，值在 text 流）
字段 6-11: 类型 0  = 空/未设置
字段 12:  类型 2   = byte
字段 13:  类型 173 = ?
字段 16-20: 类型 67/161/162/146/7 = uint 等子类型
字段 21:  类型 32  = string
字段 22-23: 类型 114/10 = ?
字段 24-26: 类型 0  = 空
```

**已确认的 DataType 分类**（索引表 `0x469c44`：DataType 0-210 → 20 个基础类型 case + 1 个 default）：

- DataType 0-7 → 索引 0-6（bool/char/byte/uint16/int16/int32/复杂）
- DataType 8-99 → default（return，不写 main 流）
- DataType 100-101 → 索引 7/8
- DataType 102-199 → default
- DataType 200-210 → 索引 9-19（double 等）

### 字段值 bit 编码（实测解出）

字段值按 DataType 编码（`TEntityField::DwgOut` 的 switch case 逆向）：

| DataType | 索引 | case 槽位 | 类型 | bit 编码 |
|---|---|---|---|---|
| 0 | 0 | 0x170 | bool | BB（1 bit）|
| 1/2 | 1 | 0xb0 | char | RC（1 字节）|
| 3 | 2 | 0x160 | byte | RC（1 字节）|
| 4 | 3 | 0x120 | uint16 | BS |
| 5 | 4 | 0x150 | int16 | BS |
| 6 | 5 | 0x110 | int32 | BL |
| 7 | 6 | 0x140 | 复杂 | 待逆向 |
| 100 | 7 | 多函数 | 复杂 | 待逆向 |
| 101 | 8 | 0xd0 | string | text 流 |
| 200-210 | 9-19 | 0x190-0xf0 | double 等 | BD |

**关键发现**：switch 的 default case 是 `return`（`0x469bda`），即 DataType 8-99、102-199（190 个 DataType）的字段值**不通过 TEntityField::DwgOut 直接写 main 流**。

**关键结论（几何数据位置）**：main 流的字段值只占约 225 bit（基础类型 bool/char/double），剩余的 326 bit（main 流 [451, 777)）是 **TEntityBase 子对象（+0xd0 偏移）的序列化**——子对象是几何数据容器（墙的路径/高度/方向），其 DwgOut（虚函数）写 main 流。即：

```
main 流 = 字段数(BL) + 字段类型×N(RC) + 字段值(基础类型)
        + 子对象几何数据(+0xd0 的 DwgOut 输出)
```

天正实体的**几何核心在子对象（+0xd0）**，字段容器只存属性（材料/等级/尺寸），子对象存几何（路径/坐标）。

### 子对象（+0xd0）几何结构（逆向确认）

子对象是几何数据容器：构造函数 `0x1c409d20`、vftable `0x1c675a60`（5 虚函数）、DwgOut `0x1c40e840`、DwgIn `0x1c40e480`。

DwgOut 写 9 个字段（几何数据）：

| # | 槽位 | 类型 | 语义 |
|---|---|---|---|
| 1 | 0xd0 | string | 类型标识 |
| 2 | 0xd0 | string | ? |
| 3 | 0x110 | int32 | 路径点数（float 截断） |
| 4 | 0xa0 | 指针 | 对象引用 |
| 5 | 0x190 | double | 高度/厚度 |
| 6 | 0x110 | int32 | 整数 |
| 7 | 0x90 | 指针 | 对象引用 |
| 8 | 0xd0 | string | ? |
| 9 | 0x1d0 | double | double |

`0x90`/`0xa0` 槽位是**指针/对象引用**（非 Point2d/Vector3d），几何坐标是 double 字段（0x190/0x1d0）+ 整数（路径点数）。

## 10. 待完成

- [ ] 数值字段值（b/d/u）从 main 流位解码（需按字段顺序读取，字段名在 text 流、数值在 main 流交错对应）。
- [x] 墙（TDbWall）的固定字段语义：墙高 `TWallData+0x70`（TDbWall+0x1A0）、墙厚 `TWallData+0x60+0x68`（TDbWall+0x190+0x198），见第 6 点。柱/梁/管段等其他派生类仍待逆。
- [ ] 字段数据类型枚举（0..210）的完整映射（已确认 s/b/d/u 四类，其余类型待展开）。
- [x] `set_thickness`（改墙厚，直墙 2 段）：`set_tianzheng_thickness` 已实现（中线不动、双线沿法线各移 (new-old)/2）；拐角/开洞墙用 `setSegment`/`moveSegment` 原语让 AI 自算坐标。
