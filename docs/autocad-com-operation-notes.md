# AutoCAD COM 操作范式笔记（对照 CADCraft 天正实现）

来源：PKPM Agent 安装盘的明文脚本
`D:/Program Files/PKPM Agent/resources/backend/PKPMAgent/skills/PKPM-Checker/scripts/`
下的 `mulckdata_cad.py`（776 行）与 `pkpm_json_cad.py`（811 行）。两者底层
COM 操作函数几乎相同（共享约 90%），差异只在数据源（MulCkDB / CheckList JSON）
与梁柱判定逻辑。本文提炼范式，不复制 PKPM 代码。

## 一、两条路线：文件级 vs 进程级

| | CADCraft（已有） | PKPM Agent（参考） |
|---|---|---|
| 层级 | **文件级**：Rust 直接解析/写 DWG 二进制 | **进程级**：win32com 驱动运行中的 AutoCAD |
| 天正实体 | ✅ 读私有数据（TCH_WALL 字段、proxy graphics 类型 6/33/38） | ❌ COM 的 `IAcadEntity` 读不到 ARX 私有数据 |
| 依赖 | 无 AutoCAD，离线 | 必须装 AutoCAD/浩辰/中望且运行 |
| 画图 | 需自写几何→DWG 序列化 | 直接调 `ModelSpace.Add*`，快 |
| 适合 | 解析/改造天正图纸、批量离线上色/渲染 | 在真实 CAD 里交互式画普通图元、校审批注 |

**核心结论**：天正实体（ObjectARX 自定义类）**没有** COM 接口暴露，PKPM Agent
的 AutoCAD-Link 只画普通图元（线/文字/标注/图层），不碰天正私有数据。所以
「操作天正」的正路仍是文件级——CADCraft 已实现，这块领先于 PKPM Agent。

## 二、AutoCAD COM 操作范式（win32com 直驱，非 pyautocad）

### 1. 连接 / 启动 AutoCAD

```python
pythoncom.CoInitialize()
try:
    acad = win32com.client.GetActiveObject("AutoCAD.Application")  # 连已运行的实例
except Exception:
    acad = win32com.client.DispatchEx("AutoCAD.Application")       # 无则启动新实例
acad.Visible = True
acad.WindowState = 3          # 最大化
```

就绪判定：轮询 `acad.Documents.Count` 直到不抛异常（超时默认 45s）。

### 2. 3D 坐标点必须包成 COM 安全数组

```python
def acad_point(x, y, z=0.0):
    return win32com.client.VARIANT(
        pythoncom.VT_ARRAY | pythoncom.VT_R8, [float(x), float(y), float(z)])
```

所有 `AddLine/AddText/AddCircle/AddMText` 的坐标参数都要这样包，直接传 list 会错。

### 3. 取消挂起命令（COM 操作 CAD 的头号坑）

CAD 停在命令提示符（`CMDNAMES` 非空）时 COM 调用会失败。开图/画图前先取消：

```python
if str(doc.GetVariable("CMDNAMES") or "").strip():
    doc.SendCommand("\x03\x03")   # 发两次 ESC；备选 "\x1b\x1b"
```

### 4. 开图 / 切换文档

- 枚举已开文档：`acad.Documents`（`for doc in docs` 或 `docs.Item(idx)`）
- 按路径匹配：`doc.FullName`（或 `doc.Path`+`doc.Name`），规范化后 casefold 比较
- 已开则 `doc.Activate()`，否则 `acad.Documents.Open(path)`（**不用** `SendCommand("OPEN")`）
- 绘图空间 = `doc.ModelSpace`

### 5. 图层 / 文字样式（幂等 ensure）

```python
try:    layer = doc.Layers.Item(name)
except: layer = doc.Layers.Add(name)
layer.Color = 6    # ACI 6 = 洋红

try:    style = doc.TextStyles.Item(name)
except: style = doc.TextStyles.Add(name)
style.FontFile = "pkpmeng.shx"        # 西文 SHX
style.BigFontFile = "pkpmchn.shx"     # 中文大字体 SHX（CJK 必需）
style.WidthFactor = 0.7
style.ObliqueAngle = 0.0
doc.ActiveLayer = layer
doc.ActiveTextStyle = style
```

### 6. 画图元（ModelSpace.Add*）

| 图元 | 调用 | 备注 |
|---|---|---|
| 直线 | `ms.AddLine(p1, p2)` | 两点 VARIANT |
| 圆 | `ms.AddCircle(center, radius)` | |
| 单行文字 | `ms.AddText(text, point, height)` | 再设 `obj.Rotation` |
| 多行文字 | `ms.AddMText(point, width, text)` | 再设 `Height`/`AttachmentPoint`(1=左上)/`LineSpacingStyle`/`LineSpacingDistance` |
| 轻量多段线 | `ms.AddLightWeightPolyline(array.array("d", [x0,y0,x1,y1,...]))` | 再设 `obj.Closed = True` |

每个图元创建后统一收尾：设 `Layer`/`Color`/`StyleName`/`Rotation` + `obj.Update()`。

### 7. 单位换算

DB/JSON 里的坐标是米，CAD 是毫米 → 坐标 `×1000`（`--coordinate-scale` 默认
1000）。这与天正真实图纸「建筑坐标 mm」一致（CADCraft 里 type 6/33/38 图元坐标
也是 mm，如 `X=137026.85`）。

## 三、引线批注的几何套路（可直接复用）

`add_oriented_note(mark, direction, text)`：从标记点沿方向画引线 `AddLine`，
端点处放 MText（AttachmentPoint=左上，旋转角 = 引线方向 −90°，文字基线垂直于
引线）。梁标注取「左右点连线的法线」（统一朝右/上，避免标到远处），柱标注取
竖直方向。同点批注按坐标容差合并成多行文字。

## 四、可组合路线（AI 操作天正的落地方案）

1. **读天正** → CADCraft（文件级，已实现）：解析 DWG 天正实体 → 几何/字段。
2. **改天正** → CADCraft（文件级，已实现）：改字段/墙厚/几何 → 写回 DWG。
3. **在真实 CAD 里展示/画图** → 可选接 AutoCAD COM（进程级，本笔记范式）：
   CADCraft 解析出的几何/文字 → `ModelSpace.Add*` 画进 AutoCAD，或做校审批注。

三者互补：CADCraft 负责「天正私有数据的读改写」，AutoCAD COM 负责「在真实
CAD 界面里画普通图元」。天正自定义实体本身没有 COM 通道，不可用进程级操作。

## 五、参考文件清单

- `PKPM-Checker/scripts/mulckdata_cad.py` — DB 校审批注画图（最完整范式）
- `PKPM-Checker/scripts/pkpm_json_cad.py` — JSON 校审批注画图（同范式，JSON 数据源）
- `PKPM-Checker/scripts/mulckdata_query.py` / `pkpm_json_query.py` — 数据查询（非 CAD）
- `PKPM-Checker/scripts/pkpm_check_drawing.py` — 调外部 `AutoRecDwg.exe` 校审（非 COM）
- `local_packages/pyautocad/` — 公开库（AutoCAD COM 更高层封装，MIT）
- `local_packages/comtypes_utf8.py` — comtypes 中文编码补丁
