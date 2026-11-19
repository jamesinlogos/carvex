#!/usr/bin/env python3
"""Run the CarveX recovery benchmark against Foremost, Scalpel and PhotoRec.

The benchmark uses the images and ground-truth manifests produced by
``bench/make_images.py``.  For every image/tool combination it runs the tool
(optionally several times), collects the recovered files, and computes:

* exact precision/recall/F1: recovered bytes must have the same SHA-256 as a
  ground-truth file;
* usable precision/recall/F1: trailing bytes after the format footer are
  ignored (JPEG EOI, PNG IEND, PDF %%EOF), which makes the comparison fair
  for tools that append a NUL byte or omit a final newline;
* false positives caused by recovering files that were *not* deleted
  (relevant for the "mixed" images), plus unknown false positives;
* wall-clock runtime, throughput and per-type statistics.

Outputs are written as JSON, CSV and a Markdown report.  Carved files are
deleted after each run unless ``--keep-outputs`` is used; only logs, reports
and metadata are retained.
"""

from __future__ import annotations

import argparse
import csv
import datetime as _dt
import hashlib
import json
import os
import platform
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Optional

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
DEFAULT_IMAGES_DIR = BENCH / "images"
DEFAULT_CORPUS_DIR = BENCH / "corpus"
DEFAULT_RESULTS_DIR = BENCH / "results"
DEFAULT_WORK_DIR = BENCH / "work"
DEFAULT_SCALPEL_CONFIG = BENCH / "scalpel.conf"
DEFAULT_FOREMOST_CONFIG = BENCH / "foremost.conf"
DEFAULT_CARVEX = ROOT / "target" / "release" / "carvex"

TARGET_TYPES = ("jpeg", "png", "pdf")
EXT_TO_TYPE = {
    ".jpg": "jpeg",
    ".jpeg": "jpeg",
    ".jpe": "jpeg",
    ".png": "png",
    ".pdf": "pdf",
}
JPEG_MAGIC = b"\xff\xd8\xff"
JPEG_EOI = b"\xff\xd9"
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
PNG_IEND = b"\x00\x00\x00\x00IEND\xaeB`\x82"
PDF_MAGIC = b"%PDF-"
PDF_EOF = b"%%EOF"

TOOL_ORDER = ["carvex-unallocated", "carvex-raw", "foremost", "scalpel", "photorec"]


# ---------------------------------------------------------------------------
# Hashing and format-aware normalization


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path, chunk_size: int = 1 << 20) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(chunk_size), b""):
            h.update(chunk)
    return h.hexdigest()


def normalized_bytes(data: bytes, ftype: str) -> Optional[bytes]:
    """Return bytes ending at the real footer, or None if no footer is found.

    JPEG EXIF files can contain an embedded thumbnail whose EOI appears before
    the end of the main image.  Using the *last* EOI avoids truncating at the
    thumbnail.  PDF incremental updates contain several %%EOF markers; the
    last one is the end of the final revision.  PNG has one IEND chunk with a
    fixed 12-byte marker (length, type and CRC).
    """
    if ftype == "jpeg":
        idx = data.rfind(JPEG_EOI)
        return data[: idx + len(JPEG_EOI)] if idx >= 0 else None
    if ftype == "png":
        idx = data.rfind(PNG_IEND)
        return data[: idx + len(PNG_IEND)] if idx >= 0 else None
    if ftype == "pdf":
        idx = data.rfind(PDF_EOF)
        if idx < 0:
            return None
        # Bytes through %%EOF; a final newline/NUL is intentionally ignored.
        return data[: idx + len(PDF_EOF)]
    return None


def detect_type(data: bytes, suffix: str) -> tuple[Optional[str], bool]:
    """Detect a target type from magic bytes; fall back to the extension.

    Returns ``(type, magic_ok)``.  A file with a target extension but invalid
    magic is still counted as an output (and therefore as a false positive),
    but it is never matched against ground truth.
    """
    if data.startswith(JPEG_MAGIC):
        return "jpeg", True
    if data.startswith(PNG_MAGIC):
        return "png", True
    if data.startswith(PDF_MAGIC):
        return "pdf", True
    ext_type = EXT_TO_TYPE.get(suffix.lower())
    return (ext_type, False) if ext_type else (None, False)


# ---------------------------------------------------------------------------
# Data model


@dataclass
class GroundTruth:
    gid: str
    path: str
    name: str
    type: str
    size: int
    sha256: str
    norm_sha256: Optional[str]


@dataclass
class OutputFile:
    index: int
    path: str
    rel_path: str
    type: str
    size: int
    sha256: str
    norm_sha256: Optional[str]
    magic_ok: bool


@dataclass
class ImageCase:
    stem: str
    manifest_path: Path
    image_path: Path
    scenario: str
    fs: str
    image_size: int
    image_sha256: str
    deleted: list[GroundTruth]
    kept: list[GroundTruth]
    manifest: dict[str, Any] = field(repr=False)


@dataclass(frozen=True)
class RunSpec:
    tool_id: str
    label: str
    mode: str
    space: str


# ---------------------------------------------------------------------------
# Ground-truth and image loading


def load_ground_truth(entry: dict[str, Any], corpus_dir: Path) -> GroundTruth:
    rel = entry["path"]
    path = corpus_dir / rel
    if not path.is_file():
        raise SystemExit(f"missing corpus file: {path}")
    data = path.read_bytes()
    raw = sha256_bytes(data)
    expected = entry.get("sha256")
    if expected and raw != expected:
        raise SystemExit(
            f"corpus hash mismatch for {path}: expected {expected}, got {raw}"
        )
    expected_size = entry.get("size")
    if expected_size is not None and len(data) != expected_size:
        raise SystemExit(
            f"corpus size mismatch for {path}: expected {expected_size}, got {len(data)}"
        )
    ftype = entry["type"]
    norm = normalized_bytes(data, ftype)
    return GroundTruth(
        gid=rel,
        path=str(path),
        name=entry.get("name", path.name),
        type=ftype,
        size=len(data),
        sha256=raw,
        norm_sha256=sha256_bytes(norm) if norm is not None else None,
    )


def load_image_cases(args: argparse.Namespace) -> list[ImageCase]:
    manifests = sorted(args.images_dir.glob("*.json"))
    if not manifests:
        raise SystemExit(f"no image manifests found in {args.images_dir}")

    wanted = {Path(x).stem for x in args.image} if args.image else None
    cases: list[ImageCase] = []
    for manifest_path in manifests:
        if manifest_path.name.startswith("."):
            continue
        try:
            meta = json.loads(manifest_path.read_text())
        except (OSError, json.JSONDecodeError) as exc:
            raise SystemExit(f"cannot read {manifest_path}: {exc}") from exc
        image_name = meta.get("image") or manifest_path.with_suffix(".img").name
        image_path = Path(meta.get("image_path") or manifest_path.parent / image_name)
        if not image_path.is_absolute():
            image_path = (manifest_path.parent / image_path).resolve()
        if not image_path.is_file():
            print(f"warning: skipping {manifest_path.name}: missing {image_path}", file=sys.stderr)
            continue
        if wanted and manifest_path.stem not in wanted and image_path.name not in args.image:
            continue
        scenario = meta.get("scenario", "unknown")
        fs = meta.get("filesystem", "unknown")
        if args.scenario and scenario not in args.scenario:
            continue
        if args.fs and fs not in args.fs:
            continue
        deleted = [load_ground_truth(x, args.corpus_dir) for x in meta.get("deleted", [])]
        kept = [load_ground_truth(x, args.corpus_dir) for x in meta.get("kept", [])]
        if not deleted:
            print(f"warning: skipping {manifest_path.name}: no deleted files", file=sys.stderr)
            continue
        cases.append(
            ImageCase(
                stem=manifest_path.stem,
                manifest_path=manifest_path,
                image_path=image_path,
                scenario=scenario,
                fs=fs,
                image_size=image_path.stat().st_size,
                image_sha256=meta.get("image_sha256", ""),
                deleted=deleted,
                kept=kept,
                manifest=meta,
            )
        )
    if not cases:
        raise SystemExit("no benchmark images selected")
    return cases


