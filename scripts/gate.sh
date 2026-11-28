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

# Keep the failing command visible in CI annotations even when the command's
# own output does not start with FAIL/XPASS (cargo, Python checks, perf, dist).
trap 'rc=$?; echo "GATE FAILED at line $LINENO: $BASH_COMMAND"; exit "$rc"' ERR

echo "== cargo fmt =="
cargo fmt --all -- --check

echo "== clippy =="
cargo clippy --all-targets --all-features --locked -- -D warnings

echo "== clippy size and argument limits (H-22) =="
cargo clippy --lib --locked -- -D clippy::too_many_lines -D clippy::too_many_arguments

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

echo "== docs check (G9) =="
python3 bench/render_docs.py --check --carvex target/release/carvex

echo "== status labels (H-31) =="
python3 scripts/check_status.py

echo "== performance gate (G8) =="
python3 scripts/perf_gate.py --carvex target/release/carvex

echo "== coverage (G5) =="
if cargo llvm-cov --version >/dev/null 2>&1; then
    scripts/coverage.sh "${CARVEX_COVERAGE_MIN:-70}"
else
    echo "SKIPPED coverage (cargo-llvm-cov not installed)"
fi

echo "== fuzz smoke (G5) =="
if command -v cargo-fuzz >/dev/null 2>&1 && cargo +nightly fuzz --version >/dev/null 2>&1; then
    scripts/fuzz.sh "${CARVEX_FUZZ_SECONDS:-60}"
else
    echo "SKIPPED fuzz smoke (cargo-fuzz or nightly toolchain not installed)"
fi

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

echo "== declared-size scan plans (X08b/X26) =="
python3 scripts/repro_x08b.py --carvex target/release/carvex
python3 scripts/repro_x26.py --carvex target/release/carvex

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

echo "== dist (R2-04) =="
bash scripts/check_dist.sh

echo "GATE GREEN"
