#!/usr/bin/env bash
# Smoke test for packaging/linux/AppRun: it launches the app with its arguments, and
# `cli <args>` runs cadcraft-cli instead.
#
# Runs in seconds with no build: packaging/linux/apprun-test.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail() { echo "apprun-test: $*" >&2; exit 1; }

APPDIR="$TMP/CADCraft.AppDir"
mkdir -p "$APPDIR/usr/bin"
cp "$HERE/AppRun" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
printf '#!/bin/sh\necho "stub: $*"\n' >"$APPDIR/usr/bin/cadcraft"
chmod +x "$APPDIR/usr/bin/cadcraft"

# 1. The app gets every argument.
out="$(env -u APPDIR "$APPDIR/AppRun" drawing.dxf --control 7979)" || fail "app launch exited non-zero"
[ "$out" = "stub: drawing.dxf --control 7979" ] || fail "app arguments not forwarded: $out"

# 2. APPDIR from the AppImage runtime wins over the script's own location.
out="$(APPDIR="$APPDIR" "$APPDIR/AppRun" x)" || fail "launch with APPDIR set exited non-zero"
[ "$out" = "stub: x" ] || fail "APPDIR launch: $out"

# 3. `cli <args>` runs cadcraft-cli with the remaining arguments.
out="$(env -u APPDIR "$APPDIR/AppRun" cli mcp --connect 127.0.0.1:7979 2>&1)" && fail "cli ran without cadcraft-cli in the AppDir"
[ "$out" = "cadcraft: this AppImage was built without cadcraft-cli" ] || fail "missing cli: $out"
printf '#!/bin/sh\necho "cli stub: $*"\n' >"$APPDIR/usr/bin/cadcraft-cli"
chmod +x "$APPDIR/usr/bin/cadcraft-cli"
out="$(env -u APPDIR "$APPDIR/AppRun" cli mcp --connect 127.0.0.1:7979)" || fail "cli launch exited non-zero"
[ "$out" = "cli stub: mcp --connect 127.0.0.1:7979" ] || fail "cli arguments not forwarded: $out"
out="$(env -u APPDIR "$APPDIR/AppRun" cli)" || fail "bare cli launch exited non-zero"
[ "$out" = "cli stub: " ] || fail "bare cli output: $out"

# 4. Only the first argument selects the CLI.
out="$(env -u APPDIR "$APPDIR/AppRun" drawing.dxf cli)" || fail "app launch with a later cli argument exited non-zero"
[ "$out" = "stub: drawing.dxf cli" ] || fail "later cli argument hijacked the launch: $out"

echo "AppRun ok"
