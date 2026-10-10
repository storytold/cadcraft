# macOS first Apple Korean key

This patch adapts source PR [pdfcraft #680](https://github.com/storytold/pdfcraft/pull/680)
and upstream [winit #4744](https://github.com/rust-windowing/winit/pull/4744) to the
vendored winit 0.30.13 macOS IME path. It retries exactly one eligible raw
compatibility-jamo callback on the first Apple Korean key, with the native
state and focus guards from the reviewed patch. The structural guard is
`apps/cadcraft/tests/winit_macos_korean_first_key.rs`.
