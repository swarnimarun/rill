#!/usr/bin/env bash
# Capture actual SDL3-rendered framebuffers on macOS and Linux.
set -euo pipefail
if [[ $# != 1 ]]; then
    printf 'Usage: %s NEW_OUTPUT_DIRECTORY\n' "$0" >&2
    printf 'The directory must not exist; existing captures are never overwritten.\n' >&2
    exit 2
fi
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for tool in pkg-config python3 "${CC:-cc}"; do
    command -v -- "$tool" >/dev/null || { printf 'Required tool missing: %s\n' "$tool" >&2; exit 2; }
done
output=$(python3 -c 'import os, sys; print(os.path.abspath(sys.argv[1]))' "$1")
if [[ -e "$1" || -L "$1" || -e "$output" || -L "$output" ]]; then
    printf 'Refusing to overwrite existing output directory: %s\n' "$output" >&2
    exit 2
fi
pkg-config --exists sdl3
frame=${RILL_CAPTURE_FRAME:-40}
if [[ ! "$frame" =~ ^[1-9][0-9]*$ ]] || (( ${#frame} > 3 )) || (( frame > 900 )); then
    printf 'RILL_CAPTURE_FRAME must be an integer from 1 to 900\n' >&2
    exit 2
fi
opt=${RILL_CAPTURE_OPT_LEVEL:-0}
if [[ "$opt" != 0 && "$opt" != 2 ]]; then
    printf 'RILL_CAPTURE_OPT_LEVEL must be 0 or 2\n' >&2
    exit 2
fi
mkdir -- "$output"
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# Parse pkg-config quoting without evaluating it as shell code.
for kind in cflags libs; do
    pkg-config --"$kind" sdl3 > "$tmp/$kind"
    python3 - "$tmp/$kind" > "$tmp/$kind.args" <<'PY'
import pathlib, shlex, sys
for argument in shlex.split(pathlib.Path(sys.argv[1]).read_text()):
    sys.stdout.buffer.write(argument.encode() + b'\0')
PY
done
cflags=()
while IFS= read -r -d '' arg; do cflags+=("$arg"); done < "$tmp/cflags.args"
libs=()
while IFS= read -r -d '' arg; do libs+=("$arg"); done < "$tmp/libs.args"
compiler=${CC:-cc}
rill=${RILL_BIN:-}
if [[ -z "$rill" ]]; then
    cargo build --locked --manifest-path "$root/Cargo.toml" --target-dir "$tmp/target"
    rill="$tmp/target/debug/rill"
fi
"$compiler" -std=c11 "-O$opt" -Wall -Wextra -Werror ${cflags[@]+"${cflags[@]}"} -c "$root/scripts/sdl3-capture.c" -o "$tmp/capture.o"
"$compiler" -std=c11 "-O$opt" -Wall -Wextra -Werror ${cflags[@]+"${cflags[@]}"} \
    -DSDL_RenderPresent=rill_capture_present -c "$root/examples/sdl3/shim.c" -o "$tmp/collector.o"
link_args=()
for arg in ${libs[@]+"${libs[@]}"}; do link_args+=(--link-arg "$arg"); done
"$rill" build "$root/examples/sdl3-direct/main.rill" --cc "$compiler" --c-arg "-O$opt" \
    --c-arg -DSDL_RenderPresent=rill_capture_present -o "$tmp/direct" --link-arg "$tmp/capture.o"
"$rill" build "$root/examples/sdl3/collector.rill" --cc "$compiler" --c-arg "-O$opt" -o "$tmp/collector" \
    --link-arg "$tmp/collector.o" --link-arg "$tmp/capture.o" ${link_args[@]+"${link_args[@]}"}
for game in direct collector; do
    SDL_VIDEODRIVER=dummy SDL_RENDER_DRIVER=software RILL_CAPTURE_FRAME="$frame" \
        RILL_CAPTURE_BMP="$output/sdl3-$game.bmp" python3 - "$tmp/$game" \
        > "$output/$game.stdout" 2> "$output/$game.stderr" <<'PY'
import os, signal, subprocess, sys
def interrupted(number, frame):
    raise SystemExit(128 + number)
for number in (signal.SIGTERM, signal.SIGHUP):
    signal.signal(number, interrupted)
process = subprocess.Popen([sys.argv[1]], start_new_session=True)
try:
    status = process.wait(timeout=30)
except BaseException:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()
    raise
sys.exit(status if status >= 0 else 128 - status)
PY
    [[ $(cat -- "$output/$game.stdout") == 0 ]]
    [[ -s "$output/sdl3-$game.bmp" ]]
    if command -v magick >/dev/null; then
        magick "$output/sdl3-$game.bmp" "$output/sdl3-$game.png"
    fi
done
{
    printf 'Native SDL3 software framebuffers, dummy video driver.\n'
    printf 'Capture frame: %s. No physical keyboard or mouse input was simulated.\n' "$frame"
    printf 'C optimization: -O%s\n' "$opt"
    printf 'SDL3: '; pkg-config --modversion sdl3
    "$compiler" --version | sed -n '1p'
    printf 'The capture wrapper saves pixels before SDL_RenderPresent and posts SDL_EVENT_QUIT.\n'
    printf 'Game source and drawing are unchanged. BMP is lossless; PNG is included when ImageMagick is installed.\n'
    cat -- "$output/direct.stderr" "$output/collector.stderr"
} > "$output/README.txt"
printf 'Captured both native headless SDL3 framebuffers in %s\n' "$output"
