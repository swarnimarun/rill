#!/bin/sh
# Install a complete Rill SDK without changing the caller's shell profile.
set -eu
umask 077
LC_ALL=C
export LC_ALL
unset TAR_OPTIONS

version=v0.1.0-alpha
prefix=${HOME:+$HOME/.local}
local_archive=
local_checksum=
stage=
lock=

fail() { printf 'rill installer: %s\n' "$*" >&2; exit 1; }
complete_tree() {
    [ -d "$1" ] && [ ! -L "$1" ] \
        && [ -f "$1/bin/rill" ] && [ -x "$1/bin/rill" ] && [ ! -L "$1/bin/rill" ] \
        && [ -s "$1/libexec/rill/toolchain.json" ] && [ ! -L "$1/libexec/rill/toolchain.json" ] \
        && [ -d "$1/share/rill/stdlib" ] && [ -d "$1/share/rill/docs" ]
}
usage() {
    cat <<'EOF'
Usage: install.sh [--version v0.1.0-alpha] [--prefix directory]
                  [--archive file.tar.gz --checksum file.sha256]

Install the native macOS or Linux alpha SDK. The default prefix is $HOME/.local.
Downloaded archives and checksums come from github.com/swarnimarun/rill releases.
Offline installation requires both local files. Existing unrelated files are
never replaced. Old installed versions remain available after an update.
EOF
}
cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if [ -n "$stage" ]; then rm -rf "$stage"; fi
    if [ -n "$lock" ]; then rmdir "$lock" 2>/dev/null || :; fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version|--prefix|--archive|--checksum)
            option=$1
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$option requires a value"
            case "$option" in
                --version) version=$2 ;;
                --prefix) prefix=$2 ;;
                --archive) local_archive=$2 ;;
                --checksum) local_checksum=$2 ;;
            esac
            shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) fail "unknown option $1; use --help" ;;
    esac
done
printf '%s\n' "$version" | LC_ALL=C awk '
    NR == 1 && /^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9][A-Za-z0-9.-]*)?$/ { valid=1; next }
    { bad=1 }
    END { exit !valid || bad }
