#!/usr/bin/env python3
"""Launch the GPU engine by default, or the bounded CPU reference with --cpu."""
import os
from pathlib import Path
import sys

root = Path(__file__).resolve().parent
arguments = sys.argv[1:]
if '--cpu' in arguments:
    arguments.remove('--cpu')
    launcher = root / 'cpu.py'
else:
    launcher = root / 'gpu' / 'run.py'
os.execv(sys.executable, [sys.executable, str(launcher), *arguments])
