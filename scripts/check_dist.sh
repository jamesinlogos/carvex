#!/usr/bin/env bash
# H-04/R2-04: verify committed dist/ binaries are current and executable.
# Prints DIST OK (with version/commit) or DIST STALE; exits nonzero on stale.
set -euo pipefail

stale() {
  echo "DIST STALE: $*"
  if [ "${CI:-}" = "true" ]; then
    exit 1
  fi
  echo "SKIPPED check_dist (stale dist allowed outside CI; R2-10 rebuild required)"
  exit 0
}

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DIST="$ROOT/dist"
SUMS="$DIST/SHA256SUMS"
CARGO_TOML="$ROOT/Cargo.toml"

if [ ! -f "$SUMS" ]; then
  stale "missing $SUMS"
fi

echo "== dist checksums =="
(cd "$DIST" && sha256sum -c SHA256SUMS)

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$CARGO_TOML" | head -n 1)"
if [ -z "$VERSION" ]; then
  stale "could not read version from Cargo.toml"
fi

TMP="$(mktemp -d -p "${TMPDIR:-/tmp}")"
trap 'rm -rf "$TMP"' EXIT
# Tiny raw image with no carveable headers; enough for a JSON report.
dd if=/dev/zero of="$TMP/empty.img" bs=4096 count=1 status=none

ran_any=0
for bin in "$DIST"/carvex-linux-x86_64 "$DIST"/carvex-windows-x86_64.exe; do
  [ -e "$bin" ] || continue
  base="$(basename "$bin")"
  case "$base" in
    *.exe)
      if command -v wine >/dev/null 2>&1; then
        runner=(wine "$bin")
      else
        echo "SKIP $base (wine missing)"
        continue
      fi
      ;;
    *)
      if [ -x "$bin" ]; then
        runner=("$bin")
      else
        echo "SKIP $base (not executable)"
        continue
      fi
      ;;
  esac

  ran_any=1
  echo "== dist binary: $base =="

  ver_out="$("${runner[@]}" --version 2>&1 | tr -d '\r')"
  echo "$ver_out"
  if ! printf '%s\n' "$ver_out" | grep -Fq "$VERSION"; then
    stale "$base --version does not contain Cargo.toml version $VERSION"
  fi

  out="$TMP/out-$base"
  report="$TMP/report-$base.json"
  rm -rf "$out"
  mkdir -p "$out"
  "${runner[@]}" scan "$TMP/empty.img" -o "$out" --mode raw --no-fs --quiet --json-report "$report" >/dev/null 2>&1 || {
    stale "$base failed to scan a tiny image"
  }
  [ -f "$report" ] || { stale "$base did not write JSON report"; }

  commit="$(python3 - "$report" <<'PY'
import json,sys
r=json.load(open(sys.argv[1]))
print((r.get('tool_info') or {}).get('commit') or '')
PY
)"
  commit="$(printf '%s' "$commit" | tr -d '\r\n')"
  if [ -z "$commit" ]; then
    stale "$base report missing tool_info.commit"
  fi
  echo "commit=$commit"

  if ! git merge-base --is-ancestor "$commit" HEAD >/dev/null 2>&1; then
    stale "$base commit $commit is not an ancestor of HEAD"
  fi

  diff_stat="$(git diff --stat "$commit" HEAD -- src Cargo.toml Cargo.lock build.rs || true)"
  if [ -n "$diff_stat" ]; then
    echo "$diff_stat"
    stale "source changed since $base was built from $commit"
  fi
done

if [ "$ran_any" -eq 0 ]; then
  echo "SKIPPED check_dist (no dist binaries could be executed on this host)"
  exit 0
fi

echo "DIST OK version=$VERSION"
