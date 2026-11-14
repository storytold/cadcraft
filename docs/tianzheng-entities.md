# 天正实体字段命名汇总

> 本文汇总天正（Tangent T20）各专业自定义实体（`TCH_*`）的字段命名与存储位置，逆向自 `tch_kernal.arx` / `tch_PipeBase.arx` / `tch_pipewire.arx`（Craft Anything 静态分析，getter 汇编 + `GetXml` XML 标签 + 序列化函数）。
>
> 配套：`docs/tianzheng-format.md`（序列化格式、加密算法、DataType case 映射的完整逆向过程）。

## 一、三大继承体系

天正所有自定义实体分属**三套继承体系**，字段容器的偏移各不相同：

| 体系 | 基类 | 字段容器偏移 | 专业 |
|---|---|---|---|
| 建筑 | `TEntityBase` | `+0xa0`（TEntityFields）/ `+0xd0`（TDbXDataMap） | 建筑 |
| 结构 | `TDbStruEntityBase` → `TDbStruBarBase` | 字段名（虚函数槽 `0x768` 按名查） | 结构 |
| 机电 | `TDbInterfacesObj`（多重继承 `AcDbCurve`+`TEntityBase`） | `+0xe8` | 给排水 / 暖通 / 电气 |

- **建筑**实体字段用 `T20xx` 命名字段容器（TEntityFields）+ 加密固定字段（TEntityBase 固定 double）。
- **结构**实体字段是 XML 自描述标签（`SecTyp`/`Thick`/`Link_Beam`…），经虚函数槽 `0x768` 从字段容器按名读。
- **机电**实体字段也是 XML 自描述标签（`Pipe_caliber`/`DuctWidth`…），字段容器统一 `+0xe8`，几何统一 `+0x180` 曲线对象。

---

## 二、建筑专业（`TEntityBase` 体系）

### 墙 `TDbWall`（`TCH_WALL`）

继承：`TDbWall(+0x00)` → `TDbCurveEntity` → `TEntityBase(+0x18)`；墙几何子对象 `TWallData @ +0x130`。

| 字段 | 类型 | 存储位置 | 语义 |
|---|---|---|---|
| `T20b2` | byte | T20 字段容器 | HatchPattern 类型 |
| `T20b3` | byte | T20 字段容器 | LineHatch 线填充标志 |
| `T20s4` | string | T20 字段容器 | HatchPattern 名（"实心材料"） |
| `T20u6` | uint | T20 字段容器 | HatchColor 填充颜色 |
| `T20s14` | string | T20 字段容器 | WallMat 墙材料（"砖墙"） |
| `T20s1` | string | T20 字段容器 | Grade 等级（"A级"） |
| `T20u11`/`T20u16` | uint | T20 字段容器 | InsulateLine 保温层线 |
| **墙高** | double | `TDbWall + 0x1A0`（`TWallData+0x70`） | `GetHeight` = `movsd [rcx+0x70]` |
| **墙厚** | double×2 | `TDbWall + 0x190 + 0x198`（`TWallData+0x60+0x68`） | `GetWidth` = `[+0x68]+[+0x60]` 取绝对值（左右半厚） |
| 起点/终点/bulge | 几何 | `TWallData`（TGGeCurveSegment） | 墙路径 |

> 注意：`T20d7`=OutCircleTextAlignment、`T20d12`=SourceExtentY **不是墙字段**，分属 `TDbSymbDrawingIndex`（图纸索引符号）/`TDbSymbIndexPointer`（索引指针符号）。TDbWall 无 `T20d`（double）字段。

### 柱 `TDbColumn`（`TCH_COLUMN`）

柱数据子对象 `TColumnData @ +0x1b8`。

| 字段 | 存储位置 | 语义 |
|---|---|---|
| 位置 X/Y/Z | `TDbColumn + 0x1b8 / 0x1c0 / 0x1c8` | `GetLocation` = `movups [rcx]` + `movsd [rcx+0x10]` |
| 标高 | `TDbColumn + 0x1c8` | `GetElevation` = `movsd [rcx+0x10]` |
| **柱高** | `TDbColumn + 0x1d0`（`TColumnData+0x18`） | `SetHeight` = `movsd [rcx+0x18]` |
| 截面形状 | 枚举 | `GetSectionShape` |
| 截面参数 | 3D 点（宽/深） | `GetParameters` |

---

## 三、结构专业（`TDbStruEntityBase` 体系）

结构柱、剪力墙**没有独立类名**（`TDbStruColumn`/`TDbStruWall` 均不存在），统一走 `TDbStruBarBase`（结构杆件基类）；只有梁具名 `TDbStruBeam`。

### 结构杆件基类 `TDbStruBarBase`（梁 `TDbStruBeam`/`TCH_STRUBEAM`、柱、墙共用）

截面宽/高**不是固定 offset 成员**，经虚函数槽 `0x768` 从字段容器按字段名读 double（`GetTotalWidth/Height` 汇编 `lea rdx,字段名` → `jmp [rax+0x768]`）。

| 字段 | 语义 |
|---|---|
| `SecTyp` | 截面类型（`Circle` 圆 / `IBeam` 工字 / `Rect` 矩形） |
| `Diameter` | 直径（圆形截面） |
| `Width` / `Height` | 宽 / 高（矩形截面） |
| `Start_point` / `End_point` | 起终点 |
| `Baseline` | 基线 |
| `Bulge` | 凸度 |
| `Central_ang` | 圆心角 |
| `Height` / `Top_height` | 高度 / 顶标高 |
| `Material` / `MatTyp` | 材料 / 材料类型 |
| `AxisAngle` / `AxAng` | 轴角 |
| `Link_Beam` / `Link_Colu` | 关联梁 / 关联柱 |

