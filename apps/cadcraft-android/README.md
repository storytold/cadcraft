# CADCraft for Android (Experimental)

An experimental Android frontend for CADCraft.

The Android application uses Kotlin, Android WebView, and
WebViewAssetLoader to host CADCraft's Rust/WebAssembly frontend.

## Requirements

- Linux, macOS, or Windows development environment
- Rust toolchain
- wasm32-unknown-unknown Rust target
- Trunk
- Android SDK
- Android SDK platform and build tools required by the Gradle project
- JDK version compatible with the Android Gradle plugin
- Android device or emulator with WebView support

## Setup

Install the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
