#!/usr/bin/env python3
"""Compare one fixed i32 loop through emitted C, C and Rust.

Build Rill first, then run with --rill target/debug/rill. Requires a POSIX C
compiler, rustc and Python 3. This measures kernel execution, not build/startup
time or whole-language performance. Temporary artifacts are removed.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--rill', type=Path, default=ROOT / 'target/debug/rill')
parser.add_argument('--cc', default=os.environ.get('CC', 'cc'))
parser.add_argument('--rustc', default=os.environ.get('RUSTC', 'rustc'))
options = parser.parse_args()
RILL = options.rill.resolve()
CC = options.cc
RUSTC = options.rustc
COUNT = 5_000_000
RILL_SOURCE = '''fn kernel(limit: i32) {
    let mut i = 0;
    let mut sum = 0;
    while i < limit {
        sum <- (sum + i % 97) % 1000003;
        i <- i + 1;
    };
    sum
}
fn main() { kernel(5000000) }
'''
C_PREFIX = '''#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <time.h>
#include <stdlib.h>
'''
C_KERNEL = '''static int32_t kernel(int32_t limit) {
    int32_t sum = 0;
    for (int32_t i = 0; i < limit; ++i) {
        sum = (sum + i % 97) % 1000003;
    }
    return sum;
}
'''
C_TIMER = '''static uint64_t nanoseconds(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t) != 0) { perror("clock_gettime"); exit(1); }
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
int main(void) {
    volatile int32_t iterations = 5000000;
    uint64_t start = nanoseconds();
    volatile int32_t result = KERNEL(iterations);
    uint64_t elapsed = nanoseconds() - start;
    printf("%" PRId32 " %" PRIu64 "\\n", (int32_t)result, elapsed);
    return 0;
}
'''
RUST_SOURCE = '''use std::{hint::black_box, time::Instant};
fn kernel(limit: i32) -> i32 {
    let mut sum: i32 = 0;
    for i in 0..limit { sum = (sum + i % 97) % 1000003; }
    sum
}
fn main() {
    let count = black_box(5_000_000_i32);
    let start = Instant::now();
    let result = black_box(kernel(black_box(count)));
    let elapsed = start.elapsed().as_nanos();
    println!("{result} {elapsed}");
}
'''

def execute(args):
    return subprocess.run([str(v) for v in args], check=True, capture_output=True, text=True).stdout.strip()

expected = sum(i % 97 for i in range(COUNT)) % 1000003
with tempfile.TemporaryDirectory(prefix='rill-native-loop-') as temporary:
    directory = Path(temporary)
    source = directory / 'loop.rill'
    source.write_text(RILL_SOURCE)
    generated = execute([RILL, 'emit-c', source])
    (directory / 'generated.c').write_text(generated)
    kernel_symbols = re.findall(r'^static int32_t (rl_fn_\d+)\(int32_t [^,()]+\);$', generated, re.MULTILINE)
    if len(kernel_symbols) != 1:
        raise RuntimeError(f'expected exactly one generated i32 -> i32 kernel, got {kernel_symbols}')
    wrapper = C_PREFIX + '#define main rill_generated_main\n#include "generated.c"\n#undef main\n' + C_TIMER.replace('KERNEL', kernel_symbols[0])
    (directory / 'rill.c').write_text(wrapper)
    (directory / 'baseline.c').write_text(C_PREFIX + C_KERNEL + C_TIMER.replace('KERNEL', 'kernel'))
    (directory / 'baseline.rs').write_text(RUST_SOURCE)
    commands = {
        'rill': [CC, '-std=c11', '-O3', directory / 'rill.c', '-o', directory / 'rill'],
        'c': [CC, '-std=c11', '-O3', directory / 'baseline.c', '-o', directory / 'c'],
        'rust': [RUSTC, '--edition', '2021', '-C', 'opt-level=3', '-C', 'overflow-checks=yes', directory / 'baseline.rs', '-o', directory / 'rust'],
    }
    for command in commands.values():
        execute(command)
    samples = {name: [] for name in commands}
    for trial in range(8):
        # Rotate order between trials; the first run per language is a warmup.
        names = list(commands)
        names = names[trial % 3:] + names[:trial % 3]
        for name in names:
            checksum, elapsed = map(int, execute([directory / name]).split())
            if checksum != expected:
                raise RuntimeError(f'{name} checksum {checksum} != {expected}')
            if trial:
                samples[name].append(elapsed)
    report = {
        'machine': platform.machine(),
        'platform': platform.platform(),
        'cc': execute([CC, '--version']).splitlines()[0],
        'rustc': execute([RUSTC, '--version']),
        'iterations': COUNT,
        'expected_checksum': expected,
        'generated_kernel': kernel_symbols[0],
        'bounds': '0 <= sum < 1000003; 0 <= i <= 5000000; largest sum+i%97 is 1000098, below i32::MAX; both divisors are positive',
        'timing': 'kernel only; monotonic nanoseconds; startup and printing excluded; one warmup and seven samples; rotated process order',
        'flags': {'rill_and_c': ['-std=c11', '-O3'], 'rust': ['--edition', '2021', '-C', 'opt-level=3', '-C', 'overflow-checks=yes']},
        'results': {name: {'median_ns': int(statistics.median(times)), 'min_ns': min(times), 'max_ns': max(times), 'samples_ns': times} for name, times in samples.items()},
    }
    print(json.dumps(report, indent=2))
