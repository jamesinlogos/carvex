# Dependencies

CarveX 0.2.0 keeps a small dependency tree, all under permissive licenses. All
versions below are taken from `Cargo.lock` at release time; builds/gates use
`--locked` so nothing is auto-upgraded.

## Direct dependencies (runtime)

| Crate | Version | Purpose | License |
|---|---:|---|---|
| `anyhow` | 1.0.104 | Context for errors in the CLI/binary; lib returns `Result` with context | MIT OR Apache-2.0 |
| `clap` (derive) | 4.6.8 | Argument parsing, `--help`, enum values | MIT OR Apache-2.0 |
| `crc32fast` | 1.5.5 | CRC check of each PNG chunk | MIT OR Apache-2.0 |
| `flate2` | 1.1.11 | Inflate PNG IDAT when `--validate decode` | MIT OR Apache-2.0 |
| `jpeg-decoder` | 0.3.2 | JPEG decoding when `--validate decode` (not used in the benchmark) | MIT OR Apache-2.0 |
| `memchr` | 2.8.3 | Fast header/footer search on large images | Unlicense OR MIT |
| `rayon` | 1.12.0 | Parallel carving by chunk, deterministic results | MIT OR Apache-2.0 |
| `serde` | 1.0.229 | Serialize report | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 | JSON report | MIT OR Apache-2.0 |
| `sha2` | 0.10.9 | File/image SHA-256, streaming | MIT OR Apache-2.0 |
| `tempfile` | 3.27.0 | Temporary spool for stdin images; temporary directories in integration tests | MIT OR Apache-2.0 |

## Dev dependencies

| Crate | Version | Purpose | License |
|---|---:|---|---|
| `proptest` | 1.12.0 | Property tests for extent arithmetic and parsers | MIT OR Apache-2.0 |

## Fuzz (separate `fuzz/` workspace)

| Crate | Version | Purpose | License |
|---|---:|---|---|
| `libfuzzer-sys` | 0.4 | cargo-fuzz runtime | MIT OR Apache-2.0 |

`fuzz/` has its own `Cargo.lock` and is not part of the crate's default build.

## Transitive

- Runtime: **46 packages** (including `carvex`), including the clap/anstyle,
  rayon/crossbeam, sha2/digest, serde, flate2/miniz_oxide, jpeg-decoder groups.
- Whole workspace (including dev + fuzz metadata): 91 packages.
- All licenses are among: `MIT`, `Apache-2.0`, `Apache-2.0 WITH
  LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `0BSD`,
  `Unlicense`, `Unicode-3.0`, or combinations of these licenses. There is no
  GPL/AGPL on the runtime path.
- `r-efi` (pulled in only by `getrandom` on UEFI) is `MIT OR Apache-2.0 OR
  LGPL-2.1-or-later`; the MIT/Apache branch is selected.

## Supply-chain controls

- `Cargo.lock` is committed and every gate/CI command uses `--locked`.
- `deny.toml` configures `cargo-deny`: license allow-list, bans wildcard
  versions, bans arbitrary git dependencies, denies yanked versions, warns on
  advisories.
- `.github/workflows/ci.yml` declares `cargo-deny` and `cargo-audit`. A GitHub
  Actions run succeeded on the pre-B-04 CI-fix commit; see `docs/VALIDATION.md §3`.
- **Local status:** `cargo-audit-audit 0.23.0` scanned 91 locked crates against
  1550 advisories with 0 vulnerabilities. `cargo-deny 0.21.0` reported
  `advisories ok, bans ok, licenses ok, sources ok` with four informational
  `license-not-encountered` warnings for currently unused allow-list entries.
  Both checks ran inside `scripts/gate.sh`; see `docs/PROGRESS.md`.

## Upgrade rules

1. Upgrading a dependency must run the full `scripts/gate.sh` and update the CHANGELOG.
2. Raise the MSRV only when a dependency requires it; record it in `docs/DECISIONS.md`.
3. Do not add a crate whose `unsafe` spreads into CarveX's parsers; crates already
   used (`sha2`, `flate2`, `rayon`, `jpeg-decoder`) are allowed internal
   `unsafe`, but CarveX's public API remains `#![forbid(unsafe_code)]`.
4. Do not use a decode library as an oracle for the benchmark (see D-15).
