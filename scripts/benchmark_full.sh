#!/usr/bin/env bash
# Run the full 4-image dev benchmark (all tools, both CarveX modes, 3 reps).
# The published 0.1.0 results in bench/results/final are never modified.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/bench/results/full-0.2.0"
cd "$ROOT"

if [ -e "$OUT" ] && [ -n "$(ls -A "$OUT")" ]; then
    printf 'refusing to overwrite non-empty %s\n' "$OUT" >&2
    exit 1
fi

cargo build --release --locked
python3 bench/run_benchmark.py \
    --tools carvex,foremost,scalpel,photorec \
    --carvex-modes unallocated,raw \
    --reps 3 \
    --out "$OUT" 2>&1 | tee "$ROOT/bench/results/full-run.log"

(
    cd "$OUT"
    find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
printf 'FULL BENCHMARK COMPLETE: %s\n' "$OUT"
