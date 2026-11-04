#!/usr/bin/env python3
"""Build and run the Rill SDL3 GPU sandbox using native Metal shaders."""
import argparse
import os
import platform
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--smoke', action='store_true', help='twelve deterministic verification frames')
    mode.add_argument('--benchmark', nargs='?', type=int, const=120, metavar='FRAMES',
                      help='measure completed GPU frames; default 120')
    mode.add_argument('--self-test', action='store_true', help='run native GPU readback assertions')
    mode.add_argument('--input-test', action='store_true', help='verify cursor capture and focus events in a real SDL window')
    parser.add_argument('--stress', action='store_true', help='benchmark walking with repeated scripted wall cuts')
    parser.add_argument('--vsync', action='store_true', help='synchronize presentation to the display instead of running uncapped')
    parser.add_argument('--no-demo', action='store_true', help='benchmark without the automatic foundation edit')
    parser.add_argument('--lean', action='store_true', help='script asymmetric foundation damage instead of complete demolition')
    parser.add_argument('--walk', action='store_true', help='start in the collision-aware first-person controller')
    parser.add_argument('--world', type=int, choices=(64, 128, 256), default=128)
    parser.add_argument('--width', type=int, default=1280)
    parser.add_argument('--height', type=int, default=720)
    parser.add_argument('--capture', type=Path, help='save the final GPU image as BMP')
    parser.add_argument('--opt', choices=('0', '2', '3'), default='3')
    parser.add_argument('--lto', choices=('auto', 'off', 'thin', 'full'), default='auto', help='link-time optimization; auto tries ThinLTO, then full LTO')
    parser.add_argument('--portable', action='store_true', help='disable native CPU tuning')
    parser.add_argument('--gi', choices=('balanced', 'full'), default='balanced', help='balanced caches diffuse GI at half resolution; primary visibility and shadows remain full resolution')
    args = parser.parse_args()
    if args.width < 1 or args.height < 1 or args.width > 4096 or args.height > 4096:
        parser.error('width and height must be between 1 and 4096')
    if args.benchmark is not None and not 1 <= args.benchmark <= 100000:
        parser.error('benchmark frames must be between 1 and 100000')
    example = Path(__file__).resolve().parent
    root = example.parents[2]
    compiler = os.environ.get('CC', 'cc')
    flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', 'sdl3'], text=True))
    rill = os.environ.get('RILL')
    if not rill:
        existing = root / 'target' / 'debug' / 'rill'
        if not existing.is_file():
            subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(root / 'Cargo.toml')], check=True)
        rill = str(existing)
    environment = os.environ.copy()
    environment['RILL_VOXEL_GPU_SHADERS'] = str(example)
    environment['RILL_VOXEL_GPU_WORLD'] = str(args.world)
    environment['RILL_VOXEL_GPU_WIDTH'] = str(args.width)
    environment['RILL_VOXEL_GPU_HEIGHT'] = str(args.height)
    environment['RILL_VOXEL_GPU_GI'] = args.gi
    if args.stress:
        if args.benchmark is None:
            parser.error('--stress requires --benchmark')
        environment['RILL_VOXEL_GPU_STRESS'] = '1'
        environment['RILL_VOXEL_GPU_WALK'] = '1'
        environment['RILL_VOXEL_GPU_NO_DEMO'] = '1'
    if args.vsync:
        environment['RILL_VOXEL_GPU_VSYNC'] = '1'
    if args.no_demo:
        environment['RILL_VOXEL_GPU_NO_DEMO'] = '1'
    if args.lean:
        environment['RILL_VOXEL_GPU_LEAN'] = '1'
    if args.walk:
        environment['RILL_VOXEL_GPU_WALK'] = '1'
    if args.smoke:
        environment['RILL_VOXEL_GPU_SMOKE'] = '1'
        environment['RILL_VOXEL_GPU_FRAMES'] = '12'
        environment['RILL_VOXEL_GPU_HEADLESS'] = '1'
    if args.benchmark is not None:
        environment['RILL_VOXEL_GPU_BENCHMARK'] = '1'
        environment['RILL_VOXEL_GPU_SMOKE'] = '1'
        environment['RILL_VOXEL_GPU_FRAMES'] = str(args.benchmark)
        environment['RILL_VOXEL_GPU_HEADLESS'] = '1'
    if args.self_test:
        environment['RILL_VOXEL_GPU_HEADLESS'] = '1'
    if args.input_test:
        environment['RILL_VOXEL_GPU_HEADLESS'] = '0'
        environment['RILL_VOXEL_GPU_SMOKE'] = '0'
        environment['RILL_VOXEL_GPU_BENCHMARK'] = '0'
        environment.pop('RILL_VOXEL_GPU_FRAMES', None)
    if args.capture:
        environment['RILL_VOXEL_GPU_CAPTURE'] = str(args.capture.expanduser().resolve())
    with tempfile.TemporaryDirectory(prefix='rill-voxel-gpu-') as temporary:
        build = Path(temporary)
        def supports(options):
            probe = subprocess.run([compiler, '-std=c11', '-Werror', *options, '-x', 'c', '-', '-o', str(build / 'probe')],
                                   input='int main(void) { return 0; }\n', text=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            return probe.returncode == 0
        optimization = ['-O' + args.opt]
        if args.opt != '0':
            native = '-mcpu=native' if platform.machine().lower() in ('arm64', 'aarch64') else '-march=native'
            if not args.portable and supports(optimization + [native]):
                optimization.append(native)
            choices = {'auto': ['-flto=thin', '-flto'], 'thin': ['-flto=thin'], 'full': ['-flto'], 'off': []}[args.lto]
            selected = next((flag for flag in choices if supports(optimization + [flag])), None)
            if selected:
                optimization.append(selected)
            elif args.lto not in ('auto', 'off'):
                parser.error('the selected compiler/linker does not support requested LTO')
        elif args.lto not in ('auto', 'off'):
            parser.error('--opt 0 requires --lto auto or off')
        print('Native game build: ' + ' '.join(optimization), file=sys.stderr)
        adapter = build / 'gpu.o'
        subprocess.run([compiler, '-std=c11', *optimization, '-Wall', '-Wextra', '-Werror',
                        *flags, '-c', str(example / 'gpu.c'), '-o', str(adapter)], check=True)
        entry = example / 'main.rill'
        if args.self_test or args.input_test:
            entry = build / 'selftest.rill'
            test_function = 'vg_input_test' if args.input_test else 'vg_test'
            entry.write_text('@include("gpu.h");\n@pkg_config("sdl3");\n'
                             f'extern fn {test_function}() -> i32;\nfn main() {{ {test_function}() }}\n')
        binary = build / 'voxel'
        command = [rill, 'build', str(entry), '--cc', compiler, '--c-arg', '-I' + str(example)]
        for option in optimization:
            command.extend(['--c-arg', option])
        command.extend(['--link-arg', str(adapter), '--link-arg', '-lm', '-o', str(binary)])
        subprocess.run(command, check=True)
        result = subprocess.run([str(binary)], env=environment, stdout=subprocess.PIPE, text=True)
        print(result.stdout, end='')
        if result.returncode != 0:
            return result.returncode if result.returncode > 0 else 1
        lines = result.stdout.splitlines()
        if not lines:
            print('GPU program did not report a Rill main result', file=sys.stderr)
            return 1
        if lines[-1] == '0':
            return 0
        if (args.self_test or args.input_test) and lines[-1] == '77':
            return 77
        print('GPU program reported failure: ' + lines[-1], file=sys.stderr)
        return 1


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, subprocess.CalledProcessError) as error:
        print('GPU launcher: ' + str(error), file=sys.stderr)
        raise SystemExit(1)
