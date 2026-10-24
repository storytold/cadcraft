---
title: Manage layers
description: Create, rename, switch and hide layers in the CADCraft Layer Properties Manager.
order: 40
verified: 0.6.2
---

Layers are managed in the **Layer Properties Manager**. Open it with **Window ▸ Layers**.

The **Layers** palette at the side of the window also lists the layers. If you closed it, choose **Window ▸ Layers Palette**.

## Create a layer and make it current

1. Choose **Window ▸ Layers**.
2. Click the plus button (tooltip **New Layer**). A layer named `Layer1` appears with its name ready to edit. Type a new name and press Enter.
3. Select the layer and click the button with the tooltip **Set Current**, or right-click the layer and choose **Set Current**.

The line at the bottom of the window shows how many layers are displayed and which layer is current.

## Hide, freeze, lock or stop plotting a layer

In the layer table, each layer has these columns: **Status**, **Name**, **On**, **Freeze**, **Lock**, **Plot**, **Color**, **Linetype**, **Lineweight**, **Transparency**, **New VP Freeze** and **Description**. In a layout with an active viewport, **VP Freeze** and **VP Color** columns appear too. Click the icon in a column to change that setting for the layer.

- **On**: shows or hides the layer.
- **Freeze**: freezes or thaws it.
- **Lock**: locks it against edits.
- **Plot**: sets whether it is plotted.

## Rename or delete a layer

1. Right-click the layer name. Double-clicking the name also starts a rename.
2. Choose **Rename Layer** or **Delete Layer**.

The layers `0` and `Defpoints` cannot be renamed.

## Save and restore layer settings

The window has a **Layer State** area. Type a name in the **New state name** box and click **Save State**. Later, pick the state in the list and click **Restore**.

## Related

- [Draw your first shapes at the command line](draw-with-the-command-line.md)
