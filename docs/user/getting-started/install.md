---
title: Install CADCraft
description: Download CADCraft for Windows 11, macOS, Linux, FreeBSD or the web, or install it through ArtCraft Launcher.
order: 10
verified: 0.6.2
---

Download the build for your system from the [latest release](https://github.com/storytold/cadcraft/releases/latest) on GitHub, or install CADCraft with ArtCraft Launcher. In the file names, `<version>` stands for the version number, for example `0.6.2`. `SHA256SUMS.txt` in the release lists a checksum for every file.

## Install with ArtCraft Launcher

ArtCraft Launcher lists CADCraft. It installs the build that fits your system and tells you when an update is out. See the [ArtCraft Launcher docs](https://getartcraft.com/docs/craft-launcher).

## Windows 11

| Build                                | Installer                              | Portable                                        |
|--------------------------------------|----------------------------------------|-------------------------------------------------|
| x64 (64-bit Intel/AMD)               | `cadcraft-<version>-windows-x64.msi`   | `cadcraft-<version>-windows-x64-portable.zip`   |
| arm64 (Snapdragon and other ARM PCs) | `cadcraft-<version>-windows-arm64.msi` | `cadcraft-<version>-windows-arm64-portable.zip` |
| x86 (32-bit)                         | `cadcraft-<version>-windows-x86.msi`   | `cadcraft-<version>-windows-x86-portable.zip`   |

1. Download the `.msi` for your processor.
2. Run it. The installer adds CADCraft to the Start menu and asks whether to add a desktop shortcut too. The shortcut box is unticked by default.
3. Open CADCraft from the Start menu.

The portable `.zip` needs no installer: unpack it and run `cadcraft.exe`. The installers and executables are code-signed.

If the window flashes and the app closes, see [The app closes right after opening on Windows](../troubleshooting/app-closes-on-windows.md).

## macOS

Download `cadcraft-<version>-macos-universal.dmg`. It runs on Apple silicon and Intel Macs and is signed and notarized.

## Linux

| Format                 | File (x86_64 / aarch64)                                                                 |
|------------------------|-----------------------------------------------------------------------------------------|
| AppImage               | `cadcraft-<version>-linux-x86_64.AppImage`, `cadcraft-<version>-linux-aarch64.AppImage` |
| Flatpak                | `cadcraft-<version>-linux-x86_64.flatpak`, `cadcraft-<version>-linux-aarch64.flatpak`   |
| Debian and Ubuntu      | `cadcraft-<version>-linux-x86_64.deb`, `cadcraft-<version>-linux-aarch64.deb`           |
| Fedora, RHEL, openSUSE | `cadcraft-<version>-linux-x86_64.rpm`, `cadcraft-<version>-linux-aarch64.rpm`           |
| Tarball                | `cadcraft-<version>-linux-x86_64.tar.gz`, `cadcraft-<version>-linux-aarch64.tar.gz`     |

Install a Flatpak with `flatpak install --user <file>`.

For RISC-V (riscv64) the release has `cadcraft-<version>-linux-riscv64.tar.gz`, `cadcraft-<version>-linux-riscv64.deb` and `cadcraft-<version>-linux-riscv64.rpm`. The riscv64 tarball needs glibc 2.39 or newer, as on Ubuntu 24.04 or Debian 13.

## FreeBSD

Download `cadcraft-<version>-freebsd-x86_64.tar.gz`.

## In the browser

`cadcraft-web-<version>.zip` runs in a modern browser. The web build cannot open or save DWG files yet: save as DXF.

## Command-line tool

`cadcraft-cli` converts and inspects drawings without the window. The release includes it for macOS as `cadcraft-cli-<version>-macos-universal.zip`. For scripting and AI assistants see the [FAQ](../faq/index.md).
