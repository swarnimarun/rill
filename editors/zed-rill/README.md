# Rill for Zed

Two manual steps, neither of which can be done from a script:

1. **Install as a dev extension.** In Zed: `zed: install dev extension`, then choose
   this directory (`editors/zed-rill`). Zed compiles the grammar it references and
   registers `.rill` as a language, which gives highlighting and structural
   selection.

2. **Attach the language server.** This extension ships a grammar, queries and a
   language config, but *not* a WebAssembly server adapter - so Zed will not start
   `rill lsp` from it. Point Zed at the installed binary directly in
   `~/.config/zed/settings.json`:

   ```json
   {
     "lsp": {
       "rill-lsp": {
         "binary": { "path": "/absolute/path/to/rill-lsp", "arguments": [] },
         "languages": ["Rill"]
       }
     }
   }
   ```

   This is the part I could not verify: Zed cannot be driven headlessly, and whether
   the `languages` key accepts a language contributed by a dev extension depends on
   Zed's version. If it is ignored, the diagnostics/hover/formatting arrive once a
   WASM adapter is added to the extension - the server itself is already verified
   (`rill lsp` answers a full session).

The grammar lives in `editors/tree-sitter-rill` and is shared with Helix, which
builds it from the same path.
