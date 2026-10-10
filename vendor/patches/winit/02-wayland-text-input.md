# Wayland text input lifecycle

This patch adapts source PR [pdfcraft #672](https://github.com/storytold/pdfcraft/pull/672)
and upstream [winit #4747](https://github.com/rust-windowing/winit/pull/4747) to the
vendored winit 0.30.13 baseline. Text input belongs to the Wayland seat, so it is
destroyed when the seat is removed rather than whenever an unrelated capability is
removed. The structural guard is `apps/cadcraft/tests/winit_wayland_text_input.rs`.