结构梁 `TDbStruBeam` 另有结构分析数据：配筋 `TReinSectData`、荷载 `TStruLoad`、内力（弯矩/剪力）、挠度、裂缝。

### 结构楼板 `TDbStruFloorSlab`（`TCH_STRU_FLOORSLAB`）

| 字段 | 语义 |
|---|---|
| `Thick` | **板厚** |
| `UpHeight` / `UpHigh` | 顶标高 / 顶高 |
| `Position` | 位置 |
| `Link_Hole` / `Link_ID` | 关联洞口 / ID |

### 其他结构实体

| 实体 | 说明 |
|---|---|
| `TDbStruHole` / `TDbStruSlabHole` | 结构洞口 / 板洞 |
| `TDbStruReinBar` | 结构钢筋 |
| `TDbStruNodePoint` | 结构节点 |
| `TDbStruWColuSect` | 结构墙柱截面 |
| `TDbStruSymbFocus` / `TDbStruSymbInsitu` | 结构符号（集中标注 / 现浇） |

---

## 四、给排水 / 暖通 / 电气（`TDbInterfacesObj` 体系，字段容器 `+0xe8`）

三大机电专业实体**完全同构**：都派生自 `TDbPipeBase` → `TDbInterfacesObj`，字段容器统一 `+0xe8`，几何统一 `+0x180` 曲线对象（`StartPoint`/`EndPoint` 经虚函数槽 `0x28`/`0x38`）。

### 管段 `TDbPipe`（`TCH_PIPE`，`tch_pipewire.arx`）

基类 `TDbPipeBase`（`tch_PipeBase.arx`），字段容器 `+0xe8`（键 `TCH_PIPEBASE`）。

| 字段 | 语义 |
|---|---|
| `Pipe_caliber` | 管径 |
| `Pipe_inradius` | 管内半径 |
| `PIPE_OUTER_DN` | 外径 DN |
| `PIPE_THICK` | 管壁厚 |
| `Start_point1` / `End_point1` | 起终点（双线另有 `Start/End_point2`） |
| `Mid_point` | 中点 |
| `Gradient` / `FlowDir` | 坡度 / 流向 |
| `Material` / `Style` | 材料 / 样式 |

### 风管 `TDbHvacDuct`（`TCH_DBHVACDUCT`，`tch_PipeBase.arx`）

继承 `TDbPipeBase`，字段容器 `+0xe8`（键 `TCH_DBHVACDUCT`）；弧风管 `IsArcDuct`/`InitArc`、圆风管 `GetCirCenter`、垂直 `IsVertical`。字段语义经风管标注 `TDbHvacDimension` 的 getter 名揭示：

| 字段 | 语义 |
|---|---|
| `DuctWidth` | 风管宽 |
| `DuctHeigh`（拼写少 t） | 风管高 |
| `DuctEle` | 风管标高 |
| `DuctLength` | 风管长度 |
| `DuctType` / `IsDuctCap` | 类型 / 是否风管帽 |
| `DuctMaterial` / `DuctSectArea` | 材料 / 截面积 |
| `DuctVelocity` / `DuctAirVol` / `DuctWayResis` | 风速 / 风量 / 沿程阻力 |

### 桥架 `TDbECabletry`（`TCH_CABLETRY`，`tch_PipeBase.arx`）

继承 `TDbPipeBase`，字段容器 `+0xe8`（键 `TCH_CABLETRY`）。桥架自身 `dwgInFields` 仅读一个类型字段（≤1），核心字段（截面/标高/起终点）全在 `TDbPipeBase` 基类。源自 ObjectARX 3.0 SampleApps 电缆桥架示例。

---

## 五、通用序列化格式（各专业一致）

1. **加密**：实体固定字段经 `TgDataEncrypt` 加密 = XOR `0x05` + Base-26（仅 DWG 2004+ / 版本 ≥ 16）。
2. **序列化顺序**（`TEntityBase::DwgOutFields`）：版本类型字段 → 固定字段（加密）→ 对象列表 → 字段容器 → TDbXDataMap → GUID。
3. **字段容器**：建筑实体在 `+0xa0`（TEntityFields，main 流位编码）+ `+0xd0`（TDbXDataMap）；机电实体在 `+0xe8`（TDbXDataMap）。
4. **加密固定字段**：`TEntityBase` 的 `+0x08`/`+0x10` 两个 double（所有建筑实体共有，解密为坐标数字），与派生类自己的字段（如墙高 `+0x1A0`）是两回事。

---

## 六、逆向方法（可复现）

| 方法 | 用途 |
|---|---|
| `feature_find` 搜字段名 / getter 名 | 字段名是常量，getter/setter 引用它，函数名即语义 |
| `native_function_study` + assembly | getter 汇编 `movsd xmm0,[rcx+offset]` 直接给字段 offset |
| `GetXml` 的 `referenced_strings` | XML 标签即字段名（自描述实体） |
| `dwgInFields`/`dwgOutFields` 伪代码 | 序列化顺序 + 字段读写位置 |

证据归档于 Craft Anything session（`fnd_1` 系列 finding，证据 id `ev_*`）。
