#!/usr/bin/env bash
# Run CarveX fuzz targets for a fixed wall-clock budget (guide-v2 §4.1 G5).
# Results are written as plain logs so the run can be audited without a GitHub
# runner.  Usage: scripts/fuzz.sh [seconds-per-target] [target ...]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SECONDS_PER_TARGET="${1:-60}"
shift || true
if [ "$#" -gt 0 ]; then
    TARGETS=("$@")
else
    TARGETS=(jpeg png pdf fat32_boot ntfs_boot ntfs_mft partition_table
             fuzz_e01 fuzz_vhd fuzz_vmdk fuzz_split_names fuzz_open_image
             fuzz_fragment_join free_stream)
fi
LOG_DIR="$ROOT/bench/results/fuzz"
mkdir -p "$LOG_DIR"

cd "$ROOT/fuzz"
for target in "${TARGETS[@]}"; do
    log="$LOG_DIR/${target}.log"
    printf '== fuzz %s (%ss) ==\n' "$target" "$SECONDS_PER_TARGET" | tee "$log"
    cargo +nightly fuzz run "$target" -- \
        -max_total_time="$SECONDS_PER_TARGET" -rss_limit_mb=4096 \
        2>&1 | tee -a "$log"
done

printf 'FUZZ COMPLETE: %s targets x %ss\n' "${#TARGETS[@]}" "$SECONDS_PER_TARGET"
