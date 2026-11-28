#!/usr/bin/env bash
# Build and package CADCraft for Linux (<arch> is x86_64 or aarch64, or riscv64 when cross-compiling):
#
#   $DIST/cadcraft-<version>-linux-<arch>.AppImage  any distro with glibc >= the build host's
#   $DIST/cadcraft-<version>-linux-<arch>.AppImage.zsync  delta updates (needs zsyncmake)
#   $DIST/cadcraft-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/cadcraft-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, ...
#   $DIST/cadcraft-<version>-linux-<arch>.tar.gz    plain FHS-style tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"] [--arch-tar-dir DIRECTORY]
#
# Needs: cargo; nfpm for deb/rpm (https://nfpm.goreleaser.com); appimagetool for the AppImage
# (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu 22.04,
# glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate, appstreamcli,
# zsyncmake (the zsync package) for the AppImage's .zsync.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=ai.storyteller.cadcraft

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
ARCH_TAR_DIR=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    --arch-tar-dir)
      [[ -n "${2:-}" ]] || { echo "error: --arch-tar-dir needs a directory" >&2; exit 2; }
      ARCH_TAR_DIR="$2"
      shift 2
      ;;
    -h | --help) sed -n '2,17p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# Cross builds (CI's riscv64 job) set CROSS_ARCH, CROSS_TARGET (the Rust target), CROSS_COMPILE
# (the binutils prefix, for strip) and optionally EMULATOR (e.g. qemu-riscv64, for the smoke test).
ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  riscv64) DEB_ARCH=riscv64 ;;
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export CADCRAFT_MAINTAINER="${CADCRAFT_MAINTAINER:-CADCraft maintainers <cadcraft@storyteller.ai>}"
IS_NIGHTLY=0
if [[ "${CADCRAFT_BUILD_CHANNEL:-}" == nightly-v1 || "$VERSION" == *-nightly* ]]; then
  IS_NIGHTLY=1
  export CADCRAFT_BUILD_CHANNEL=nightly-v1
  export CADCRAFT_OVERRIDE_VERSION="$VERSION"
fi
BASENAME="cadcraft-$VERSION-linux-$ARCH"

if [[ -n "$ARCH_TAR_DIR" && "$IS_NIGHTLY" != 1 ]]; then
  echo "error: --arch-tar-dir is only for Nightly builds" >&2
  exit 2
fi

echo "==> CADCraft $VERSION for Linux $ARCH ($FORMATS, channel=${CADCRAFT_BUILD_CHANNEL:-stable})"

# The man-and-completions example writes cadcraft-cli's man pages and shell completions below. It
# is built in its own cargo run: built with the binaries, its dev-dependencies' features would be
# unified into them.
if [[ "$SKIP_BUILD" = 0 ]]; then
  if [[ -n "${CROSS_TARGET:-}" ]]; then
    (cd "$ROOT" && cargo build --release --locked -p cadcraft -p cadcraft-cli --target "$CROSS_TARGET")
    (cd "$ROOT" && cargo build --release --locked -p cadcraft-cli --example man-and-completions --target "$CROSS_TARGET")
  else
    (cd "$ROOT" && cargo build --release --locked -p cadcraft -p cadcraft-cli)
    (cd "$ROOT" && cargo build --release --locked -p cadcraft-cli --example man-and-completions)
  fi
fi
BIN="$CARGO_TARGET_DIR/${CROSS_TARGET:+$CROSS_TARGET/}release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/cadcraft" "$STAGE/usr/bin/cadcraft"
install -Dm755 "$BIN/cadcraft-cli" "$STAGE/usr/bin/cadcraft-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/cadcraft" "$STAGE/usr/bin/cadcraft-cli" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
if [[ "$IS_NIGHTLY" = 1 ]]; then
  sed -e 's|^StartupWMClass=.*|StartupWMClass=ai.storyteller.cadcraft.nightly-v1|' \
      "$STAGE/usr/share/applications/$APP_ID.desktop" >"$STAGE/usr/share/applications/$APP_ID.desktop.tmp"
  mv "$STAGE/usr/share/applications/$APP_ID.desktop.tmp" "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$CADCRAFT_BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/cadcraft"
