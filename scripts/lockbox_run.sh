#!/usr/bin/env bash
# Execute the frozen 20-image lockbox exactly once (protocol-v1 §7).
# Never use these results to tune code; a real bug requires fresh seeds.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/bench/results/lockbox-v1"
cd "$ROOT"

if [ ! -f bench/lockbox/lockbox.json ]; then
    printf 'lockbox dataset missing: run python3 bench/lockbox.py first\n' >&2
    exit 1
fi
if [ -e "$OUT" ] && [ -n "$(ls -A "$OUT")" ]; then
    printf 'refusing to overwrite non-empty %s\n' "$OUT" >&2
    exit 1
fi

python3 bench/run_benchmark.py \
    --images-dir bench/lockbox/images \
    --corpus bench/lockbox/corpus \
    --tools carvex,foremost,scalpel,photorec \
    --carvex-modes unallocated,raw \
    --reps 1 \
    --out "$OUT" 2>&1 | tee "$ROOT/bench/results/lockbox-run.log"

(
    cd "$OUT"
    find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
# Never rewrite the dataset checksum file; verify the frozen set and copy it
# into the result directory as evidence.
(cd bench/lockbox && sha256sum -c SHA256SUMS)
cp bench/lockbox/SHA256SUMS "$OUT/DATASET_SHA256SUMS"
printf 'LOCKBOX RUN COMPLETE: %s\n' "$OUT"
