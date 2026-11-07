# 繁體中文（台灣）Windows 測試版

`Vik1n9/cadcraft` 是 CADCraft 的繁體中文測試分支，在
[storytold/cadcraft](https://github.com/storytold/cadcraft) 之上加入 `zh-TW` 介面。
這個頁面給母語 CAD 使用者試用 Windows 測試包時參考。

## 下載

到 **[Releases](https://github.com/Vik1n9/cadcraft/releases/latest)** 下載：

| 檔案 | 說明 |
|------|------|
| `cadcraft-<版本>-zhTW.<N>-windows-x64-portable.zip` | 解壓即用，不需系統管理員權限（建議） |
| `cadcraft-<版本>-zhTW.<N>-windows-x64.msi` | 安裝版，預設裝到 `C:\Program Files\CADCraft` |
| `SHA256SUMS.txt` | SHA-256 雜湊，可自行核對 |

系統需求：Windows 10 或 11，x64。程式靜態連結 C 執行期，不需另外安裝 VC++ 可轉散發套件。

## 這個版本有什麼

- 介面語言新增 **繁體中文（台灣）**（選單名稱：繁體中文），涵蓋選單、工具提示、指令提示、
  對話框與選項；用語順序以 AutoCAD 台灣官方術語為先，其次微軟台灣用語。
- Windows 測試包內嵌 **源流明體 GenRyuMin2 TW（月版）2.100**（Regular 與 Bold），
  繁中介面優先使用它，不必另外安裝字型；找不到時才用系統的微軟正黑體等字型。
- 其餘功能與 upstream `main` 相同。

## 開始使用

**解壓版**

1. 解壓縮到任意資料夾（例如 `D:\CADCraft-test`）。
2. 執行 `cadcraft.exe`。
3. 第一次啟動會出現「Windows 已保護您的電腦」，因為這是未簽章的測試建置：
   點「其他資訊」→「仍要執行」。

**安裝版**

1. 執行 `.msi`，依精靈指示安裝（同樣會遇到 SmartScreen，處理方式相同）。

## 切換成繁體中文

程式會跟隨系統語言；若介面不是中文：**編輯 ▸ 介面語言**（Edit ▸ Interface Language）→
選 **繁體中文**。設定會被記住。

## 請幫忙留意

- **術語**：圖面、圖塊、標註、圖層性質、掣點、物件鎖點、視埠、配置、剖面線、聚合線、
  雲形線、出圖等 AutoCAD 繁中慣用語是否正確、一致。
- **翻譯品質**：功能表、工具提示、指令提示、對話框、選項都要看；語意不通、簡繁混用或
  中國用語（例如「文件」應為「檔案」、「保存」應為「儲存」）請回報。
- **字型**：有沒有缺字方框（□）、字距怪異、文字被裁切。
- **版面**：文字變長後被截斷、按鈕擠在一起。
- 指令關鍵字（`Top`、`Zero`、`_tan to`、`nX`/`nXP`）刻意保持英文，與引擎接受的輸入一致。

## 回報方式

在 <https://github.com/Vik1n9/cadcraft/issues> 開 issue，標題加 `[zh-TW]`，附上：

1. 畫面截圖（`Win` + `Shift` + `S`）。
2. 操作步驟：哪個功能表／指令／對話框。
3. 目前文字與建議文字（知道 AutoCAD 繁中的說法更好）。
4. Windows 版本與介面語言。

## 已知限制

- 翻譯以 AutoCAD 繁中術語表為準則、AI 協助完成，尚未經母語者逐行校對。
- 測試建置未經程式碼簽章；正式版才會有簽章。
- 指令列、引擎診斷訊息與指令歷史仍為英文（目前設計）。
- 圖層名稱、圖塊名稱、圖面文字等使用者資料不翻譯。

## 授權與致謝

- 內嵌字型：**源流明體 GenRyuMin2 TW（月版）2.100**，作者 Zihi／ButTaiwan，
  SIL Open Font License 1.1，<https://github.com/ButTaiwan/genryu-font>。
  授權全文隨程式附上（`OFL-genryu-min-tw.txt`），字型檔不進本 repository
  （建置時由 workflow 下載並以 `CRAFT_FONTS_DIR` 內嵌）。
- CADCraft 為 Apache-2.0／MIT 雙授權，詳見
  [`LICENSE-MIT`](../../LICENSE-MIT)、[`LICENSE-APACHE`](../../LICENSE-APACHE)、`NOTICE`
  與 [`ATTRIBUTION.md`](../../ATTRIBUTION.md)。
