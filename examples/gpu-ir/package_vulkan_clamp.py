#!/usr/bin/env python3
"""Package the reviewed LLVM 22.1.8 v0 clamp as Shader-only Vulkan 1.2 SPIR-V.

Requires SPIRV-Tools v2026.1 on PATH. The raw module remains the default.
"""

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile


# The Rill-source-generated module and the standalone reference LLVM fixture.
REVIEWED_INPUTS = {
    "8076911e39c96d5abcfe27538d1b5e593a20cfab9216683d6eb8d848c28d1afa",
    "ddaa3517f4f33d8e0002688686f0cac49162be3fa2f2f40620dfcdc35334b552",
}
REVIEWED_SHADER_ONLY = "01a77deea42daf173b9c4a8cb5394c31f5cead7e13acfe49559c53cb0a5ce566"


def validate(path):
    subprocess.run(["spirv-val", "--target-env", "vulkan1.2", path], check=True)


def remove_reviewed_capability(source):
    lines = source.splitlines()
    instructions = [line.strip() for line in lines]
    capabilities = [line for line in instructions if line.startswith("OpCapability ")]
    if capabilities != ["OpCapability Shader", "OpCapability Int8"]:
        raise ValueError("expected exactly Shader and Int8 after optimization")

    # Require the inspected compute entry, all three descriptors, and the exact
    # surviving integer type. The final artifact digest covers every other word.
    required = {
        "; Version: 1.5",
        "OpMemoryModel Logical GLSL450",
        'OpEntryPoint GLCompute %rill_map "rill_map" %count %output %__spirv_BuiltInGlobalInvocationId %input',
        "OpExecutionMode %rill_map LocalSize 64 1 1",
        "OpDecorate %input Binding 0",
        "OpDecorate %output Binding 1",
        "OpDecorate %count Binding 2",
    }
    if not required.issubset(instructions):
        raise ValueError("optimized module differs from the reviewed clamp ABI")
    integers = [line for line in instructions if " = OpTypeInt " in line]
    if integers != ["%uint = OpTypeInt 32 0"]:
        raise ValueError("unexpected integer types in the optimized module")
    return "\n".join(line for line in lines if line.strip() != "OpCapability Int8") + "\n"


def package(source, destination):
    if source.resolve() == destination.resolve() or (
        destination.exists() and source.samefile(destination)
    ):
        raise ValueError("choose separate input and output files")
    # Snapshot once so optimization uses precisely the bytes that we validate.
    with source.open("rb") as stream:
        data = stream.read(64 * 1024 + 1)
    if len(data) > 64 * 1024:
        raise ValueError("input exceeds the clamp fixture size limit")

    # Stage on the destination filesystem. A failed check or external command
    # leaves any existing output intact.
    with tempfile.TemporaryDirectory(prefix=".rill-spirv-", dir=destination.parent) as staging:
        directory = Path(staging)
        raw = directory / "raw.spv"
        optimized = directory / "optimized.spv"
        assembly = directory / "optimized.spvasm"
        candidate = directory / "candidate.spv"
        raw.write_bytes(data)
        validate(raw)
        if hashlib.sha256(data).hexdigest() not in REVIEWED_INPUTS:
            raise ValueError("input digest is outside the reviewed LLVM 22.1.8 clamp fixtures")

        subprocess.run(["spirv-opt", "--target-env=vulkan1.2", "-O", "--trim-capabilities",
                        raw, "-o", optimized], check=True)
        validate(optimized)
        subprocess.run(["spirv-dis", optimized, "-o", assembly], check=True)
        assembly.write_text(remove_reviewed_capability(assembly.read_text(encoding="utf-8")),
                            encoding="utf-8")
        subprocess.run(["spirv-as", "--target-env", "vulkan1.2", assembly, "-o", candidate],
                       check=True)
        validate(candidate)
        if hashlib.sha256(candidate.read_bytes()).hexdigest() != REVIEWED_SHADER_ONLY:
            raise ValueError("candidate differs from the validated and dispatched Shader-only fixture")
        os.replace(candidate, destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="raw LLVM SPIR-V clamp")
    parser.add_argument("output", type=Path, help="separate Shader-only SPIR-V artifact")
    args = parser.parse_args()
    try:
        package(args.input, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"clamp packaging failed: {error}\n")
    print(f"{REVIEWED_SHADER_ONLY}  {args.output}")


if __name__ == "__main__":
    main()
