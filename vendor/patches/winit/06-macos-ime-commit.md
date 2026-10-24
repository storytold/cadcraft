# macOS IME commit state

This patch adapts source PR [PdfCraft #676](https://github.com/storytold/pdfcraft/pull/676)
and upstream [winit #4650](https://github.com/rust-windowing/winit/pull/4650) to the
vendored winit 0.30.13 baseline. It preserves commits that follow cleared marked
text. The input-method commit-state handling also accounts for commits outside a
key press ([winit #4748](https://github.com/rust-windowing/winit/pull/4748)) so the
next ordinary key is not swallowed. The structural guard is
`apps/cadcraft/tests/winit_macos_ime_commit.rs`.
