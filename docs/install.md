# Install Rill

The installer handles the complete alpha SDK, including the compiler, bundled
native libraries, shader tools, standard-library sources, examples and docs.
It supports macOS arm64, macOS x86_64 and glibc-based Linux x86_64. There is no
Windows archive or PowerShell installer for this checkpoint.

A C11 compiler and the platform's development SDK are still required to build
native programs. Automatic C header imports also need a compatible libclang.
The curated bundled bindings use explicit declarations and do not need automatic
header discovery. See [distribution details](distribution.md).

## Install a published release

These download commands become usable after the tag and release assets have
been published. Preparing or testing the installer does not publish them.

```sh
curl -fsSL --proto '=https' \
  https://raw.githubusercontent.com/swarnimarun/rill/v0.1.0-alpha/tools/install/install.sh \
  -o install-rill.sh
sh install-rill.sh --version v0.1.0-alpha
```

To run the script directly from a download:

```sh
curl -fsSL --proto '=https' \
  https://raw.githubusercontent.com/swarnimarun/rill/v0.1.0-alpha/tools/install/install.sh \
  | sh -s -- --version v0.1.0-alpha
```

The default prefix is `$HOME/.local`. Choose another writable directory with
`--prefix "/path/to/rill-prefix"`. The script needs a POSIX shell, `curl`, `tar`,
`awk`, standard Unix file utilities and either `sha256sum` or `shasum`.
Linux also needs `getconf` to identify the GNU libc runtime. Musl Linux is
unsupported by the current archive.

Installation does not require Python, Rust or CMake. Prefix paths containing
spaces are supported. The installer does not request elevated permissions.

The script downloads the matching archive and its `.sha256` file from the
selected GitHub release, checks the exact asset name and SHA-256 digest, and
checks archive paths and link types before extraction. It does not execute the
downloaded compiler. It rejects unsupported platforms and missing release
assets with an error.

## Install a local archive

Use the same installer before publication or in an offline environment. Supply
both the archive and the corresponding checksum file produced by the package
builder. This example runs from a source checkout; use `sh install-rill.sh`
instead if you downloaded the script above:

```sh
sh tools/install/install.sh \
  --version v0.1.0-alpha \
  --prefix "$HOME/.local" \
  --archive /path/to/rill-0.1.0-alpha-aarch64-apple-darwin.tar.gz \
  --checksum /path/to/rill-0.1.0-alpha-aarch64-apple-darwin.tar.gz.sha256
```

Use the archive for the current host. Other target names are
`x86_64-apple-darwin` and `x86_64-unknown-linux-gnu`. The checksum file must have
one record for the selected archive name, such as the output of `sha256sum`.
The installer verifies local archives exactly as it verifies downloads.

Current packages contain regular files, directories and shared-library aliases
that point to a basename in the same directory. The installer rejects absolute
paths, traversal, duplicate members, hardlinks, special files and broader
symlink forms. It also restricts member names to the ASCII path characters used
by these release packages, so GNU and BSD `tar` listings can be checked without
a separate extraction runtime. A future archive format may need an updated
installer.

## Installed layout and PATH

For the default prefix, installation creates this layout:

```text
~/.local/bin/rill -> ../lib/rill/v0.1.0-alpha-<target>/bin/rill
~/.local/lib/rill/v0.1.0-alpha-<target>/
    .rill-install
    bin/rill
    libexec/rill/toolchain.json
    libexec/rill/...
    share/rill/...
```

The full SDK stays together under the version directory. The compiler resolves
the launcher symlink before discovering its adjacent SDK. Keep the installed
directory tree together if moving it.

Add the selected prefix's `bin` directory to your current shell if needed:

```sh
export PATH="$HOME/.local/bin:$PATH"
rill new hello
rill project run hello
```

The installer prints the selected path and does not edit shell profiles or
system settings. Copy the engine example out of `share/rill/examples` to a
writable project directory before building it.

## Updates and removal

Run the installer with another published `--version` to install and select that
version. Each package has its own directory. Installation first verifies and
extracts into a private stage, then renames the complete package into place and
atomically replaces the launcher symlink. A failed verification or staging step
leaves the selected installation intact. Old versions remain available.
Running the installer again with the same verified archive is idempotent. An
existing package with a different recorded checksum is an error. If activation
is interrupted after the complete version directory is published, that package
remains available for a later retry.

Only a launcher pointing into an installer-marked directory under this prefix
may be replaced. Existing unrelated files or symlinks at `bin/rill` are errors,
as are symlinked `bin`, `lib` or `lib/rill` installation directories. Choose a
different prefix or move an unrelated command yourself.

To uninstall, remove the selected `bin/rill` symlink and the version directories
under `lib/rill` that contain `.rill-install`. Keep unrelated prefix contents.
To reselect an older installed version, run the installer with that version
and its original archive/checksum pair. Old packages are never removed
automatically.

Concurrent installs in one prefix are prevented by `lib/rill/.install-lock`.
An uncatchable interruption can leave that lock behind. Remove the empty lock
directory only after confirming no installer is running, then retry.

## Verify the installer

The integration tests use temporary prefixes and generated local archives:

```sh
sh -n tools/install/install.sh
python3 -m unittest discover -s tools/install -p 'test_*.py'
```

They cover installation paths with spaces, updates, checksum failures, archive
escapes and link types, unsupported platforms, unrelated existing files,
staging cleanup and release URL selection. Actual package relocation and SDK
discovery through the installed launcher are separate distribution smoke checks.
