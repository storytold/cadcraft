---
title: Export a drawing to PDF, SVG or PNG
description: Save a CADCraft drawing as a PDF with File Export to PDF or File Print, or as an SVG or PNG image with File Save As.
order: 30
verified: 0.6.2
---

To get a PDF quickly, choose **File ▸ Export to PDF...**. To pick the paper, the area and the scale first, choose **File ▸ Print...**. For an SVG or PNG image, choose **File ▸ Save As...** and pick the SVG or PNG file type. On macOS, use Cmd in place of Ctrl.

## Export to PDF

1. Open the drawing.
2. Choose **File ▸ Export to PDF...**.
3. In the dialog, pick a folder and a name. The suggested name is the drawing's name with `.pdf`.
4. Save. The command line reports `Plotted to` followed by the path.

## Print to a PDF with the Plot dialog

1. Open the drawing.
2. Choose **File ▸ Print...**. The shortcut is Ctrl+P. Typing `plot` or `print` at the command line does the same. The **Plot** window opens.
3. In **Layout**, pick **Model** or one of the drawing's layouts.
4. Pick the **Paper size**, **Orientation** and **Plot area**. With **Printer/plotter** set to **None**, the result is a PDF. For **Model**, the **Plot area** can be **Window**: click **Window <** and pick two corners in the drawing.
5. Click **Preview** to see the sheet, or **OK** to plot.
6. After **OK**, pick a folder and a name. The suggested name is the drawing's name, a dash and the layout name.

The command line reports `Plotted` followed by the layout name and the path. For a layout, **Apply to Layout** keeps the settings in that layout's page setup.

## Export to SVG or PNG

1. Open the drawing.
2. Choose **File ▸ Save As...**. The shortcut is Ctrl+Shift+S.
3. Pick the file type **SVG** or **PNG**, or type a name that ends in `.svg` or `.png`.
4. Save.

An export only writes the image or PDF. The drawing keeps its own name and still counts as unsaved if you had changes, so save it as DXF or DWG too.

## From the command line

`cadcraft-cli convert drawing.dxf drawing.svg` converts a drawing without opening the window.

## Related

- [Open and save DXF and DWG drawings](open-and-save-drawings.md)
