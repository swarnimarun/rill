# Embedded revision database

Store signed 32-bit keys and values in one file. A write adds an immutable revision. Reading a key returns its most recent value as `Option<i32>`. Replicas can apply an incremental change feed and safely repeat an acknowledged revision.

The Rill [store module](store.rill) implements lookup, aggregation, revision ordering and conflict detection. The [command reader](main.rill) maps typed outcomes to console results. The [C11 adapter](file.c) supplies file operations, fixed-width encoding and process I/O through explicit scalar foreign declarations. It needs a host C compiler and the ordinary Rill compiler.

## Try two replicas

From the repository root on a POSIX shell, create a fresh directory for the binaries and store files.

```sh
work=$(mktemp -d)
cc -std=c11 -O2 -c examples/embedded-db/file.c -o "$work/file.o"
cargo run --locked -- build examples/embedded-db/main.rill --link-arg "$work/file.o" -o "$work/db"

printf 'put 10 30\nput 10 42\nput 20 -1\nget 10\n' | RILL_DB_PATH="$work/local" "$work/db"
printf 'changes 0\n' | RILL_DB_PATH="$work/local" "$work/db" > "$work/batch"
RILL_DB_PATH="$work/replica" "$work/db" < "$work/batch"
printf 'get 20\ncount_ge 0 0\n' | RILL_DB_PATH="$work/replica" "$work/db"
```

The first query prints `42`. The last two print `-1` and `2`. To transfer subsequent changes, request `changes 3` after the replica has accepted revisions 1 through 3. Preserve the batch and the last verified cursor until the consumer exits successfully. Retrying the batch is idempotent.

| Command | Effect |
| --- | --- |
| `put key value` | Append at the next revision. |
| `head` | Print the last accepted revision, or zero for an empty file. |
| `get key` | Print the current value, or `missing` for an absent key. |
| `changes cursor` | Emit `apply revision key value` lines for all revisions strictly after the cursor. Zero starts at the beginning. |
| `apply revision key value` | Append the next revision or verify an identical previously applied row. A duplicate with different contents or a gap fails. |
| `count_ge cursor floor` | Count revision values at least `floor` after the cursor, including superseded values. |

Commands read one line at a time and stop with an error on invalid input. Earlier successful commands in the same batch may already be persisted. Supply a cursor within the file's revision range. Reuse a nonzero cursor only for replicas with an established identical prefix. To compare independently modified replicas, replay from zero. The protocol identifies the first conflicting revision and leaves reconciliation to the caller.

## Storage and access costs

The four-byte ASCII header is `RDB1`. Each subsequent eight-byte row stores two little-endian, two's-complement i32 values, key then value. Its one-based position is its revision. File size determines the revision count. Each append writes eight bytes. Validating an old revision takes one positioned read. A suffix scan seeks once and reads exactly the requested rows into a fixed-size buffer. Looking up a key takes one full sequential scan. All queries use constant working memory relative to file size. The revision limit is 2,147,483,647, with a lower file-size limit on hosts with a narrow C `long`.

Serialize file access externally. The adapter supports one open database per process and uses buffered I/O. Closing checks for write errors. Opening validates the format and length, including truncated final rows. Whole-record corruption, concurrent writers, crash durability, transaction rollback, indexing and network transport remain open. File I/O errors terminate the host. For a reusable app runtime, returning explicit I/O errors and tracking file-handle lifetimes are prerequisites. An indexed snapshot and a transactional journal are the next storage increments. A sync performance claim needs measured rows/second, working set and recovery latency on the target device.

Run the executable acceptance test with `cargo test --locked --test embedded_db`. It builds both unoptimized and optimized C programs, checks file bytes after rejected mutations, replays a delta twice across process restarts, and filters a 100,000-row store.

## Measure local sync

With the executable built above, run `python3 examples/embedded-db/bench.py "$work/db"`. The driver writes a 4,000,000-row source (32,000,004 bytes), gives each receiver the same 3,900,000-row prefix, then pipes the remaining 100,000 updates from one native process into the other. It compares the entire resulting file after every run. Use `--rows`, `--delta` and `--runs` to vary the workload. Temporary storage must accommodate three store files.

A Linux x86-64 development run with optimized C took a median of 0.020 seconds over five runs, approximately 4.9 million rows/s. This wall time includes process launch, replay, and closing buffered files. File generation, prefix copying and final verification are outside the timed interval. The local pipe and OS page cache are part of this measurement. Network latency and durable commits need separate workloads.