def verify_image(case: ImageCase, enabled: bool) -> None:
    if case.image_size != case.manifest.get("image_size"):
        raise SystemExit(
            f"image size mismatch for {case.image_path}: "
            f"manifest={case.manifest.get('image_size')}, actual={case.image_size}"
        )
    if not enabled:
        return
    actual = sha256_file(case.image_path)
    expected = case.image_sha256
    if expected and actual != expected:
        raise SystemExit(
            f"image hash mismatch for {case.image_path}: expected {expected}, got {actual}"
        )


# ---------------------------------------------------------------------------
# Collecting and matching recovered files


def collect_outputs(directories: Iterable[Path]) -> list[OutputFile]:
    candidates: dict[str, Path] = {}
    for directory in directories:
        if not directory.exists():
            continue
        for path in sorted(directory.rglob("*")):
            if not path.is_file() or path.is_symlink():
                continue
            try:
                resolved = str(path.resolve())
            except OSError:
                resolved = str(path)
            candidates.setdefault(resolved, path)

    outputs: list[OutputFile] = []
    for resolved, path in sorted(candidates.items()):
        suffix = path.suffix.lower()
        try:
            data = path.read_bytes()
        except OSError as exc:
            print(f"warning: cannot read output {path}: {exc}", file=sys.stderr)
            continue
        ftype, magic_ok = detect_type(data, suffix)
        if ftype is None:
            continue
        norm = normalized_bytes(data, ftype) if magic_ok else None
        outputs.append(
            OutputFile(
                index=len(outputs),
                path=resolved,
                rel_path=str(path),
                type=ftype,
                size=len(data),
                sha256=sha256_bytes(data),
                norm_sha256=sha256_bytes(norm) if norm is not None else None,
                magic_ok=magic_ok,
            )
        )
    return outputs


def assign_matches(
    outputs: list[OutputFile],
    ground_truth: list[GroundTruth],
    allow_normalized: bool,
) -> tuple[dict[str, dict[str, Any]], set[int]]:
    """Greedy one-to-one assignment, exact matches first.

    A ground-truth file can be counted at most once.  Extra copies of the same
    bytes stay unassigned and are counted as false positives, which mirrors a
    forensic examiner's view of redundant output.
    """
    assigned: dict[str, dict[str, Any]] = {}
    used: set[int] = set()

    def one_pass(kind: str) -> None:
        for output in outputs:
            if output.index in used or not output.magic_ok:
                continue
            for gt in ground_truth:
                if gt.gid in assigned or gt.type != output.type:
                    continue
                if kind == "exact":
                    if gt.sha256 != output.sha256:
                        continue
                else:
                    if not gt.norm_sha256 or not output.norm_sha256:
                        continue
                    if gt.norm_sha256 != output.norm_sha256:
                        continue
                assigned[gt.gid] = {
                    "output_index": output.index,
                    "match": kind,
                }
                used.add(output.index)
                break

    one_pass("exact")
    if allow_normalized:
        one_pass("normalized")
    return assigned, used


def match_any(
    outputs: list[OutputFile],
    ground_truth: list[GroundTruth],
    allow_normalized: bool,
) -> set[str]:
    """All ground-truth ids matched by at least one output (not one-to-one)."""
    matched: set[str] = set()
    for output in outputs:
        if not output.magic_ok:
            continue
        for gt in ground_truth:
            if gt.gid in matched or gt.type != output.type:
                continue
            if gt.sha256 == output.sha256:
                matched.add(gt.gid)
            elif (
                allow_normalized
                and output.norm_sha256
                and gt.norm_sha256
                and gt.norm_sha256 == output.norm_sha256
            ):
                matched.add(gt.gid)
    return matched


def _prf(tp: int, fp: int, fn: int) -> dict[str, float]:
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return {
        "precision": precision,
        "recall": recall,
        "f1": f1,
    }


