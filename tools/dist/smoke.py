#!/usr/bin/env python3
"""Exercise a completed distribution without system shader/SDL discovery."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def run(binary, *arguments, env, cwd):
    output = subprocess.run([str(binary), *map(str, arguments)], env=env, cwd=cwd,
                            capture_output=True, text=True, timeout=180)
    if output.returncode:
        raise RuntimeError(f"command failed: {arguments}\n{output.stdout}\n{output.stderr}")
    return output.stdout


def smoke(prefix):
    binary = prefix / "bin/rill"
    sdk = prefix / "libexec/rill"
    marker = json.loads((sdk / "toolchain.json").read_text())
    # Remove inherited provider overrides so resolution must use this SDK.
    env = {key: value for key, value in os.environ.items()
           if key not in {"RILL_TOOLCHAIN_ROOT", "RILL_SLANGC", "SLANGC", "RILL_SPIRV_VAL",
                          "RILL_GLSLANG_VALIDATOR", "DYLD_LIBRARY_PATH", "LD_LIBRARY_PATH",
                          "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR"}}
    # A missing system package directory demonstrates bundled SDL doesn't depend
    # on the host's pkg-config paths. CC remains an explicit system prerequisite.
    env["PKG_CONFIG_LIBDIR"] = str(prefix / "no-system-packages")
    with tempfile.TemporaryDirectory(prefix="rill-dist-smoke-") as temporary:
        root = Path(temporary)
        # A working system installation must not conceal a broken SDK lookup.
        # Preserve CC on PATH, but make fallback to shader/package tools fail.
        trap = root / "system-tools-disabled"
        trap.mkdir()
        for name in ("slangc", "spirv-val", "pkg-config"):
            path = trap / name
            path.write_text("#!/bin/sh\nexit 99\n")
            path.chmod(0o755)
        env["PATH"] = str(trap) + os.pathsep + env.get("PATH", "")
        (root / "shader.slang").write_text('[shader("compute")][numthreads(1,1,1)] void main() {}\n')
        recipe = {"version": 1, "output": "build/shaders", "shaders": [
            {"name": "smoke", "language": "slang", "stage": "compute", "entry": "main",
             "source": "shader.slang", "target": "spirv"}]}
        (root / "rill.shaders.json").write_text(json.dumps(recipe))
        run(binary, "shader", "build", env=env, cwd=root)
        if not (root / "build/shaders/smoke.spv").is_file():
            raise RuntimeError("shader build did not produce SPIR-V")
        source = root / "sdl.rill"
        source.write_text('import std.sdl as sdl;\nfn main() {\n'
                          'if sdl.SDL_Init(u32(0)) { sdl.SDL_Delay(u32(1)); sdl.SDL_Quit(); 42 } else { 1 }\n}\n')
        result = run(binary, "run", source, env=env, cwd=root)
        if result != "42\n":
            raise RuntimeError(f"SDL smoke result: {result!r}")
        source.write_text('import std.raylib as ray; fn main() { '
                          'if ray.RillRaylib_InitWindow(0, 240, c"invalid") { 99 } '
                          'else { ray.RillRaylib_Status() } }\n')
        result = run(binary, "run", source, env=env, cwd=root)
        if result != "1\n":
            raise RuntimeError(f"Raylib SDK smoke result: {result!r}")
        source.write_text('import std.slang as slang; import std.shader as shader;\n'
                          'fn main() { slang.RillSlang_compile_spirv(c"shader.slang", c"main", '
                          'shader.stage_code(shader.Compute), c"sdk.spv") }\n')
        result = run(binary, "run", source, env=env, cwd=root)
        if result != "0\n":
            raise RuntimeError(f"Slang SDK smoke result: {result!r}")
        run(sdk / marker["tools"]["spirv-val"], "--target-env", "vulkan1.0", root / "sdk.spv", env=env, cwd=root)
        run(binary, "docs", "--language", "-o", root / "docs", env=env, cwd=root)
        if not (root / "docs/index.html").is_file():
            raise RuntimeError("bundled docs smoke failed")
    print("Distribution smoke passed: bundled Slang, SPIR-V validation, SDL, Raylib, SDK and docs")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: smoke.py <distribution-prefix>")
    smoke(Path(sys.argv[1]).resolve())
