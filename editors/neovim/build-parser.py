#!/usr/bin/env python3
"""Compile the checked-in Rill Tree-sitter parser for local Neovim."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    directory = Path(__file__).resolve().parent
    arguments = argparse.ArgumentParser(description=__doc__)
    arguments.add_argument("--cc", default=os.environ.get("CC", "cc"))
    arguments.add_argument("--output", type=Path, default=directory / "parser/rill.so")
    options = arguments.parse_args()
    source = directory.parent / "tree-sitter-rill/src"
    output = options.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".rill-parser-", dir=output.parent) as temporary:
        library = Path(temporary) / "rill.so"
        subprocess.run(
            [options.cc, "-shared", "-fPIC", "-I", str(source), str(source / "parser.c"), "-o", str(library)],
            check=True,
        )
        os.replace(library, output)
    print(f"Built {output}")


if __name__ == "__main__":
    main()
