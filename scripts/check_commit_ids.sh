#!/usr/bin/env bash
# H-01: warn when commits after the harness baseline are missing [H-xx]/[N-xx] IDs.
# Warning-only for pre-existing history.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
BASELINE="${1:-706a12a}"
if ! git rev-parse --verify "$BASELINE" >/dev/null 2>&1; then
  echo "SKIPPED check_commit_ids (baseline $BASELINE not found)"
  exit 0
fi
missing=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  sha="${line%% *}"
  msg="${line#* }"
  if ! echo "$msg" | grep -Eq '\[(H-[0-9]{2}|N-[0-9]{2})\]'; then
    echo "WARN commit missing [H-xx]/[N-xx]: $sha $msg"
    missing=$((missing + 1))
  fi
done < <(git log --format='%h %s' "${BASELINE}..HEAD")
if [ "$missing" -eq 0 ]; then
  echo "OK commit IDs"
else
  echo "WARN ${missing} commit(s) missing finding IDs (warning-only)"
fi
exit 0
