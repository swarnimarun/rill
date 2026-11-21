# A modal editor in Rill

Open a small text file in a POSIX terminal. The Rill program owns a fixed
`bytes<256>` document, two editing modes, cursor movement, insertion, deletion and
dirty-state handling. A C11 adapter reads keys, draws the terminal and saves
files. It passes only scalar bytes, offsets and status across the boundary.

Build from the repository root. Use a temporary directory for build products.

```sh
cargo build --locked
build_dir=$(mktemp -d)
cc -std=c11 -O2 -c examples/modal-editor/terminal.c -o "$build_dir/terminal.o"
target/debug/rill build examples/modal-editor/main.rill --link-arg "$build_dir/terminal.o" -o "$build_dir/editor"
RILL_EDITOR_FILE=notes.txt "$build_dir/editor"
```

| Mode | Keys |
| --- | --- |
| Normal | `h`/`l` move one byte. `j`/`k` move down/up, clamping to the adjacent line length. `0`/`$` go to the beginning/end of the document. |
| Normal | `i` enters insert mode. `x` deletes under the cursor. `w` saves. `q` quits a clean document and `Q` discards pending edits. |
| Insert | Printable ASCII inserts before the cursor. Enter inserts LF, Backspace deletes before the cursor and Escape returns to normal mode. |

An asterisk marks unsaved edits. A full buffer shows `[full]` and accepts
deletions and movement. The terminal adapter rejects files over 256 bytes,
control bytes other than LF, symlinks and nonregular inputs. A missing file
opens empty. Cursor positions include the one-past-end insertion position.

Saving stages a same-directory temporary file, flushes it, then replaces the
original pathname. A new document claims a previously unused name. Existing
documents preserve ordinary permission bits. Replacement changes the inode.
This example assumes a single writer and a trusted directory. It detects
ordinary external changes by file identity, length and modification time.
A failed save keeps pending edits in memory. Power-loss recovery, Unicode
grapheme movement, undo, LSP integration and multi-buffer editing remain open.

Piped input drives the same Rill command loop. This appends one line to a file
whose previous contents end in LF, then saves and quits.

```sh
printf '$ihello\n\033wq' | RILL_EDITOR_FILE=notes.txt "$build_dir/editor"
```

Run `cargo test --locked --test modal_editor` for the native file round trips.
