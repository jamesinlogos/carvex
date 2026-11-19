#!/usr/bin/env bash
# Measure line coverage with cargo-llvm-cov (guide-v2 §4.1 G5).
# Usage: scripts/coverage.sh [minimum-line-percentage]
# The default floor is the measured 0.2.0 baseline (63.42%) rounded down to 60%.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIN="${1:-60}"
OUT="$ROOT/bench/results/coverage"
mkdir -p "$OUT"
cd "$ROOT"

cargo llvm-cov --all-features --locked \
    --lcov --output-path "$OUT/lcov.info" \
    --fail-under-lines "$MIN" \
    2>&1 | tee "$OUT/coverage.log"

printf 'COVERAGE COMPLETE: minimum %s%% lines\n' "$MIN"
