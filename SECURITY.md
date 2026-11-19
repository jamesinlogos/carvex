# Security (SECURITY.md)

## Support

| Version | Status |
|---|---|
| 0.2.x | Supported (current branch) |
| 0.1.0 | Critical fixes only; upgrade recommended |

## Reporting vulnerabilities

There is no public security email address yet. Once the repository is
published, use this repository's **GitHub Security Advisory** ("Report a
vulnerability" section); if no private channel is available, contact the
repository owner directly and do **not** open a public issue for an unpatched
vulnerability.

When reporting, please include: version/commit, platform, reproduction steps
(minimal sample image or fixture-generating script), expected impact, and
whether you have already disclosed it publicly.

## Scope

**In scope:**

- File reads/writes on the runner's machine via the CLI (output paths, `--json-report`, …).
- Processing **untrusted** data from disk images: JPEG/PNG/PDF parsers, FAT32/NTFS,
  partition tables — i.e., any user-supplied image.
- Memory allocations whose size/source is influenced by image data (capped allocation).

**Out of scope:**

- Vulnerabilities in build-layer dependencies (see `deny.toml` / `cargo audit` —
  the actual run status is recorded in `docs/PROGRESS.md`).
- CarveX does not open files from the network and does not run code from images;
  "open image ⇒ code execution" attacks are outside this model.

## Security-related design commitments

- **No `unsafe`** in the CarveX source (`#![deny(unsafe_code)]` at the crate root).
- **The source image is opened read-only**; all write paths are confined to the
  output directory.
- **No panics on image data:** the `src/fs/*` and `src/signatures/*` modules
  deny `unwrap_used`/`expect_used`/`indexing_slicing`; corrupt-data errors
  return `Err` or are filtered out rather than crashing the process (see
  `docs/DECISIONS.md` D-03).
- **Image-derived memory limits:** every allocation derived from image data is
  bounded by `--max-memory` / `--max-size` / `--max-truncated-size`.
- Fuzz status: the skeleton is in the repo but **has not been run** (missing
  nightly/cargo-fuzz in the development environment) — recorded as `SKIPPED`
  in `docs/PROGRESS.md`; no claim of "clean fuzzing" is made.

## Reporting non-vulnerability issues

Panics/ICEs on truncated images, incorrect figures, incorrect exit codes: open
a regular issue with the sample image and the output of `scripts/review_repro.py`.
