#!/usr/bin/env bash
# Validate the staged Arch launcher without building or opening CADCraft.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/source"
dst="$work/dist"
base=cadcraft-0.0.1-nightly.1-linux-x86_64
mkdir -p "$stage/bin" "$dst" "$work/extract"

# A local argv/env recorder stands in for the compiled binaries. It never opens a UI.
cat >"$stage/bin/cadcraft" <<'RECORDER'
#!/bin/sh
printf 'config=<%s>\n' "${CADCRAFT_CONFIG_DIR-<unset>}"
for arg in "$@"; do
  printf '<%s>\n' "$arg"
done
RECORDER
chmod 755 "$stage/bin/cadcraft"
cp "$stage/bin/cadcraft" "$stage/bin/cadcraft-cli"

bash "$HERE/nightly-arch-tar.sh" "$stage" "$work/stage" "$dst" "$base"
tar -xzf "$dst/$base.tar.gz" -C "$work/extract"
bin="$work/extract/$base/bin"
# Simulate the existing Arch bin-tree copy and public launcher rename.
mkdir -p "$work/installed"
cp -R "$bin" "$work/installed/bin"
bin="$work/installed/bin"
mv "$bin/cadcraft" "$bin/cadcraft-nightly"
mv "$bin/cadcraft-cli" "$bin/cadcraft-nightly-cli"

test ! -e "$stage/bin/.cadcraft-nightly"
cmp "$stage/bin/cadcraft" "$bin/.cadcraft-nightly/cadcraft"
cmp "$stage/bin/cadcraft-cli" "$bin/cadcraft-nightly-cli"
export HOME="$work/home with space"
unset XDG_DATA_HOME CADCRAFT_CONFIG_DIR
expect() {
  expected="$1"
  shift
  actual=$(PATH="$bin:$PATH" cadcraft-nightly "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}
expect "config=<$HOME/.local/share/cadcraft-nightly>"
export XDG_DATA_HOME="$work/xdg with space"
expect "$(printf 'config=<%s>\n<%s>\n<%s>\n' "$XDG_DATA_HOME/cadcraft-nightly" 'drawing with spaces.dxf' '--other-name.dxf')" \
  'drawing with spaces.dxf' '--other-name.dxf'
expect "$(printf 'config=<<unset>>\n<--version>\n')" --version
expect "$(printf 'config=<<unset>>\n<-V>\n')" -V
CADCRAFT_CONFIG_DIR="$work/explicit with space" expect "$(printf 'config=<%s>\n<%s>\n' "$work/explicit with space" 'sample.dxf')" \
  sample.dxf

echo "PASS: Arch Nightly handoff isolates the profile and preserves launch arguments"
