#!/usr/bin/env python3
"""Check that status labels in PROGRESS and REVIEW_RESPONSE are reconcilable.

Rules (harness H-31):

* A guide item may be `DONE`/`FIXED` only if every linked `H-xx` finding is
  `CLOSED`, has a commit hash and has at least one existing test/control file.
* `PROGRESS.md` and `docs/REVIEW_RESPONSE.md` must assign the same canonical
  status to every guide item. `DONE` (progress) and `FIXED` (response) are the
  same canonical status; `MITIGATED` is accepted as the older spelling of
  `PARTIAL`.
* Both documents must contain exactly one row for every guide item A-01…D-03
  and for every harness finding H-01…H-34, and each H-xx status must agree.

Usage:
    python3 scripts/check_status.py
    python3 scripts/check_status.py --progress PATH --review PATH
"""
from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

GUIDE_IDS = (
    [f"A-{i:02d}" for i in range(1, 16)]
    + [f"B-{i:02d}" for i in range(1, 12)]
    + [f"C-{i:02d}" for i in range(1, 10)]
    + [f"D-{i:02d}" for i in range(1, 4)]
)
HARNESS_IDS = [f"H-{i:02d}" for i in range(1, 35)]

# Links are declared explicitly rather than guessed from prose. Only IDs that
# claim DONE/FIXED are constrained by these links; PARTIAL and OPEN items list
# their residual work in the tables themselves.
H_LINKS: dict[str, list[str]] = {
    "A-01": ["H-07", "H-19", "H-20"],
    "A-02": ["H-19"],
    "A-05": ["H-21"],
    "A-07": ["H-26", "H-27"],
    "A-03": ["H-10", "H-14"],
    "A-04": ["H-16", "H-17", "H-18", "H-19"],
    "A-06": ["H-05", "H-06", "H-08", "H-09", "H-15"],
    "A-08": ["H-22", "H-25"],
    "A-09": ["H-26"],
    "A-10": ["H-34"],
    "A-11": ["H-30"],
    "A-12": ["H-04", "H-30"],
    "A-13": ["H-28"],
    "A-14": ["H-16", "H-25", "H-28"],
    "A-15": ["H-08", "H-09"],
    "B-04": ["H-11", "H-12", "H-13", "H-14", "H-22", "H-23", "H-24"],
    "B-06": ["H-29"],
    "B-07": ["H-21"],
    "B-08": ["H-30"],
    "D-01": ["H-33"],
    "D-03": ["H-04", "H-31", "H-32", "H-33", "H-34"],
}

COMMIT_RE = re.compile(r"^[0-9a-f]{7,40}$")


def section_lines(lines: list[str], predicate) -> list[str]:
    start = next((i for i, line in enumerate(lines) if predicate(line)), None)
    if start is None:
        raise ValueError("section heading not found")
    level = len(lines[start]) - len(lines[start].lstrip("#"))
    end = len(lines)
    for i in range(start + 1, len(lines)):
        line = lines[i]
        if line.startswith("#"):
            other = len(line) - len(line.lstrip("#"))
            if other <= level:
                end = i
                break
    return lines[start:end]


def cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def parse_id_table(lines: list[str], id_pattern: str) -> dict[str, list[str]]:
    rows: dict[str, list[str]] = {}
    for line in lines:
        if not line.lstrip().startswith("|"):
            continue
        parts = cells(line)
        if not parts or not re.fullmatch(id_pattern, parts[0]):
            continue
        if parts[0] in rows:
            raise ValueError(f"duplicate row for {parts[0]}")
        rows[parts[0]] = parts
    return rows


def canonical_status(raw: str, origin: str) -> str:
    word = re.sub(r"\*", "", raw).strip().split()[0].upper().rstrip(",;")
    if origin == "progress" and word == "DONE":
        return "FIXED"
    if word == "MITIGATED":
        return "PARTIAL"
    if word in {"FIXED", "PARTIAL", "OPEN"}:
        return word
    return word


def first_path(fragment: str, root: Path) -> Path | None:
    paths = re.findall(
        r"(?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+\."
        r"(?:rs|py|sh|yml|json|md|toml)",
        fragment,
    )
    for token in paths:
        candidate = root / token
        if candidate.exists():
            return candidate
    return None


