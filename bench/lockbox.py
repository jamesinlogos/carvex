#!/usr/bin/env python3
"""Build a frozen, single-use "lockbox" benchmark dataset for CarveX.

This script creates a sealed evaluation set from seeds 1001-1020.  The
resulting images, corpus, ground-truth manifests and logs must be treated as
read-only evaluation artifacts:

* Results obtained on the lockbox must NEVER be used to modify code, tweak
  scoring, adjust thresholds, repair a tool, or otherwise tune the system.
  Any such feedback loop would invalidate the evaluation.
* If a bug is discovered while inspecting lockbox results, do not regenerate
  or repair the lockbox.  Define and generate a NEW seed set instead, and keep
  the original lockbox as the frozen record of what was tested.

Only this script is added; the existing corpus/image builders and the runner
are left untouched.  All heavy lifting is delegated to ``bench/make_corpus.py``
and ``bench/make_images.py`` with fresh, seed-scoped output directories.
"""

import argparse
import hashlib
import json
import shlex
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Set, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent
BENCH = REPO_ROOT / "bench"
DEFAULT_DEV_CORPUS = BENCH / "corpus"
DEFAULT_OUT = BENCH / "lockbox"
DEFAULT_SEEDS = "1001..1020"
DEFAULT_PROFILE = "d1"
PROTOCOL_NAME = "carvex-lockbox-v1"

# Balanced design: five seeds per scenario/filesystem cell.
D1_DESIGN: Tuple[Tuple[int, int, str, str], ...] = (
    (1001, 1005, "all-deleted", "fat32"),
    (1006, 1010, "all-deleted", "ntfs"),
    (1011, 1015, "mixed", "fat32"),
    (1016, 1020, "mixed", "ntfs"),
)


# ---------------------------------------------------------------------------
# Helpers


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_of_hashes(hashes: Any) -> str:
    """Stable digest over a collection of SHA-256 hex strings."""
    return sha256_bytes(json.dumps(sorted(set(hashes)), separators=(",", ":")).encode())


def repo_rel(path: Path) -> str:
    """Return a repo-root-relative POSIX path; fall back to a bare name if outside."""
    resolved = path.resolve()
    try:
        return resolved.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return resolved.name


def out_rel(path: Path, out: Path) -> str:
    return path.resolve().relative_to(out.resolve()).as_posix()


