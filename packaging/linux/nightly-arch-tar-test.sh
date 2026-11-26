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
printf 'config=<%s>\n' "${CADCRAFT_CONFIG_DIR:-}"
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
# The launcher names the profile the flavor's way and keeps every argument after the delimiter.
launch() {
  local profile="$1"
  shift
  printf 'config=<>\n<--launch-profile>\n<nightly-v1>\n<--profile-root>\n<%s>\n<-->\n' "$profile"
  for file in "$@"; do printf '<%s>\n' "$file"; done
}
expect "$(launch "$HOME/.local/share/cadcraft-nightly" 'sample.dxf')" sample.dxf
export XDG_DATA_HOME="$work/xdg with space"
expect "$(launch "$XDG_DATA_HOME/cadcraft-nightly" 'drawing with spaces.dxf' '--other-name.dxf')" \
  'drawing with spaces.dxf' '--other-name.dxf'
# A version query answers with the plain argument, so it needs no profile at all.
expect "$(printf 'config=<>\n<--version>\n')" --version
expect "$(printf 'config=<>\n<-V>\n')" -V
# The Launcher's own form is passed through once, never doubled.
expect "$(printf 'config=<>\n<--launch-profile>\n<nightly-v1>\n<--profile-root>\n<%s>\n<--version>\n' "$work/launcher root")" \
  --launch-profile nightly-v1 --profile-root "$work/launcher root" --version
# A CADCRAFT_CONFIG_DIR from an older wrapper is neither honoured nor rewritten: the profile is the
# launcher's, and the flavor refuses a variable outside it.
CADCRAFT_CONFIG_DIR="$work/explicit with space" \
  expect "$(launch "$XDG_DATA_HOME/cadcraft-nightly" 'sample.dxf' | sed '1s|.*|config=<'"$work"'/explicit with space>|')" \
  sample.dxf

echo "PASS: Arch Nightly handoff starts the flavor's profile and preserves launch arguments"
