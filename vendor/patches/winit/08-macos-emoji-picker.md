# macOS emoji and system insertion

This patch adapts source PR [PdfCraft #678](https://github.com/storytold/pdfcraft/pull/678)
and the emoji portion of upstream [winit #4749](https://github.com/rust-windowing/winit/pull/4749)
to the vendored winit 0.30.13 IME path. Text inserted outside a key press is
delivered as an IME commit when text input is enabled. The separate upstream
`selectedRange` behavior is intentionally excluded because its effect on CADCraft's
held-key/press-and-hold behavior is not established. The structural guard is
`apps/cadcraft/tests/winit_macos_emoji_picker.rs`.
