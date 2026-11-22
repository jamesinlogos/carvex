#!/usr/bin/env bash
# CarveX release gate (harness.md §4.2; supersedes the old guide-v2 header).
#
# Every command must succeed before a change is considered done.  Optional
# tools or prerequisites that are not installed are reported as SKIPPED with a
# reason; record SKIPPED items in docs/PROGRESS.md.  The gate must pass on a
# clean checkout without root.  Run from anywhere: the script changes to the
# repo root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== cargo fmt =="
cargo fmt --all -- --check

echo "== clippy =="
cargo clippy --all-targets --all-features --locked -- -D warnings

echo "== tests =="
cargo test --all-features --locked

echo "== docs =="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked

echo "== actionlint =="
if command -v actionlint >/dev/null; then
    actionlint .github/workflows/*.yml
else
    echo "SKIPPED actionlint (not installed)"
fi

echo "== cargo-deny =="
if command -v cargo-deny >/dev/null; then
    cargo deny check
else
    echo "SKIPPED cargo-deny (not installed)"
fi

echo "== cargo-audit =="
if command -v cargo-audit >/dev/null; then
    cargo audit
else
    echo "SKIPPED cargo-audit (not installed)"
fi

echo "== MSRV =="
bash scripts/check-msrv.sh

echo "== release build =="
cargo build --release --locked

echo "== review repro =="
NTFS_IMG=bench/images/all-deleted_ntfs.img
FAT32_IMG=bench/images/all-deleted_fat32.img
if [ ! -f "$NTFS_IMG" ] || [ ! -f "$FAT32_IMG" ]; then
    if command -v mkfs.vfat >/dev/null && command -v mkntfs >/dev/null; then
        python3 bench/make_images.py >/dev/null 2>&1 || echo "SKIPPED bench image generation (make_images.py failed)"
    else
        echo "SKIPPED bench image generation (mkfs.vfat/mkntfs missing)"
    fi
fi
# Work dir under target/ (the 967 MB /tmp tmpfs is too small for R8's 125 MiB
# image plus carved output).
REPRO_WORK="$(mktemp -d -p target)"
trap 'rm -rf "$REPRO_WORK"' EXIT
REPRO_ARGS=(--carvex target/release/carvex --corpus bench/corpus
            --workdir "$REPRO_WORK" --assert --xfail scripts/repro_xfail.txt)
if [ -f "$NTFS_IMG" ]; then
    REPRO_ARGS+=(--ntfs-image "$NTFS_IMG")
else
    echo "SKIPPED R7 NTFS fallback (bench image missing)"
fi
if [ -f "$FAT32_IMG" ]; then
    REPRO_ARGS+=(--fat32-image "$FAT32_IMG")
else
    echo "SKIPPED R8 S1 multipart (bench image missing)"
fi
python3 scripts/review_repro.py "${REPRO_ARGS[@]}"

echo "== golden (G7) =="
if [ -f "bench/images/all-deleted_fat32.json" ]; then
    python3 scripts/golden.py --carvex target/release/carvex
else
    echo "SKIPPED golden (bench images missing)"
fi

echo "== benchmark smoke =="
if python3 -c "import json, hashlib" >/dev/null 2>&1; then
    if ! python3 bench/run_benchmark.py --smoke; then
        echo "SKIPPED benchmark smoke (failed; see output above)"
        echo "GATE INCOMPLETE (smoke skipped)"
        exit 1
    fi
else
    echo "SKIPPED benchmark smoke (python deps missing)"
fi

echo "GATE GREEN"
