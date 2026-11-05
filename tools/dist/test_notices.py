"""Offline provenance and mutation checks for the retained Raylib notices."""
import copy
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile
import unittest
from unittest.mock import patch

import build


REVISION = "02566453880e06bec97f5b8160fa6fe1e98397ca"
NOTICE_ROOT = build.REPO / "tools/dist/raylib-notices"
INVENTORY = build.REPO / "tools/dist/raylib-notices.json"


class NoticeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rill notice tests ")
        self.root = Path(self.temporary.name)
        self.notices = self.root / "notices"
        shutil.copytree(NOTICE_ROOT, self.notices)
        self.inventory = self.root / "inventory.json"
        self.value = json.loads(INVENTORY.read_text())
        self.write_inventory()

    def tearDown(self):
        self.temporary.cleanup()

    def write_inventory(self):
        self.inventory.write_text(json.dumps(self.value))

    def verify(self):
        build.verify_raylib_notices(self.notices, self.inventory)

    def test_all_112_notices_have_pinned_source_and_range_provenance(self):
        with patch("urllib.request.urlopen") as network:
            self.verify()
            network.assert_not_called()
        self.assertEqual(self.value["revision"], REVISION)
        self.assertEqual(build.LOCK["raylib"]["revision"], REVISION)
        self.assertEqual(self.value["release"], "6.2")
        entries = self.value["files"]
        self.assertEqual(len(entries), 112)
        sources = {entry["source_path"] for entry in entries}
        self.assertEqual(len(sources), 112)
        for expected in ["cgltf.h", "cgltf_write.h", "dr_flac.h", "dr_mp3.h", "dr_wav.h",
                         "glad.h", "glad_gles2.h", "miniaudio.h", "m3d.h", "par_shapes.h",
                         "pep.h", "qoa.h", "qoi.h", "rlsw.h", "rltexgpu.h", "rprand.h",
                         "sdefl.h", "sinfl.h", "stb_image.h", "stb_image_resize2.h",
                         "stb_image_write.h", "stb_perlin.h", "stb_rect_pack.h",
                         "stb_truetype.h", "stb_vorbis.c", "tinyobj_loader_c.h", "vox_loader.h",
                         "glfw/LICENSE.md", "RGFW/RGFW.h", "RGFW/deps/minigamepad.h",
                         "glfw/deps/wayland/wayland.xml"]:
            self.assertIn("src/external/" + expected, sources)
        for entry in entries:
            self.assertEqual(entry["source_url"], f"https://raw.githubusercontent.com/raysan5/raylib/{REVISION}/{entry['source_path']}")
            for key, length in [("source_sha256", 64), ("notice_sha256", 64), ("source_git_blob_sha1", 40)]:
                self.assertRegex(entry[key], re.compile(rf"^[0-9a-f]{{{length}}}$"))
            previous = 0
            for span in entry["ranges"]:
                self.assertGreaterEqual(span["start"], previous)
                self.assertGreater(span["end"], span["start"])
                self.assertLessEqual(span["end"], entry["source_bytes"])
                self.assertRegex(span["sha256"], r"^[0-9a-f]{64}$")
                previous = span["end"]
        self.assertTrue((self.notices / "README.md").is_file())

    def test_modified_missing_and_unlisted_notice_files_fail(self):
        entry = self.value["files"][0]
        path = self.notices / entry["notice_file"]
        original = path.read_bytes()
        path.write_bytes(original + b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            self.verify()
        path.unlink()
        with self.assertRaisesRegex(ValueError, "missing Raylib notice"):
            self.verify()
        path.write_bytes(original)
        (self.notices / "unlisted.txt").write_text("unreviewed")
        with self.assertRaisesRegex(ValueError, "unlisted"):
            self.verify()

    def test_wrong_revision_and_unpinned_source_url_fail(self):
        original = copy.deepcopy(self.value)
        self.value["revision"] = "0" * 40
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "SDK lock"):
            self.verify()
        self.value = original
        self.value["files"][0]["source_url"] = "https://raw.githubusercontent.com/raysan5/raylib/master/src/external/RGFW/RGFW.h"
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "not pinned"):
            self.verify()

    def test_escaping_and_duplicate_notice_paths_fail(self):
        original = copy.deepcopy(self.value)
        for path in ["../outside", "/outside", "sub/../outside", "sub\\outside"]:
            with self.subTest(path=path):
                self.value = copy.deepcopy(original)
                self.value["files"][0]["notice_file"] = path
                self.write_inventory()
                with self.assertRaisesRegex(ValueError, "notice path"):
                    self.verify()
        self.value = original
        self.value["files"].append(copy.deepcopy(self.value["files"][0]))
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.verify()

    def test_symlinked_notices_are_not_followed(self):
        path = self.notices / self.value["files"][0]["notice_file"]
        outside = self.root / "outside.txt"
        outside.write_bytes(path.read_bytes())
        path.unlink()
        try:
            path.symlink_to(outside)
        except OSError as error:
            self.skipTest(f"host cannot create symlinks: {error}")
        with self.assertRaisesRegex(ValueError, "symlinked"):
            self.verify()
        self.assertTrue(outside.is_file())

    def test_out_of_range_source_spans_and_wrong_block_hash_fail(self):
        original = copy.deepcopy(self.value)
        for span in [{"start": -1, "end": 1}, {"start": 0, "end": original["files"][0]["source_bytes"] + 1}]:
            self.value = copy.deepcopy(original)
            self.value["files"][0]["ranges"][0].update(span)
            self.write_inventory()
            with self.assertRaisesRegex(ValueError, "source range"):
                self.verify()
        self.value = original
        self.value["files"][0]["ranges"][0]["sha256"] = "0" * 64
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "block checksum"):
            self.verify()

    def test_rehashing_an_altered_separator_does_not_hide_mutation(self):
        entry = next(entry for entry in self.value["files"] if len(entry["ranges"]) > 1)
        path = self.notices / entry["notice_file"]
        offset = entry["ranges"][0]["end"] - entry["ranges"][0]["start"]
        data = bytearray(path.read_bytes())
        self.assertEqual(data[offset:offset + 2], b"\n\n")
        data[offset:offset + 2] = b"\nX"
        path.write_bytes(data)
        entry["notice_sha256"] = hashlib.sha256(data).hexdigest()
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "separator"):
            self.verify()

    def test_unaccounted_bytes_are_rejected_even_with_updated_file_hash(self):
        entry = self.value["files"][0]
        path = self.notices / entry["notice_file"]
        data = path.read_bytes() + b"extra"
        path.write_bytes(data)
        entry["notice_bytes"] = len(data)
        entry["notice_sha256"] = hashlib.sha256(data).hexdigest()
        self.write_inventory()
        with self.assertRaisesRegex(ValueError, "unaccounted"):
            self.verify()


if __name__ == "__main__":
    unittest.main()