def status_word(raw: str) -> str:
    return re.sub(r"\*", "", raw).strip().split()[0].upper().rstrip(",;")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--progress", type=Path, default=ROOT / "docs/PROGRESS.md")
    parser.add_argument("--review", type=Path, default=ROOT / "docs/REVIEW_RESPONSE.md")
    args = parser.parse_args()

    errors: list[str] = []
    progress_lines = args.progress.read_text().splitlines()
    review_lines = args.review.read_text().splitlines()

    try:
        p_rows = parse_id_table(
            section_lines(progress_lines, lambda l: l.strip() == "## Tasks"),
            r"[ABCD]-\d{2}",
        )
        h_rows = parse_id_table(
            section_lines(
                progress_lines, lambda l: l.strip() == "### Harness findings (H-xx)"
            ),
            r"H-\d{2}",
        )
        r_rows = parse_id_table(
            section_lines(
                review_lines, lambda l: l.startswith("## Summary table")
            ),
            r"[ABCD]-\d{2}",
        )
        r_h_rows = parse_id_table(
            section_lines(
                review_lines, lambda l: l.strip() == "## Harness findings (H-xx)"
            ),
            r"H-\d{2}",
        )
    except ValueError as exc:
        print(f"STATUS FAIL: {exc}")
        return 1

    for doc_name, rows, expected in (
        ("PROGRESS guide", p_rows, GUIDE_IDS),
        ("REVIEW_RESPONSE guide", r_rows, GUIDE_IDS),
        ("PROGRESS harness", h_rows, HARNESS_IDS),
        ("REVIEW_RESPONSE harness", r_h_rows, HARNESS_IDS),
    ):
        missing = [item for item in expected if item not in rows]
        extra = [item for item in rows if item not in expected]
        if missing:
            errors.append(f"{doc_name}: missing rows: {', '.join(missing)}")
        if extra:
            errors.append(f"{doc_name}: unexpected rows: {', '.join(extra)}")

    for item in GUIDE_IDS:
        if item in p_rows and item in r_rows:
            p_status = canonical_status(p_rows[item][1], "progress")
            r_status = canonical_status(r_rows[item][1], "review")
            if p_status != r_status:
                errors.append(
                    f"{item}: PROGRESS says {p_rows[item][1]}, "
                    f"REVIEW_RESPONSE says {r_rows[item][1]}"
                )

    for item in HARNESS_IDS:
        if item in h_rows and item in r_h_rows:
            p_status = status_word(h_rows[item][3])
            r_status = status_word(r_h_rows[item][3])
            if p_status != r_status:
                errors.append(
                    f"{item}: PROGRESS harness says {h_rows[item][3]}, "
                    f"REVIEW_RESPONSE says {r_h_rows[item][3]}"
                )

    for item, links in H_LINKS.items():
        raw = p_rows.get(item, ["", "OPEN"])[1]
        if canonical_status(raw, "progress") != "FIXED":
            continue
        for hid in links:
            row = h_rows.get(hid)
            if row is None:
                errors.append(f"{item}: linked {hid} has no harness row")
                continue
            status = row[3].split()[0].upper()
            commit = row[4] if len(row) > 4 else ""
            tests = row[5] if len(row) > 5 else ""
            if status != "CLOSED":
                errors.append(f"{item}: linked {hid} is {status}, not CLOSED")
            if not COMMIT_RE.fullmatch(commit):
                errors.append(f"{item}: linked {hid} has no commit hash ({commit!r})")
            if first_path(tests, ROOT) is None:
                errors.append(f"{item}: linked {hid} has no existing test/control")

    # R2-06: flag obvious CI-run status contradictions in current-state docs.
    ci_docs = [ROOT / "docs/PROGRESS.md", ROOT / "docs/VALIDATION.md", ROOT / "CHANGELOG.md", ROOT / "dist/README.md"]
    run_status: dict[str, set[str]] = {}
    for doc in ci_docs:
        if not doc.exists():
            continue
        for line in doc.read_text().splitlines():
            # ignore historical/archive sections by simple markers
            if "historical" in line.lower() and "run" not in line.lower():
                continue
            for m in re.finditer(r"\b(\d{8,12})\b", line):
                rid = m.group(1)
                l = line.lower()
                # detect status claims tied to this run id
                if "failure" in l or " failed" in l:
                    # avoid "earlier runs failed; ... green run <id>" by requiring proximity not perfect; conservative
                    if rid in l[l.find(rid)-40:l.find(rid)+40] if rid in l else l:
                        pass
                # simpler: classify line per id
            # per-line classification
        for line in doc.read_text().splitlines():
            l = line.lower()
            ids = re.findall(r"\b(\d{8,12})\b", line)
            if not ids:
                continue
            # skip lines that are clearly historical narrative about old failed runs when also mentioning green fix
            has_fail = (" completed failure" in l) or (" failed (" in l) or ("status fail" in l)
            has_green = ("completed success" in l) or (" success (" in l) or ("green run" in l) or ("27/27" in l and "success" in l)
            # a line saying "earlier runs failed; ... run X completed success" should mark X as green only
            for rid in ids:
                # local window around id
                idx = l.find(rid)
                win = l[max(0, idx-60): idx+60]
                local_fail = ("completed failure" in win) or (" failed" in win and "fixed" not in win)
                local_green = ("completed success" in win) or ("success" in win) or ("green" in win)
                if local_fail and not local_green:
                    run_status.setdefault(rid, set()).add("failed")
                if local_green and not local_fail:
                    run_status.setdefault(rid, set()).add("green")
    for rid, statuses in sorted(run_status.items()):
        if "failed" in statuses and "green" in statuses:
            errors.append(f"CI run {rid} is described as both failed and green in current-state docs")

    # R2-06: test-count claims in PROGRESS should be consistent for current state.
    progress_text = (ROOT / "docs/PROGRESS.md").read_text()
    test_counts = set()
    for m in re.finditer(r"\b(\d+)\s+tests?\b", progress_text, re.I):
        n = int(m.group(1))
        if n < 30:
            continue
        win = progress_text[max(0,m.start()-60):m.end()+60].lower()
        if "historical" in win:
            continue
        # only current-gate claims
        if "current gate" in win or "in the current gate" in win or "a-11" in win:
            test_counts.add(n)
    if len(test_counts) > 1:
        errors.append(f"PROGRESS.md has conflicting current-gate test counts: {sorted(test_counts)}")

    if errors:
        print("STATUS FAIL")
        for error in errors:
            print(f"- {error}")
        return 1

    print(
        f"STATUS GREEN: {len(GUIDE_IDS)} guide IDs and {len(HARNESS_IDS)} "
        "harness IDs agree between documents; DONE/FIXED links verified"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