def build_metrics(
    outputs: list[OutputFile],
    deleted: list[GroundTruth],
    kept: list[GroundTruth],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    exact_assigned, exact_used = assign_matches(outputs, deleted, allow_normalized=False)
    usable_assigned, usable_used = assign_matches(outputs, deleted, allow_normalized=True)

    deleted_count = len(deleted)
    tp_exact = len(exact_assigned)
    fp_exact = len(outputs) - tp_exact
    fn_exact = deleted_count - tp_exact
    tp_usable = len(usable_assigned)
    fp_usable = len(outputs) - tp_usable
    fn_usable = deleted_count - tp_usable

    exact_prf = _prf(tp_exact, fp_exact, fn_exact)
    usable_prf = _prf(tp_usable, fp_usable, fn_usable)

    # Classify false positives in the usable view.
    kept_matched = match_any(outputs, kept, allow_normalized=True)
    duplicate_outputs = 0
    kept_outputs = 0
    unknown_fp = 0
    invalid_outputs = 0
    output_detail: list[dict[str, Any]] = []
    usable_by_output = {
        item["output_index"]: (gid, item["match"])
        for gid, item in usable_assigned.items()
    }
    exact_by_output = {
        item["output_index"] for item in exact_assigned.values()
    }

    for output in outputs:
        detail: dict[str, Any] = {
            "path": output.rel_path,
            "type": output.type,
            "size": output.size,
            "sha256": output.sha256,
            "norm_sha256": output.norm_sha256,
            "magic_ok": output.magic_ok,
            "exact_match": output.index in exact_by_output,
            "usable_match": output.index in usable_by_output,
        }
        if not output.magic_ok:
            invalid_outputs += 1
        if output.index in usable_by_output:
            gid, kind = usable_by_output[output.index]
            detail["matched_gt"] = gid
            detail["match_kind"] = kind
            detail["fp_reason"] = None
        else:
            # Is this a duplicate of a deleted ground-truth file?
            duplicate_of: Optional[str] = None
            if output.magic_ok:
                for gt in deleted:
                    if gt.type != output.type:
                        continue
                    if gt.sha256 == output.sha256 or (
                        output.norm_sha256
                        and gt.norm_sha256
                        and gt.norm_sha256 == output.norm_sha256
                    ):
                        duplicate_of = gt.gid
                        break
            if duplicate_of:
                duplicate_outputs += 1
                detail["matched_gt"] = None
                detail["match_kind"] = None
                detail["fp_reason"] = "duplicate"
                detail["duplicate_of_gt"] = duplicate_of
            else:
                kept_hit: Optional[str] = None
                if output.magic_ok:
                    for gt in kept:
                        if gt.type != output.type:
                            continue
                        if gt.sha256 == output.sha256 or (
                            output.norm_sha256
                            and gt.norm_sha256
                            and gt.norm_sha256 == output.norm_sha256
                        ):
                            kept_hit = gt.gid
                            break
                if kept_hit:
                    kept_outputs += 1
                    detail["matched_gt"] = None
                    detail["match_kind"] = None
                    detail["fp_reason"] = "kept_file"
                    detail["kept_gt"] = kept_hit
                else:
                    unknown_fp += 1
                    detail["matched_gt"] = None
                    detail["match_kind"] = None
                    detail["fp_reason"] = "invalid" if not output.magic_ok else "unknown"
        output_detail.append(detail)

    tp_bytes_exact = sum(
        gt.size for gt in deleted if gt.gid in exact_assigned
    )
    tp_bytes_usable = sum(
        gt.size for gt in deleted if gt.gid in usable_assigned
    )

    by_type: dict[str, dict[str, int]] = {}
    for ftype in TARGET_TYPES:
        deleted_type = [g for g in deleted if g.type == ftype]
        kept_type = [g for g in kept if g.type == ftype]
        outputs_type = [o for o in outputs if o.type == ftype]
        exact_type = [g for g in deleted_type if g.gid in exact_assigned]
        usable_type = [g for g in deleted_type if g.gid in usable_assigned]
        by_type[ftype] = {
            "deleted": len(deleted_type),
            "kept": len(kept_type),
            "outputs": len(outputs_type),
            "tp_exact": len(exact_type),
            "fp_exact": len(outputs_type) - len(exact_type),
            "fn_exact": len(deleted_type) - len(exact_type),
            "tp_usable": len(usable_type),
            "fp_usable": len(outputs_type) - len(usable_type),
            "fn_usable": len(deleted_type) - len(usable_type),
            "kept_recovered": len([g for g in kept_type if g.gid in kept_matched]),
        }

    metrics: dict[str, Any] = {
        "deleted": deleted_count,
        "kept": len(kept),
        "outputs": len(outputs),
        "invalid_outputs": invalid_outputs,
        "tp_exact": tp_exact,
        "fp_exact": fp_exact,
        "fn_exact": fn_exact,
        "precision_exact": exact_prf["precision"],
        "recall_exact": exact_prf["recall"],
        "f1_exact": exact_prf["f1"],
        "tp_usable": tp_usable,
        "fp_usable": fp_usable,
        "fn_usable": fn_usable,
        "precision_usable": usable_prf["precision"],
        "recall_usable": usable_prf["recall"],
        "f1_usable": usable_prf["f1"],
        "duplicate_outputs": duplicate_outputs,
        "kept_outputs": kept_outputs,
        "unknown_fp": unknown_fp,
        "kept_files_recovered": len(kept_matched),
        "tp_bytes_exact": tp_bytes_exact,
        "tp_bytes_usable": tp_bytes_usable,
        "output_bytes": sum(o.size for o in outputs),
        "by_type": by_type,
    }
    return metrics, output_detail


# ---------------------------------------------------------------------------
# Running the tools


def check_executable(path: Path, what: str) -> Path:
    if not path.is_file():
        raise SystemExit(f"{what} not found: {path}")
    if not os.access(path, os.X_OK):
        raise SystemExit(f"{what} is not executable: {path}")
    return path


def tool_version(cmd: list[str]) -> str:
    try:
        proc = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=20,
            env=bench_env(),
        )
        lines = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
        return lines[0] if lines else f"exit {proc.returncode}"
    except (OSError, subprocess.TimeoutExpired) as exc:
        return f"unavailable: {exc}"


def bench_env() -> dict[str, str]:
    env = os.environ.copy()
    env["LC_ALL"] = "C"
    env["LANG"] = "C"
    env["TZ"] = "UTC"
    return env


def execute(
    cmd: list[str],
    cwd: Path,
    log_path: Path,
    timeout: int,
) -> dict[str, Any]:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8", errors="replace") as log:
        log.write(f"$ {shlex.join(cmd)}\n")
        log.write(f"cwd: {cwd}\n\n")
        log.flush()
        start = time.perf_counter()
        timed_out = False
        error: Optional[str] = None
        returncode: Optional[int] = None
        try:
            proc = subprocess.run(
                cmd,
                cwd=str(cwd),
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                env=bench_env(),
            )
            returncode = proc.returncode
        except subprocess.TimeoutExpired:
            timed_out = True
            error = f"timeout after {timeout}s"
            log.write(f"\n*** {error} ***\n")
        except OSError as exc:
            error = str(exc)
            log.write(f"\n*** cannot execute: {exc} ***\n")
        elapsed = time.perf_counter() - start
        log.write(f"\nreturncode={returncode} timed_out={timed_out} elapsed={elapsed:.6f}s\n")
    return {
        "returncode": returncode,
        "timed_out": timed_out,
        "error": error,
        "runtime_s": elapsed,
        "log": str(log_path),
    }


