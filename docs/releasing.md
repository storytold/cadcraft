# Releasing CADCraft

This file describes how CADCraft is released.

- **Version:** `[workspace.package] version` in the root `Cargo.toml` is the single source. `cargo xtask version` prints it; `cargo xtask version set X.Y.Z[-pre]` changes it. The app shows it in About, `cadcraft --version` and `cadcraft-cli --version`; release builds also embed the commit (`CADCRAFT_BUILD_SHA`).
- **Cut a release:** bump the version on `main`, then push `main` to `release` (`git push origin main:release`). Every push to `release` runs `.github/workflows/release.yml` and creates or updates the draft GitHub Release `CADCraft v<version>`. A `workflow_dispatch` run with `version: X.Y.Z-rc.N` makes a test build. A dispatch on any other branch is a dry run: the unsigned jobs (version, Linux, Linux riscv64, Flatpak, FreeBSD, web) build, the `release` environment refuses the signing jobs (macOS, Windows), and no release is drafted.
- **Artifacts:** macOS universal `.dmg` (Developer ID signed, notarized, stapled) and CLI `.zip`; Windows x64, x86 and arm64 `.msi` + portable `.zip` (Azure Trusted Signing); Linux x86_64 and aarch64 `.AppImage` (+ `.AppImage.zsync` for AppImageUpdate delta updates), `.flatpak` bundle (repackages the tarball, `packaging/linux/flatpak-bundle.sh`), `.deb`, `.rpm`, `.tar.gz` (+ the from-source Flathub manifest in `packaging/linux/flatpak/`); Linux riscv64 `.tar.gz` (cross-compiled on Ubuntu 24.04 against ports.ubuntu.com riscv64 libraries, CLI smoke-tested under QEMU); FreeBSD x86_64 `.tar.gz` (built in a FreeBSD VM); web `.zip` (wasm + `index.html`, see `packaging/web/README.md`); `SHA256SUMS.txt`.
- **Secrets:** the twelve signing secrets live in the `release` environment (deployable only from the `release` branch, which only org admins and release-managers can push). All are optional: a missing one produces unsigned artifacts with a warning.
- **Windows installer:** `packaging/windows/cadcraft.wxs` (WiX v5 + `WixToolset.UI.wixext`). Its wizard has a Shortcuts page: the Start Menu shortcut is always created, the desktop shortcut is opt-in (`DESKTOPSHORTCUT=1` for silent installs, `DESKTOPSHORTCUT=0` drops it on an upgrade; otherwise upgrades keep the previous choice).
- **File types:** `.dxf` and `.dwg` (macOS document types, Windows "Open with", Linux MIME types).
- **Local packaging test:** `packaging/macos/package.sh` (ad-hoc signed DMG), `packaging/linux/package.sh`, `packaging/web/package.sh`.

## Release-ref nightly

CADCraft's Windows, Linux and macOS nightly is documented in
[`nightly-release-preparation.md`](nightly-release-preparation.md). `.github/workflows/nightly.yml`
and `nightly-sign.yml` are enabled for **supervised manual runs only** by the reviewed release
managers: there is no schedule, and there is no automatic transport yet — a run is dispatched by
hand, and the completion step is manual too. No platform secret is granted to the producer. Stable `release.yml`, its triggers, environment and secrets are unchanged.

The signer-owned files (`.github/workflows/nightly-sign.yml`,
`.github/nightly-signing-policy.json`, `packaging/windows/sign.ps1`, the macOS nightly signing
scripts `nightly-sign.sh`/`nightly-verify.sh`/`nightly-lib.sh`/`import-cert.sh` and their
entitlements, `.github/nightly-linux-signing-key.asc`, and every transitive reusable pin) travel
on the dedicated `nightly-signer` branch, which only fast-forwards to reviewed `main` commits, and
are compared against the previous trusted release, not a manifest from `main`. Its trust closure is
all of `.github/` and `packaging/`, including `packaging/env.sh`, which `import-cert.sh` sources with
the Apple keys. Nightlies share the existing Windows signing identity with stable, so a
suspected key compromise affects both channels: stop nightlies too, and have the identity's owner
revoke/rotate it.

The shared `craft-common-actions` signer admits CADCraft through its reviewed `nightly-apps.json`
row at the pinned commit, and the app-side flavor must be in the built channel: a nightly binary
is refused without `--launch-profile nightly-v1 --profile-root <dir>`, so every Launcher wrapper
must pass them (see the preparation doc).

