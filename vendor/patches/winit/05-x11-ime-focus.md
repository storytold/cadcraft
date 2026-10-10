# X11 input-context focus

This patch adapts source PR [pdfcraft #675](https://github.com/storytold/pdfcraft/pull/675)
and upstream [winit #4727](https://github.com/rust-windowing/winit/pull/4727) to the
vendored winit 0.30.13 baseline. When egui replaces an active X11 input context,
the new context is focused after the request batch is processed. The structural
guard is `apps/cadcraft/tests/winit_x11_ime_focus.rs`.
