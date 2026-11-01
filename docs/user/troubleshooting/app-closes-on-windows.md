---
title: The app closes right after opening on Windows
description: Fix CADCraft on Windows 11 closing a moment after its window appears, by updating to 0.6 or later or starting it with another graphics backend.
order: 10
verified: 0.6.2
---

If CADCraft opens a window and closes again within a second, [install the latest release](../getting-started/install.md). CADCraft 0.5.0 had this problem on some PCs, and 0.6.0 and later fix it. If it still happens, start CADCraft with the Vulkan graphics backend as shown below.

## Symptoms

- The window flashes, or appears for a moment, and the app quits.
- `%APPDATA%\CADCraft\logs\cadcraft.log` contains `Exiting because of error: WGPU error: Parent device is lost`.

## Fix

1. Check the version in **Help ▸ About CADCraft**, or in the file name you downloaded. If it is older than 0.6.0, [install the latest release](../getting-started/install.md).
2. If the latest release also closes right away, open PowerShell in the folder that contains `cadcraft.exe`.
3. Run these three lines:

        $env:WGPU_BACKEND = "vulkan"
        & .\cadcraft.exe
        Remove-Item Env:WGPU_BACKEND

The last line clears the setting so later launches use the default again. You need the first two lines each time you start CADCraft this way.

On Intel graphics (UHD, Iris), use `gl` in place of `vulkan`. Intel's Vulkan driver (`igvk64.dll`) has crashed CADCraft at start ([issue #33](https://github.com/storytold/cadcraft/issues/33)).

On AMD graphics do not use `gl`: it can bring back the `atio6axx.dll` driver crash.

## Why this happens

CADCraft draws with DirectX 12 on Windows. CADCraft 0.5.0 compiled its shaders with DXC, and the 0.5.0 Windows build does not ship the DXC compiler files (`dxcompiler.dll` and `dxil.dll`). On some PCs the graphics device was lost before the first frame and the app exited. This is issue [#608](https://github.com/storytold/cadcraft/issues/608).

CADCraft 0.6.0 and later use FXC instead, the shader compiler built into Windows ([pull request #613](https://github.com/storytold/cadcraft/pull/613)).

If the graphics device fails while CADCraft is already running, 0.6.0 and later keep the app open. A **Graphics error** window says what happened and whether recovery copies of your unsaved drawings were saved. Save your work and restart CADCraft.

## Still stuck?

Follow [Report a problem](index.md#report-a-problem) and attach `cadcraft.log`. Mention your graphics card.
