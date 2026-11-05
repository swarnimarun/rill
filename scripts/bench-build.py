#!/usr/bin/env python3
"""Measure warm Rill CLI compilation and C compile/link latency.

Build the compiler with `cargo build --locked --release --bin rill`, then run
this script. Output is JSON; all generated files stay in a temporary directory.
Times include process startup and warm filesystem caches, but exclude execution
of the resulting program. Repeated Rill builds still perform full builds.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def execute(arguments):
    result = subprocess.run([str(value) for value in arguments], capture_output=True, check=False)
    if result.returncode:
        raise RuntimeError(f"command failed: {arguments[0]}\n{result.stderr.decode(errors='replace')}")
    return result.stdout


def measure(arguments, samples):
    execute(arguments)
    times = []
    for _ in range(samples):
        start = time.perf_counter_ns()
        execute(arguments)
        times.append((time.perf_counter_ns() - start) / 1_000_000)
    return {
        "median_ms": statistics.median(times),
        "min_ms": min(times),
        "max_ms": max(times),
        "samples_ms": times,
    }


def unused_library(directory, count):
    library = "export fn answer(value: i32) -> i32 { value + 1 }\n"
    library += "".join(
        f"fn unused{index:05}(value: i32) -> i32 {{ value + 1 }}\n"
        for index in range(count)
    )
    (directory / "library.rill").write_text(library)
    entry = directory / "main.rill"
    entry.write_text("import library; fn main() { library.answer(41) }\n")
    return entry, b"42\n", count + 2


def reached_functions(directory, count):
    source = "".join(
        f"fn leaf{index:05}(value: i32) -> i32 {{ (value + {index}) % 97 }}\n"
        for index in range(count)
    )
    names = [f"leaf{index:05}" for index in range(count)]
    layer = 0
    functions = count
    while len(names) > 1:
        next_names = []
        for start in range(0, len(names), 16):
            name = f"group{layer}_{start // 16}"
            expression = " + ".join(f"{child}(value)" for child in names[start:start + 16])
            source += f"fn {name}(value: i32) -> i32 {{ {expression} }}\n"
            next_names.append(name)
            functions += 1
        names = next_names
        layer += 1
    source += f"fn main() {{ {names[0]}(1) }}\n"
    entry = directory / "main.rill"
    entry.write_text(source)
    expected = sum((1 + index) % 97 for index in range(count))
    return entry, f"{expected}\n".encode(), functions + 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rill", type=Path, default=ROOT / "target/release/rill")
    parser.add_argument("--debug-rill", type=Path)
    parser.add_argument("--cc", default=os.environ.get("CC", "cc"))
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--unused", type=int, nargs="*", default=[100, 1000, 5000])
    parser.add_argument("--reached", type=int, nargs="*", default=[100, 900])
    arguments = parser.parse_args()
    if not 1 <= arguments.samples <= 100:
        parser.error("--samples must be between 1 and 100")
    if any(not 1 <= size <= 5000 for size in arguments.unused + arguments.reached):
        parser.error("synthetic sizes must be between 1 and 5000")
    if any(size > 900 for size in arguments.reached):
        parser.error("--reached must not exceed 900; group helpers also count toward the 1024-instance limit")
    rill = arguments.rill.resolve()
    debug_rill = arguments.debug_rill.resolve() if arguments.debug_rill else None
    report = {
        "platform": platform.system(),
        "architecture": platform.machine(),
        "compiler_sha256": hashlib.sha256(rill.read_bytes()).hexdigest(),
        "c_compiler": execute([arguments.cc, "--version"]).decode().splitlines()[0],
        "samples": arguments.samples,
        "warmups": 1,
        "workloads": [],
    }
    with tempfile.TemporaryDirectory(prefix="rill-build-bench-") as temporary:
        root = Path(temporary)
        fixtures = [
            ("factorial", ROOT / "examples/factorial.rill", b"120\n", None),
            ("stdlib_utilities", ROOT / "examples/stdlib/utilities.rill", b"42\n", None),
            ("stdlib_callbacks", ROOT / "examples/stdlib/options.rill", b"42\n", None),
        ]
        for count in arguments.unused:
            directory = root / f"unused-{count}"
            directory.mkdir()
            fixtures.append((f"unused_{count}", *unused_library(directory, count)))
        for count in arguments.reached:
            directory = root / f"reached-{count}"
            directory.mkdir()
            fixtures.append((f"reached_{count}", *reached_functions(directory, count)))
        for ordinal, (name, entry, expected, declared) in enumerate(fixtures):
            generated = execute([rill, "emit-c", entry, "--cc", arguments.cc])
            source = root / f"generated-{ordinal}.c"
            source.write_bytes(generated)
            output = root / f"program-{ordinal}"
            result = {
                "name": name,
                "synthetic_declarations": declared,
                "c_bytes": len(generated),
                "c_sha256": hashlib.sha256(generated).hexdigest(),
                "emitted_functions": len(re.findall(rb"^static [^\n]+ rl_fn_\d+\([^;\n]*\) \{", generated, re.M)),
                "check": measure([rill, "check", entry, "--cc", arguments.cc], arguments.samples),
                "emit_c": measure([rill, "emit-c", entry, "--cc", arguments.cc], arguments.samples),
            }
            if debug_rill:
                result["debug_check"] = measure([debug_rill, "check", entry, "--cc", arguments.cc], arguments.samples)
                result["debug_emit_c"] = measure([debug_rill, "emit-c", entry, "--cc", arguments.cc], arguments.samples)
            for level in [0, 3]:
                result[f"c_compile_link_o{level}"] = measure(
                    [arguments.cc, "-std=c11", f"-O{level}", source, "-o", output], arguments.samples
                )
                if execute([output]) != expected:
                    raise RuntimeError(f"{name}: independently expected output differs at C O{level}")
                result[f"rill_build_o{level}"] = measure(
                    [rill, "build", entry, "--cc", arguments.cc, "--opt-level", level, "-o", output],
                    arguments.samples,
                )
                if execute([output]) != expected:
                    raise RuntimeError(f"{name}: independently expected output differs at Rill O{level}")
            report["workloads"].append(result)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
