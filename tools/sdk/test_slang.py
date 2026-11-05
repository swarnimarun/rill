#!/usr/bin/env python3
"""Exercise the real Slang C ABI bridge. Pass a built shared-library path."""
import ctypes
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest


if len(sys.argv) < 2:
    raise SystemExit("usage: test_slang.py <librill_slang> [unittest options]")
LIBRARY = Path(sys.argv.pop(1)).resolve()


def entries(code):
    """Read OpEntryPoint names/stages without pretending to validate SPIR-V."""
    words = struct.unpack(f"<{len(code) // 4}I", code)
    if len(words) < 5 or words[0] != 0x07230203:
        raise AssertionError("compiler output is not SPIR-V")
    result = []
    offset = 5
    while offset < len(words):
        size, opcode = words[offset] >> 16, words[offset] & 0xFFFF
        if size < 1 or offset + size > len(words):
            raise AssertionError("malformed SPIR-V instruction")
        if opcode == 15:
            name = struct.pack(f"<{size - 3}I", *words[offset + 3:offset + size])
            result.append((words[offset + 1], name.split(b"\0", 1)[0].decode()))
        offset += size
    return result


class SlangBridge(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.library = ctypes.CDLL(str(LIBRARY))
        for name in ("RillSlang_compile_spirv", "RillSlang_compile_msl"):
            function = getattr(cls.library, name)
            function.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_int32, ctypes.c_char_p]
            function.restype = ctypes.c_int32

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rill slang sdk ")
        self.root = Path(self.temporary.name)
        self.source = self.root / "shader.slang"
        self.source.write_text(
            '[shader("vertex")] float4 vertex_main(float3 p : TEXCOORD0) : SV_Position '
            '{ return float4(p, 1); }\n'
            '[shader("fragment")] float4 fragment_main() : SV_Target0 '
            '{ return float4(1, 0, 0, 1); }\n'
            '[shader("compute")][numthreads(1,1,1)] void compute_main() {}\n'
        )
        self.output = self.root / "shader.output"

    def tearDown(self):
        self.temporary.cleanup()

    def compile(self, target, entry, stage, source=None, output=None):
        function = getattr(self.library, f"RillSlang_compile_{target}")
        return function(
            str(source or self.source).encode(), entry.encode(), stage,
            str(output or self.output).encode(),
        )

    def test_compiles_all_stages_to_named_spirv_and_msl(self):
        for entry, stage, execution in (("vertex_main", 0, 0), ("fragment_main", 1, 4), ("compute_main", 2, 5)):
            with self.subTest(entry=entry):
                self.assertEqual(self.compile("spirv", entry, stage), 0)
                code = self.output.read_bytes()
                self.assertEqual(struct.unpack_from("<I", code, 4)[0], 0x00010000)
                self.assertEqual(entries(code), [(execution, entry)])
                # Validation belongs to the offline toolchain. Exercise it when
                # present, while keeping the SDK bridge independent of a CLI.
                if shutil.which("spirv-val"):
                    subprocess.run(["spirv-val", "--target-env", "vulkan1.0", str(self.output)], check=True)
                self.assertEqual(self.compile("msl", entry, stage), 0)
                source = self.output.read_text()
                self.assertIn(entry, source)
                self.assertIn("metal_stdlib", source)

    def test_includes_and_imports_resolve_from_source_directory(self):
        (self.root / "included.slang").write_text("float4 shade() { return float4(1, 0, 0, 1); }\n")
        (self.root / "helper.slang").write_text("public float helper_value() { return 0.5; }\n")
        self.source.write_text(
            '#include "included.slang"\nimport helper;\n'
            '[shader("fragment")] float4 fragment_main() : SV_Target0 '
            '{ return shade() * helper_value(); }\n'
        )
        self.assertEqual(self.compile("spirv", "fragment_main", 1), 0)
        self.assertEqual(entries(self.output.read_bytes()), [(4, "fragment_main")])

    def test_invalid_arguments_do_not_create_or_replace_output(self):
        function = self.library.RillSlang_compile_spirv
        source, output = str(self.source).encode(), str(self.output).encode()
        self.output.write_bytes(b"old output")
        for arguments in (
            (None, b"compute_main", 2, output),
            (b"", b"compute_main", 2, output),
            (source, None, 2, output),
            (source, b"bad-name", 2, output),
            (source, b"compute_main", -1, output),
            (source, b"compute_main", 3, output),
            (source, b"compute_main", 2, None),
        ):
            self.assertEqual(function(*arguments), 1)
            self.assertEqual(self.output.read_bytes(), b"old output")
        original = self.source.read_bytes()
        self.assertEqual(self.compile("spirv", "compute_main", 2, output=self.source), 1)
        self.assertEqual(self.source.read_bytes(), original)

    def test_compiler_failures_preserve_existing_output_and_remove_temporaries(self):
        self.output.write_bytes(b"old output")
        self.assertEqual(self.compile("spirv", "missing", 2), 2)
        self.assertEqual(self.compile("spirv", "compute_main", 0), 2)
        self.source.write_text("this is not Slang source\n")
        self.assertEqual(self.compile("spirv", "compute_main", 2), 2)
        self.assertEqual(self.output.read_bytes(), b"old output")
        self.assertEqual(list(self.root.glob("*.rill-slang-*")), [])

    def test_io_failures_leave_directories_and_unrelated_outputs_intact(self):
        missing = self.root / "missing" / "output.spv"
        self.assertEqual(self.compile("spirv", "compute_main", 2, output=missing), 3)
        directory = self.root / "directory.spv"
        directory.mkdir()
        sentinel = directory / "sentinel"
        sentinel.write_bytes(b"unchanged")
        self.assertEqual(self.compile("spirv", "compute_main", 2, output=directory), 3)
        self.assertEqual(sentinel.read_bytes(), b"unchanged")
        self.assertEqual(list(self.root.glob("*.rill-slang-*")), [])


if __name__ == "__main__":
    unittest.main()
