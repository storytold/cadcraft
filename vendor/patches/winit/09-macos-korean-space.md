# macOS Korean duplicate space

This patch adapts source PR [PdfCraft #679](https://github.com/storytold/pdfcraft/pull/679)
and the matching goal in [winit #4478](https://github.com/rust-windowing/winit/pull/4478)
to the vendored winit 0.30.13 IME commit state. A key whose text was committed by
IME in `keyDown:` is not emitted a second time as `KeyboardInput`; the per-key
marker is consumed after each key, and the existing command-forwarding override
remains in place. The structural guard is
`apps/cadcraft/tests/winit_macos_korean_space.rs`.
