#!/usr/bin/env bash
set -euo pipefail

ANDROID_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$ANDROID_DIR/../.." && pwd)"

WEB_DIR="$REPO_ROOT/apps/cadcraft-web"
WEB_DIST="$REPO_ROOT/dist/web"
ANDROID_ASSETS="$ANDROID_DIR/app/src/main/assets/web"

echo "==> Building CADCraft WebAssembly frontend"
cd "$WEB_DIR"
trunk build --release

test -f "$WEB_DIST/index.html" || {
    echo "ERROR: Missing WebAssembly frontend build."
    exit 1
}

echo "==> Copying frontend to Android assets"
rm -rf "$ANDROID_ASSETS"
mkdir -p "$ANDROID_ASSETS"
cp -a "$WEB_DIST/." "$ANDROID_ASSETS/"

echo "==> Building Android APK"
cd "$ANDROID_DIR"
./gradlew assembleDebug

echo
echo "Build complete:"
echo "$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
