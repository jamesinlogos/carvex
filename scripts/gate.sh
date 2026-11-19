#!/usr/bin/env bash
# CarveX release gate (guide-v2 §3.2).
#
# Every command must succeed before a change is considered done.  Optional
# tools that are not installed are reported as SKIPPED; record those in
# docs/PROGRESS.md.  Run from anywhere: the script changes to the repo root.
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
MSRV=$(grep -m1 '^rust-version' Cargo.toml | cut -d'"' -f2)
if rustup toolchain list 2>/dev/null | grep -q "^${MSRV}"; then
    cargo +"${MSRV}" check --locked
else
    echo "SKIPPED MSRV ${MSRV} (toolchain missing)"
fi

echo "== release build =="
cargo build --release --locked

echo "== review repro =="
NTFS_IMG=bench/images/all-deleted_ntfs.img
FAT32_IMG=bench/images/all-deleted_fat32.img
if [ ! -f "$NTFS_IMG" ] || [ ! -f "$FAT32_IMG" ]; then
    python3 bench/make_images.py >/dev/null 2>&1 || true
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

echo "== benchmark smoke =="
python3 bench/run_benchmark.py --smoke

echo "GATE GREEN"
