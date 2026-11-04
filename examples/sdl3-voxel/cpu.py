#!/usr/bin/env python3
"""Compile the SDL adapter and Rill example with structured process arguments."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--smoke', action='store_true', help='eight scripted frames, no frame delay')
    parser.add_argument('--opt', choices=('0', '2'), default='2')
    args = parser.parse_args()
    example = Path(__file__).resolve().parent
    root = example.parent.parent
    compiler = os.environ.get('CC', 'cc')
    flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', 'sdl3'], text=True))
    rill = os.environ.get('RILL')
    if not rill:
        subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(root / 'Cargo.toml')], check=True)
        rill = str(root / 'target' / 'debug' / 'rill')
    with tempfile.TemporaryDirectory(prefix='rill-sdl3-voxel-') as temporary:
        build = Path(temporary)
        adapter = build / 'platform.o'
        subprocess.run([compiler, '-std=c11', '-O' + args.opt, '-Wall', '-Wextra', '-Werror',
                        *flags, '-c', str(example / 'platform.c'), '-o', str(adapter)], check=True)
        binary = build / 'voxel'
        subprocess.run([rill, 'build', str(example / 'main.rill'), '--cc', compiler,
                        '--c-arg', '-O' + args.opt, '--c-arg', '-I' + str(example),
                        '--link-arg', str(adapter), '-o', str(binary)], check=True)
        environment = os.environ.copy()
        if args.smoke:
            environment['RILL_VOXEL_SMOKE'] = '1'
            environment.setdefault('SDL_VIDEODRIVER', 'dummy')
        result = subprocess.run([str(binary)], env=environment, stdout=subprocess.PIPE, text=True, check=True)
        print(result.stdout, end='')
        # Rill prints main's integer result; the C process itself exits zero.
        return 0 if result.stdout.strip() == '0' else 1


if __name__ == '__main__':
    raise SystemExit(main())
