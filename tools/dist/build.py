#!/usr/bin/env python3
"""Build a native, relocatable Rill graphics distribution. Never publishes it."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request

REPO = Path(__file__).resolve().parents[2]
LOCK = json.loads((REPO / "tools/dist/dependencies.json").read_text())
VERSION = tomllib.loads((REPO / "Cargo.toml").read_text())["package"]["version"]


def run(*args, cwd=REPO, env=None):
    print("+", " ".join(str(arg) for arg in args), flush=True)
    subprocess.run([str(arg) for arg in args], cwd=cwd, env=env, check=True)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def fetch(spec, downloads):
    """Only a verified complete file gets the cache name."""
    path = downloads / (spec["sha256"] + ".tar.gz")
    if path.exists():
        if digest(path) == spec["sha256"]:
            return path
        raise ValueError(f"checksum mismatch in cached download {path}")
    request = urllib.request.Request(spec["url"], headers={"User-Agent": "rill-distribution"})
    with tempfile.NamedTemporaryFile(dir=downloads, delete=False) as temporary:
        candidate = Path(temporary.name)
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                shutil.copyfileobj(response, temporary)
            temporary.close()
            if digest(candidate) != spec["sha256"]:
                raise ValueError(f"SHA-256 mismatch for {spec['url']}")
            candidate.replace(path)
        finally:
            candidate.unlink(missing_ok=True)
    return path


def unpack(archive, destination):
    # Python's data filter rejects absolute paths, escaping symlinks and special
    # files. Extract into a fresh private directory before choosing the root.
    destination.mkdir()
    with tarfile.open(archive) as source:
        source.extractall(destination, filter="data")
    entries = list(destination.iterdir())
    if len(entries) == 1 and entries[0].is_dir():
        return entries[0]
    return destination


def host_target():
    machine = platform.machine().lower()
    arch = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64", "amd64": "x86_64"}.get(machine)
    suffix = {"Darwin": "apple-darwin", "Linux": "unknown-linux-gnu"}.get(platform.system())
    target = f"{arch}-{suffix}"
    if target not in LOCK["slang"]["assets"]:
        raise ValueError(f"no default SDK lock for host {machine}/{platform.system()}")
    return target


def cmake(source, build, install, jobs, extra=()):
    run("cmake", "-S", source, "-B", build, "-DCMAKE_BUILD_TYPE=Release",
        f"-DCMAKE_INSTALL_PREFIX={install}", *extra)
    run("cmake", "--build", build, "--config", "Release", "--parallel", jobs)
    run("cmake", "--install", build, "--config", "Release")


def licenses(source, destination):
    destination.mkdir(parents=True, exist_ok=True)
    found = False
    for path in source.iterdir():
        if path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "THIRD-PARTY-NOTICES")):
            if path.is_dir():
                shutil.copytree(path, destination / path.name, symlinks=True)
            else:
                shutil.copy2(path, destination / path.name)
            found = True
    if not found:
        raise ValueError(f"no license notices found in {source}")


def build(work, output, jobs, binary=None):
    target = host_target()
    name = f"rill-{VERSION}-{target}"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{name}.tar.gz"
    if archive.exists() or (output / name).exists():
        raise ValueError(f"output already exists for {name}; choose a fresh output directory")
    downloads = work / "downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    specs = {"slang": LOCK["slang"]["assets"][target],
             **{key: LOCK[key] for key in ("sdl3", "spirv-tools", "spirv-headers")}}
    archives = {key: fetch(spec, downloads) for key, spec in specs.items()}
    # A unique stage prevents overlapping package builds from sharing object
    # files, source mutations, install directories or partially built archives.
    with tempfile.TemporaryDirectory(prefix="rill-dist-", dir=work) as temporary:
        stage = Path(temporary)
        prefix = stage / name
        sdk = prefix / "libexec/rill"
        (prefix / "bin").mkdir(parents=True)
        sdk.mkdir(parents=True)
        sources = {key: unpack(path, stage / key) for key, path in archives.items()}
        shutil.copytree(sources["slang"], sdk / "slang", symlinks=True)
        rpath = "@loader_path" if sys.platform == "darwin" else "$ORIGIN"
        cmake(sources["sdl3"], stage / "sdl-build", sdk / "sdl3", jobs, [
            "-DSDL_SHARED=ON", "-DSDL_STATIC=OFF", "-DSDL_TESTS=OFF",
            "-DSDL_TEST_LIBRARY=OFF", "-DSDL_EXAMPLES=OFF", "-DSDL_FRAMEWORK=OFF",
            "-DCMAKE_INSTALL_LIBDIR=lib", "-DCMAKE_INSTALL_NAME_DIR=@rpath",
            f"-DCMAKE_INSTALL_RPATH={rpath}"])
        # Only the validator and its static implementation are needed. Building
        # upstream tests/optimizers would pull unrelated source dependencies.
        spirv_build = stage / "spirv-build"
        run("cmake", "-S", sources["spirv-tools"], "-B", spirv_build,
            "-DCMAKE_BUILD_TYPE=Release", "-DSPIRV_SKIP_TESTS=ON",
            "-DSPIRV_SKIP_EXECUTABLES=OFF", "-DSPIRV_TOOLS_BUILD_STATIC=ON",
            f"-DSPIRV-Headers_SOURCE_DIR={sources['spirv-headers']}")
        run("cmake", "--build", spirv_build, "--target", "spirv-val", "--parallel", jobs)
        (sdk / "spirv/bin").mkdir(parents=True)
        shutil.copy2(spirv_build / "tools/spirv-val", sdk / "spirv/bin/spirv-val")
        cmake(REPO / "tools/sdk", stage / "sdk-build", sdk / "sdk", jobs,
              [f"-DSLANG_ROOT={sdk / 'slang'}", "-DCMAKE_INSTALL_LIBDIR=lib"])
        test_env = os.environ.copy()
        test_env["PATH"] = str(sdk / "spirv/bin") + os.pathsep + test_env.get("PATH", "")
        run("ctest", "--test-dir", stage / "sdk-build", "--output-on-failure", env=test_env)
        if binary is None:
            cargo_target = work / "cargo"
            run("cargo", "build", "--release", "--locked", "--target", target,
                "--target-dir", cargo_target)
            binary = cargo_target / target / "release/rill"
        shutil.copy2(binary, prefix / "bin/rill")
        marker = {
            "version": 1, "rill": VERSION,
            "tools": {"slangc": "slang/bin/slangc", "spirv-val": "spirv/bin/spirv-val"},
            "packages": {
                "sdl3": {"include_dirs": ["sdl3/include"], "library_dirs": ["sdl3/lib"], "libraries": ["SDL3"]},
                "rill-slang": {"include_dirs": ["sdk/include"], "library_dirs": ["sdk/lib"], "libraries": ["rill_slang"]}},
            "runtime_dirs": ["slang/lib", "sdl3/lib", "sdk/lib"]}
        (sdk / "toolchain.json").write_text(json.dumps(marker, indent=2) + "\n")
        share = prefix / "share/rill"
        share.mkdir(parents=True)
        for directory in ("stdlib", "docs", "examples"):
            shutil.copytree(REPO / directory, share / directory, symlinks=True,
                            ignore=shutil.ignore_patterns("build", "target", ".rill-cache", "__pycache__"))
        for filename in ("README.md", "PLAN.md"):
            shutil.copy2(REPO / filename, share / filename)
        notices = share / "licenses"
        for key, source in sources.items():
            licenses(source, notices / key)
        # Keep the source lock with the package for attribution and rebuilding.
        shutil.copy2(REPO / "tools/dist/dependencies.json", share / "dependencies.json")
        run(sys.executable, REPO / "tools/dist/smoke.py", prefix)
        # Relative archive members make extraction independent of the build host.
        candidate = stage / archive.name
        with tarfile.open(candidate, "w:gz") as tar:
            tar.add(prefix, arcname=name)
        shutil.copy2(candidate, archive)
        checksum = digest(archive)
        (output / f"{archive.name}.sha256").write_text(f"{checksum}  {archive.name}\n")
        shutil.move(prefix, output / name)
    print(f"Created {archive}\nSHA-256 {checksum}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, default=REPO / "target/distribution")
    parser.add_argument("--output", type=Path, default=REPO / "target/dist")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 2, 8))
    parser.add_argument("--rill-bin", type=Path, help="use an already built binary of this version")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    build(args.work_dir.resolve(), args.output.resolve(), args.jobs,
          args.rill_bin.resolve() if args.rill_bin else None)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
