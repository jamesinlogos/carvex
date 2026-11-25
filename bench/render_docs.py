#!/usr/bin/env python3
"""G9 documentation renderer/checker.

The generated blocks are delimited by markers in README.md.  The flag table is
derived only from `carvex scan --help`, and the test count/breakdown is derived
from `cargo test --all-features --locked -- --list`, so stale documentation
fails the check.

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
TEST_START = "<!-- BEGIN GENERATED TEST COUNT -->"
TEST_END = "<!-- END GENERATED TEST COUNT -->"
ROOT = Path(__file__).resolve().parent.parent

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


RUNNING_RE = re.compile(r"^\s*Running (?:unittests )?(\S+)")
COUNT_RE = re.compile(r"^(\d+) tests?, \d+ benchmarks?$")


def parse_test_list(text: str) -> tuple[int, list[tuple[str, int]]]:
    """Count registered tests per binary from `cargo test -- --list` output."""
    counts: dict[str, int] = {}
    current = ""
    for raw in text.splitlines():
        running = RUNNING_RE.match(raw)
        if running:
            path = running.group(1)
            current = "unit" if path == "src/lib.rs" else Path(path).stem
            continue
        count = COUNT_RE.match(raw.strip())
        if count and current and current != "main" and int(count.group(1)):
            counts[current] = counts[current] + int(count.group(1)) if current in counts else int(count.group(1))
            current = ""
    total = sum(counts.values())
    if total == 0:
        raise SystemExit("no tests parsed from cargo test --list")
    groups = [("unit", counts.pop("unit"))] if "unit" in counts else []
    groups.extend(sorted(counts.items()))
    return total, groups


def generated_test_block() -> tuple[str, int]:
    result = subprocess.run(
        ["cargo", "test", "--all-features", "--locked", "--", "--list"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
        stderr=subprocess.STDOUT,
    )
    total, groups = parse_test_list(result.stdout)
    breakdown = " + ".join(
        f"{count} {name if name == 'unit' else f'`{name}`'}" for name, count in groups
    )
    block = "\n".join(
        [TEST_START, f"The current suite has {total} tests ({breakdown}).", TEST_END]
    )
    return block, total


def replace_block(text: str, start_marker: str, end_marker: str, block: str) -> str:
    start = text.find(start_marker)
    end = text.find(end_marker)
    if start < 0 or end < 0 or end <= start:
        raise SystemExit(f"generated markers missing: {start_marker}")
    return text[:start] + block + text[end + len(end_marker):]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if README is stale")
    parser.add_argument("--write", action="store_true", help="rewrite the generated block")
    parser.add_argument("--carvex", type=Path, default=Path("target/release/carvex"))
    parser.add_argument("--readme", type=Path, default=Path("README.md"))
    args = parser.parse_args()
    if args.check == args.write:
        parser.error("choose exactly one of --check or --write")

    flags = generated_block(args.carvex.resolve())
    tests, total = generated_test_block()
    readme = args.readme.resolve()
    text = readme.read_text()

    if args.write:
        text = replace_block(text, START, END, flags)
        text = replace_block(text, TEST_START, TEST_END, tests)
        readme.write_text(text)
        print(f"G9 WROTE generated flag table and test count in {readme}")
        return 0

    actual_flags = text[text.find(START):text.find(END) + len(END)]
    if actual_flags != flags:
        print("G9 FAIL: README flag table does not match carvex scan --help")
        print(f"expected block in {readme}:")
        print(flags)
        return 1
    actual_tests = text[text.find(TEST_START):text.find(TEST_END) + len(TEST_END)]
    if actual_tests != tests:
        print("G9 FAIL: README test count does not match cargo test --list")
        print(f"expected block in {readme}:")
        print(tests)
        return 1
    options = len(parse_scan_help(subprocess.run(
        [str(args.carvex.resolve()), "scan", "--help"],
        check=True, capture_output=True, text=True,
    ).stdout))
    print(f"G9 GREEN: {options} scan options and {total} tests match README")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
