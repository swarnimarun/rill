#!/bin/sh
# Builds all three v1 parsers with -O2-style optimisation and times them on the
# same input, 3 runs each (best of 3).
set -e
cd "$(dirname "$0")/.."
REPS=${REPS:-3}
python3 - <<'PY'
import json, random
random.seed(7)
rows = []
for i in range(4000):
    rows.append({"id": i, "name": "user-" + str(i) + "-name", "active": i % 3 == 0,
                 "score": round(random.random() * 1000, 3),
                 "tags": ["alpha", "beta", "gamma"][: (i % 3) + 1],
                 "meta": {"depth": i % 5, "note": "escaped \\\" quote inside"}})
open("/tmp/bench.json", "w").write(json.dumps({"rows": rows}, separators=(",", ":")))
PY
echo "input: $(wc -c < /tmp/bench.json) bytes"
cargo run -q -p rill-driver -- build bench/json.v1.rill -o /tmp/json_rill >/dev/null
rustc -O --crate-name json_v1 -o /tmp/json_rust bench/json.v1.rs
zig build-exe -O ReleaseFast -femit-bin=/tmp/json_zig bench/json.v1.zig
printf '%-8s %-12s %s\n' program run1 run2 run3
for prog in json_rill json_rust json_zig; do
  line=$(printf '%-8s' "$prog")
  for run in 1 2 3; do
    t=$( { /usr/bin/time -p /tmp/$prog >/dev/null 2>/tmp/time_out; } 2>&1; grep real /tmp/time_out | awk '{print $2}' )
    line="$line $(printf '%-12s' "${t}s")"
  done
  echo "$line"
done
