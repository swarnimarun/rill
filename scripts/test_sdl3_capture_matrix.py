"""Pure harness tests. Tiny format fixtures are not application screenshots.

Run separately with: python3 -B -m unittest discover -s scripts -p 'test_sdl3_capture_matrix.py'
These tests never build or launch Rill, C, SDL, or a mock game.
"""

import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock
import zlib


spec = importlib.util.spec_from_file_location(
    "capture_matrix", Path(__file__).with_name("capture-sdl3-matrix.py"))
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)


def bmp24(top_down=False):
    # A 2x2 RGB format fixture with row padding, intentionally not a game image.
    top = bytes((0, 0, 255, 0, 255, 0, 0, 0))
    bottom = bytes((255, 0, 0, 255, 255, 255, 0, 0))
    header = struct.pack("<2sIHHI", b"BM", 70, 0, 0, 54)
    dib = struct.pack("<IiiHHIIiiII", 40, 2, -2 if top_down else 2,
                      1, 24, 0, 16, 0, 0, 0, 0)
    return header + dib + (top + bottom if top_down else bottom + top)


def bmp32(compression=3, masks=(0xFF0000, 0xFF00, 0xFF, 0xFF000000)):
    # BITMAPV4HEADER with explicit channel masks and one nonopaque pixel.
    header = struct.pack("<2sIHHI", b"BM", 126, 0, 0, 122)
    dib = struct.pack("<IiiHHIIiiII", 108, 1, 1, 1, 32, compression, 4, 0, 0, 0, 0)
    dib += struct.pack("<IIII", *masks) + bytes(52)
    return header + dib + struct.pack("<I", 0x7F112233)


class FormatTests(unittest.TestCase):
    def test_24_bit_orientation_and_padding(self):
        expected = bytes((255, 0, 0, 255, 0, 255, 0, 255,
                          0, 0, 255, 255, 255, 255, 255, 255))
        for top_down in (True, False):
            self.assertEqual(matrix.read_bmp(bmp24(top_down)), (2, 2, expected))

    def test_32_bit_explicit_alpha(self):
        self.assertEqual(matrix.read_bmp(bmp32()), (1, 1, bytes((17, 34, 51, 127))))

    def test_32_bit_rgb_unused_byte_is_opaque(self):
        self.assertEqual(matrix.read_bmp(bmp32(compression=0)),
                         (1, 1, bytes((17, 34, 51, 255))))

    def test_alternative_channel_order(self):
        self.assertEqual(matrix.read_bmp(bmp32(masks=(0xFF, 0xFF00, 0xFF0000, 0xFF000000))),
                         (1, 1, bytes((51, 34, 17, 127))))

    def test_rejects_bad_headers_and_encodings(self):
        for data in (b"", b"not a bitmap", bmp24()[:-1], bmp24() + b"x"):
            with self.assertRaises(ValueError):
                matrix.read_bmp(data)
        for position, fmt, value in ((14, "I", 12), (18, "i", 0), (22, "i", 4097),
                                     (26, "H", 2), (28, "H", 8), (30, "I", 1),
                                     (10, "I", 53)):
            data = bytearray(bmp24())
            struct.pack_into("<" + fmt, data, position, value)
            with self.assertRaises(ValueError, msg=str((position, value))):
                matrix.read_bmp(data)

    def test_rejects_overlapping_or_nonbyte_masks(self):
        for masks in ((0xFF, 0xFF, 0xFF0000, 0), (0xF0, 0xFF00, 0xFF0000, 0)):
            with self.assertRaises(ValueError):
                matrix.read_bmp(bmp32(masks=masks))

    def test_png_round_trip_and_existing_output_refusal(self):
        width, height, rgba = matrix.read_bmp(bmp24())
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pixels.png"
            matrix.write_png(path, width, height, rgba)
            data = path.read_bytes()
            self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
            pos, chunks = 8, {}
            while pos < len(data):
                length = struct.unpack_from(">I", data, pos)[0]
                kind, payload = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + length]
                crc = struct.unpack_from(">I", data, pos + 8 + length)[0]
                self.assertEqual(crc, zlib.crc32(kind + payload))
                chunks[kind] = payload
                pos += length + 12
            self.assertEqual(struct.unpack(">IIBBBBB", chunks[b"IHDR"]), (2, 2, 8, 6, 0, 0, 0))
            self.assertEqual(zlib.decompress(chunks[b"IDAT"]), b"\0" + rgba[:8] + b"\0" + rgba[8:])
            self.assertEqual(chunks[b"IEND"], b"")
            with self.assertRaises(FileExistsError):
                matrix.write_png(path, width, height, rgba)
            self.assertEqual(path.read_bytes(), data)

    def test_pixel_gate_rejects_blank_wrong_size_or_wrong_color(self):
        with self.assertRaises(ValueError):
            matrix.check_pixels("direct", 40, 2, 2, bytes(16))
        with self.assertRaises(ValueError):
            matrix.check_pixels("direct", 40, 640, 400, bytes(640 * 400 * 4))


