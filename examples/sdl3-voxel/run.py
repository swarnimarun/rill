#!/usr/bin/env python3
"""Launch the bounded CPU reference."""
import os
from pathlib import Path
import sys

arguments = [argument for argument in sys.argv[1:] if argument != '--cpu']
launcher = Path(__file__).resolve().with_name('cpu.py')
os.execv(sys.executable, [sys.executable, str(launcher), *arguments])
