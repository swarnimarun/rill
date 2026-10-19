//! Builds the Rill tree-sitter grammar as a Zig module.
//!
//! Editors that build their syntax layer in Zig (flow) import this module with
//! `b.dependency("tree_sitter_rill", ...)` and get `src/parser.c` compiled into
//! their own artifact, so the grammar is consumed from this repository instead
//! of being vendored. Helix does the same through `grammar.js` and
//! `hx --grammar build`, and the Zed extension through `extension.toml`.

const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const mod = b.addModule("tree-sitter-rill", .{
        .root_source_file = b.path("bindings.zig"),
        .target = target,
        .optimize = optimize,
    });
    mod.addCSourceFile(.{ .file = b.path("src/parser.c"), .flags = &.{"-fno-sanitize=undefined"} });
    mod.addIncludePath(b.path("src"));
    mod.link_libc = true;
}