class SourceTests(unittest.TestCase):
    def test_output_refuses_file_directory_and_dangling_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = matrix.new_output(root / "new output with spaces")
            (output / "retained.bmp").write_bytes(b"untouched")
            file = root / "file"
            file.write_bytes(b"untouched")
            dangling = root / "dangling"
            dangling.symlink_to(root / "does-not-exist")
            link = root / "link"
            link.symlink_to(output, target_is_directory=True)
            for path in (output, file, dangling, link):
                with self.assertRaises(ValueError):
                    matrix.new_output(path)
            self.assertEqual(file.read_bytes(), b"untouched")
            self.assertEqual((output / "retained.bmp").read_bytes(), b"untouched")

    def test_adaptation_changes_only_quit_argument(self):
        root = Path(__file__).resolve().parent.parent
        original = (root / "examples/sdl3-direct/main.rill").read_text()
        generated = matrix.generated_source(original)
        normalized = "sdl.SDL_HasEvent(u32(sdl.SDL_EVENT_QUIT))"
        legacy = "sdl.SDL_HasEvent(sdl.SDL_EVENT_QUIT)"
        self.assertEqual(generated, original.replace(legacy, normalized))
        self.assertEqual(matrix.generated_source(generated), generated)
        self.assertIn("frame < 900", generated)
        self.assertIn("SDL_Delay(u32(16))", generated)

    def test_adaptation_fails_closed_on_source_drift(self):
        for source in ("", "sdl.SDL_HasEvent(sdl.SDL_EVENT_QUIT)" * 2,
                       "sdl.SDL_HasEvent(u32(sdl.SDL_EVENT_QUIT))" * 2,
                       "sdl.SDL_HasEvent(sdl.SDL_EVENT_QUIT)"
                       "sdl.SDL_HasEvent(u32(sdl.SDL_EVENT_QUIT))"):
            with self.assertRaises(ValueError):
                matrix.generated_source(source)

    def test_uniform_representation_rejected(self):
        for source in ("", "typedef int rl_value;", "#define RL_CAP 32"):
            with self.assertRaises(ValueError):
                matrix.require_typed(source)
        matrix.require_typed("int main(void) { return 0; }")

    def test_dependencies_preserve_escaped_paths(self):
        text = "game: /tmp/file\\ with\\ spaces.c \\\n /usr/include/SDL3/SDL.h /tmp/dollar$$.h /tmp/hash\\#.h\n"
        self.assertEqual(matrix.dependency_paths(text),
                         ["/tmp/file with spaces.c", "/usr/include/SDL3/SDL.h",
                          "/tmp/dollar$.h", "/tmp/hash#.h"])

    def test_dependencies_reject_unknown_or_empty_rule(self):
        for text in ("no rule", "game:", "game: x.h\nother: y.h", "game: dangling\\"):
            with self.assertRaises(ValueError):
                matrix.dependency_paths(text)


class RunnerTests(unittest.TestCase):
    # subprocess.run is always replaced here: no external tool is executed.
    def records(self, output):
        return [json.loads(line) for line in (output / "commands.jsonl").read_text().splitlines()]

    def test_start_record_precedes_launch_and_return_records_status(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runner = matrix.Runner(output, output, "/tool path/timeout")

            def simulated_run(argv, **kwargs):
                records = self.records(output)
                self.assertEqual(len(records), 1)
                self.assertEqual(records[0]["event"], "started")
                self.assertEqual(records[0]["argv"], argv)
                self.assertEqual(argv, ["/tool path/timeout", "--kill-after=5s", "30s",
                                        "/tool path/compiler", "one argument; no shell"])
                self.assertNotIn("shell", kwargs)
                self.assertEqual(kwargs["env"]["TEST_OVERRIDE"], "value")
                kwargs["stdout"].write(b"output\n")
                kwargs["stderr"].write(b"diagnostic\n")
                return matrix.subprocess.CompletedProcess(argv, 0)

            with mock.patch.object(matrix.subprocess, "run", side_effect=simulated_run) as launch:
                result = runner.run(["/tool path/compiler", "one argument; no shell"],
                                    env={"TEST_OVERRIDE": "value"}, seconds=30)
            launch.assert_called_once()
            self.assertEqual(result, (b"output\n", "diagnostic\n"))
            records = self.records(output)
            self.assertEqual(len(records), 2)
            self.assertEqual(records[1]["command_id"], records[0]["command_id"])
            self.assertEqual(records[1]["event"], "result")
            self.assertEqual(records[1]["status"], "returned")
            self.assertEqual(records[1]["returncode"], 0)

    def test_spawn_failure_retains_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runner = matrix.Runner(output, output, "/missing/timeout")
            with mock.patch.object(matrix.subprocess, "run", side_effect=FileNotFoundError("missing")):
                with self.assertRaises(FileNotFoundError):
                    runner.run(["unlaunched-tool"])
            records = self.records(output)
            self.assertEqual(records[0]["argv"][-1], "unlaunched-tool")
            self.assertEqual(records[1]["status"], "error")
            self.assertEqual(records[1]["error_type"], "FileNotFoundError")
            self.assertIsNone(records[1]["returncode"])

    def test_interruption_retains_attempt_without_claiming_child_status(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runner = matrix.Runner(output, output, "/mock/timeout")
            with mock.patch.object(matrix.subprocess, "run", side_effect=KeyboardInterrupt):
                with self.assertRaises(KeyboardInterrupt):
                    runner.run(["unlaunched-tool"])
            records = self.records(output)
            self.assertEqual(records[0]["event"], "started")
            self.assertEqual(records[1]["status"], "interrupted")
            self.assertEqual(records[1]["error_type"], "KeyboardInterrupt")
            self.assertIsNone(records[1]["returncode"])

    def test_nonzero_exit_is_recorded_then_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            runner = matrix.Runner(output, output, "/mock/timeout")
            with mock.patch.object(matrix.subprocess, "run",
                                   return_value=matrix.subprocess.CompletedProcess([], 124)):
                with self.assertRaises(ValueError):
                    runner.run(["unlaunched-tool"])
            records = self.records(output)
            self.assertEqual(records[1]["status"], "returned")
            self.assertEqual(records[1]["returncode"], 124)


if __name__ == "__main__":
    unittest.main()
