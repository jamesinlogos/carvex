#!/usr/bin/env python3
"""Generate a minimal SPDX software bill of materials from Cargo metadata.

No new dependency or network access is required. The output is deterministic
for a committed Cargo.lock and is suitable as a CI artifact.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path("bench/results/sbom.spdx.json"))
    args = parser.parse_args()

    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    root = metadata["resolve"]["root"] if metadata.get("resolve") else None
    packages = sorted(
        metadata["packages"],
        key=lambda p: (p["name"], p["version"], str(p.get("source") or "")),
    )

    doc = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "CarveX",
        "documentNamespace": "https://example.invalid/carvex/spdx/cargo-lock",
        "creationInfo": {"created": "1970-01-01T00:00:00Z", "creators": ["Tool: cargo-metadata"]},
        "documentDescribes": ["SPDXRef-Package-carvex"],
        "packages": [],
        "relationships": [],
    }
    seen: set[str] = set()
    for package in packages:
        ident = "SPDXRef-Package-carvex" if package["id"] == root else "SPDXRef-Package-" + hashlib.sha256(
            f"{package['name']}@{package['version']}@{package.get('source')}".encode()
        ).hexdigest()[:16]
        if ident in seen:
            continue
        seen.add(ident)
        doc["packages"].append(
            {
                "SPDXID": ident,
                "name": package["name"],
                "versionInfo": package["version"],
                "licenseDeclared": package.get("license") or "NOASSERTION",
                "downloadLocation": package.get("source") or "NOASSERTION",
                "filesAnalyzed": False,
            }
        )
        if package["id"] != root:
            doc["relationships"].append(
                {
                    "spdxElementId": "SPDXRef-Package-carvex",
                    "relationshipType": "DEPENDS_ON",
                    "relatedSpdxElement": ident,
                }
            )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n")
    print(f"SBOM OK: {len(doc['packages'])} packages -> {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