def remove_path(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.is_dir():
        shutil.rmtree(path)


def prepare_dir(path: Path) -> None:
    remove_path(path)
    path.mkdir(parents=True, exist_ok=True)


def build_command(
    spec: RunSpec,
    case: ImageCase,
    args: argparse.Namespace,
    run_dir: Path,
    report_path: Optional[Path],
) -> tuple[list[str], Path, list[Path]]:
    """Return (command, cwd, output directories to scan after the run)."""
    tool = spec.tool_id
    if tool.startswith("carvex"):
        out = run_dir / "out"
        prepare_dir(out)
        cmd = [
            str(args.carvex),
            "scan",
            str(case.image_path),
            "-o",
            str(out),
            "--mode",
            spec.mode,
            "--types",
            "jpeg,png,pdf",
            # Pin the published v0.1.0 space while the CLI default scans all
            # volumes and gaps (A-07 step 2, D-23).
            "--partition",
            "largest",
            "--no-gaps",
            "--min-score",
            str(args.min_score),
            "--quiet",
            "--json-report",
            str(report_path),
        ]
        if spec.mode == "raw":
            cmd.append("--no-fs")
        if args.repair:
            cmd.append("--repair")
        if args.threads is not None:
            cmd.extend(["--threads", str(args.threads)])
        if args.chunk_size:
            cmd.extend(["--chunk-size", args.chunk_size])
        return cmd, ROOT, [out]

    if tool == "foremost":
        out = run_dir / "out"
        prepare_dir(out)
        cmd = [
            "foremost",
            "-Q",
            "-c",
            str(args.foremost_config),
            "-t",
            "jpg,png,pdf",
            "-i",
            str(case.image_path),
            "-o",
            str(out),
        ]
        return cmd, ROOT, [out]

    if tool == "scalpel":
        out = run_dir / "out"
        remove_path(out)  # Scalpel creates the output directory itself.
        cmd = [
            "scalpel",
            "-c",
            str(args.scalpel_config),
            "-o",
            str(out),
            str(case.image_path),
        ]
        return cmd, ROOT, [out]

    if tool == "photorec":
        for old in run_dir.glob("OUT*"):
            remove_path(old)
        out = run_dir / "OUT"
        cmd = [
            "photorec",
            "/log",
            "/d",
            "OUT",
            "/cmd",
            str(case.image_path),
            "partition_i386,1,fileopt,everything,disable,jpg,enable,png,enable,pdf,enable,search",
        ]
        return cmd, run_dir, [out]

    raise ValueError(f"unknown tool: {tool}")


def parse_carvex_report(report_path: Path) -> dict[str, Any]:
    try:
        return json.loads(report_path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"cannot read CarveX JSON report {report_path}: {exc}") from exc


def run_one(
    spec: RunSpec,
    case: ImageCase,
    rep: int,
    args: argparse.Namespace,
    work_root: Path,
    results_dir: Path,
) -> dict[str, Any]:
    run_dir = work_root / case.stem / spec.tool_id / f"rep{rep}"
    prepare_dir(run_dir)
    log_path = results_dir / "logs" / case.stem / f"{spec.tool_id}-rep{rep}.log"
    report_path: Optional[Path] = None
    if spec.tool_id.startswith("carvex"):
        report_path = results_dir / "reports" / case.stem / f"{spec.tool_id}-rep{rep}.json"
        report_path.parent.mkdir(parents=True, exist_ok=True)

    cmd, cwd, output_dirs = build_command(spec, case, args, run_dir, report_path)
    execution = execute(cmd, cwd, log_path, args.timeout)
    # CarveX exits 3 when it finished but left regions unscanned (A-07
    # warnings); the carved output is still complete and scorable.
    expected_codes = {0, 3} if spec.tool_id.startswith("carvex") else {0}
    ok = (
        execution["returncode"] in expected_codes
        and not execution["timed_out"]
        and execution["error"] is None
    )
    error = execution["error"]
    tool_stats: dict[str, Any] = {}

    # PhotoRec writes to OUT.1 when OUT already exists.  Locate every OUT*
    # directory actually produced by this run.
    if spec.tool_id == "photorec":
        produced = sorted(
            p for p in run_dir.glob("OUT*") if p.is_dir()
        )
        output_dirs = produced
        if not produced and ok:
            ok = False
            error = "PhotoRec produced no OUT* directory"

    if spec.tool_id.startswith("carvex"):
        if report_path is not None and report_path.exists():
            try:
                report = parse_carvex_report(report_path)
                tool_stats = {
                    "carve_time_ms": report.get("duration_ms"),
                    "bytes_scanned": report.get("bytes_scanned"),
                    "throughput_mib_s": report.get("throughput_mib_s"),
                    "scan_stats": report.get("stats", {}),
                    "reported_mode": report.get("mode"),
                }
            except RuntimeError as exc:
                if ok:
                    ok = False
                    error = str(exc)
        elif ok:
            ok = False
            error = f"missing CarveX report: {report_path}"

    if ok and not any(d.exists() for d in output_dirs):
        ok = False
        error = f"tool produced no output directory ({', '.join(str(d) for d in output_dirs)})"

    outputs: list[OutputFile] = []
    metrics: dict[str, Any] = {}
    output_detail: list[dict[str, Any]] = []
    if ok:
        outputs = collect_outputs(output_dirs)
        metrics, output_detail = build_metrics(outputs, case.deleted, case.kept)

    bytes_scanned = tool_stats.get("bytes_scanned")
    if not bytes_scanned:
        bytes_scanned = case.image_size if not spec.tool_id.startswith("carvex") else 0

    result = {
        "image": case.stem,
        "scenario": case.scenario,
        "fs": case.fs,
        "tool": spec.label,
        "tool_id": spec.tool_id,
        "mode": spec.mode,
        "space": spec.space,
        "rep": rep,
        "command": cmd,
        "cwd": str(cwd),
        "output_dirs": [str(d) for d in output_dirs],
        "runtime_s": execution["runtime_s"],
        "returncode": execution["returncode"],
        "timed_out": execution["timed_out"],
        "ok": ok,
        "error": error,
        "image_size": case.image_size,
        "bytes_scanned": bytes_scanned,
        "tool_stats": tool_stats,
        "metrics": metrics,
        "outputs": output_detail,
        "log": str(log_path),
        "carvex_report": str(report_path) if report_path else None,
    }
    if not args.keep_outputs:
        remove_path(run_dir)
    return result


# ---------------------------------------------------------------------------
# Aggregation and reports


def median_int(values: Iterable[int]) -> int:
    values = list(values)
    return int(round(statistics.median(values))) if values else 0


def median_float(values: Iterable[float]) -> float:
    values = list(values)
    return statistics.median(values) if values else 0.0


def aggregate_runs(runs: list[dict[str, Any]], spec: RunSpec, case: ImageCase) -> dict[str, Any]:
    ok_runs = [r for r in runs if r["ok"]]
    all_ok = len(ok_runs) == len(runs) and bool(runs)
    errors = [r["error"] for r in runs if r["error"]]
    runtimes = [r["runtime_s"] for r in runs]
    image_mibs = case.image_size / (1024 * 1024)

    # All ground-truth counts must be stable across repetitions.  If a tool is
    # non-deterministic, report the median and flag the varying fields.
    metric_fields = [
        "outputs",
        "tp_exact",
        "fp_exact",
        "fn_exact",
        "tp_usable",
        "fp_usable",
        "fn_usable",
        "invalid_outputs",
        "duplicate_outputs",
        "kept_outputs",
        "unknown_fp",
        "kept_files_recovered",
    ]
    metrics: dict[str, Any] = {}
    variance: list[str] = []
    for field_name in metric_fields:
        values = [r["metrics"].get(field_name, 0) for r in ok_runs]
        metrics[field_name] = median_int(values)
        if len(set(values)) > 1:
            variance.append(field_name)
    precision = _prf(metrics["tp_exact"], metrics["fp_exact"], metrics["fn_exact"])
    usable = _prf(metrics["tp_usable"], metrics["fp_usable"], metrics["fn_usable"])
    metrics.update(
        {
            "precision_exact": precision["precision"],
            "recall_exact": precision["recall"],
            "f1_exact": precision["f1"],
            "precision_usable": usable["precision"],
            "recall_usable": usable["recall"],
            "f1_usable": usable["f1"],
            "tp_bytes_exact": median_int(r["metrics"].get("tp_bytes_exact", 0) for r in ok_runs),
            "tp_bytes_usable": median_int(r["metrics"].get("tp_bytes_usable", 0) for r in ok_runs),
            "output_bytes": median_int(r["metrics"].get("output_bytes", 0) for r in ok_runs),
        }
    )

    runtime_median = median_float(runtimes)
    bytes_scanned = median_int(
        r.get("bytes_scanned", case.image_size) for r in ok_runs
    )
    if not bytes_scanned:
        bytes_scanned = case.image_size
    by_type = next(
        (
            r["metrics"].get("by_type", {})
            for r in ok_runs
            if r["metrics"].get("by_type")
        ),
        {},
    )

    row = {
        "image": case.stem,
        "image_path": str(case.image_path),
        "scenario": case.scenario,
        "fs": case.fs,
        "tool": spec.label,
        "tool_id": spec.tool_id,
        "mode": spec.mode,
        "space": spec.space,
        "reps": len(runs),
        "successful_reps": len(ok_runs),
        "ok": all_ok,
        "error": "; ".join(sorted(set(errors))),
        "deleted": len(case.deleted),
        "kept": len(case.kept),
        "image_size": case.image_size,
        "image_mib": image_mibs,
        "bytes_scanned": bytes_scanned,
        "scan_mib": bytes_scanned / (1024 * 1024),
        "runtime_s": runtime_median,
        "runtime_min_s": min(runtimes) if runtimes else 0.0,
        "runtime_max_s": max(runtimes) if runtimes else 0.0,
        "throughput_image_mib_s": image_mibs / runtime_median if runtime_median else 0.0,
        "throughput_scan_mib_s": (bytes_scanned / (1024 * 1024)) / runtime_median
        if runtime_median
        else 0.0,
        "metric_variance": variance,
        "by_type": by_type,
        "carvex_stats": next(
            (r.get("tool_stats") for r in ok_runs if r.get("tool_stats")), {}
        ),
        **metrics,
    }
    return row


def pool_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    pooled: list[dict[str, Any]] = []
    for tool_id in TOOL_ORDER:
        selected = [r for r in rows if r["tool_id"] == tool_id]
        if not selected:
            continue
        tp_exact = sum(r["tp_exact"] for r in selected)
        fp_exact = sum(r["fp_exact"] for r in selected)
        fn_exact = sum(r["fn_exact"] for r in selected)
        tp_usable = sum(r["tp_usable"] for r in selected)
        fp_usable = sum(r["fp_usable"] for r in selected)
        fn_usable = sum(r["fn_usable"] for r in selected)
        exact = _prf(tp_exact, fp_exact, fn_exact)
        usable = _prf(tp_usable, fp_usable, fn_usable)
        runtime = sum(r["runtime_s"] for r in selected)
        image_mib = sum(r["image_mib"] for r in selected)
        scan_mib = sum(r["scan_mib"] for r in selected)
        pooled.append(
            {
                "tool": selected[0]["tool"],
                "tool_id": tool_id,
                "mode": selected[0]["mode"],
                "space": selected[0]["space"],
                "images": len(selected),
                "ok": all(r["ok"] for r in selected),
                "deleted": sum(r["deleted"] for r in selected),
                "kept": sum(r["kept"] for r in selected),
                "outputs": sum(r["outputs"] for r in selected),
                "tp_exact": tp_exact,
                "fp_exact": fp_exact,
                "fn_exact": fn_exact,
                "precision_exact": exact["precision"],
                "recall_exact": exact["recall"],
                "f1_exact": exact["f1"],
                "tp_usable": tp_usable,
                "fp_usable": fp_usable,
                "fn_usable": fn_usable,
                "precision_usable": usable["precision"],
                "recall_usable": usable["recall"],
                "f1_usable": usable["f1"],
                "kept_outputs": sum(r["kept_outputs"] for r in selected),
                "unknown_fp": sum(r["unknown_fp"] for r in selected),
                "duplicate_outputs": sum(r["duplicate_outputs"] for r in selected),
                "runtime_s": runtime,
                "image_mib": image_mib,
                "throughput_image_mib_s": image_mib / runtime if runtime else 0.0,
                "throughput_scan_mib_s": scan_mib / runtime if runtime else 0.0,
            }
        )
    return pooled


CSV_FIELDS = [
    "image",
    "scenario",
    "fs",
    "tool",
    "tool_id",
    "mode",
    "space",
    "reps",
    "successful_reps",
    "ok",
    "deleted",
    "kept",
    "outputs",
    "invalid_outputs",
    "tp_exact",
    "fp_exact",
    "fn_exact",
    "precision_exact",
    "recall_exact",
    "f1_exact",
    "tp_usable",
    "fp_usable",
    "fn_usable",
    "precision_usable",
    "recall_usable",
    "f1_usable",
    "duplicate_outputs",
    "kept_outputs",
    "unknown_fp",
    "kept_files_recovered",
    "tp_bytes_exact",
    "tp_bytes_usable",
    "output_bytes",
    "image_size",
    "bytes_scanned",
    "runtime_s",
    "runtime_min_s",
    "runtime_max_s",
    "throughput_image_mib_s",
    "throughput_scan_mib_s",
    "metric_variance",
    "error",
]


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_FIELDS, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            out = dict(row)
            out["metric_variance"] = ",".join(row.get("metric_variance", []))
            for key in (
                "precision_exact",
                "recall_exact",
                "f1_exact",
                "precision_usable",
                "recall_usable",
                "f1_usable",
                "runtime_s",
                "runtime_min_s",
                "runtime_max_s",
                "throughput_image_mib_s",
                "throughput_scan_mib_s",
            ):
                out[key] = f"{row.get(key, 0.0):.6f}"
            writer.writerow(out)


def pct(value: float) -> str:
    return f"{value * 100:.2f}%"


def markdown_table(headers: list[str], rows: list[list[str]]) -> list[str]:
    lines = ["| " + " | ".join(headers) + " |"]
    lines.append("|" + "|".join(["---"] * len(headers)) + "|")
    for row in rows:
        lines.append("| " + " | ".join(row) + " |")
    return lines


def write_markdown(
    path: Path,
    rows: list[dict[str, Any]],
    pooled: list[dict[str, Any]],
    args: argparse.Namespace,
    cases: list[ImageCase],
    versions: dict[str, str],
) -> None:
    lines: list[str] = []
    lines.append("# CarveX benchmark results")
    lines.append("")
    lines.append(f"Generated: `{_dt.datetime.now().isoformat(timespec='seconds')}`")
    lines.append("")
    lines.append(
        f"Images: **{len(cases)}**, deleted ground-truth files: "
        f"**{sum(len(c.deleted) for c in cases)}**, kept files: "
        f"**{sum(len(c.kept) for c in cases)}**, repetitions: **{args.reps}**."
    )
    lines.append("")
    lines.append("Tool versions:")
    for tool, version in versions.items():
        lines.append(f"- {tool}: `{version}`")
    lines.append("")
    lines.append("## Method")
    lines.append("")
    lines.append(
        "The benchmark compares byte-exact recovery and *usable* recovery. "
        "An output is an exact true positive only if its SHA-256 equals the "
        "original deleted file. For usable matching, bytes after the real "
        "footer are ignored: the last JPEG `FF D9`, the PNG IEND chunk, or the "
        "last PDF `%%EOF`. This compensates for tools that append a NUL byte "
        "or omit a final newline. Every deleted ground-truth file can be "
        "matched once; duplicate or extra outputs are false positives. In the "
        "mixed images, recovering a file that was never deleted is also a "
        "false positive."
    )
    lines.append("")
    lines.append(
        "CarveX unallocated scans only free clusters from the FAT or NTFS "
        "`$Bitmap`; the raw rows and the other tools scan the whole image. "
        "All runtime figures are wall-clock medians of the repeated runs."
    )
    lines.append("")
    lines.append("## Usable results (normalized)")
    lines.append("")
    headers = [
        "Image",
        "FS",
        "Scenario",
        "Tool",
        "Space",
        "Out",
        "TP",
        "FP",
        "FN",
        "Prec.",
        "Rec.",
        "F1",
        "Time (s)",
        "MiB/s",
    ]
    table_rows = []
    for row in rows:
        table_rows.append(
            [
                row["image"],
                row["fs"],
                row["scenario"],
                row["tool"],
                row["space"],
                str(row["outputs"]),
                str(row["tp_usable"]),
                str(row["fp_usable"]),
                str(row["fn_usable"]),
                pct(row["precision_usable"]),
                pct(row["recall_usable"]),
                pct(row["f1_usable"]),
                f"{row['runtime_s']:.3f}",
                f"{row['throughput_image_mib_s']:.1f}",
            ]
        )
    lines.extend(markdown_table(headers, table_rows))
    lines.append("")
    lines.append("## Exact results (SHA-256)")
    lines.append("")
    table_rows = []
    for row in rows:
        table_rows.append(
            [
                row["image"],
                row["fs"],
                row["scenario"],
                row["tool"],
                row["space"],
                str(row["outputs"]),
                str(row["tp_exact"]),
                str(row["fp_exact"]),
                str(row["fn_exact"]),
                pct(row["precision_exact"]),
                pct(row["recall_exact"]),
                pct(row["f1_exact"]),
            ]
        )
    lines.extend(markdown_table(headers[:-2], table_rows))
    lines.append("")
    lines.append("## Pooled by tool")
    lines.append("")
    headers2 = [
        "Tool",
        "Space",
        "Deleted",
        "Out",
        "TP",
        "FP",
        "FN",
        "Prec.",
        "Rec.",
        "F1",
        "Total time (s)",
        "MiB/s",
    ]
    table_rows = []
    for row in pooled:
        table_rows.append(
            [
                row["tool"],
                row["space"],
                str(row["deleted"]),
                str(row["outputs"]),
                str(row["tp_usable"]),
                str(row["fp_usable"]),
                str(row["fn_usable"]),
                pct(row["precision_usable"]),
                pct(row["recall_usable"]),
                pct(row["f1_usable"]),
                f"{row['runtime_s']:.3f}",
                f"{row['throughput_image_mib_s']:.1f}",
            ]
        )
    lines.extend(markdown_table(headers2, table_rows))
    lines.append("")
    lines.append("### Pooled exact results")
    lines.append("")
    headers3 = ["Tool", "Space", "TP", "FP", "FN", "Prec.", "Rec.", "F1"]
    table_rows = []
    for row in pooled:
        table_rows.append(
            [
                row["tool"],
                row["space"],
                str(row["tp_exact"]),
                str(row["fp_exact"]),
                str(row["fn_exact"]),
                pct(row["precision_exact"]),
                pct(row["recall_exact"]),
                pct(row["f1_exact"]),
            ]
        )
    lines.extend(markdown_table(headers3, table_rows))
    lines.append("")
    lines.append("## Commands")
    lines.append("")
    lines.append("```text")
    lines.append(
        "carvex scan IMAGE -o OUT --mode unallocated --types jpeg,png,pdf "
        f"--partition largest --no-gaps --min-score {args.min_score} --quiet"
    )
    lines.append(
        "carvex scan IMAGE -o OUT --mode raw --no-fs --types jpeg,png,pdf "
        f"--partition largest --no-gaps --min-score {args.min_score} --quiet"
    )
    lines.append(
        "foremost -Q -c bench/foremost.conf -t jpg,png,pdf -i IMAGE -o OUT"
    )
    lines.append("scalpel -c bench/scalpel.conf -o OUT IMAGE")
    lines.append(
        "photorec /log /d OUT /cmd IMAGE "
        "partition_i386,1,fileopt,everything,disable,jpg,enable,png,enable,pdf,enable,search"
    )
    lines.append("```")
    lines.append("")
    lines.append("## Notes")
    lines.append("")
    lines.append(
        "- `Out` counts every recognized target file in the tool output, "
        "including duplicates and invalid files with a target extension."
    )
    lines.append(
        "- `kept_outputs` (in the CSV/JSON) separates false positives caused by "
        "recovering allocated files from other false positives."
    )
    lines.append(
        "- Foremost's built-in signatures are used (its packaged config file "
        "has no enabled types). Adding custom JPEG/PDF entries to the config "
        "registers a second signature and produces duplicate carvings."
    )
    lines.append(
        "- The 64 MiB images are small, warm-cache benchmark media. Runtime is "
        "reported for comparison, not as a claim about large disks."
    )
    lines.append("")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


# ---------------------------------------------------------------------------
# Main


def build_run_specs(args: argparse.Namespace) -> list[RunSpec]:
    specs: list[RunSpec] = []
    tools = set(args.tools)
    if "carvex" in tools:
        modes = [m.strip() for m in args.carvex_modes.split(",") if m.strip()]
        for mode in modes:
            if mode == "unallocated":
                specs.append(RunSpec("carvex-unallocated", "CarveX", "unallocated", "unallocated"))
            elif mode == "raw":
                specs.append(RunSpec("carvex-raw", "CarveX", "raw", "raw"))
            else:
                raise SystemExit(f"invalid CarveX mode: {mode}")
    if "foremost" in tools:
        specs.append(RunSpec("foremost", "Foremost", "raw", "raw"))
    if "scalpel" in tools:
        specs.append(RunSpec("scalpel", "Scalpel", "raw", "raw"))
    if "photorec" in tools:
        specs.append(RunSpec("photorec", "PhotoRec", "raw", "raw"))
    if not specs:
        raise SystemExit("no tools selected")
    return specs


def build_smoke_image(corpus_dir: Path, out_dir: Path) -> Path:
    """Build a small synthetic FAT32 image for the gate smoke test.

    The gate must not require root, loop devices or mkfs tools, so the volume
    is written directly: boot sector, two FATs, and a handful of corpus files
    in the data area.  Deleted files stay in free clusters; one kept file is
    marked allocated to exercise the benchmark's kept-file FP accounting.
    """
    manifest = json.loads((corpus_dir / "manifest.json").read_text())
    by_type: dict[str, list[dict[str, Any]]] = {"jpeg": [], "png": [], "pdf": []}
    for entry in manifest.get("files", []):
        if entry.get("type") in by_type:
            by_type[entry["type"]].append(entry)
    for entries in by_type.values():
        entries.sort(key=lambda e: (e.get("size", 0), e.get("path", "")))
    if not all(by_type[t] for t in by_type):
        raise SystemExit("smoke image needs at least one JPEG, PNG and PDF in the corpus")
    deleted_entries = [by_type[t][0] for t in ("jpeg", "png", "pdf")]
    kept_entry = next(
        (e for t in ("png", "jpeg") for e in by_type[t][1:]),
        None,
    )

    bps = 512
    spc = 8
    cluster_size = bps * spc
    reserved = 32
    num_fats = 2
    fat_sectors = 64
    total_sectors = 16384  # 8 MiB
    data_start_sector = reserved + num_fats * fat_sectors
    total_clusters = (total_sectors - data_start_sector) // spc

    image = bytearray(total_sectors * bps)
    boot = bytearray(512)
    boot[0:3] = b"\xEB\x3C\x90"
    boot[3:11] = b"MSWIN4.1"
    boot[0x0B:0x0D] = bps.to_bytes(2, "little")
    boot[0x0D] = spc
    boot[0x0E:0x10] = reserved.to_bytes(2, "little")
    boot[0x10] = num_fats
    boot[0x13:0x15] = (0).to_bytes(2, "little")
    boot[0x15] = 0xF8
    boot[0x16:0x18] = (0).to_bytes(2, "little")
    boot[0x18:0x1A] = (63).to_bytes(2, "little")
    boot[0x1A:0x1C] = (255).to_bytes(2, "little")
    boot[0x20:0x24] = total_sectors.to_bytes(4, "little")
    boot[0x24:0x28] = fat_sectors.to_bytes(4, "little")
    boot[0x28:0x2C] = (0).to_bytes(4, "little")
    boot[0x2C:0x30] = (2).to_bytes(4, "little")
    boot[0x30:0x32] = (1).to_bytes(2, "little")
    boot[0x32:0x34] = (6).to_bytes(2, "little")
    boot[0x40] = 0x80
    boot[0x42] = 0x29
    boot[0x43:0x47] = bytes.fromhex("c0ffee00")
    boot[0x47:0x52] = b"CARVEXSMOKE"
    boot[0x52:0x57] = b"FAT32"
    boot[510:512] = b"\x55\xAA"
    image[0:512] = boot

    fat = bytearray(fat_sectors * bps)
    fat[0:4] = (0x0FFFFFF8).to_bytes(4, "little")
    fat[4:8] = (0x0FFFFFFF).to_bytes(4, "little")
    fat[8:12] = (0x0FFFFFFF).to_bytes(4, "little")  # root directory cluster 2

    def cluster_offset(cluster: int) -> int:
        return (data_start_sector + (cluster - 2) * spc) * bps

    cursor = 3
    allocated: list[int] = []
    written = {"deleted": [], "kept": []}
    for kind, entry, keep in [("deleted", e, False) for e in deleted_entries] + [
        ("kept", kept_entry, True)
    ] if kept_entry is not None else [("deleted", e, False) for e in deleted_entries]:
        src = corpus_dir / entry["path"]
        data = src.read_bytes()
        clusters = max(1, (len(data) + cluster_size - 1) // cluster_size)
        if cursor + clusters >= total_clusters + 2:
            raise SystemExit("smoke image is too small for the selected corpus files")
        off = cluster_offset(cursor)
        image[off : off + len(data)] = data
        if keep:
            for i in range(clusters):
                c = cursor + i
                nxt = 0x0FFFFFFF if i == clusters - 1 else c + 1
                fat[c * 4 : c * 4 + 4] = nxt.to_bytes(4, "little")
            allocated.extend(range(cursor, cursor + clusters))
        written[kind].append(
            {
                "path": entry["path"],
                "name": entry.get("name", Path(entry["path"]).name),
                "type": entry["type"],
                "size": len(data),
                "sha256": sha256_bytes(data),
                "description": entry.get("description", "smoke fixture"),
            }
        )
        cursor += clusters

    fat_off = reserved * bps
    image[fat_off : fat_off + len(fat)] = fat
    image[fat_off + len(fat) : fat_off + 2 * len(fat)] = fat

    out_dir.mkdir(parents=True, exist_ok=True)
    image_path = out_dir / "smoke_fat32.img"
    image_path.write_bytes(image)
    meta = {
        "image": image_path.name,
        "image_path": str(image_path.resolve()),
        "image_size": len(image),
        "image_sha256": sha256_bytes(image),
        "scenario": "smoke",
        "filesystem": "fat32",
        "partition_offset": 0,
        "deleted": written["deleted"],
        "kept": written["kept"],
        "deleted_count": len(written["deleted"]),
        "kept_count": len(written["kept"]),
        "deleted_bytes": sum(x["size"] for x in written["deleted"]),
        "kept_bytes": sum(x["size"] for x in written["kept"]),
        "generated_by": "bench/run_benchmark.py --smoke",
        "allocated_clusters": allocated,
        "total_clusters": total_clusters,
    }
    (out_dir / "smoke_fat32.json").write_text(json.dumps(meta, indent=2) + "\n")
    return image_path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Benchmark CarveX against Foremost, Scalpel and PhotoRec",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--images-dir", type=Path, default=DEFAULT_IMAGES_DIR)
    parser.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS_DIR)
    parser.add_argument("--image", action="append", default=[], help="image stem or filename; repeatable")
    parser.add_argument("--scenario", action="append", choices=["all-deleted", "mixed"])
    parser.add_argument("--fs", action="append", choices=["fat32", "ntfs"])
    parser.add_argument(
        "--tools",
        default="carvex,foremost,scalpel,photorec",
        help="comma-separated: carvex,foremost,scalpel,photorec",
    )
    parser.add_argument(
        "--carvex-modes",
        default="unallocated,raw",
        help="comma-separated CarveX modes: unallocated,raw",
    )
    parser.add_argument("--carvex", type=Path, default=DEFAULT_CARVEX)
    parser.add_argument("--scalpel-config", type=Path, default=DEFAULT_SCALPEL_CONFIG)
    parser.add_argument("--foremost-config", type=Path, default=DEFAULT_FOREMOST_CONFIG)
    parser.add_argument("--min-score", type=int, default=50)
    parser.add_argument("--repair", action="store_true", help="pass --repair to CarveX")
    parser.add_argument("--threads", type=int, default=None)
    parser.add_argument("--chunk-size", default="")
    parser.add_argument("--reps", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=900, help="per-run timeout in seconds")
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--work", type=Path, default=None)
    parser.add_argument("--keep-outputs", action="store_true")
    parser.add_argument("--no-verify", action="store_true", help="do not re-hash images")
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="build an 8 MiB synthetic FAT32 dev image, run CarveX only, one rep",
    )
    args = parser.parse_args()

    smoke_root = None
    if args.smoke:
        smoke_root = Path(tempfile.mkdtemp(prefix="carvex-smoke-"))
        args.tools = "carvex"
        args.reps = 1
        args.carvex_modes = "unallocated,raw"
        args.no_verify = False
        args.out = smoke_root / "results"
        args.work = smoke_root / "work"
        print(f"Smoke fixture: {smoke_root}")

    if args.reps < 1:
        raise SystemExit("--reps must be >= 1")
    args.tools = [t.strip() for t in args.tools.split(",") if t.strip()]
    invalid_tools = sorted(set(args.tools) - {"carvex", "foremost", "scalpel", "photorec"})
    if invalid_tools:
        raise SystemExit(f"invalid tools: {', '.join(invalid_tools)}")
    args.images_dir = args.images_dir.resolve()
    args.corpus_dir = args.corpus.resolve()
    args.scalpel_config = args.scalpel_config.resolve()
    args.foremost_config = args.foremost_config.resolve()
    if "carvex" in args.tools:
        args.carvex = check_executable(args.carvex.resolve(), "CarveX binary")
    for name, path in (("Scalpel config", args.scalpel_config), ("Foremost config", args.foremost_config)):
        if not path.is_file():
            raise SystemExit(f"{name} not found: {path}")
    if "foremost" in args.tools and not shutil.which("foremost"):
        raise SystemExit("foremost not found in PATH")
    if "scalpel" in args.tools and not shutil.which("scalpel"):
        raise SystemExit("scalpel not found in PATH")
    if "photorec" in args.tools and not shutil.which("photorec"):
        raise SystemExit("photorec not found in PATH")

    if args.smoke:
        images_dir = smoke_root / "images"
        build_smoke_image(args.corpus_dir, images_dir)
        args.images_dir = images_dir

    cases = load_image_cases(args)
    print(f"Verifying {len(cases)} image(s) and ground truth ...")
    for case in cases:
        verify_image(case, not args.no_verify)
    print(
        "  "
        + ", ".join(
            f"{c.stem} ({len(c.deleted)} deleted, {len(c.kept)} kept)" for c in cases
        )
    )

    run_id = _dt.datetime.now().strftime("run-%Y%m%d-%H%M%S")
    results_dir = (args.out.resolve() if args.out else (DEFAULT_RESULTS_DIR / run_id))
    if results_dir.exists() and any(results_dir.iterdir()):
        if args.out:
            raise SystemExit(f"output directory is not empty: {results_dir}")
    results_dir.mkdir(parents=True, exist_ok=True)

    if args.keep_outputs:
        work_root = results_dir / "outputs"
    elif args.work:
        # Never use a user-supplied directory directly: cleanup at the end
        # removes only the run-specific subdirectory.
        work_root = args.work.resolve() / run_id
    else:
        work_root = DEFAULT_WORK_DIR / run_id
    work_root.mkdir(parents=True, exist_ok=True)
    if work_root == Path("/"):
        raise SystemExit("refusing to use / as work directory")

    specs = build_run_specs(args)
    # CarveX unallocated needs a supported filesystem.
    prepared: list[tuple[ImageCase, RunSpec]] = []
    for case in cases:
        for spec in specs:
            if spec.tool_id == "carvex-unallocated" and case.fs not in ("fat32", "ntfs"):
                print(f"skip {case.stem}: unsupported filesystem for unallocated scan", file=sys.stderr)
                continue
            prepared.append((case, spec))
    if not prepared:
        raise SystemExit("no image/tool combinations selected")

    versions: dict[str, str] = {}
    if "carvex" in args.tools:
        versions["CarveX"] = tool_version([str(args.carvex), "--version"])
    if "foremost" in args.tools:
        versions["Foremost"] = tool_version(["foremost", "-V"])
    if "scalpel" in args.tools:
        versions["Scalpel"] = tool_version(["scalpel", "-V"])
    if "photorec" in args.tools:
        versions["PhotoRec"] = tool_version(["photorec", "/version"])

    meta = {
        "runner": "bench/run_benchmark.py",
        "generated_at": _dt.datetime.now(_dt.timezone.utc).isoformat(),
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "cpu_count": os.cpu_count(),
        },
        "args": {
            key: (str(value) if isinstance(value, Path) else value)
            for key, value in vars(args).items()
        },
        "versions": versions,
        "images": [
            {
                "image": c.stem,
                "path": str(c.image_path),
                "scenario": c.scenario,
                "filesystem": c.fs,
                "size": c.image_size,
                "sha256": c.image_sha256,
                "deleted": len(c.deleted),
                "kept": len(c.kept),
                "deleted_bytes": sum(g.size for g in c.deleted),
                "kept_bytes": sum(g.size for g in c.kept),
            }
            for c in cases
        ],
        "methodology": {
            "exact": "SHA-256 equality with the original deleted file",
            "usable": "SHA-256 after trimming to the last JPEG EOI, PNG IEND or PDF %%EOF",
            "false_positive": "any output not assigned to a deleted ground-truth file",
            "dedupe": "one-to-one greedy assignment; extra copies count as false positives",
            "runtime": "wall-clock seconds, median of repetitions",
        },
        "runs": [],
        "rows": [],
        "pooled": [],
    }

    total_runs = len(prepared) * args.reps
    run_number = 0
    for case, spec in prepared:
        runs: list[dict[str, Any]] = []
        for rep in range(1, args.reps + 1):
            run_number += 1
            print(
                f"[{run_number:>3}/{total_runs}] {case.stem:22} {spec.tool_id:20} rep {rep}/{args.reps} ...",
                end="",
                flush=True,
            )
            try:
                result = run_one(spec, case, rep, args, work_root, results_dir)
            except Exception as exc:  # keep the benchmark going, report the failure
                result = {
                    "image": case.stem,
                    "scenario": case.scenario,
                    "fs": case.fs,
                    "tool": spec.label,
                    "tool_id": spec.tool_id,
                    "mode": spec.mode,
                    "space": spec.space,
                    "rep": rep,
                    "ok": False,
                    "error": f"{type(exc).__name__}: {exc}",
                    "runtime_s": 0.0,
                    "bytes_scanned": 0,
                    "image_size": case.image_size,
                    "metrics": {},
                    "outputs": [],
                    "command": [],
                }
            runs.append(result)
            meta["runs"].append(result)
            if result["ok"]:
                m = result["metrics"]
                print(
                    f" usable TP={m['tp_usable']:2} FP={m['fp_usable']:2} "
                    f"FN={m['fn_usable']:2} exact TP={m['tp_exact']:2} "
                    f"({result['runtime_s']:.3f}s)"
                )
            else:
                print(f" FAILED: {result.get('error')}")
        row = aggregate_runs(runs, spec, case)
        meta["rows"].append(row)

    meta["pooled"] = pool_rows(meta["rows"])
    with (results_dir / "results.json").open("w", encoding="utf-8") as f:
        json.dump(meta, f, indent=2)
    write_csv(results_dir / "results.csv", meta["rows"])
    write_markdown(
        results_dir / "BENCHMARK.md",
        meta["rows"],
        meta["pooled"],
        args,
        cases,
        versions,
    )

    if not args.keep_outputs:
        remove_path(work_root)

    print()
    print("=" * 108)
    print(
        f"{'Image':22} {'Tool':12} {'Space':12} {'TP':>3} {'FP':>3} {'FN':>3} "
        f"{'P':>7} {'R':>7} {'F1':>7} {'Time':>8}"
    )
    print("-" * 108)
    for row in meta["rows"]:
        print(
            f"{row['image']:22} {row['tool']:12} {row['space']:12} "
            f"{row['tp_usable']:3} {row['fp_usable']:3} {row['fn_usable']:3} "
            f"{row['precision_usable']*100:6.1f}% {row['recall_usable']*100:6.1f}% "
            f"{row['f1_usable']*100:6.1f}% {row['runtime_s']:7.3f}s"
        )
    print("=" * 108)
    print(f"Results: {results_dir}")
    print(f"  {results_dir / 'BENCHMARK.md'}")
    print(f"  {results_dir / 'results.csv'}")
    print(f"  {results_dir / 'results.json'}")
    return 0 if all(row["ok"] for row in meta["rows"]) else 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\ninterrupted", file=sys.stderr)
        sys.exit(130)
