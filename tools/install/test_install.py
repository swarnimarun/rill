"""Installer integration checks. Every write stays in a temporary prefix."""
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).with_name("install.sh").resolve()
VERSION = "v0.1.0-alpha"
TARGETS = {
    ("Darwin", "arm64"): "aarch64-apple-darwin",
    ("Darwin", "aarch64"): "aarch64-apple-darwin",
    ("Darwin", "x86_64"): "x86_64-apple-darwin",
    ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("Linux", "amd64"): "x86_64-unknown-linux-gnu",
}


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rill installer ")
        self.root = Path(self.temporary.name).resolve()
        self.prefix = self.root / "prefix with spaces"
        self.target = TARGETS.get((platform.system(), platform.machine().lower()))
        if not self.target:
            self.skipTest("the installer has no release asset for this test host")
        self.env = os.environ.copy()
        self.env["HOME"] = str(self.root / "home")
        Path(self.env["HOME"]).mkdir()
        self.profile = Path(self.env["HOME"]) / ".profile"
        self.profile.write_text("preserve shell settings\n")

    def tearDown(self):
        self.temporary.cleanup()

    def archive(self, version=VERSION, extra=(), complete=True, result=42):
        package = f"rill-{version[1:]}-{self.target}"
        asset = f"{package}.tar.gz"
        folder = self.root / f"assets-{version}"
        folder.mkdir(exist_ok=True)
        archive = folder / asset
        with tarfile.open(archive, "w:gz") as output:
            directories = [package, f"{package}/bin"]
            if complete:
                directories += [f"{package}/libexec/rill/lib", f"{package}/share/rill/stdlib", f"{package}/share/rill/docs"]
            for name in directories:
                entry = tarfile.TarInfo(name)
                entry.type = tarfile.DIRTYPE
                entry.mode = 0o755
                output.addfile(entry)
            files = {"bin/rill": f"#!/bin/sh\nprintf '{result}\\n'\n".encode()}
            if complete:
                files.update({"libexec/rill/toolchain.json": json.dumps({"version": 1, "rill": version[1:]}).encode(), "libexec/rill/lib/runtime.so.1": b"fixture", "share/rill/stdlib/example.rill": b"fn main() { 42 }"})
            for name, data in files.items():
                entry = tarfile.TarInfo(f"{package}/{name}")
                entry.size = len(data)
                entry.mode = 0o755 if name == "bin/rill" else 0o644
                output.addfile(entry, io.BytesIO(data))
            if complete:
                entry = tarfile.TarInfo(f"{package}/libexec/rill/lib/runtime.so")
                entry.type = tarfile.SYMTYPE
                entry.linkname = "runtime.so.1"
                output.addfile(entry)
            for entry in extra:
                output.addfile(entry, io.BytesIO(b"x") if entry.isreg() else None)
        checksum = folder / f"{asset}.sha256"
        checksum.write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {asset}\n")
        return archive, checksum

    def install(self, archive, checksum, version=VERSION, env=None):
        return subprocess.run(["sh", str(INSTALLER), "--prefix", str(self.prefix), "--version", version, "--archive", str(archive), "--checksum", str(checksum)], env=env or self.env, capture_output=True, text=True, timeout=30)

    def success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def clean_staging(self):
        managed = self.prefix / "lib/rill"
        self.assertFalse((managed / ".install-lock").exists())
        self.assertFalse(list(managed.glob(".stage.*")))

    def test_install_and_idempotence_keep_the_sdk_together_without_profile_changes(self):
        archive, checksum = self.archive()
        self.success(self.install(archive, checksum))
        link = self.prefix / "bin/rill"
        package = self.prefix / f"lib/rill/{VERSION}-{self.target}"
        self.assertEqual(os.readlink(link), f"../lib/rill/{VERSION}-{self.target}/bin/rill")
        self.assertTrue((package / "libexec/rill/toolchain.json").is_file())
        self.assertEqual((package / "libexec/rill/lib/runtime.so").read_bytes(), b"fixture")
        self.assertEqual(subprocess.check_output([str(link)]), b"42\n")
        marker = (package / ".rill-install").read_bytes()
        self.success(self.install(archive, checksum))
        self.assertEqual((package / ".rill-install").read_bytes(), marker)
        self.assertEqual(self.profile.read_text(), "preserve shell settings\n")
        self.clean_staging()

    def test_updates_and_reselecting_an_old_version_preserve_both_packages(self):
        old = self.archive()
        new_version = "v0.1.1-alpha"
        new = self.archive(new_version, result=43)
        self.success(self.install(*old))
        self.success(self.install(*new, version=new_version))
        self.assertEqual(subprocess.check_output([str(self.prefix / "bin/rill")]), b"43\n")
        self.assertTrue((self.prefix / f"lib/rill/{VERSION}-{self.target}/bin/rill").is_file())
        self.success(self.install(*old))
        self.assertEqual(subprocess.check_output([str(self.prefix / "bin/rill")]), b"42\n")
        self.assertTrue((self.prefix / f"lib/rill/{new_version}-{self.target}/bin/rill").is_file())
        self.clean_staging()

    def test_bad_checksum_and_incomplete_stage_preserve_the_previous_installation(self):
        old = self.archive()
        self.success(self.install(*old))
        previous = os.readlink(self.prefix / "bin/rill")
        version = "v0.1.1-alpha"
        archive, checksum = self.archive(version)
        archive.write_bytes(archive.read_bytes() + b"tampered")
        result = self.install(archive, checksum, version)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        archive, checksum = self.archive(version, complete=False)
        result = self.install(archive, checksum, version)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing the complete", result.stderr)
        self.assertEqual(os.readlink(self.prefix / "bin/rill"), previous)
        self.assertFalse((self.prefix / f"lib/rill/{version}-{self.target}").exists())
        self.clean_staging()

    def test_unsafe_archive_paths_links_and_duplicate_members_are_rejected(self):
        package = f"rill-{VERSION[1:]}-{self.target}"
        outside = self.root / "outside"
        outside.write_text("preserve")
        cases = []
        for name in ["../outside", str(outside), f"{package}/../../outside", "different-prefix/file"]:
            entry = tarfile.TarInfo(name)
            entry.size = 1
            cases.append(entry)
        for target in ["../../outside", str(outside), f"{outside} -> safe"]:
            entry = tarfile.TarInfo(f"{package}/escape")
            entry.type = tarfile.SYMTYPE
            entry.linkname = target
            cases.append(entry)
        entry = tarfile.TarInfo(f"{package}/hardlink")
        entry.type = tarfile.LNKTYPE
        entry.linkname = f"{package}/bin/rill"
        cases.append(entry)
        entry = tarfile.TarInfo(f"{package}/bin/rill")
        entry.size = 1
        cases.append(entry)
        for entry in cases:
            with self.subTest(name=entry.name, target=entry.linkname):
                result = self.install(*self.archive(extra=[entry]))
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(outside.read_text(), "preserve")
                self.assertFalse((self.prefix / "bin/rill").exists())
                self.clean_staging()

    def test_unrelated_files_symlinks_and_directories_are_preserved(self):
        archive, checksum = self.archive()
        (self.prefix / "bin").mkdir(parents=True)
        link = self.prefix / "bin/rill"
        link.write_text("unrelated")
        result = self.install(archive, checksum)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(link.read_text(), "unrelated")
        link.unlink()
        external = self.root / "other-command"
        external.write_text("unrelated link target")
        link.symlink_to(external)
        result = self.install(archive, checksum)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(link.resolve(), external)
        self.assertEqual(external.read_text(), "unrelated link target")
        link.unlink()
        (self.prefix / "lib/rill").rmdir()
        (self.prefix / "lib").rmdir()
        (self.prefix / "lib").symlink_to(self.root)
        result = self.install(archive, checksum)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symlinked installation directory", result.stderr)

    def test_checksum_records_are_bounded_and_match_the_selected_asset(self):
        archive, checksum = self.archive()
        original = checksum.read_text()
        for text in [original + original, original.replace(archive.name, "other.tar.gz"), "x" * 4097]:
            checksum.write_text(text)
            result = self.install(archive, checksum)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("checksum", result.stderr)
            self.assertFalse((self.prefix / "bin/rill").exists())
            self.clean_staging()

    def test_multiline_versions_are_rejected_before_creating_a_prefix(self):
        archive, checksum = self.archive()
        for version in [VERSION + "\ninvalid", "invalid\n" + VERSION]:
            result = self.install(archive, checksum, version=version)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--version requires", result.stderr)
            self.assertFalse(self.prefix.exists())

    def test_a_launcher_created_during_staging_is_not_overwritten(self):
        archive, checksum = self.archive()
        tools = self.root / "fake tools"
        tools.mkdir()
        tar = tools / "tar"
        real_tar = subprocess.check_output(["sh", "-c", "command -v tar"], text=True).strip()
        tar.write_text("#!/usr/bin/env python3\nimport pathlib, subprocess, sys\n"
                       f"result = subprocess.run([{real_tar!r}, *sys.argv[1:]])\n"
                       f"launcher = pathlib.Path({str(self.prefix / 'bin/rill')!r})\n"
                       "if sys.argv[1] == '-xzf' and result.returncode == 0: launcher.write_text('unrelated command')\n"
                       "sys.exit(result.returncode)\n")
        tar.chmod(0o755)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env["PATH"])
        result = self.install(archive, checksum, env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.prefix / "bin/rill").read_text(), "unrelated command")
        self.clean_staging()

    def test_interruption_after_activation_keeps_the_selected_package_complete(self):
        archive, checksum = self.archive()
        tools = self.root / "fake tools"
        tools.mkdir()
        mv = tools / "mv"
        real_mv = subprocess.check_output(["sh", "-c", "command -v mv"], text=True).strip()
        mv.write_text("#!/usr/bin/env python3\nimport os, signal, subprocess, sys\n"
                      f"result = subprocess.run([{real_mv!r}, *sys.argv[1:]])\n"
                      f"if sys.argv[-1] == {str(self.prefix / 'bin/rill')!r} and result.returncode == 0: os.kill(os.getppid(), signal.SIGTERM)\n"
                      "sys.exit(result.returncode)\n")
        mv.chmod(0o755)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env["PATH"])
        result = self.install(archive, checksum, env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(subprocess.check_output([str(self.prefix / "bin/rill")]), b"42\n")
        self.assertTrue((self.prefix / f"lib/rill/{VERSION}-{self.target}/libexec/rill/toolchain.json").is_file())
        self.clean_staging()

    def test_unsupported_platform_fails_before_creating_a_prefix(self):
        archive, checksum = self.archive()
        tools = self.root / "fake tools"
        tools.mkdir()
        uname = tools / "uname"
        uname.write_text("#!/bin/sh\ncase \"$1\" in -s) echo Windows ;; -m) echo x86_64 ;; esac\n")
        uname.chmod(0o755)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env["PATH"])
        result = self.install(archive, checksum, env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported platform Windows/x86_64", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_musl_linux_is_not_mistaken_for_the_glibc_asset(self):
        archive, checksum = self.archive()
        tools = self.root / "fake tools"
        tools.mkdir()
        uname = tools / "uname"
        uname.write_text("#!/bin/sh\ncase \"$1\" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n")
        uname.chmod(0o755)
        getconf = tools / "getconf"
        getconf.write_text("#!/bin/sh\nexit 1\n")
        getconf.chmod(0o755)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env["PATH"])
        result = self.install(archive, checksum, env=env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("musl Linux is unsupported", result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_release_download_uses_the_pinned_https_assets_and_ignores_tar_options(self):
        archive, checksum = self.archive()
        tools = self.root / "fake tools"
        tools.mkdir()
        log = self.root / "requests"
        curl = tools / "curl"
        curl.write_text("#!/usr/bin/env python3\nimport pathlib, shutil, sys\n"
                        f"archive = pathlib.Path({str(archive)!r})\nchecksum = pathlib.Path({str(checksum)!r})\nlog = pathlib.Path({str(log)!r})\n"
                        "args = sys.argv[1:]\nassert args[0] == '--disable'\n"
                        "url = args[-1]\nassert url.startswith('https://github.com/swarnimarun/rill/releases/download/v0.1.0-alpha/')\n"
                        "with log.open('a') as output: output.write(url + '\\n')\n"
                        "shutil.copyfile(checksum if url.endswith('.sha256') else archive, args[args.index('--output') + 1])\n")
        curl.chmod(0o755)
        env = dict(self.env, PATH=str(tools) + os.pathsep + self.env["PATH"], TAR_OPTIONS="--this-option-must-not-reach-tar")
        result = subprocess.run(["sh", str(INSTALLER), "--prefix", str(self.prefix)], env=env, capture_output=True, text=True, timeout=30)
        self.success(result)
        self.assertEqual(log.read_text().splitlines(), [f"https://github.com/swarnimarun/rill/releases/download/{VERSION}/{archive.name}", f"https://github.com/swarnimarun/rill/releases/download/{VERSION}/{archive.name}.sha256"])
        self.clean_staging()


if __name__ == "__main__":
    unittest.main()
