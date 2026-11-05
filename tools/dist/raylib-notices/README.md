# Raylib 6.2 third-party notices

These files retain upstream notices from Raylib 6.2 at immutable commit
`02566453880e06bec97f5b8160fa6fe1e98397ca`:

https://github.com/raysan5/raylib/tree/02566453880e06bec97f5b8160fa6fe1e98397ca/src/external

The repository inventory is `tools/dist/raylib-notices.json`. It records each
source URL, its complete SHA-256 and Git blob SHA-1, the exact source byte ranges
retained, and the resulting notice file's SHA-256. Every downloaded source was
verified against the pinned commit's Git tree. Each retained block is unmodified;
two newline bytes separate multiple blocks from one source file.

The collection covers all C/header, Objective-C and XML vendor files under
`src/external`, plus GLFW's explicit license file. It preserves license-bearing
C comment blocks and complete XML copyright elements. It includes optional
backend/platform notices as a conservative superset; presence here does not
claim a component is enabled or separately exposed by Rill. The only selected
file with no inline notice is the Windows-only `win32_clipboard.h`; its complete
source is retained rather than inventing a license or dropping attribution.
Raylib's own LICENSE is packaged separately from these dependency notices.

The upstream sources identify their authors and terms, including the GLFW,
RGFW, Wayland protocol, OpenGL loader, image, font, audio and model-loading
components. No uniform license is imposed on this collection. Preserve the
individual notice texts when redistributing it. Raylib's overview is available
at https://github.com/raysan5/raylib/wiki/raylib-dependencies; the pinned source
files and their actual notices take precedence over that evolving summary.

For an update, review a new immutable Raylib revision, enumerate its external
sources again, verify their Git objects and complete SHA-256 hashes, and review
all changed notices. Existing inventory byte ranges can reproduce these files
exactly from the pinned sources: concatenate the recorded ranges in order with
`b"\n\n"`, then verify the recorded notice hash. The distribution build uses
these committed files and does not fetch notices at installation time.
