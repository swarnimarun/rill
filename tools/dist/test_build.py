"""Offline checks for package input integrity and archive confinement."""
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import build


class PackagingTests(unittest.TestCase):
    def test_sdl_pkgconfig_uses_its_installed_location(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "lib/pkgconfig/sdl3.pc"
            path.parent.mkdir(parents=True)
            path.write_text("prefix=/deleted/staging\nlibdir=${prefix}/lib\nVersion: 3.4.20\n")
            build.relocate_sdl_metadata(root)
            self.assertEqual(path.read_text(), "prefix=${pcfiledir}/../..\nlibdir=${prefix}/lib\nVersion: 3.4.20\n")

    def test_every_dependency_is_https_and_sha256_pinned(self):
        specs = list(build.LOCK["slang"]["assets"].values())
        specs += list(build.LOCK["raylib"]["assets"].values())
        specs += [build.LOCK[key] for key in ("sdl3", "spirv-tools", "spirv-headers")]
        for spec in specs:
            self.assertTrue(spec["url"].startswith("https://"))
            self.assertEqual(len(bytes.fromhex(spec["sha256"])), 32)

    def test_corrupt_cached_archive_never_downloads_or_extracts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            spec = {"url": "https://example.invalid/archive", "sha256": "0" * 64}
            (root / (spec["sha256"] + ".tar.gz")).write_bytes(b"corrupt")
            with patch("urllib.request.urlopen") as request:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    build.fetch(spec, root)
                request.assert_not_called()

    def test_archive_parent_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                entry = tarfile.TarInfo("../outside")
                entry.size = 3
                tar.addfile(entry, io.BytesIO(b"bad"))
            with self.assertRaises(tarfile.FilterError):
                build.unpack(archive, root / "source")
            self.assertFalse((root / "outside").exists())

    def test_archive_escaping_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                entry = tarfile.TarInfo("link")
                entry.type = tarfile.SYMTYPE
                entry.linkname = "../../outside"
                tar.addfile(entry)
            with self.assertRaises(tarfile.FilterError):
                build.unpack(archive, root / "source")


if __name__ == "__main__":
    unittest.main()
