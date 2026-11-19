#!/usr/bin/env bash
# Download and extract the NIST CFReDS Data Leakage Case RM#2 (FAT32, DD image).
#
# Usage:
#   ./scripts/fetch_cfreds.sh [DEST_DIR]
#
# Default destination: ~/cfreds-data
# The archive is ~219 MB; the extracted DD image is 4 GB. Ensure enough free
# space before running. NIST publishes a SHA-1 value for the archive; it is
# verified after download when sha1sum is available.

set -euo pipefail

DEST="${1:-$HOME/cfreds-data}"
URL="https://cfreds-archive.nist.gov/data_leakage_case/images/rm%232/cfreds_2015_data_leakage_rm%232.7z"
ARCHIVE="$DEST/cfreds_2015_data_leakage_rm2.7z"
EXPECTED_ARCHIVE_SHA1="DDFE97AA3D8D0B33CC6092123090A8154945F38E"
DD_NAME="cfreds_2015_data_leakage_rm#2.dd"
EXPECTED_DD_SHA1="048961A85CA3ECED8CC73F1517442D31D4DCA0A3"

mkdir -p "$DEST"

if [[ ! -f "$ARCHIVE" ]]; then
  echo "[*] Downloading $URL"
  curl -L --fail --retry 3 --progress-bar -o "$ARCHIVE.part" "$URL"
  mv "$ARCHIVE.part" "$ARCHIVE"
else
  echo "[*] Archive already exists: $ARCHIVE"
fi

if command -v sha1sum >/dev/null 2>&1; then
  echo "[*] Verifying archive SHA-1"
  actual="$(sha1sum "$ARCHIVE" | awk '{print toupper($1)}')"
  if [[ "$actual" != "$EXPECTED_ARCHIVE_SHA1" ]]; then
    echo "[!] Archive SHA-1 mismatch" >&2
    echo "    expected: $EXPECTED_ARCHIVE_SHA1" >&2
    echo "    actual  : $actual" >&2
    exit 1
  fi
  echo "[+] Archive SHA-1 OK"
else
  echo "[!] sha1sum not found; skipping integrity check" >&2
fi

if ! command -v 7z >/dev/null 2>&1; then
  echo "[!] 7z is required to extract the archive" >&2
  exit 1
fi

echo "[*] Archive contents:"
7z l "$ARCHIVE" | sed -n '1,80p'

echo "[*] Extracting to $DEST (the DD image is 4 GB)"
7z x -y -o"$DEST" "$ARCHIVE" >/dev/null

IMAGE="$DEST/$DD_NAME"
if [[ ! -f "$IMAGE" ]]; then
  IMAGE="$(find "$DEST" -maxdepth 2 -type f \( -iname '*.dd' -o -iname '*.img' -o -iname '*.raw' \) | head -n 1)"
fi
if [[ -z "${IMAGE:-}" || ! -f "$IMAGE" ]]; then
  echo "[!] No extracted DD/IMG/RAW image found in $DEST" >&2
  exit 1
fi
echo "[+] Image: $IMAGE ($(stat -c%s "$IMAGE") bytes)"

if command -v sha1sum >/dev/null 2>&1; then
  echo "[*] Verifying DD image SHA-1 from NIST answer document"
  actual="$(sha1sum "$IMAGE" | awk '{print toupper($1)}')"
  if [[ "$actual" != "$EXPECTED_DD_SHA1" ]]; then
    echo "[!] DD SHA-1 mismatch" >&2
    echo "    expected: $EXPECTED_DD_SHA1" >&2
    echo "    actual  : $actual" >&2
    exit 1
  fi
  echo "[+] DD SHA-1 OK"
fi

cat <<EOF

Next steps:
  carvex info  <image>
  carvex scan  <image> -o recovered-cfreds --mode unallocated \\
      --types jpeg,png,pdf --json-report cfreds-report.json

Notes:
  - Work only on a copy of the image.
  - CarveX reads raw/DD images; it does not open E01 directly.
  - This dataset is for validation, not for measuring accuracy: the archive
    does not include a per-file ground-truth manifest for CarveX's benchmark.
EOF
