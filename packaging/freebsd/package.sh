#!/usr/bin/env bash
# Build and package CADCraft for FreeBSD:
#
#   $DIST/cadcraft-<version>-freebsd-<arch>.tar.gz   a /usr/local-style tree (<arch> is x86_64, or
#       aarch64 when CARGO_BUILD_TARGET=aarch64-unknown-freebsd):
#       cadcraft-<version>-freebsd-<arch>/{bin, man/man1, share/applications, share/icons,
#       share/mime, share/metainfo, share/doc/cadcraft, share/{bash-completion,zsh,fish}}
#
# Install by copying the tree's contents into /usr/local, e.g.
#   tar -xzf cadcraft-*-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local
#
# Usage: packaging/freebsd/package.sh [--skip-build] [--dry-run]
#   --dry-run  print the resolved target, archive and cargo commands, then exit without building
#
# Needs: bash, cargo and the build dependencies listed in .github/workflows/freebsd.yml. FreeBSD's
# install(1) has no -D, so directories are created first.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
LINUX="$ROOT/packaging/linux"
APP_ID=ai.storyteller.cadcraft

SKIP_BUILD=0
DRY_RUN=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# FreeBSD calls x86_64 "amd64"; release names use the Rust/Linux spelling.
release_arch() {
  case "$1" in
    amd64 | x86_64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) echo "unsupported architecture $1" >&2; return 2 ;;
  esac
}
# The release workflow cross-compiles aarch64 in an x86_64 VM with CARGO_BUILD_TARGET set to
# aarch64-unknown-freebsd (craft-common-actions#39): name the archive after the target.
# host-tuple means the build host's triple, as rustc names it.
if [[ "${CARGO_BUILD_TARGET:-}" == host-tuple ]]; then
  CARGO_BUILD_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
fi
case "${CARGO_BUILD_TARGET:-}" in
  "") ARCH="$(release_arch "$(uname -m)")" ;;
  aarch64-unknown-freebsd | x86_64-unknown-freebsd) ARCH="$(release_arch "${CARGO_BUILD_TARGET%%-*}")" ;;
  *) echo "CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET is not a supported FreeBSD target" >&2; exit 2 ;;
esac
# Only binaries for this machine run here (the --version check at the end); a FreeBSD target built
# on another OS never does.
HOST_ARCH="$(release_arch "$(uname -m)" 2>/dev/null)" || HOST_ARCH="$(uname -m)"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]] && [[ "$(uname -s)" != FreeBSD ]]; then
  HOST_ARCH="$(uname -s) $HOST_ARCH"
fi
BASENAME="cadcraft-$VERSION-freebsd-$ARCH"

echo "==> CADCraft $VERSION for FreeBSD $ARCH"

# cadcraft-cli's man-and-completions example is built on its own, not with the binaries: its
# dev-dependencies would otherwise be unified into them (packaging/linux/package.sh). It runs on
# this machine, so a cross build builds it without the target.
CARGO_BUILD_FLAGS=(--release --locked ${CARGO_BUILD_TARGET:+--target "$CARGO_BUILD_TARGET"})
if [[ "$DRY_RUN" = 1 ]]; then
  echo "dry run: archive $DIST/$BASENAME.tar.gz"
  echo "dry run: (cd $ROOT && cargo build ${CARGO_BUILD_FLAGS[*]} -p cadcraft -p cadcraft-cli)"
  echo "dry run: (cd $ROOT && env -u CARGO_BUILD_TARGET cargo build --release --locked -p cadcraft-cli --example man-and-completions)"
  exit 0
fi
if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build "${CARGO_BUILD_FLAGS[@]}" -p cadcraft -p cadcraft-cli)
  (cd "$ROOT" && env -u CARGO_BUILD_TARGET cargo build --release --locked -p cadcraft-cli --example man-and-completions)
fi
# Cargo puts a cross build under target/<triple>/release.
BIN="$CARGO_TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release"
WORK="$CARGO_TARGET_DIR/freebsd-package"
STAGE="$WORK/$BASENAME"
rm -rf "$WORK"

# ---- stage a /usr/local-style tree ---------------------------------------------------------------
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$STAGE/share/mime/packages" "$STAGE/share/metainfo" \
  "$STAGE/share/icons" "$STAGE/share/doc/cadcraft"
install -m 755 "$BIN/cadcraft" "$BIN/cadcraft-cli" "$STAGE/bin/"
strip "$STAGE/bin/cadcraft" "$STAGE/bin/cadcraft-cli" 2>/dev/null || true
# The desktop entry, MIME type and metainfo are the freedesktop files the Linux packages use.
install -m 644 "$LINUX/$APP_ID.desktop" "$STAGE/share/applications/$APP_ID.desktop"
install -m 644 "$LINUX/$APP_ID.mime.xml" "$STAGE/share/mime/packages/$APP_ID.xml"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$CADCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$STAGE/share/metainfo/$APP_ID.metainfo.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/share/icons/"
copy_docs "$STAGE/share/doc/cadcraft"
# cadcraft-cli's man pages and shell completions, from its clap definition, written by the
# example built above.
CLI_DOCS="$CARGO_TARGET_DIR/release/examples/man-and-completions"
if [[ ! -x "$CLI_DOCS" ]]; then
  echo "error: $CLI_DOCS is missing: build it with the binaries (without --skip-build)" >&2
  exit 1
fi
"$CLI_DOCS" "$WORK/cli-docs"
mkdir -p "$STAGE/man/man1" "$STAGE/share/bash-completion/completions" \
  "$STAGE/share/zsh/site-functions" "$STAGE/share/fish/vendor_completions.d"
install -m 644 "$WORK"/cli-docs/*.1 "$STAGE/man/man1/"
install -m 644 "$WORK/cli-docs/cadcraft-cli.bash" "$STAGE/share/bash-completion/completions/cadcraft-cli"
install -m 644 "$WORK/cli-docs/_cadcraft-cli" "$STAGE/share/zsh/site-functions/_cadcraft-cli"
install -m 644 "$WORK/cli-docs/cadcraft-cli.fish" "$STAGE/share/fish/vendor_completions.d/cadcraft-cli.fish"

# ---- .tar.gz -------------------------------------------------------------------------------------
mkdir -p "$DIST"
tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"

if [[ -z "${CARGO_BUILD_TARGET:-}" ]] && [[ "$ARCH" = "$HOST_ARCH" ]]; then
  "$STAGE/bin/cadcraft-cli" --version
else
  # They cannot run here, so check what they are instead: executables, not libraries or objects.
  # An older file(1) calls a PIE a shared object; its interpreter still marks it as a program.
  want="$ARCH"; [[ "$ARCH" != x86_64 ]] || want=x86-64
  for b in "$STAGE"/bin/*; do
    case "$(file -b "$b")" in
      ELF*" executable, "*"$want"*FreeBSD* | ELF*"$want"*FreeBSD*interpreter*) ;;
      *) echo "error: $b is not a FreeBSD $ARCH executable: $(file -b "$b")" >&2; exit 1 ;;
    esac
  done
  echo "skipping --version: a CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET build does not run on this $HOST_ARCH machine"
fi
echo "==> done"
ls -lh "$DIST"
