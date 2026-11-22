#!/usr/bin/env python3
"""G9 documentation renderer/checker.

The generated block is delimited by markers in README.md.  Its contents are
derived only from `carvex scan --help`, so a flag that exists in the binary but
not in the README (or a stale default) fails the check.

Usage:
    python3 bench/render_docs.py --check [--carvex PATH]
    python3 bench/render_docs.py --write [--carvex PATH]
"""
from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

START = "<!-- BEGIN GENERATED SCAN FLAG TABLE -->"
END = "<!-- END GENERATED SCAN FLAG TABLE -->"

OPTION_RE = re.compile(r"^\s{2,6}(?:-(\w),\s+)?(--[\w-]+)(?:\s+<([^>]+)>)?\s*$")
DEFAULT_RE = re.compile(r"\[default:\s*([^\]]+)\]")
ALIAS_RE = re.compile(r"\[alias:\s*([^\]]+)\]")


def parse_scan_help(text: str) -> list[dict[str, str]]:
    """Return ordered option records from clap's long-help output."""
    records: list[dict[str, str]] = []
    current: dict[str, str] | None = None
    collecting_description = False

    for raw in text.splitlines():
        match = OPTION_RE.match(raw)
        if match:
            short, long, value = match.groups()
            current = {
                "short": short or "",
                "long": long,
                "value": value or "",
                "description": "",
                "default": "",
                "alias": "",
            }
            records.append(current)
            collecting_description = True
            continue
        if current is None:
            continue

        stripped = raw.strip()
        if not stripped:
            collecting_description = False
            continue

        default = DEFAULT_RE.search(stripped)
        if default:
            current["default"] = default.group(1).strip()
            collecting_description = False
            continue
        alias = ALIAS_RE.search(stripped)
        if alias:
            current["alias"] = alias.group(1).strip()
            collecting_description = False
            continue
        if stripped.startswith("[possible values:") or stripped.startswith("- "):
            collecting_description = False
            continue
        if stripped.startswith("Possible values:"):
            collecting_description = False
            continue
        if collecting_description and not stripped.startswith("["):
            current["description"] = stripped
            if "on by default" in stripped:
                current["default"] = "on"
            implied = re.search(r"'([^']+)'\s+\(default\)", stripped)
            if implied:
                current["default"] = implied.group(1)
            collecting_description = False

    return records


def render_table(records: list[dict[str, str]]) -> str:
    lines = [
        "| Option | Default | Meaning |",
        "|---|---|---|",
    ]
    for option in records:
        if option["long"] in {"--help"}:
            continue
        name = option["long"]
        if option["value"]:
            name += f" <{option['value']}>"
        meaning = option["description"]
        if option["alias"]:
            meaning += f" (alias: `{option['alias']}`)"
        default = option["default"] or (
            "required" if option["long"] == "--output" else "off"
        )
        lines.append(
            f"| `{name}` | `{default}` | {meaning} |"
        )
    return "\n".join(lines)


def generated_block(carvex: Path) -> str:
    if not carvex.is_file():
        raise SystemExit(f"carvex binary not found: {carvex}")
    text = subprocess.run(
        [str(carvex), "scan", "--help"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    records = parse_scan_help(text)
    if not records:
        raise SystemExit("no scan options parsed from --help")
    return "\n".join([START, render_table(records), END])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if README is stale")
    parser.add_argument("--write", action="store_true", help="rewrite the generated block")
    parser.add_argument("--carvex", type=Path, default=Path("target/release/carvex"))
    parser.add_argument("--readme", type=Path, default=Path("README.md"))
    args = parser.parse_args()
    if args.check == args.write:
        parser.error("choose exactly one of --check or --write")

    block = generated_block(args.carvex.resolve())
    readme = args.readme.resolve()
    text = readme.read_text()
    start = text.find(START)
    end = text.find(END)
    if start < 0 or end < 0 or end <= start:
        raise SystemExit(f"generated markers missing in {readme}")

    if args.write:
        prefix = text[:start]
        suffix = text[end + len(END):]
        readme.write_text(prefix + block + suffix)
        print(f"G9 WROTE generated flag table in {readme}")
        return 0

    actual = text[start:end + len(END)]
    if actual != block:
        print("G9 FAIL: README flag table does not match carvex scan --help")
        print(f"expected block in {readme}:")
        print(block)
        return 1
    print(f"G9 GREEN: {len(parse_scan_help(subprocess.run([str(args.carvex.resolve()), 'scan', '--help'], check=True, capture_output=True, text=True).stdout))} scan options match README")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
