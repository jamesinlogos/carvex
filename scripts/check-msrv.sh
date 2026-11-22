#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
MSRV=$(grep -m1 '^rust-version' Cargo.toml | cut -d'"' -f2)
if [ -z "$MSRV" ]; then
  echo "rust-version not found in Cargo.toml" >&2
  exit 1
fi
if ! rustup toolchain list 2>/dev/null | grep -q "^${MSRV}"; then
  echo "SKIPPED MSRV ${MSRV} (toolchain missing)"
  exit 0
fi
echo "== MSRV ${MSRV} =="
cargo +"${MSRV}" check --locked
