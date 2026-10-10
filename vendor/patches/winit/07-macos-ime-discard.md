# macOS stale composition discard

This patch adapts source PR [pdfcraft #677](https://github.com/storytold/pdfcraft/pull/677)
and upstream [winit #4745](https://github.com/rust-windowing/winit/pull/4745) to the
vendored winit 0.30.13 macOS IME path. Disabling IME clears the native marked
text before the next field can receive it. The structural guard is
`apps/cadcraft/tests/winit_macos_ime_discard.rs`.
