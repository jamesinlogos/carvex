# Contributing (CONTRIBUTING.md)

## Development environment

- Rust **1.85+** (MSRV, see `Cargo.toml` `rust-version`); edition 2021.
- Python 3 for the `bench/` and `scripts/` scripts.
- (Optional) `foremost`, `scalpel`, `photorec` for competitor benchmarks.

```bash
cargo build --release --locked
cargo test
```

## Workflow for a change

1. **Read the objective in `guide.md`** (if relevant) and identify the ID
   (A-xx/B-xx/C-xx/D-xx) or issue.
2. **Write the red test first** (correct behavior not yet present), make the
   minimal fix, and get the test green. New tests must cite the ID in the name
   or doc-comment.
3. **Run the quality gate** — every command must be green:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features --locked -- -D warnings
   cargo test
   cargo doc --no-deps --locked
   bash scripts/gate.sh          # full: fmt/clippy/test/doc/repro/bench --smoke
   ```

4. **Update documentation:** `CHANGELOG.md` (the *Changed*/*Behavior changes*
   section for behavior changes), `docs/DECISIONS.md` (every consequential
   decision — mandatory, no need to ask again except for the §3.5 cases of
   the guide), `docs/PROGRESS.md`.
5. **Commit:** in English, using Conventional Commits, with the ID, e.g.
   `fix(pdf): follow incremental updates past the read window [A-01]`.
   One commit = one logical change + tests + PROGRESS update; do **not** mix
   refactoring with behavior changes.

## Hard rules

- **No `unsafe`.** The crate root forbids `unsafe_code` (`#![forbid(unsafe_code)]`).
- **No panic/`unwrap`/`expect`/unchecked indexing on image data.** The
  `src/fs/*` and `src/signatures/*` modules enable
  `#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]`;
  test modules use `#[allow]` with an explanation.
- **Source image is read-only.** Never open the image file for writing; all
  writes go to the output directory.
- **Image-derived allocations must be bounded** (`--max-memory`,
  `--max-size`, `--max-truncated-size`, number of chunks/extents checked before
  allocation).
- **Backward compatibility:** do not rename/retype/change the meaning of
  published JSON/CSV fields; only add new ones. Changing the shape ⇒ add a
  schema version and keep the old `--report-schema`.
- **Documented figures must be reproducible from `results.json`** or cite
  their source; do not fabricate scientific citations — open the DOI/publisher
  page before including one.

## Testing

- Unit (`#[cfg(test)]` inside modules), integration (`tests/*.rs`), CLI
  contract (`tests/cli_contract.rs` — exit codes, schema), evidence formats
  (`tests/evidence_formats.rs` — split raw, E01, VHD, VMDK, stdin), FS detection
  (`tests/fs_detect.rs`), property (`tests/properties.rs`).
- Self-generated image fixtures in `tests/common/mod.rs`; do not commit large
  disk images.
- Reproduce independent review scenarios: `scripts/review_repro.py` (R1–R8),
  run with `--assert --xfail scripts/repro_xfail.txt`.

## Decision documentation

Every consequential decision (behavior change, rename, dropping a feature,
deviation from the guide) must have an entry in `docs/DECISIONS.md` using the
template:

```markdown
## D-xx. Title
- **Context:** …
- **Decision:** …
- **Consequences:** …
```