copy_docs "$STAGE/usr/share/doc/cadcraft"
# cadcraft-cli's man pages and shell completions, from its clap definition, written by the
# example built above. A cross build runs it through $EMULATOR, like the --version check at the
# end, and leaves them out without one, as a --skip-build run whose prebuilt binaries came without
# the example does. zsh reads site-functions on Fedora and openSUSE.
CLI_DOCS="$BIN/examples/man-and-completions"
if [[ -n "${CROSS_TARGET:-}" ]] && [[ -z "${EMULATOR:-}" ]]; then
  warn "no EMULATOR to run the $CROSS_TARGET build, so cadcraft-cli's man pages and completions are left out"
elif [[ ! -x "$CLI_DOCS" ]]; then
  warn "$CLI_DOCS is not built, so cadcraft-cli's man pages and completions are left out"
else
  if [[ -n "${CROSS_TARGET:-}" ]]; then
    "$EMULATOR" "$CLI_DOCS" "$WORK/cli-docs"
  else
    "$CLI_DOCS" "$WORK/cli-docs"
  fi
  install -Dm644 -t "$STAGE/usr/share/man/man1" "$WORK"/cli-docs/*.1
  install -Dm644 "$WORK/cli-docs/cadcraft-cli.bash" "$STAGE/usr/share/bash-completion/completions/cadcraft-cli"
  install -Dm644 "$WORK/cli-docs/_cadcraft-cli" "$STAGE/usr/share/zsh/site-functions/_cadcraft-cli"
  install -Dm644 "$WORK/cli-docs/cadcraft-cli.fish" "$STAGE/usr/share/fish/vendor_completions.d/cadcraft-cli.fish"
fi

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { local format="$1"; case " $FORMATS " in *" $format "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz ------------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# The Arch handoff keeps the same basename as the plain tarball in a separate directory.
# Flatpak, OCI and direct tar consumers still get the original binary, not this launcher.
if [[ -n "$ARCH_TAR_DIR" ]]; then
  mkdir -p "$ARCH_TAR_DIR"
  if [[ "$(cd "$ARCH_TAR_DIR" && pwd -P)" == "$(cd "$DIST" && pwd -P)" ]]; then
    echo "error: the Arch tar directory must differ from the release directory" >&2
    exit 2
  fi
  bash "$HERE/nightly-arch-tar.sh" "$STAGE/usr" "$WORK/arch-tar" "$ARCH_TAR_DIR" "$BASENAME"
fi

# ---- .deb / .rpm --------------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export VERSION
  export NFPM_ARCH="$DEB_ARCH"
  NFPM_STAGE="$STAGE"
  export NFPM_NAME="cadcraft"
  export NFPM_BIN="cadcraft"
  export NFPM_CLI="cadcraft-cli"
  if [[ "$IS_NIGHTLY" = 1 ]]; then
    NIGHTLY_DESK_ID="$APP_ID.nightly-v1"
    export NFPM_NAME="cadcraft-nightly"
    export NFPM_BIN="cadcraft-nightly"
    export NFPM_CLI="cadcraft-nightly-cli"
    NFPM_STAGE="$WORK/nfpm-root"
    rm -rf "$NFPM_STAGE"
    cp -R "$STAGE" "$NFPM_STAGE"
    install -Dm755 "$NFPM_STAGE/usr/bin/cadcraft" "$NFPM_STAGE/usr/lib/cadcraft-nightly/cadcraft"
    cat >"$NFPM_STAGE/usr/bin/$NFPM_BIN" <<'WRAPPER'
#!/bin/sh
# Start the Nightly GUI the flavor's way: this channel's own launch profile, so an installed
# nightly never reads or writes stable's settings, window state or recovery files.
set -eu
case "${1-}" in
  # The Launcher already named the profile (its version query does too); never pass it twice. A
  # plain version query answers without touching any profile.
  --launch-profile | --version | -V) exec /usr/lib/cadcraft-nightly/cadcraft "$@" ;;
esac
profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
exec /usr/lib/cadcraft-nightly/cadcraft \
  --launch-profile nightly-v1 --profile-root "$profile_base/cadcraft-nightly" -- "$@"
WRAPPER
    chmod 755 "$NFPM_STAGE/usr/bin/$NFPM_BIN"
    rm -f "$NFPM_STAGE/usr/bin/cadcraft"
    install -m755 "$NFPM_STAGE/usr/bin/cadcraft-cli" "$NFPM_STAGE/usr/lib/cadcraft-nightly/cadcraft-cli"
    cat >"$NFPM_STAGE/usr/bin/$NFPM_CLI" <<'CLI_WRAPPER'
#!/bin/sh
# The Nightly CLI keeps its settings (plotter configurations, support) in the same channel folder
# as the GUI. An explicitly empty CADCRAFT_CONFIG_DIR still means "no settings folder", and
# --config-dir still wins.
set -eu
if [ "${CADCRAFT_CONFIG_DIR+x}" != x ]; then
  profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
  CADCRAFT_CONFIG_DIR="$profile_base/cadcraft-nightly"
  export CADCRAFT_CONFIG_DIR
fi
exec /usr/lib/cadcraft-nightly/cadcraft-cli "$@"
CLI_WRAPPER
    chmod 755 "$NFPM_STAGE/usr/bin/$NFPM_CLI"
    rm -f "$NFPM_STAGE/usr/bin/cadcraft-cli"
    mv "$NFPM_STAGE/usr/share/doc/cadcraft" "$NFPM_STAGE/usr/share/doc/$NFPM_NAME"
    sed -e "s#^Exec=cadcraft\([[:space:]]\|$\)#Exec=$NFPM_BIN\1#" \
        -e "s|^TryExec=cadcraft$|TryExec=$NFPM_BIN|" \
        -e "s|^Icon=$APP_ID$|Icon=$NIGHTLY_DESK_ID|" \
        -e "s|^Name=CADCraft$|Name=CADCraft Nightly|" \
        -e '/^MimeType=/d' \
        "$NFPM_STAGE/usr/share/applications/$APP_ID.desktop" >"$NFPM_STAGE/usr/share/applications/$NIGHTLY_DESK_ID.desktop"
    rm -f "$NFPM_STAGE/usr/share/applications/$APP_ID.desktop"
    sed -e "s|<id>$APP_ID</id>|<id>$NIGHTLY_DESK_ID</id>|g" \
        -e "s|<name>CADCraft</name>|<name>CADCraft Nightly</name>|g" \
        -e "s|<launchable type=\"desktop-id\">$APP_ID.desktop</launchable>|<launchable type=\"desktop-id\">$NIGHTLY_DESK_ID.desktop</launchable>|g" \
        -e "s|<icon type=\"stock\">$APP_ID</icon>|<icon type=\"stock\">$NIGHTLY_DESK_ID</icon>|g" \
        -e "s|<binary>cadcraft</binary>|<binary>$NFPM_BIN</binary>|g" \
        "$NFPM_STAGE/usr/share/metainfo/$APP_ID.metainfo.xml" >"$NFPM_STAGE/usr/share/metainfo/$NIGHTLY_DESK_ID.metainfo.xml"
    rm -f "$NFPM_STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
    rm -rf "$NFPM_STAGE/usr/share/mime"
    rm -rf "$NFPM_STAGE/usr/share/icons/hicolor"
    NIGHTLY_ICON="$ROOT/assets/app-icon/cadcraft-nightly.png"
    [[ -f "$NIGHTLY_ICON" ]] || NIGHTLY_ICON="$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png"
    install -Dm644 "$NIGHTLY_ICON" \
      "$NFPM_STAGE/usr/share/icons/hicolor/256x256/apps/$NIGHTLY_DESK_ID.png"
    if [[ -d "$NFPM_STAGE/usr/share/man/man1" ]]; then
      for man_file in "$NFPM_STAGE/usr/share/man/man1"/cadcraft-cli*.1; do
        [[ -f "$man_file" ]] || continue
        man_base="$(basename "$man_file")"
        sed -e 's/cadcraft-cli/cadcraft-nightly-cli/g' -e 's/CADCRAFT-CLI/CADCRAFT-NIGHTLY-CLI/g' \
          "$man_file" >"$man_file.tmp"
        mv "$man_file.tmp" "$NFPM_STAGE/usr/share/man/man1/${NFPM_CLI}${man_base#cadcraft-cli}"
        rm -f "$man_file"
      done
    fi
    if [[ -f "$NFPM_STAGE/usr/share/bash-completion/completions/cadcraft-cli" ]]; then
      sed -e 's/_cadcraft-cli/_cadcraft-nightly-cli/g' -e 's/cadcraft-cli/cadcraft-nightly-cli/g' \
        "$NFPM_STAGE/usr/share/bash-completion/completions/cadcraft-cli" >"$NFPM_STAGE/usr/share/bash-completion/completions/$NFPM_CLI"
      rm -f "$NFPM_STAGE/usr/share/bash-completion/completions/cadcraft-cli"
    fi
    if [[ -f "$NFPM_STAGE/usr/share/zsh/site-functions/_cadcraft-cli" ]]; then
      sed -e 's/_cadcraft-cli/_cadcraft-nightly-cli/g' -e 's/cadcraft-cli/cadcraft-nightly-cli/g' \
        "$NFPM_STAGE/usr/share/zsh/site-functions/_cadcraft-cli" >"$NFPM_STAGE/usr/share/zsh/site-functions/_$NFPM_CLI"
      rm -f "$NFPM_STAGE/usr/share/zsh/site-functions/_cadcraft-cli"
    fi
    if [[ -f "$NFPM_STAGE/usr/share/fish/vendor_completions.d/cadcraft-cli.fish" ]]; then
      sed -e 's/cadcraft-cli/cadcraft-nightly-cli/g' \
        "$NFPM_STAGE/usr/share/fish/vendor_completions.d/cadcraft-cli.fish" >"$NFPM_STAGE/usr/share/fish/vendor_completions.d/$NFPM_CLI.fish"
      rm -f "$NFPM_STAGE/usr/share/fish/vendor_completions.d/cadcraft-cli.fish"
    fi
  fi
  # nfpm expands env vars in fields like `version` and `arch`, but not in `contents[].src`.
  DROP_LIB_TREE=
  # shellcheck disable=SC2016
  [[ -d "$NFPM_STAGE/usr/lib" ]] || DROP_LIB_TREE='\|^  - src: \${STAGE}/usr/lib/|,+2d;'
  # shellcheck disable=SC2016
  sed "${DROP_LIB_TREE}s|\${STAGE}|$NFPM_STAGE|g;s|\${NFPM_BIN}|$NFPM_BIN|g;s|\${NFPM_CLI}|$NFPM_CLI|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage -----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/CADCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  if [[ "$IS_NIGHTLY" = 1 ]]; then
    APPIMAGE_ID="$APP_ID.nightly-v1"
    cat >"$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
# The nightly AppImage runs this launcher rather than a symlink to the binary, so an extracted or
# integrated AppImage starts with this channel's own launch profile too.
set -eu
APPDIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
case "${1-}" in
  # The Launcher already named the profile (its version query does too); never pass it twice. A
  # plain version query answers without touching any profile.
  --launch-profile | --version | -V) exec "$APPDIR/usr/bin/cadcraft" "$@" ;;
esac
profile_base="${XDG_DATA_HOME:-${HOME:?HOME is required}/.local/share}"
exec "$APPDIR/usr/bin/cadcraft" \
  --launch-profile nightly-v1 --profile-root "$profile_base/cadcraft-nightly" -- "$@"
APPRUN
    chmod 755 "$APPDIR/AppRun"
    sed -e "s|^Icon=$APP_ID$|Icon=$APPIMAGE_ID|" \
        -e "s|^Name=CADCraft$|Name=CADCraft Nightly|" \
        -e '/^MimeType=/d' \
        "$APPDIR/usr/share/applications/$APP_ID.desktop" >"$APPDIR/usr/share/applications/$APPIMAGE_ID.desktop"
    rm -f "$APPDIR/usr/share/applications/$APP_ID.desktop"
    cp "$APPDIR/usr/share/applications/$APPIMAGE_ID.desktop" "$APPDIR/$APPIMAGE_ID.desktop"
    sed -e "s|<id>$APP_ID</id>|<id>$APPIMAGE_ID</id>|g" \
        -e "s|<name>CADCraft</name>|<name>CADCraft Nightly</name>|g" \
        -e "s|<launchable type=\"desktop-id\">$APP_ID.desktop</launchable>|<launchable type=\"desktop-id\">$APPIMAGE_ID.desktop</launchable>|g" \
        -e "s|<icon type=\"stock\">$APP_ID</icon>|<icon type=\"stock\">$APPIMAGE_ID</icon>|g" \
        "$APPDIR/usr/share/metainfo/$APP_ID.metainfo.xml" >"$APPDIR/usr/share/metainfo/$APPIMAGE_ID.metainfo.xml"
    rm -f "$APPDIR/usr/share/metainfo/$APP_ID.metainfo.xml"
    rm -rf "$APPDIR/usr/share/icons/hicolor"
    NIGHTLY_ICON="$ROOT/assets/app-icon/cadcraft-nightly.png"
    [[ -f "$NIGHTLY_ICON" ]] || NIGHTLY_ICON="$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png"
    install -Dm644 "$NIGHTLY_ICON" \
      "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APPIMAGE_ID.png"
    cp "$NIGHTLY_ICON" "$APPDIR/$APPIMAGE_ID.png"
    ln -s "$APPIMAGE_ID.png" "$APPDIR/.DirIcon"
  else
    ln -s usr/bin/cadcraft "$APPDIR/AppRun"
    cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
    cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
    ln -s "$APP_ID.png" "$APPDIR/.DirIcon"
  fi

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [[ -z "$TOOL" ]]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [[ ! -x "$TOOL" ]]; then
      curl --proto '=https' -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  # Absolute, because appimagetool runs in $DIST below (CARGO_TARGET_DIR or APPIMAGETOOL may be
  # relative).
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  APPDIR="$(cd "$APPDIR" && pwd)"
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published next to
  # each AppImage on GitHub Releases. `latest` is the newest published release that is not a
  # pre-release. A fork's builds point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-storytold/cadcraft}"
  UPDATE_ARGS=()
  if [[ "$IS_NIGHTLY" = 0 ]]; then
    UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|cadcraft-*-linux-$ARCH.AppImage.zsync"
    UPDATE_ARGS=(-u "$UPDATE_INFO")
  else
    # Nightly releases are GitHub pre-releases (`latest` only selects stable releases), so point
    # zsync update metadata at pre-release tags rather than stable `latest`.
    UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest-pre|cadcraft-*-linux-$ARCH.AppImage.zsync"
    UPDATE_ARGS=(-u "$UPDATE_INFO")
  fi
  # Extract-and-run: works without FUSE (containers, CI). The output embeds the static runtime,
  # so users don't need libfuse2 either. With zsyncmake on the host (CI installs the zsync
  # package) appimagetool also writes the .zsync, into its working directory, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "${UPDATE_ARGS[@]}" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [[ -s "$OUT.zsync" ]]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [[ -z "${CROSS_TARGET:-}" ]]; then
  "$STAGE/usr/bin/cadcraft-cli" --version
elif [[ -n "${EMULATOR:-}" ]]; then
  "$EMULATOR" "$STAGE/usr/bin/cadcraft-cli" --version
fi
echo "==> done"
ls -lh "$DIST"
