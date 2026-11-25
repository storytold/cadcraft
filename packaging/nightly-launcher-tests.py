#!/usr/bin/env python3
"""Every Linux Nightly launcher starts the GUI the flavor's way; no build, no window.

The deb/rpm wrapper, the AppImage `AppRun` and the Flatpak wrapper are generated inside
package.sh/flatpak-bundle.sh; the plain tar and the Arch handoff ship a launcher file. All of them
must hand the binary the one protocol the flavor accepts -- `--launch-profile nightly-v1
--profile-root <dir> -- files...` -- and pass a version query through untouched.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
LINUX = HERE / "linux"
PROFILE = "cadcraft-nightly"
RECORDER = '#!/bin/sh\nfor arg in "$@"; do printf "<%s>\\n" "$arg"; done\n'
CONFIG_RECORDER = (
    '#!/bin/sh\nprintf "config=%s\\n" "${CADCRAFT_CONFIG_DIR+<$CADCRAFT_CONFIG_DIR>}"\n'
    'for arg in "$@"; do printf "<%s>\\n" "$arg"; done\n'
)


def heredoc(script, marker, needle):
    """The one heredoc body in `script` delimited by `marker` that contains `needle`."""
    text = Path(script).read_text(encoding="utf-8")
    bodies = []
    start = 0
    while (opening := text.find("<<'%s'" % marker, start)) != -1:
        body_start = text.index("\n", opening) + 1
        body_end = text.index("\n%s\n" % marker, body_start)
        bodies.append(text[body_start:body_end] + "\n")
        start = body_end + 1
    matched = [body for body in bodies if needle in body]
    if len(matched) != 1:
        raise AssertionError(f"expected one {marker} heredoc with {needle!r} in {script}, found {len(matched)}")
    return matched[0]


class Launcher(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        self.home = self.root / "home with space"
        self.home.mkdir()
        self.real = self.root / "cadcraft-real"
        self.real.write_text(RECORDER, encoding="utf-8")
        self.real.chmod(0o755)

    def launcher(self, name, body):
        path = self.root / name
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)
        return path

    def generated(self, script, marker, needle, replacement):
        body = heredoc(script, marker, needle)
        return self.launcher("cadcraft-nightly", body.replace(needle, str(replacement)))

    def recording(self, launcher, *args, env=None):
        environment = {"PATH": "/usr/bin:/bin", "HOME": str(self.home), **(env or {})}
        result = subprocess.run([str(launcher), *args], capture_output=True, text=True, env=environment, timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        return [line[1:-1] for line in result.stdout.splitlines() if line.startswith("<")]

    def profile_base(self, env=None, fallback=(".local", "share")):
        return Path((env or {}).get("XDG_DATA_HOME") or self.home.joinpath(*fallback)) / PROFILE

    def assert_starts_the_profile(self, launcher, env=None, fallback=(".local", "share")):
        root = str(self.profile_base(env, fallback))
        files = ["drawing.dxf", "with spaces and -- dashes.dxf"]
        self.assertEqual(
            self.recording(launcher, *files, env=env),
            ["--launch-profile", "nightly-v1", "--profile-root", root, "--", *files],
            launcher,
        )
        self.assertEqual(self.recording(launcher, env=env), ["--launch-profile", "nightly-v1", "--profile-root", root, "--"])
        for flag in ("--version", "-V"):
            self.assertEqual(self.recording(launcher, flag, env=env), [flag], launcher)
        query = ["--launch-profile", "nightly-v1", "--profile-root", str(self.root / "launcher root"), "--version"]
        self.assertEqual(self.recording(launcher, *query, env=env), query, "a Launcher query is passed through once")

    def test_the_plain_tar_launcher_starts_the_profile(self):
        launcher = LINUX / "nightly-launch.sh"
        (self.root / "cadcraft-bin").write_text(RECORDER, encoding="utf-8")
        (self.root / "cadcraft-bin").chmod(0o755)
        installed = self.launcher("cadcraft", launcher.read_text(encoding="utf-8"))
        self.assert_starts_the_profile(installed)
        self.assert_starts_the_profile(installed, {"XDG_DATA_HOME": str(self.root / "xdg with space")})

    def test_the_tar_launcher_refuses_a_missing_or_aliased_gui(self):
        launcher = self.launcher("cadcraft", (LINUX / "nightly-launch.sh").read_text(encoding="utf-8"))
        for args in ([], ["--version"]):
            with self.subTest(args=args):
                result = subprocess.run([str(launcher), *args], capture_output=True, text=True, timeout=60, env={"PATH": "/usr/bin:/bin", "HOME": str(self.home)})
                self.assertEqual(result.returncode, 2, result.stdout)
                self.assertIn("cadcraft-bin is missing", result.stderr)
        (self.root / "elsewhere").write_text(RECORDER, encoding="utf-8")
        (self.root / "elsewhere").chmod(0o755)
        (self.root / "cadcraft-bin").symlink_to(self.root / "elsewhere")
        result = subprocess.run([str(launcher)], capture_output=True, text=True, timeout=60, env={"PATH": "/usr/bin:/bin", "HOME": str(self.home)})
        self.assertEqual(result.returncode, 2, "an aliased GUI is not started")

    def test_the_arch_launcher_starts_the_profile(self):
        (self.root / ".cadcraft-nightly").mkdir()
        shutil.copy(self.real, self.root / ".cadcraft-nightly" / "cadcraft")
        installed = self.launcher("cadcraft-nightly", (LINUX / "nightly-arch-launch.sh").read_text(encoding="utf-8"))
        self.assert_starts_the_profile(installed)
        self.assert_starts_the_profile(installed, {"XDG_DATA_HOME": str(self.root / "xdg with space")})

    def test_the_deb_and_rpm_wrapper_starts_the_profile(self):
        generated = self.generated(LINUX / "package.sh", "WRAPPER", "/usr/lib/cadcraft-nightly/cadcraft", self.real)
        self.assert_starts_the_profile(generated)
        self.assert_starts_the_profile(generated, {"XDG_DATA_HOME": str(self.root / "xdg with space")})

    def test_the_appimage_apprun_starts_the_profile(self):
        appdir = self.root / "CADCraft.AppDir"
        (appdir / "usr/bin").mkdir(parents=True)
        shutil.copy(self.real, appdir / "usr/bin" / "cadcraft")
        apprun = self.launcher("AppRun", heredoc(LINUX / "package.sh", "APPRUN", "$APPDIR/usr/bin/cadcraft"))
        shutil.move(str(apprun), str(appdir / "AppRun"))
        self.assert_starts_the_profile(appdir / "AppRun")
        self.assert_starts_the_profile(appdir / "AppRun", {"XDG_DATA_HOME": str(self.root / "xdg with space")})

    def test_the_nightly_cli_keeps_its_settings_in_the_profile(self):
        body = heredoc(LINUX / "package.sh", "CLI_WRAPPER", "/usr/lib/cadcraft-nightly/cadcraft-cli")
        cli = self.launcher("cadcraft-nightly-cli", body.replace("/usr/lib/cadcraft-nightly/cadcraft-cli", str(self.real)))
        self.real.write_text(CONFIG_RECORDER, encoding="utf-8")
        self.real.chmod(0o755)
        # A downstream process sees the variable the CLI wrapper exported.
        def recorded(*args, config=None, xdg=None):
            environment = {"PATH": "/usr/bin:/bin", "HOME": str(self.home)}
            if xdg is not None:
                environment["XDG_DATA_HOME"] = xdg
            if config is not None:
                environment["CADCRAFT_CONFIG_DIR"] = config
            result = subprocess.run([str(cli), *args], capture_output=True, text=True, env=environment, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            lines = result.stdout.splitlines()
            return lines[0], lines[1:]

        base = Path(str(self.home / ".local" / "share")) / PROFILE
        self.assertEqual(recorded("convert", "in.dxf"), (f"config=<{base}>", ["<convert>", "<in.dxf>"]))
        self.assertEqual(recorded("convert", xdg=str(self.root / "xdg with space"))[0], f"config=<{self.root}/xdg with space/{PROFILE}>")
        # An explicitly empty value still means "no settings folder", and an explicit one is kept.
        self.assertEqual(recorded("run")[0], f"config=<{base}>")
        self.assertEqual(recorded("run", config="")[0], "config=<>")
        self.assertEqual(recorded("run", config=str(self.root / "explicit"))[0], f"config=<{self.root}/explicit>")

    def test_the_flatpak_wrapper_starts_the_profile(self):
        generated = self.generated(LINUX / "flatpak-bundle.sh", "WRAPPER", "/app/libexec/cadcraft", self.real)
        # Inside the bundle XDG_DATA_HOME is the nightly app's own sandbox folder.
        sandbox = self.home / ".var/app/ai.storyteller.cadcraft.nightly/data"
        self.assert_starts_the_profile(generated, {"XDG_DATA_HOME": str(sandbox)})
        self.assert_starts_the_profile(generated, fallback=(".var", "app", "ai.storyteller.cadcraft.nightly", "data"))


if __name__ == "__main__":
    unittest.main()