def is_within(path: Path, parent: Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def parse_seeds(spec: str) -> List[int]:
    """Parse comma-separated ints, inclusive ranges (``a..b`` or ``a-b``)."""
    values: List[int] = []
    for token in spec.split(","):
        token = token.strip()
        if not token:
            continue
        if ".." in token:
            left, right = token.split("..", 1)
            start, end = int(left), int(right)
            if end < start:
                raise SystemExit(f"invalid seed range: {token}")
            values.extend(range(start, end + 1))
        elif token.count("-") == 1 and token.replace("-", "").isdigit():
            left, right = token.split("-", 1)
            start, end = int(left), int(right)
            if end < start:
                raise SystemExit(f"invalid seed range: {token}")
            values.extend(range(start, end + 1))
        else:
            values.append(int(token))
    if not values:
        raise SystemExit("no seeds selected")
    unique: List[int] = []
    seen: Set[int] = set()
    for seed in values:
        if seed <= 0:
            raise SystemExit(f"invalid seed: {seed}")
        if seed not in seen:
            seen.add(seed)
            unique.append(seed)
    return unique


def design_for(seed: int) -> Tuple[str, str]:
    for low, high, scenario, fs in D1_DESIGN:
        if low <= seed <= high:
            return scenario, fs
    raise SystemExit(
        f"seed {seed} is outside the d1 design (1001..1020); "
        "use a new seed set rather than extending a lockbox"
    )


def load_json(path: Path) -> Dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"cannot read JSON {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise SystemExit(f"expected a JSON object in {path}")
    return data


def write_json(path: Path, obj: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def run_logged(cmd: List[str], log_path: Path) -> None:
    """Run a command from the repo root, teeing both streams into a log file."""
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8", errors="replace") as log:
        log.write("$ " + shlex.join(cmd) + "\n")
        log.write("cwd: " + repo_rel(REPO_ROOT) + "\n\n")
        log.flush()
        proc = subprocess.run(
            cmd,
            cwd=str(REPO_ROOT),
            stdout=log,
            stderr=subprocess.STDOUT,
            check=False,
        )
    if proc.returncode != 0:
        raise SystemExit(
            f"command failed with exit code {proc.returncode}: {shlex.join(cmd)}\n"
            f"see {repo_rel(log_path)}"
        )


def prepare_out(out: Path, dev_corpus: Path, force: bool) -> None:
    """Create a fresh output directory, refusing to clobber existing artifacts."""
    if out == REPO_ROOT or out == BENCH:
        raise SystemExit(f"refusing to use a protected directory as --out: {out}")
    if out == dev_corpus or is_within(dev_corpus, out) or is_within(out, dev_corpus):
        raise SystemExit(
            f"--out {out} overlaps the dev corpus {dev_corpus}; refusing to touch it"
        )
    if out.exists() and any(out.iterdir()):
        if not force:
            raise SystemExit(
                f"output directory is not empty: {out} (use --force to regenerate)"
            )
        shutil.rmtree(out)
    out.mkdir(parents=True, exist_ok=True)


def dev_corpus_hashes(dev_corpus: Path) -> Dict[str, str]:
    dev_manifest = dev_corpus / "manifest.json"
    if not dev_manifest.is_file():
        raise SystemExit(f"missing dev corpus manifest: {dev_manifest}")
    manifest = load_json(dev_manifest)
    hashes: Dict[str, str] = {}
    for i, entry in enumerate(manifest.get("files", [])):
        digest = str(entry.get("sha256", "")).lower()
        if not digest:
            raise SystemExit(f"dev manifest entry {i} has no sha256")
        hashes.setdefault(digest, str(entry.get("path", f"entry-{i}")))
    if not hashes:
        raise SystemExit(f"dev corpus manifest has no files: {dev_manifest}")
    return hashes


def generated_corpus_entries(corpus_dir: Path, seed: int) -> List[Dict[str, Any]]:
    """Re-hash every generated corpus file and validate the seed manifest."""
    manifest_path = corpus_dir / "manifest.json"
    manifest = load_json(manifest_path)
    entries: List[Dict[str, Any]] = []
    for i, entry in enumerate(manifest.get("files", [])):
        rel = Path(str(entry.get("path", "")))
        if rel.is_absolute() or ".." in rel.parts or not rel.parts:
            raise SystemExit(f"seed {seed}: unsafe manifest path at entry {i}: {rel}")
        path = corpus_dir / rel
        if not path.is_file():
            raise SystemExit(f"seed {seed}: missing generated file: {path}")
        digest = sha256_file(path)
        size = path.stat().st_size
        expected_hash = str(entry.get("sha256", "")).lower()
        if expected_hash and expected_hash != digest:
            raise SystemExit(
                f"seed {seed}: hash mismatch for {rel}: "
                f"manifest={expected_hash}, actual={digest}"
            )
        if entry.get("size") is not None and int(entry["size"]) != size:
            raise SystemExit(
                f"seed {seed}: size mismatch for {rel}: "
                f"manifest={entry['size']}, actual={size}"
            )
        entries.append(
            {
                "source": path,
                "path": rel.as_posix(),
                "name": str(entry.get("name", rel.name)),
                "type": str(entry.get("type", "")),
                "size": size,
                "sha256": digest,
                "description": str(entry.get("description", "")),
            }
        )
    if not entries:
        raise SystemExit(f"seed {seed}: generated corpus has no files")
    return entries


def build_combined_manifest(entries: List[Dict[str, Any]], profile: str, seeds: List[int]) -> Dict[str, Any]:
    return {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "protocol": PROTOCOL_NAME,
        "split": "lockbox",
        "profile": profile,
        "seeds": seeds,
        "file_count": len(entries),
        "total_bytes": sum(e["size"] for e in entries),
        "files": [
            {k: v for k, v in e.items() if k != "source"} for e in entries
        ],
    }


def generate_one_corpus(seed: int, out: Path, logs: Path) -> Path:
    corpus_dir = out / "corpora" / f"s{seed}"
    if corpus_dir.exists():
        raise SystemExit(f"fresh corpus directory already exists: {corpus_dir}")
    cmd = [
        "python3",
        repo_rel(BENCH / "make_corpus.py"),
        "--out",
        repo_rel(corpus_dir),
        "--seed",
        str(seed),
    ]
    run_logged(cmd, logs / f"corpus-s{seed}.log")
    if not (corpus_dir / "manifest.json").is_file():
        raise SystemExit(f"make_corpus.py produced no manifest in {corpus_dir}")
    return corpus_dir


def generate_one_image(
    seed: int,
    scenario: str,
    fs: str,
    corpus_dir: Path,
    out: Path,
    logs: Path,
    work: Path,
    profile: str,
) -> Dict[str, Any]:
    stem = f"s{seed}_{scenario}_{fs}"
    temp_dir = Path(tempfile.mkdtemp(prefix=f"images-s{seed}-", dir=str(work)))
    try:
        cmd = [
            "python3",
            repo_rel(BENCH / "make_images.py"),
            "--corpus",
            repo_rel(corpus_dir),
            "--out",
            repo_rel(temp_dir),
            "--scenario",
            scenario,
            "--fs",
            fs,
            "--size-mb",
            "64",
            "--force",
        ]
        run_logged(cmd, logs / f"images-s{seed}.log")

        img_src = temp_dir / f"{scenario}_{fs}.img"
        meta_src = temp_dir / f"{scenario}_{fs}.json"
        if not img_src.is_file() or not meta_src.is_file():
            raise SystemExit(f"make_images.py did not produce {stem} in {temp_dir}")

        images_dir = out / "images"
        images_dir.mkdir(parents=True, exist_ok=True)
        img_dst = images_dir / f"{stem}.img"
        meta_dst = images_dir / f"{stem}.json"
        shutil.copy2(img_src, img_dst)

        meta = load_json(meta_src)
        meta["image"] = img_dst.name
        # The runner resolves relative image_path values against the manifest's
        # own directory, so keep this as a bare filename (and avoid absolute
        # /home/kali-style paths in generated artifacts).
        meta["image_path"] = img_dst.name
        meta["corpus_manifest"] = repo_rel(out / "corpus" / "manifest.json")
        for key in ("deleted", "kept"):
            for entry in meta.get(key, []):
                if not isinstance(entry, dict):
                    raise SystemExit(f"seed {seed}: bad {key} entry in {meta_src}")
                rel = str(entry.get("path", ""))
                entry["path"] = f"s{seed}/{rel}" if rel else f"s{seed}/"
        meta["seed"] = seed
        meta["profile"] = profile
        meta["split"] = "lockbox"
        write_json(meta_dst, meta)

        return {
            "stem": stem,
            "scenario": scenario,
            "filesystem": fs,
            "corpus": repo_rel(corpus_dir),
            "corpus_manifest": repo_rel(corpus_dir / "manifest.json"),
            "image": img_dst.name,
            "image_path": meta["image_path"],
            "image_manifest": repo_rel(meta_dst),
            "image_sha256": sha256_file(img_dst),
            "image_manifest_sha256": sha256_file(meta_dst),
            "image_size": img_dst.stat().st_size,
            "deleted_count": len(meta.get("deleted", [])),
            "kept_count": len(meta.get("kept", [])),
        }
    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)


def write_sha256sums(out: Path) -> Path:
    """Hash every artifact under --out; paths are relative to --out."""
    sums = out / "SHA256SUMS"
    lines: List[str] = []
    for path in sorted(p for p in out.rglob("*") if p.is_file()):
        if path == sums:
            continue
        lines.append(f"{sha256_file(path)}  {out_rel(path, out)}")
    sums.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return sums


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate a frozen 20-image lockbox dataset (never tune code on it).",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--dev-corpus", type=Path, default=DEFAULT_DEV_CORPUS)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--seeds", default=DEFAULT_SEEDS, help="e.g. 1001..1020 or 1001,1002")
    parser.add_argument("--profile", default=DEFAULT_PROFILE)
    parser.add_argument(
        "--force",
        action="store_true",
        help="allow regenerating a non-empty --out directory",
    )
    args = parser.parse_args()

    seeds = parse_seeds(args.seeds)
    for seed in seeds:
        design_for(seed)

    dev_corpus = args.dev_corpus.resolve()
    out = args.out.resolve()
    prepare_out(out, dev_corpus, args.force)

    logs = out / "logs"
    work = out / "work"
    logs.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)

    dev_hashes = dev_corpus_hashes(dev_corpus)
    dev_manifest_path = dev_corpus / "manifest.json"
    dev_manifest_hash = sha256_file(dev_manifest_path)
    dev_hashes_digest = sha256_of_hashes(dev_hashes.keys())

    # hash -> owner description; starts with the dev corpus for strict dedupe.
    seen: Dict[str, str] = {h: f"dev corpus:{owner}" for h, owner in dev_hashes.items()}
    combined_entries: List[Dict[str, Any]] = []
    seed_mapping: Dict[str, Any] = {}
    image_info: Dict[str, Any] = {}

    for seed in seeds:
        scenario, fs = design_for(seed)
        corpus_dir = generate_one_corpus(seed, out, logs)
        entries = generated_corpus_entries(corpus_dir, seed)

        for entry in entries:
            digest = entry["sha256"]
            owner = f"s{seed}:{entry['path']}"
            if digest in seen:
                raise SystemExit(
                    f"strict dedupe failure: {owner} has sha256 {digest}, "
                    f"already present in {seen[digest]}"
                )
            seen[digest] = owner

            dst = out / "corpus" / f"s{seed}" / entry["path"]
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(entry["source"], dst)
            combined_entries.append(
                {
                    "path": f"s{seed}/{entry['path']}",
                    "name": entry["name"],
                    "type": entry["type"],
                    "size": entry["size"],
                    "sha256": digest,
                    "description": entry["description"],
                    "seed": seed,
                    "profile": args.profile,
                }
            )

        image = generate_one_image(
            seed, scenario, fs, corpus_dir, out, logs, work, args.profile
        )
        image_info[image["stem"]] = image
        seed_mapping[str(seed)] = {
            "seed": seed,
            "profile": args.profile,
            "scenario": scenario,
            "filesystem": fs,
            "corpus": {
                "dir": repo_rel(corpus_dir),
                "manifest": repo_rel(corpus_dir / "manifest.json"),
                "file_count": len(entries),
            },
            "image": image,
        }
        print(
            f"seed {seed}: {scenario}/{fs} -> {image['stem']} "
            f"({len(entries)} corpus files)"
        )

    try:
        work.rmdir()
    except OSError:
        pass

    combined_manifest_path = out / "corpus" / "manifest.json"
    combined_manifest = build_combined_manifest(combined_entries, args.profile, seeds)
    write_json(combined_manifest_path, combined_manifest)
    combined_manifest_hash = sha256_file(combined_manifest_path)
    combined_hashes_digest = sha256_of_hashes(e["sha256"] for e in combined_entries)

    design_by_seed = {
        str(seed): {
            "scenario": design_for(seed)[0],
            "filesystem": design_for(seed)[1],
        }
        for seed in seeds
    }
    profile_mapping: Dict[str, Any] = {
        args.profile: {
            "name": args.profile,
            "description": "balanced 5/5/5/5 all-deleted/mixed by FAT32/NTFS",
            "ranges": {
                "1001-1005": {"scenario": "all-deleted", "filesystem": "fat32"},
                "1006-1010": {"scenario": "all-deleted", "filesystem": "ntfs"},
                "1011-1015": {"scenario": "mixed", "filesystem": "fat32"},
                "1016-1020": {"scenario": "mixed", "filesystem": "ntfs"},
            },
            "seeds": [str(seed) for seed in seeds],
            "design": design_by_seed,
        }
    }

    lockbox: Dict[str, Any] = {
        "protocol": PROTOCOL_NAME,
        "name": PROTOCOL_NAME,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "profile": args.profile,
        "split": "lockbox",
        "run_once": True,
        "seeds": [str(seed) for seed in seeds],
        "dev_corpus": {
            "path": repo_rel(dev_corpus),
            "manifest": repo_rel(dev_manifest_path),
            "manifest_sha256": dev_manifest_hash,
            "file_hashes_sha256": dev_hashes_digest,
            "file_count": len(dev_hashes),
        },
        "combined_corpus": {
            "path": repo_rel(out / "corpus"),
            "manifest": repo_rel(combined_manifest_path),
            "manifest_sha256": combined_manifest_hash,
            "file_hashes_sha256": combined_hashes_digest,
            "file_count": len(combined_entries),
        },
        "dev_corpus_manifest_sha256": dev_manifest_hash,
        "combined_corpus_manifest_sha256": combined_manifest_hash,
        "seed_mapping": seed_mapping,
        "profile_mapping": profile_mapping,
        "image_hashes": {
            stem: {
                "image_sha256": info["image_sha256"],
                "image_manifest_sha256": info["image_manifest_sha256"],
            }
            for stem, info in image_info.items()
        },
    }
    write_json(out / "lockbox.json", lockbox)
    sums = write_sha256sums(out)

    print(
        f"lockbox ready: {len(seeds)} seeds, {len(combined_entries)} corpus files, "
        f"{len(image_info)} images -> {repo_rel(out)}"
    )
    print(f"manifest: {repo_rel(out / 'lockbox.json')}")
    print(f"checksums: {repo_rel(sums)}")


if __name__ == "__main__":
    main()
