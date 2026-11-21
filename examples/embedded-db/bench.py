"""Measure local suffix replication into a replica with an established prefix."""
import argparse
import filecmp
import os
from pathlib import Path
import shutil
import statistics
import struct
import subprocess
import tempfile
import time


def write_rows(file, start, stop):
    # At most 8192 rows per allocation, including fixture setup.
    for first in range(start, stop, 8192):
        file.write(b"".join(
            struct.pack("<ii", revision % 65536, revision)
            for revision in range(first, min(first + 8192, stop))
        ))


def replicate(exe, source, replica, cursor):
    start = time.perf_counter()
    with subprocess.Popen(
        [exe], env={**os.environ, "RILL_DB_PATH": str(source)},
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    ) as sender:
        sender.stdin.write(f"changes {cursor}\n".encode("ascii"))
        sender.stdin.close()
        with subprocess.Popen(
            [exe], env={**os.environ, "RILL_DB_PATH": str(replica)}, stdin=sender.stdout
        ) as receiver:
            sender.stdout.close()
            receiver_status = receiver.wait()
        sender_status = sender.wait()
    if receiver_status or sender_status:
        raise RuntimeError(f"replication failed: sender={sender_status} receiver={receiver_status}")
    return time.perf_counter() - start


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--rows", type=int, default=4_000_000)
    parser.add_argument("--delta", type=int, default=100_000)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if not (0 < args.delta <= args.rows <= 2**31 - 1 and args.runs >= 1):
        parser.error("require 0 < delta <= rows <= 2147483647 and runs >= 1")
    exe = args.executable.resolve(strict=True)
    with tempfile.TemporaryDirectory() as tmp:
        prefix, source, replica = [Path(tmp) / name for name in ("prefix", "source", "replica")]
        cursor = args.rows - args.delta
        with prefix.open("wb") as file:
            file.write(b"RDB1")
            write_rows(file, 1, cursor + 1)
        shutil.copyfile(prefix, source)
        with source.open("ab") as file:
            write_rows(file, cursor + 1, args.rows + 1)
        timings = []
        for _ in range(args.runs):
            shutil.copyfile(prefix, replica)
            timings.append(replicate(exe, source, replica, cursor))
            if not filecmp.cmp(source, replica, shallow=False):
                raise RuntimeError("replica differs from source")
        elapsed = statistics.median(timings)
        print(f"{args.rows:,} rows, {args.delta:,} replicated, "
              f"{args.runs} runs, median {elapsed:.3f}s ({args.delta / elapsed:,.0f} rows/s)")


if __name__ == "__main__":
    main()
