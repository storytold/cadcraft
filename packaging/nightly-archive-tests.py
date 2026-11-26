#!/usr/bin/env python3
"""Synthetic app-owned archive/output tests; no native build or executable launch."""
import gzip
import importlib.util
import os
import runpy
import sys
from unittest import mock
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("photo_nightly_archive", HERE / "nightly-archive.py")
ARCHIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ARCHIVE)


class ArchivePolicy(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name).resolve()
        previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        self.output = self.root / "nightly.tar.gz"
        self.required = {"cadcraft", "cadcraft-bin", "cadcraft-nightly.png", "README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ATTRIBUTION.md"}
        for name in self.required:
            (self.stage / name).write_bytes(b"generated fixture: " + name.encode())
        (self.stage / "cadcraft").chmod(0o755)

    def test_flat_regular_ustar_with_all_notices(self):
        (self.stage / "OFL-Font.txt").write_text("generated fixture license", encoding="utf-8")
        ARCHIVE.linux_archive(self.stage, self.output)
        with tarfile.open(self.output) as archive:
            self.assertEqual({m.name for m in archive}, self.required | {"OFL-Font.txt"})
            self.assertTrue(all(m.isfile() and not m.pax_headers for m in archive))
            self.assertEqual(archive.extractfile("cadcraft").read(), (self.stage / "cadcraft").read_bytes())
        with gzip.open(self.output, "rb") as stream:
            while (header := stream.read(512)) != bytes(512):
                self.assertEqual(len(header), 512)
                self.assertIn(header[156:157], {b"0", b"\0"})
                self.assertEqual(header[257:263], b"ustar\0")
                size = int(header[124:136].rstrip(b"\0 ").lstrip(b" ") or b"0", 8)
                stream.read(((size + 511) // 512) * 512)

    def test_each_required_member_is_required_before_output(self):
        for name in self.required:
            with self.subTest(name=name):
                content = (self.stage / name).read_bytes()
                (self.stage / name).unlink()
                with self.assertRaisesRegex(ValueError, "required"):
                    ARCHIVE.linux_archive(self.stage, self.output)
                self.assertFalse(self.output.exists())
                (self.stage / name).write_bytes(content)

    def test_unexpected_cli_portable_or_installer_member_refuses(self):
        for name in ["cadcraft-cli", "portable.txt", "cadcraft.desktop"]:
            with self.subTest(name=name):
                bad = self.stage / name
                bad.write_bytes(b"generated fixture")
                with self.assertRaisesRegex(ValueError, "unexpected"):
                    ARCHIVE.linux_archive(self.stage, self.output)
                self.assertFalse(self.output.exists())
                bad.unlink()

    def test_symlink_refuses_before_output(self):
        icon = self.stage / "cadcraft-nightly.png"
        icon.unlink()
        icon.symlink_to(self.stage / "README.md")
        with self.assertRaisesRegex(ValueError, "regular file|symlinks or aliases"):
            ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())

    def test_directory_refuses_before_output(self):
        icon = self.stage / "cadcraft-nightly.png"
        icon.unlink()
        icon.mkdir()
        with self.assertRaisesRegex(ValueError, "regular file"):
            ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())

    def test_long_notice_refuses_before_output(self):
        (self.stage / ("OFL-" + "x" * 100 + ".txt")).write_bytes(b"fixture")
        with self.assertRaisesRegex(ValueError, "short regular"):
            ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())

    def test_case_colliding_notices_refuse_before_output(self):
        for name in ["OFL-Font.txt", "OFL-font.txt"]:
            (self.stage / name).write_bytes(b"fixture")
        with self.assertRaisesRegex(ValueError, "case-colliding"):
            ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())

    def test_cli_dispatches_only_the_supported_archive_kind(self):
        helper = HERE / "nightly-archive.py"
        with mock.patch.object(sys, "argv", [str(helper), "linux", str(self.stage), str(self.output)]):
            runpy.run_path(str(helper), run_name="__main__")
        with tarfile.open(self.output) as archive:
            self.assertEqual({member.name for member in archive}, self.required)
        refused = self.root / "refused.tar.gz"
        for args in ([str(helper)], [str(helper), "unknown", str(self.stage), str(refused)]):
            with self.subTest(args=args), mock.patch.object(sys, "argv", args):
                with self.assertRaises(SystemExit):
                    runpy.run_path(str(helper), run_name="__main__")
                self.assertFalse(refused.exists())

    def test_existing_archive_is_unchanged(self):
        self.output.write_bytes(b"owned earlier artifact")
        with self.assertRaises(FileExistsError):
            ARCHIVE.linux_archive(self.stage, self.output)
        self.assertEqual(self.output.read_bytes(), b"owned earlier artifact")


    def test_archive_output_outside_build_root_refuses(self):
        with tempfile.TemporaryDirectory() as external:
            output = Path(external) / "nightly.tar.gz"
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(self.stage, output)
            self.assertFalse(output.exists())

    def test_archive_stage_outside_build_root_refuses(self):
        with tempfile.TemporaryDirectory() as external:
            stage = Path(external)
            for name in self.required:
                (stage / name).write_bytes(b"generated external fixture")
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(stage, self.output)
            self.assertFalse(self.output.exists())

    def test_archive_stage_alias_refuses(self):
        alias = self.root / "stage-alias"
        alias.symlink_to(self.stage, target_is_directory=True)
        with self.assertRaises(ValueError):
            ARCHIVE.linux_archive(alias, self.output)
        self.assertFalse(self.output.exists())

    def test_archive_output_parent_alias_refuses(self):
        with tempfile.TemporaryDirectory() as external:
            alias = self.root / "output-alias"
            alias.symlink_to(external, target_is_directory=True)
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(self.stage, alias / "nightly.tar.gz")
            self.assertFalse((Path(external) / "nightly.tar.gz").exists())

    def test_archive_parent_traversal_refuses(self):
        with tempfile.TemporaryDirectory(dir=self.root.parent) as external:
            output = Path("..") / Path(external).name / "nightly.tar.gz"
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(self.stage, output)
            self.assertFalse((Path(external) / "nightly.tar.gz").exists())

    def test_nonflat_member_name_refuses(self):
        stage_root = os.path.realpath(self.stage)
        with open(self.stage / "README.md", "rb") as incoming:
            for bad in ("../escape", "/absolute", "nested/name", "x/../README.md", "", ".", ".."):
                with self.subTest(name=bad), self.assertRaises(ValueError):
                    ARCHIVE.member_info(stage_root, bad, incoming)

    def test_member_header_matches_what_gettarinfo_records(self):
        stage_root = os.path.realpath(self.stage)
        for name in sorted(self.required):
            with self.subTest(name=name), open(self.stage / name, "rb") as incoming:
                ours = ARCHIVE.member_info(stage_root, name, incoming)
                with tarfile.open(fileobj=__import__("io").BytesIO(), mode="w") as reference:
                    theirs = reference.gettarinfo(fileobj=incoming, arcname=name)
                self.assertEqual(ours.get_info(), theirs.get_info())

    def test_member_read_budget_refuses_before_output(self):
        (self.stage / "OFL-Large.txt").write_bytes(b"x" * 65)
        with mock.patch.object(ARCHIVE, "MAX_MEMBER_BYTES", 64, create=True):
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())


    def test_member_growth_is_rechecked_before_copy_and_output_is_cleaned(self):
        growing = self.stage / "OFL-Growing.txt"
        growing.write_bytes(b"x" * 32)
        original = ARCHIVE.member_info

        def grow_before_stat(stage_root, name, incoming):
            if name == growing.name:
                growing.write_bytes(b"x" * 65)
            return original(stage_root, name, incoming)

        with mock.patch.object(ARCHIVE, "MAX_MEMBER_BYTES", 64), \
                mock.patch.object(ARCHIVE, "member_info", grow_before_stat):
            with self.assertRaises(ValueError):
                ARCHIVE.linux_archive(self.stage, self.output)
        self.assertFalse(self.output.exists())


class ExclusiveOutput(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.source = self.root / "input archive"
        self.output = self.root / "final archive"
        self.source.write_bytes(b"generated archive fixture" * 1024)

    def publish(self):
        return subprocess.run(["bash", "-c", 'set -euo pipefail; source "$1"; nightly_publish_archive "$2" "$3"', "test", str(HERE / "nightly-common.sh"), str(self.source), str(self.output)], capture_output=True, text=True, timeout=15)

    def test_success_copies_exact_bytes(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.output.read_bytes(), self.source.read_bytes())

    def test_existing_output_is_never_replaced(self):
        self.output.write_bytes(b"existing artifact")
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertEqual(self.output.read_bytes(), b"existing artifact")

    def test_missing_source_does_not_leave_a_fake_empty_artifact(self):
        self.source.unlink()
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