' || fail "--version requires a release tag such as v0.1.0-alpha"
[ -n "$prefix" ] || fail 'set HOME or specify --prefix'
case "$prefix" in /*) ;; *) prefix=$PWD/$prefix ;; esac
if [ -n "$local_archive" ] || [ -n "$local_checksum" ]; then
    [ -n "$local_archive" ] && [ -n "$local_checksum" ] || fail '--archive and --checksum must be supplied together'
fi

system=$(uname -s)
machine=$(uname -m)
case "$system/$machine" in
    Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin ;;
    Darwin/x86_64) target=x86_64-apple-darwin ;;
    Linux/x86_64|Linux/amd64)
        libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || fail 'the Linux SDK requires glibc and getconf; musl Linux is unsupported'
        case "$libc" in 'glibc '*) ;; *) fail 'the Linux SDK requires glibc; musl Linux is unsupported' ;; esac
        target=x86_64-unknown-linux-gnu ;;
    *) fail "unsupported platform $system/$machine; this release has macOS arm64/x86_64 and Linux x86_64 assets only" ;;
esac
for tool in tar awk mktemp readlink; do command -v "$tool" >/dev/null 2>&1 || fail "required tool is missing: $tool"; done
if command -v sha256sum >/dev/null 2>&1; then
    hasher=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    hasher=shasum
else
    fail 'SHA-256 verification requires sha256sum or shasum'
fi
package=rill-${version#v}-$target
asset=$package.tar.gz
install_id=$version-$target

mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd -P)
for directory in "$prefix/bin" "$prefix/lib" "$prefix/lib/rill"; do
    [ ! -L "$directory" ] || fail "refusing symlinked installation directory: $directory"
    if [ -e "$directory" ]; then
        [ -d "$directory" ] || fail "installation path is not a directory: $directory"
    else
        mkdir "$directory"
    fi
done
managed=$prefix/lib/rill
if ! mkdir "$managed/.install-lock" 2>/dev/null; then
    fail "installation is locked at $managed/.install-lock; another installer may be running"
fi
lock=$managed/.install-lock

check_launcher() {
    for directory in "$prefix/bin" "$prefix/lib" "$managed"; do
        [ -d "$directory" ] && [ ! -L "$directory" ] || fail "installation directory changed: $directory"
    done
    # Only a link into a marked package under this prefix belongs to us.
    if [ -e "$prefix/bin/rill" ] || [ -L "$prefix/bin/rill" ]; then
        [ -L "$prefix/bin/rill" ] || fail "refusing to replace unrelated file: $prefix/bin/rill"
        previous=$(readlink "$prefix/bin/rill")
        case "$previous" in
            ../lib/rill/*/bin/rill) previous_id=${previous#../lib/rill/}; previous_id=${previous_id%/bin/rill} ;;
            *) fail "refusing to replace unrelated symlink: $prefix/bin/rill" ;;
        esac
        case "$previous_id" in ''|*/*|.|..) fail 'existing installation link escapes the managed directory' ;; esac
        complete_tree "$managed/$previous_id" \
            && [ -f "$managed/$previous_id/.rill-install" ] \
            && [ ! -L "$managed/$previous_id/.rill-install" ] \
            && [ "$(head -n 1 "$managed/$previous_id/.rill-install")" = rill-installer-v1 ] \
            || fail 'existing symlink does not select a complete installer-owned package'
    fi
}
check_launcher

stage=$(mktemp -d "$managed/.stage.XXXXXX")
archive=$stage/archive.tar.gz
checksum_file=$stage/archive.sha256
if [ -n "$local_archive" ]; then
    case "$local_archive" in /*) ;; *) local_archive=$PWD/$local_archive ;; esac
    case "$local_checksum" in /*) ;; *) local_checksum=$PWD/$local_checksum ;; esac
    [ -f "$local_archive" ] && [ -f "$local_checksum" ] || fail 'offline archive and checksum must be regular files'
    [ "$(wc -c <"$local_checksum")" -le 4096 ] || fail 'checksum file exceeds 4096 bytes'
    cp "$local_archive" "$archive"
    cp "$local_checksum" "$checksum_file"
else
    command -v curl >/dev/null 2>&1 || fail 'downloads require curl; use --archive and --checksum for offline installation'
    base=https://github.com/swarnimarun/rill/releases/download/$version
    printf 'Downloading %s\n' "$asset"
    curl --disable --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 --output "$archive" "$base/$asset" \
        || fail "download failed; release $version may not have been published yet"
    curl --disable --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 --max-filesize 4096 --output "$checksum_file" "$base/$asset.sha256" \
        || fail 'checksum download failed'
fi
[ "$(wc -c <"$checksum_file")" -le 4096 ] || fail 'checksum file exceeds 4096 bytes'
expected=$(LC_ALL=C awk -v asset="$asset" '
    NR == 1 && NF == 2 && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ && ($2 == asset || $2 == "*" asset) { value=tolower($1); valid=1; next }
    { valid=0; bad=1 }
    END { if (!valid || bad) exit 1; print value }
' "$checksum_file") || fail "checksum must contain exactly one SHA-256 for $asset"
if [ "$hasher" = sha256sum ]; then
    actual=$(sha256sum <"$archive")
else
    actual=$(shasum -a 256 <"$archive")
fi
actual=${actual%% *}
[ "$actual" = "$expected" ] || fail 'SHA-256 mismatch; the existing installation was preserved'

# Current release members deliberately use an ASCII name vocabulary. It makes
# GNU/BSD tar listings unambiguous without a Python installation. Same-directory
# links cover the bundled shared-library aliases and cannot escape extraction.
tar -tzf "$archive" >"$stage/names" || fail 'cannot list release archive'
LC_ALL=C awk -v root="$package" '
    function confined(name, parts, n, i) {
        sub(/\/$/, "", name)
        if (name != root && index(name, root "/") != 1) return 0
        n=split(name, parts, "/")
        for (i=1;i<=n;i++) if (parts[i] == "" || parts[i] == "." || parts[i] == "..") return 0
        return 1
    }
    { name=$0; sub(/\/$/, "", name); if ($0 !~ /^[A-Za-z0-9_.\/+@,=-]+$/ || !confined($0) || seen[name]++ || ++count > 200000) exit 1 }
    END { if (!count) exit 1 }
' "$stage/names" || fail 'archive contains an unsupported, escaping or malformed member name'
tar -tvzf "$archive" >"$stage/types" || fail 'cannot inspect archive member types'
LC_ALL=C awk '
    /^[d-]/ { next }
    /^l/ { if (split($0, fields, " -> ") == 2 && fields[2] ~ /^[A-Za-z0-9_.+@,=-]+$/ && fields[2] != "." && fields[2] != "..") next }
    { exit 1 }
' "$stage/types" || fail 'archive contains a special file, hardlink or unsafe symlink'
mkdir "$stage/unpack"
tar -xzf "$archive" --no-same-owner --no-same-permissions -C "$stage/unpack" || fail 'archive extraction failed'
tree=$stage/unpack/$package
complete_tree "$tree" || fail 'archive is missing the complete compiler/SDK/share layout'
printf 'rill-installer-v1\n%s\n%s\n%s\n' "$version" "$target" "$expected" >"$tree/.rill-install"
destination=$managed/$install_id
if [ -e "$destination" ] || [ -L "$destination" ]; then
    complete_tree "$destination" \
        && [ -f "$destination/.rill-install" ] && [ ! -L "$destination/.rill-install" ] \
        && cmp -s "$tree/.rill-install" "$destination/.rill-install" \
        || fail "refusing to overwrite existing package: $destination"
else
    mv "$tree" "$destination"
fi
ln -s "../lib/rill/$install_id/bin/rill" "$stage/rill"
check_launcher
mv -f "$stage/rill" "$prefix/bin/rill"
printf 'Installed Rill %s for %s\nCompiler: %s/bin/rill\nSDK: %s\n' "$version" "$target" "$prefix" "$destination"
printf 'Add %s/bin to PATH if needed. No shell files were changed.\n' "$prefix"
