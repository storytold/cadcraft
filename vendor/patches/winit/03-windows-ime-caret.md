# Windows Korean IME caret

This patch adapts source PR [pdfcraft #673](https://github.com/storytold/pdfcraft/pull/673)
and upstream [winit #4746](https://github.com/rust-windowing/winit/pull/4746) to the
vendored winit 0.30.13 baseline. When the Korean IME does not report a composition
cursor, the caret is placed after the composed text. The structural guard is
`apps/cadcraft/tests/winit_windows_ime_cursor.rs`.
