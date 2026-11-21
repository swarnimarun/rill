#!/usr/bin/env python3
"""Run real headless Neovim grammar and optional LSP integration checks."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    directory = Path(__file__).resolve().parent.parent
    root = directory.parent.parent
    nvim = os.environ.get("NVIM", "nvim")
    if shutil.which(nvim) is None:
        print("SKIP Neovim integration: set NVIM to a Neovim 0.11+ executable")
        return
    with tempfile.TemporaryDirectory(prefix="rill neovim ") as temporary:
        workspace = Path(temporary)
        parser = workspace / "rill.so"
        subprocess.run([sys.executable, str(directory / "build-parser.py"), "--output", str(parser)], check=True)
        environment = os.environ.copy()
        environment.update({
            "RILL_REPO": str(root),
            "RILL_PARSER": str(parser),
            "RILL_TEST_WORKSPACE": str(workspace),
            "XDG_CONFIG_HOME": str(workspace / "config"),
            "XDG_DATA_HOME": str(workspace / "data"),
            "XDG_STATE_HOME": str(workspace / "state"),
            "XDG_CACHE_HOME": str(workspace / "cache"),
            "NVIM_LOG_FILE": str(workspace / "nvim.log"),
        })
        subprocess.run(
            [nvim, "--headless", "--clean", "-n", "-l", str(directory / "test/smoke.lua")],
            check=True, env=environment, timeout=45,
        )


if __name__ == "__main__":
    main()
