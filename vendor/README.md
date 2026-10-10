# Vendored dependencies

This directory contains published third-party crates with narrowly scoped local
patches. Each crate keeps its upstream license and provenance in its own
directory.

| Crate | Version | License | Local patch | Regression evidence |
|---|---|---|---|---|
| winit | 0.30.13 | Apache-2.0 | Windows 11 uses the `WM_DPICHANGED` rectangle suggested by the OS; Windows 10 retains the released adjustment path. Backport of [winit #4341](https://github.com/rust-windowing/winit/pull/4341). | `apps/cadcraft/tests/winit_windows_11_dpi.rs` |
