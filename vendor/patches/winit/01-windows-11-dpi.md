# Windows 11 DPI suggested rectangle

Backport of [winit#4341](https://github.com/rust-windowing/winit/pull/4341) to the published winit 0.30.13 source. Windows 11 uses the rectangle supplied by `WM_DPICHANGED`; Windows 10 keeps the released monitor-adjustment path. The structural guard is `apps/cadcraft/tests/winit_windows_11_dpi.rs`. This is source-level evidence; native Windows DPI behavior was not run on the macOS host.
