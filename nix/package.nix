# CADCraft — the desktop app (`cadcraft`) and the CLI/MCP server (`cadcraft-cli`), built from this
# repository:
#
#   nix build          # → ./result/bin/cadcraft, ./result/bin/cadcraft-cli
#
# Kept in step with the release workflow (.github/workflows/release.yml) and packaging/:
#   * `Cargo.lock` drives the dependencies (`cargoLock`), so there is no vendorHash to bump. The
#     git sources (storytold/craft-libs) are fetched at the revision the lock file pins;
#   * the desktop file, hicolor icons, MIME types and AppStream metadata are the same files the
#     .deb/.rpm ship (packaging/linux/), so `apt` and NixOS users see one identical CADCraft;
#   * `CRAFT_FONTS_DIR` embeds the fonts from storytold/craft-fonts (the `craft-fonts` flake input, pinned to
#     the revision the release workflow uses); without it everything builds and runs, but some
#     scripts (e.g. Japanese, Chinese) have no glyphs;
#   * `doCheck` is off (`cargo xtask ci` is the gate). `pkgs.cadcraft.overrideAttrs { doCheck = true; }`
#     runs the workspace tests except cadcraft-ui-egui's, whose GPU tests need a wgpu adapter the
#     Nix sandbox does not have (see `cargoTestFlags`); the AutoLISP corpus test skips itself
#     without CADCRAFT_LISP_CORPUS.
#
# Everything in the product is pure Rust, so this needs no build system beyond cargo plus the
# libraries winit/wgpu `dlopen` at run time (see `runtimeLibs`).
#
# Desktop sessions: the window opens natively on Wayland (GNOME/Mutter, Hyprland and other
# wlroots-style compositors) and falls back to X11/XWayland. File dialogs go through
# xdg-desktop-portal, so the session needs a portal backend: GNOME ships one
# (`xdg-desktop-portal-gnome`); on Hyprland enable
#   xdg.portal = { enable = true; extraPortals = [ pkgs.xdg-desktop-portal-gtk ]; };
# next to `programs.hyprland.enable = true;`.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  makeWrapper,
  # Windowing/GPU libraries: needed at build time by the crates that link them, at run time by the
  # ones that open them with dlopen() (see `runtimeLibs`), and by the dev shell (flake.nix).
  wayland,
  wayland-scanner,
  libxkbcommon,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
  vulkan-loader,
  libglvnd,
  # rfd opens libdbus-1 at run time to reach the xdg-desktop-portal file chooser (Open/Save dialogs).
  dbus,
  # Validates the desktop file and the AppStream metadata we install (as CI does).
  desktop-file-utils,
  appstream,
  # Adds the driver search path (/run/opengl-driver/lib) to the binaries' RPATH so wgpu finds the
  # installed Vulkan/GL driver on NixOS. Optional: harmless on nixpkgs without it.
  autoAddDriverRunpath ? null,
  # storytold/craft-fonts checkout (flake input). `null` builds without the embedded fonts.
  craft-fonts ? null,
  # Release date for the AppStream metadata (YYYY-MM-DD); the flake derives it from its own commit.
  buildDate ? "1970-01-01",
}:

let
  root = ../.;

  # The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
  version = (builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"))).workspace.package.version;

  isLinux = stdenv.hostPlatform.isLinux;

  # AppStream id: also the desktop file name, and the icon name in share/icons/hicolor.
  appId = "ai.storyteller.cadcraft";

  # Libraries the binaries open at run time with dlopen(): nothing links them, so no RPATH points at
  # them. winit loads libwayland-client, libxkbcommon(-x11) and the X11 libraries; wgpu loads the
  # Vulkan loader or libEGL (libglvnd); rfd loads libdbus-1 for the file chooser portal. NixOS patches the loader to find the drivers in
  # /run/opengl-driver. The wrapper below points LD_LIBRARY_PATH here.
  runtimeLibs = lib.optionals isLinux [
    libxkbcommon
    wayland
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
    vulkan-loader
    libglvnd
    dbus
  ];

  # The two native binaries. The wasm app (apps/cadcraft-web) is built by `cargo xtask`, not here.
  binaries = [
    "cadcraft"
    "cadcraft-cli"
  ];

in
rustPlatform.buildRustPackage {
  pname = "cadcraft";
  inherit version;

  src = lib.cleanSourceWith {
    src = lib.cleanSource root;
    # The Nix files are not part of the build: editing them must not rebuild any Rust.
    filter =
      path: _type:
      !(builtins.elem (baseNameOf path) [
        "flake.nix"
        "flake.lock"
        "nix"
      ]);
  };

  cargoLock = {
    lockFile = root + "/Cargo.lock";
    # The git dependencies are pinned by revision in Cargo.lock.
    allowBuiltinFetchGit = true;
  };

  # Both binaries, as the .deb/.rpm install them (`packaging/linux/package.sh`).
  cargoBuildFlags = [
    "--locked"
    "-p"
    "cadcraft"
    "-p"
    "cadcraft-cli"
  ];

  # `overrideAttrs { doCheck = true; }`: every crate but the UI, whose gpu_* tests render through a
  # wgpu adapter (tests/gpu_viewport_stencil.rs reads pixels back) and panic without one.
  cargoTestFlags = [
    "--workspace"
    "--exclude"
    "cadcraft-ui-egui"
  ];

  doCheck = false;

  strictDeps = true;

  nativeBuildInputs = [
    pkg-config
  ]
  ++ lib.optionals isLinux [
    makeWrapper
    wayland-scanner
    desktop-file-utils
    appstream
  ]
  ++ lib.optionals (isLinux && autoAddDriverRunpath != null) [ autoAddDriverRunpath ];

  buildInputs = lib.optionals isLinux [
    wayland
    libxkbcommon
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];

  env = lib.optionalAttrs (craft-fonts != null) {
    CRAFT_FONTS_DIR = toString craft-fonts;
    # A malformed font input must fail the build rather than silently ship without glyphs.
    CRAFT_FONTS_REQUIRED = "1";
  };

  postInstall = ''
    # Desktop integration: byte-for-byte the files the .deb/.rpm/.AppImage install.
    install -Dm644 packaging/linux/${appId}.desktop $out/share/applications/${appId}.desktop
    install -Dm644 packaging/linux/${appId}.mime.xml $out/share/mime/packages/${appId}.xml
    install -d $out/share/icons
    cp -r assets/app-icon/hicolor $out/share/icons/
    install -d $out/share/metainfo
    substitute packaging/linux/${appId}.metainfo.xml.in \
      $out/share/metainfo/${appId}.metainfo.xml \
      --subst-var-by VERSION ${version} \
      --subst-var-by DATE ${buildDate}

    # Licences for everything embedded in the binaries.
    doc=$out/share/doc/cadcraft
    for f in README.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md assets/ATTRIBUTION.md; do
      if [ -f "$f" ]; then install -Dm644 "$f" "$doc/$(basename "$f")"; fi
    done
    for f in assets/fonts/OFL-*.txt; do
      if [ -f "$f" ]; then install -Dm644 "$f" "$doc/$(basename "$f")"; fi
    done
  ''
  + lib.optionalString (craft-fonts != null) ''
    for lic in ${toString craft-fonts}/fonts/*/OFL.txt; do
      install -Dm644 "$lic" "$doc/OFL-$(basename "$(dirname "$lic")").txt"
    done
  ''
  + lib.optionalString isLinux ''
    desktop-file-validate $out/share/applications/${appId}.desktop
    XDG_CACHE_HOME=$TMPDIR/.cache appstreamcli validate --no-net \
      $out/share/metainfo/${appId}.metainfo.xml

    # makeWrapper keeps the binaries working when the libs above are only dlopen'd.
    for bin in ${lib.concatStringsSep " " (map (b: "$out/bin/" + b) binaries)}; do
      wrapProgram "$bin" --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibs}"
    done
  '';

  passthru = {
    # Shared with the dev shell so `cargo run` inside `nix develop` finds the same dlopen'd libs.
    inherit runtimeLibs;
  };

  meta = {
    description = "Computer-aided design application";
    longDescription = ''
      CADCraft is a computer-aided design application for 2D drafting with DWG/DXF interchange.
      It ships with cadcraft-cli, a headless converter, command runner and MCP server for AI agents.
    '';
    homepage = "https://getartcraft.com/apps/cadcraft";
    # The craft-fonts build input embeds OFL fonts in the binaries.
    license =
      with lib.licenses;
      [
        mit
        asl20
      ]
      ++ lib.optional (craft-fonts != null) ofl;
    sourceProvenance = with lib.sourceTypes; [ fromSource ];
    # Apple Silicon only on the Darwin side: the pinned nixpkgs (unstable, 26.11) has dropped
    # x86_64-darwin, so the flake cannot offer it although the universal macOS release covers it.
    platforms = lib.platforms.linux ++ [ "aarch64-darwin" ];
    # `nix run` and the desktop entry both start the GUI.
    mainProgram = "cadcraft";
  };
}
