#!/usr/bin/env python3
"""Opt-in F3 gate: capture real SDL3 pixels with an already-built Rill compiler.

No Cargo invocation, installation, synthetic drawing, or existing-output reuse.
This is a Linux/GNU-linker harness, not a portable screenshot utility.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import struct
import subprocess
import sys
import time
import zlib


FUNCTIONS = (
    "SDL_Init", "SDL_CreateWindow", "SDL_CreateRenderer", "SDL_PumpEvents",
    "SDL_HasEvent", "SDL_GetMouseState", "SDL_SetRenderDrawColor",
    "SDL_RenderClear", "SDL_RenderLine", "SDL_RenderPresent", "SDL_Delay",
    "SDL_DestroyRenderer", "SDL_DestroyWindow", "SDL_Quit",
)
CONSTANTS = ("SDL_INIT_VIDEO", "SDL_EVENT_QUIT")
ENVIRONMENT = (
    "PATH", "LD_LIBRARY_PATH", "LD_PRELOAD", "LIBCLANG_PATH", "CLANG_PATH", "CC",
    "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR",
    "CPATH", "C_INCLUDE_PATH", "LIBRARY_PATH", "RILL_TEST_CLANG_RESOURCE_DIR",
    "LANG", "LC_ALL",
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def executable(value):
    found = shutil.which(value)
    require(found is not None, f"Required executable not found: {value}")
    # Preserve the selected executable's basename/argv[0], including wrappers
    # or compiler driver symlinks whose behavior depends on that spelling.
    return str(Path(found).absolute())


def new_output(path):
    require(not os.path.lexists(path), f"Refusing existing output: {path}")
    output = path.resolve()
    require(not os.path.lexists(output), f"Refusing existing output: {output}")
    output.mkdir()  # Atomic refusal also covers concurrent creators.
    return output


def generated_source(source):
    # The importer keeps the C enum constant's i32 type. Fail closed if this
    # narrow, documented adaptation no longer matches the actual game.
    old = "sdl.SDL_HasEvent(sdl.SDL_EVENT_QUIT)"
    require(source.count(old) == 1, "Expected exactly one direct quit-event call")
    return source.replace(old, "sdl.SDL_HasEvent(u32(sdl.SDL_EVENT_QUIT))", 1)


def require_typed(c_source):
    require(bool(c_source.strip()), "Empty emitted C")
    require(not re.search(r"\b(?:rl_value|RL_CAP)\b", c_source),
            "Emitted C still uses rl_value/RL_CAP; the typed F2 gate is not met")


def dependency_paths(text):
    """Read GCC/Clang -MD output (one rule), including escaped spaces/$/#.

    Inputs are trusted toolchain files, but never shell-evaluated. Refuse an
    unexpected makefile shape rather than report incomplete header identities.
    """
    text = text.replace("\\\n", "")
    colon = re.search(r"(?<!\\):", text)
    require(colon is not None, "Missing target in compiler dependency file")
    words, word, escaped = [], [], False
    for character in text[colon.end():]:
        if escaped:
            word.append(character)
            escaped = False
        elif character == "\\":
            escaped = True
        elif character.isspace():
            if word:
                words.append("".join(word).replace("$$", "$"))
                word = []
        else:
            require(character not in "#:", "Unexpected dependency-file syntax")
            word.append(character)
    require(not escaped, "Truncated dependency-file escape")
    if word:
        words.append("".join(word).replace("$$", "$"))
    require(words, "Empty compiler dependency list")
    return words


def read_bmp(data):
    """Decode SDL's uncompressed 24/32-bit BMP into top-down RGBA bytes.

    BI_RGB 32-bit's unused byte is not alpha. Explicit alpha masks are honored.
    Reject other encodings rather than guess or silently redraw their pixels.
    """
    require(len(data) >= 54 and data[:2] == b"BM", "Not a Windows BMP")
    size, offset, dib = (struct.unpack_from("<I", data, p)[0] for p in (2, 10, 14))
    require(size == len(data), "BMP size does not match the file")
    require(dib in (40, 52, 56, 108, 124) and 14 + dib <= offset <= len(data),
            "Unsupported or truncated BMP header")
    width, signed_height, planes, bits, compression = struct.unpack_from("<iiHHI", data, 18)
    height = abs(signed_height)
    require(0 < width <= 4096 and 0 < height <= 4096 and planes == 1,
            "Invalid or oversized BMP dimensions")
    require((bits == 24 and compression == 0) or
            (bits == 32 and compression in (0, 3, 6)), "Unsupported BMP encoding")
    masks = (0x00FF0000, 0x0000FF00, 0x000000FF, 0)
    if compression in (3, 6):
        mask_count = 4 if dib >= 56 or compression == 6 else 3
        require(offset >= 54 + 4 * mask_count, "Truncated BMP channel masks")
        masks = struct.unpack_from("<" + "I" * mask_count, data, 54)
        if mask_count == 3:
            masks += (0,)
        require(all(mask in (0xFF, 0xFF00, 0xFF0000, 0xFF000000) for mask in masks[:3])
                and masks[3] in (0, 0xFF, 0xFF00, 0xFF0000, 0xFF000000),
                "Unsupported BMP channel masks")
        nonzero = [mask for mask in masks if mask]
        require(len(set(nonzero)) == len(nonzero), "Overlapping BMP channel masks")
    stride = ((width * bits + 31) // 32) * 4
    require(offset + stride * height == len(data), "Truncated or trailing BMP pixels")
    shifts = [(mask.bit_length() - 8) if mask else None for mask in masks]
    rgba = bytearray()
    for y in range(height):
        row = y if signed_height < 0 else height - 1 - y
        start = offset + row * stride
        for x in range(width):
            pos = start + x * (bits // 8)
            if bits == 24:
                b, g, r = data[pos:pos + 3]
                rgba.extend((r, g, b, 255))
            else:
                value = struct.unpack_from("<I", data, pos)[0]
                rgba.extend((value >> shift) & 255 if shift is not None else 255
                            for shift in shifts)
    return width, height, bytes(rgba)


def write_png(path, width, height, rgba):
    """Lossless format conversion only: no resizing, annotation, or drawing."""
    require(len(rgba) == width * height * 4, "Invalid RGBA byte count")

    def chunk(kind, payload):
        return (struct.pack(">I", len(payload)) + kind + payload +
                struct.pack(">I", zlib.crc32(kind + payload)))

    scanlines = b"".join(b"\0" + rgba[y * width * 4:(y + 1) * width * 4]
                         for y in range(height))
    with path.open("xb") as stream:
        stream.write(b"\x89PNG\r\n\x1a\n")
        stream.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)))
        stream.write(chunk(b"IDAT", zlib.compress(scanlines)))
        stream.write(chunk(b"IEND", b""))


def check_pixels(game, frame, width, height, rgba):
    """Check distinctive actual game pixels, without manufacturing a frame."""
    expected_height = 480 if game == "collector" else 400
    require((width, height) == (640, expected_height), "Unexpected game dimensions")
    if game == "collector":
        probes = ((10, 10, (18, 22, 32, 255)), (110, 110, (242, 190, 58, 255)),
                  (310, 230, (80, 165, 250, 255)))
    else:
        x = 40 + ((frame - 1) * 7) % 560
        probes = ((10, 10, (20, 24, 32, 255)), (280, 130, (70, 210, 140, 255)),
                  (360, 130, (70, 210, 140, 255)), (x, 200, (250, 190, 60, 255)),
                  (40, 350, (20, 24, 32, 255)))
    for x, y, expected in probes:
        start = (y * width + x) * 4
        actual = tuple(rgba[start:start + 4])
        require(actual == expected, f"{game} pixel ({x},{y}): {actual}, expected {expected}")
    return [{"x": x, "y": y, "rgba": list(color)} for x, y, color in probes]


class Runner:
    def __init__(self, root, output, timeout):
        self.root, self.output, self.timeout = root, output, timeout
        self.count = 0
        (output / "logs").mkdir()

    def record(self, event):
        with (self.output / "commands.jsonl").open("a") as stream:
            stream.write(json.dumps(event, sort_keys=True) + "\n")

    def run(self, argv, *, env=None, stdout=None, seconds=60):
        self.count += 1
        stem = f"logs/{self.count:03d}"
        stdout = stdout or self.output / (stem + ".stdout")
        stderr = self.output / (stem + ".stderr")
        argv = [str(arg) for arg in argv]
        command = [self.timeout, "--kill-after=5s", f"{seconds}s", *argv]
        environment = dict(os.environ, **(env or {}))
        started = time.time()
        record = {"command_id": self.count, "event": "started", "argv": command,
                  "cwd": str(self.root), "environment_overrides": env or {},
                  "started_unix": started,
                  "stdout": str(stdout.relative_to(self.output)),
                  "stderr": str(stderr.relative_to(self.output))}
        # Close the start record before launching, so even a spawn failure or
        # interruption retains the exact attempted argv and log destinations.
        self.record(record)
        outcome = {"command_id": self.count, "event": "result"}
        try:
            with stdout.open("xb") as out, stderr.open("xb") as err:
                result = subprocess.run(command, cwd=self.root, env=environment,
                                        stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                        check=False)
            outcome.update(status="returned", returncode=result.returncode)
        except (OSError, KeyboardInterrupt) as error:
            # This describes the observer's result, not a promise that every
            # descendant terminated after an interruption or host failure.
            outcome.update(status="interrupted" if isinstance(error, KeyboardInterrupt) else "error",
                           error_type=type(error).__name__, error=str(error), returncode=None)
            raise
        finally:
            outcome["duration_seconds"] = time.time() - started
            self.record(outcome)
        require(result.returncode == 0,
                f"Command {self.count} failed ({result.returncode}); see {stderr}")
        return stdout.read_bytes(), stderr.read_text(errors="replace")


def capture(args, root, output, manifest, run, rill, cc, clang, pkg_config, git):
    manifest["tools"] = {
        name: {"path": path, "sha256": digest(Path(path))}
        for name, path in (("rill", rill), ("cc", cc), ("clang", clang),
                           ("python", sys.executable), ("timeout", run.timeout),
                           ("pkg_config", pkg_config), ("git", git))
    }
    for name, argv in (("cc", [cc, "--version"]), ("clang", [clang, "--version"]),
                       ("pkg_config", [pkg_config, "--version"]), ("git", [git, "--version"])):
        manifest["tools"].setdefault(name, {})["version"] = run.run(argv)[0].decode().strip()
    manifest["git_commit"] = run.run([git, "rev-parse", "HEAD"])[0].decode().strip()
    manifest["git_status"] = run.run([git, "status", "--porcelain=v1"])[0].decode()
    run.run([git, "diff", "HEAD", "--binary"], stdout=output / "checkout.diff")
    manifest["sdl_version"] = run.run([pkg_config, "--modversion", "sdl3"])[0].decode().strip()
    cflags = shlex.split(run.run([pkg_config, "--cflags", "sdl3"])[0].decode())
    libs = shlex.split(run.run([pkg_config, "--libs", "sdl3"])[0].decode())
    manifest["pkg_config"] = {"cflags": cflags, "libs": libs}
    resource = args.clang_resource_dir
    if not resource:
        resource = run.run([clang, "-print-resource-dir"])[0].decode().strip()
    require(Path(resource).is_dir(), "Clang resource directory is missing")
    resource = str(Path(resource).resolve())
    manifest["clang_resource_dir"] = resource

    sources = output / "sources"
    sources.mkdir()
    for game in ("direct", "generated", "collector"):
        (sources / game).mkdir()
    copies = {
        "examples/sdl3-direct/main.rill": "direct/main.rill",
        "examples/sdl3-direct/sdl.rill": "direct/sdl.rill",
        "examples/sdl3/collector.rill": "collector/main.rill",
        "examples/sdl3/shim.c": "collector/shim.c",
        "scripts/sdl3-capture.c": "capture.c",
        "scripts/capture-sdl3-matrix.py": "capture-sdl3-matrix.py",
    }
    manifest["inputs"] = {}
    for source, destination in copies.items():
        data = (root / source).read_bytes()
        with (sources / destination).open("xb") as stream:
            stream.write(data)
        manifest["inputs"][source] = hashlib.sha256(data).hexdigest()
    original = (sources / "direct/main.rill").read_text()
    (sources / "generated/main.rill").write_text(generated_source(original))
    manifest["generated_adaptation"] = "Only SDL_HasEvent's SDL_EVENT_QUIT argument gains u32(...)."
    command = [rill, "import-c", "--header", "SDL3/SDL.h", "--pkg-config", "sdl3",
               "--link", "SDL3", "--cc", cc, "--clang-resource-dir", resource]
    for name in CONSTANTS:
        command.extend(("--constant", name))
    for name in FUNCTIONS:
        command.extend(("--function", name))
    command.extend(("-o", sources / "generated/sdl.rill"))
    run.run(command)

    pixels = {}
    for opt in (0, 2):
        opt_dir = output / f"O{opt}"
        opt_dir.mkdir()
        flags = ["-std=c11", *cflags, f"-O{opt}"]
        objects = {}
        for name, source in (("capture", sources / "capture.c"),
                             ("collector", sources / "collector/shim.c")):
            objects[name] = opt_dir / f"{name}.o"
            run.run([cc, *flags, "-Wall", "-Wextra", "-Werror", "-MD", "-MF",
                     opt_dir / f"{name}.d", "-c", source, "-o", objects[name]])
        for game in ("direct", "generated", "collector"):
            cell = opt_dir / game
            cell.mkdir()
            c_source = cell / "program.c"
            run.run([rill, "emit-c", sources / game / "main.rill", "--cc", cc,
                     "--c-arg", f"-O{opt}"], stdout=c_source)
            require_typed(c_source.read_text())
            # Compile the exact retained C whose representation was inspected.
            # This is not a substitute for separate Rill build/run CLI tests.
            command = [cc, *flags, "-Werror=incompatible-pointer-types",
                       "-Werror=implicit-function-declaration", "-MD", "-MF", cell / "program.d",
                       c_source, objects["capture"]]
            if game == "collector":
                command.append(objects["collector"])
            command.extend(("-Wl,--wrap=SDL_RenderPresent", *libs, "-o", cell / "game"))
            run.run(command)
            bmp = cell / "frame.bmp"
            stdout, stderr = run.run([cell / "game"], seconds=30, env={
                "SDL_VIDEODRIVER": "dummy", "SDL_RENDER_DRIVER": "software",
                "RILL_CAPTURE_FRAME": str(args.frame), "RILL_CAPTURE_BMP": str(bmp),
            })
            require(stdout == b"0\n", f"Unexpected {game} result: {stdout!r}")
            width, height, rgba = read_bmp(bmp.read_bytes())
            line = rf"^captured frame {args.frame}: {width}x{height}, SDL [0-9]+, renderer software, video dummy$"
            require(len(re.findall(line, stderr, re.MULTILINE)) == 1,
                    f"Missing real software/dummy capture identity for {game}")
            probes = check_pixels(game, args.frame, width, height, rgba)
            write_png(cell / "frame.png", width, height, rgba)
            key = f"O{opt}/{game}"
            pixels[key] = (width, height, rgba)
            manifest["captures"][key] = {
                "width": width, "height": height, "rgba_sha256": hashlib.sha256(rgba).hexdigest(),
                "bmp_sha256": digest(bmp), "png_sha256": digest(cell / "frame.png"),
                "typed_representation_check": "passed", "pixel_probes": probes,
            }
    # Preserve content identities of the actual preprocessed C inputs, including
    # transitive and system headers, rather than only SDL's reported version.
    manifest["c_dependencies"] = {}
    for depfile in sorted(output.glob("O*/**/*.d")):
        for name in dependency_paths(depfile.read_text()):
            path = Path(name)
            path = (root / path).resolve() if not path.is_absolute() else path.resolve()
            manifest["c_dependencies"][str(path)] = digest(path)
    for left, right in (("O0/direct", "O0/generated"), ("O2/direct", "O2/generated"),
                        ("O0/direct", "O2/direct"), ("O0/generated", "O2/generated"),
                        ("O0/collector", "O2/collector")):
        equal = pixels[left] == pixels[right]
        manifest["comparisons"].append({"left": left, "right": right, "equal_rgba": equal})
        require(equal, f"Pixel mismatch: {left} versus {right}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new output directory; existing paths are refused")
    parser.add_argument("--rill", default=os.environ.get("RILL_BIN"),
                        help="already-built c-import-enabled executable; or set RILL_BIN")
    parser.add_argument("--cc", default=os.environ.get("CC", "cc"), help="one C compiler executable")
    parser.add_argument("--clang", default=os.environ.get("CLANG_PATH", "clang"))
    parser.add_argument("--clang-resource-dir", default=os.environ.get("RILL_TEST_CLANG_RESOURCE_DIR"))
    parser.add_argument("--frame", type=int, default=os.environ.get("RILL_CAPTURE_FRAME", "40"))
    args = parser.parse_args()
    require(args.rill, "Supply --rill or RILL_BIN; this script never builds a compiler")
    require(1 <= args.frame <= 900, "Capture frame must be from 1 to 900")
    require(sys.platform.startswith("linux"), "This harness requires Linux/GNU linker --wrap")
    root = Path(__file__).resolve().parent.parent
    rill, cc, clang = (executable(value) for value in (args.rill, args.cc, args.clang))
    timeout = executable("timeout")
    pkg_config = executable("pkg-config")
    git = executable("git")
    output = new_output(args.output)
    manifest = {"schema": 1, "status": "failed", "frame": args.frame,
                "checkout": str(root), "captures": {}, "comparisons": [],
                "environment": {key: value for key, value in os.environ.items()
                                if key in ENVIRONMENT or key.startswith("SDL_")},
                "python_version": sys.version, "platform": list(os.uname()),
                "limits": ["Real native headless software frames, not desktop screenshots.",
                           "No physical keyboard/mouse input or hardware renderer tested.",
                           "Rill binary hash recorded; matching it to checkout requires build provenance.",
                           "This bounded SDL gate alone does not complete F3, P1, or P5."]}
    try:
        capture(args, root, output, manifest, Runner(root, output, timeout),
                rill, cc, clang, pkg_config, git)
        manifest["status"] = "passed"
    except (OSError, ValueError, KeyboardInterrupt) as error:
        manifest["error"] = str(error) or "Interrupted"
        raise
    finally:
        manifest["artifacts"] = {
            str(path.relative_to(output)): {"sha256": digest(path), "bytes": path.stat().st_size}
            for path in sorted(output.rglob("*")) if path.is_file()
        }
        with (output / "manifest.json").open("x") as stream:
            json.dump(manifest, stream, indent=2, sort_keys=True)
            stream.write("\n")
    print(f"Passed six real SDL3 captures and five pixel comparisons: {output}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyboardInterrupt) as error:
        print(f"SDL3 capture gate failed: {error}", file=sys.stderr)
        sys.exit(1)
