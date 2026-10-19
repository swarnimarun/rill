# Rill for Zed

Two manual steps, neither of which can be done from a script:

1. **Install as a dev extension.** In Zed: `zed: install dev extension`, then choose
   this directory (`editors/zed-rill`). Zed compiles the grammar it references and
   registers `.rill` as a language, which gives highlighting and structural
   selection.

2. **Attach the language server.** This extension ships a grammar, queries and a
   language config, but *not* a WebAssembly server adapter - so Zed will not start
   `rillc lsp` from it. Point Zed at the installed binary in
   `~/.config/zed/settings.json`; a bare name is resolved from Zed's process PATH:

   ```json
   {
     "lsp": {
       "rill-lsp": {
         "binary": { "path": "rill-lsp", "arguments": [] },
         "languages": ["Rill"]
       }
     }
   }
   ```

   A GUI-launched Zed on macOS does not inherit your shell's PATH. If the server
   does not start, use the absolute path `command -v rill-lsp` prints in place of
   the bare name:

   ```console
   $ command -v rill-lsp
   ```

   This is the part I could not verify: Zed cannot be driven headlessly, and whether
   the `languages` key accepts a language contributed by a dev extension depends on
   Zed's version. If it is ignored, the diagnostics/hover/formatting arrive once a
   WASM adapter is added to the extension - the server itself is already verified
   (`rillc lsp` answers a full session).

The grammar lives in `editors/tree-sitter-rill` and is shared with Helix, which
builds it from the same path.
