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
    # Upstream source archives may keep dependency notices below their root.
    for path in source.rglob("*"):
        if path.is_file() and any(term in path.name.upper() for term in ("LICENSE", "COPYING", "NOTICE", "COPYRIGHT")):
            target = destination / path.relative_to(source)
            if not target.exists():
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target)


def relocate_sdl_metadata(prefix):
    # SDL's CMake install writes an absolute staging prefix into pkg-config.
    # Its other paths already reference this variable; anchor it at the .pc file.
    path = prefix / "lib/pkgconfig/sdl3.pc"
    lines = path.read_text().splitlines(keepends=True)
    if sum(line.startswith("prefix=") for line in lines) != 1:
        raise ValueError("unexpected SDL pkg-config prefix metadata")
    path.write_text("".join("prefix=${pcfiledir}/../..\n" if line.startswith("prefix=") else line
                            for line in lines))


def verify_raylib_notices(notices_dir, manifest_path):
    """Verify the retained notices against their immutable source inventory."""
    notices_dir = Path(notices_dir).resolve(strict=True)
    value = json.loads(Path(manifest_path).read_text())
    if (value.get("version") != 1 or value.get("revision") != LOCK["raylib"]["revision"]
            or value.get("release") != LOCK["raylib"]["version"]):
        raise ValueError("Raylib notice inventory does not match the SDK lock")
    files = value.get("files")
    if not isinstance(files, list) or not 1 <= len(files) <= 4096:
        raise ValueError("invalid Raylib notice inventory")
    allowed = {"README.md"}
    for entry in files:
        name = entry.get("notice_file", "")
        source = entry.get("source_path", "")
        for relative in (name, source):
            if (not isinstance(relative, str) or "\\" in relative
                    or any(part in ("", ".", "..") for part in relative.split("/"))):
                raise ValueError("invalid Raylib notice path")
        if name in allowed or not source.startswith("src/external/"):
            raise ValueError("duplicate or invalid Raylib notice path")
        allowed.add(name)
        expected_url = f"https://raw.githubusercontent.com/raysan5/raylib/{value['revision']}/{source}"
        if entry.get("source_url") != expected_url:
            raise ValueError("Raylib notice source is not pinned")
        path = notices_dir
        for part in name.split("/"):
            path = path / part
            if path.is_symlink():
                raise ValueError("symlinked Raylib notice")
        try:
            data = path.read_bytes()
        except OSError as error:
            raise ValueError(f"missing Raylib notice {name}") from error
        if len(data) != entry.get("notice_bytes") or hashlib.sha256(data).hexdigest() != entry.get("notice_sha256"):
            raise ValueError(f"Raylib notice checksum mismatch: {name}")
        source_size = entry.get("source_bytes")
        ranges = entry.get("ranges")
        if type(source_size) is not int or source_size < 1 or not isinstance(ranges, list) or not ranges:
            raise ValueError("invalid Raylib notice source ranges")
        cursor, previous = 0, 0
        for span in ranges:
            start, end = span.get("start"), span.get("end")
            if type(start) is not int or type(end) is not int or not previous <= start < end <= source_size:
                raise ValueError("invalid Raylib notice source range")
            block = data[cursor:cursor + end - start]
            if hashlib.sha256(block).hexdigest() != span.get("sha256"):
                raise ValueError("Raylib notice block checksum mismatch")
            cursor += end - start
            previous = end
            if span is not ranges[-1]:
                if data[cursor:cursor + 2] != b"\n\n":
                    raise ValueError("invalid Raylib notice block separator")
                cursor += 2
        if cursor != len(data):
            raise ValueError("unaccounted Raylib notice bytes")
    for path in notices_dir.rglob("*"):
        if path.is_symlink() or (path.is_file() and path.relative_to(notices_dir).as_posix() not in allowed):
            raise ValueError("unlisted or symlinked Raylib notice file")


def build(work, output, jobs, binary=None):
    verify_raylib_notices(REPO / "tools/dist/raylib-notices", REPO / "tools/dist/raylib-notices.json")
    target = host_target()
    name = f"rill-{VERSION}-{target}"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{name}.tar.gz"
    if archive.exists() or (output / name).exists():
        raise ValueError(f"output already exists for {name}; choose a fresh output directory")
    downloads = work / "downloads"
    downloads.mkdir(parents=True, exist_ok=True)
    specs = {"slang": LOCK["slang"]["assets"][target],
             "raylib": LOCK["raylib"]["assets"][target],
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
        shutil.copytree(sources["raylib"], sdk / "raylib", symlinks=True)
        rpath = "@loader_path" if sys.platform == "darwin" else "$ORIGIN"
        cmake(sources["sdl3"], stage / "sdl-build", sdk / "sdl3", jobs, [
            "-DSDL_SHARED=ON", "-DSDL_STATIC=OFF", "-DSDL_TESTS=OFF",
            "-DSDL_TEST_LIBRARY=OFF", "-DSDL_EXAMPLES=OFF", "-DSDL_FRAMEWORK=OFF",
            "-DCMAKE_INSTALL_LIBDIR=lib", "-DCMAKE_INSTALL_NAME_DIR=@rpath",
            f"-DCMAKE_INSTALL_RPATH={rpath}"])
        relocate_sdl_metadata(sdk / "sdl3")
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
              [f"-DSLANG_ROOT={sdk / 'slang'}", f"-DRAYLIB_ROOT={sdk / 'raylib'}",
               "-DCMAKE_INSTALL_LIBDIR=lib"])
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
                "raylib": {"include_dirs": ["raylib/include"], "library_dirs": ["raylib/lib"], "libraries": ["raylib"]},
                "rill-slang": {"include_dirs": ["sdk/include"], "library_dirs": ["sdk/lib"], "libraries": ["rill_slang"]},
                "rill-raylib": {"include_dirs": ["sdk/include"], "library_dirs": ["sdk/lib"], "libraries": ["rill_raylib"]}},
            "runtime_dirs": ["slang/lib", "sdl3/lib", "raylib/lib", "sdk/lib"]}
        (sdk / "toolchain.json").write_text(json.dumps(marker, indent=2) + "\n")
        share = prefix / "share/rill"
        share.mkdir(parents=True)
        for directory in ("stdlib", "docs", "examples"):
            shutil.copytree(REPO / directory, share / directory, symlinks=True,
                            ignore=shutil.ignore_patterns("build", "target", ".rill-cache", "__pycache__"))
        for filename in ("README.md", "PLAN.md"):
            shutil.copy2(REPO / filename, share / filename)
        shutil.copy2(REPO / "tools/install/install.sh", share / "install.sh")
        notices = share / "licenses"
        for key, source in sources.items():
            licenses(source, notices / key)
        shutil.copytree(REPO / "tools/dist/raylib-notices", notices / "raylib/third-party")
        shutil.copy2(REPO / "tools/dist/raylib-notices.json", notices / "raylib/third-party/raylib-notices.json")
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
