# macOS Korean duplicate space

This patch adapts source PR [pdfcraft #679](https://github.com/storytold/pdfcraft/pull/679)
and the matching goal in [winit #4478](https://github.com/rust-windowing/winit/pull/4478)
to the vendored winit 0.30.13 IME commit state. A space already committed by
Apple Korean is not emitted a second time as keyboard text, while command
forwarding remains available. The structural guard is
`apps/cadcraft/tests/winit_macos_korean_space.rs`.
