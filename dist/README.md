# CarveX 0.3.1 prebuilt binaries

These binaries were rebuilt from current `main` after the H-04 crop-repair
implementation, build commit `1307adcaadb68f137e7e5c5151e4b7bd74cef46a`
(descendant of release tag `v0.3.1` at
`d5be769b93c5a9989cd53a69d7d662c10eb13996`). Both report `carvex 0.3.1` and
write `tool_info.commit = 1307adcaadb6` in schema-2 reports.

| File | Platform | Notes |
|---|---|---|
| `carvex-linux-x86_64` | Linux x86_64 | glibc-linked PIE, x86_64-unknown-linux-gnu, stripped; minimum GLIBC 2.34 |
| `carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain (x86_64-pc-windows-gnu) |
| `SHA256SUMS` | — | SHA-256 checksums for the two binaries |

## R2-02 / H-15 allocator note

The 0.3.0 static musl Linux binary failed X23/R5. For 0.3.1, the shipped Linux
binary is glibc-linked (`x86_64-unknown-linux-gnu`) and passes X23/R5. A musl
build from the same 0.3.1 line was measured and **not shipped** because it failed
X23 (default 2.452s vs 1t 1.048s, 2.34x) and R5 (5.020s, target <= 1.0s).
Minimum required GLIBC version is 2.34 (from ELF symbol versions). No allocator
dependency was added; remedy is the glibc-linked Linux binary (R2-02).

## Build environment

- Date: 2026-11-26
- Host: Linux x86_64 (Kali container)
- Rust toolchain: rustup 1.100.0-x86_64-unknown-linux-gnu (per `rust-toolchain.toml`)
- Targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`
- Windows linker: `/usr/bin/x86_64-w64-mingw32-gcc`
- Wine: `/usr/bin/wine` (wine64 not installed; PE32+ executed via wine)
- Source: `main` at `1307adcaadb68f137e7e5c5151e4b7bd74cef46a`

## Build commands

```bash
# from repository root at 1307adca
CARGO_NET_OFFLINE=true cargo build --release --locked --target x86_64-unknown-linux-gnu
CARGO_NET_OFFLINE=true cargo build --release --locked --target x86_64-pc-windows-gnu
cp target/x86_64-unknown-linux-gnu/release/carvex dist/carvex-linux-x86_64
cp target/x86_64-pc-windows-gnu/release/carvex.exe dist/carvex-windows-x86_64.exe
strip dist/carvex-linux-x86_64
strip dist/carvex-windows-x86_64.exe
(cd dist && sha256sum carvex-linux-x86_64 carvex-windows-x86_64.exe > SHA256SUMS)
```

## Checksums

```text
63d0ce0415255c933301e1ab35383a89b93e31d15cab29a3bb5d55d4b50a5e2d  carvex-linux-x86_64
7e05e9bf5f528b4dc102ae452100761d0e44844cdddb11e15b6efe747e71738b  carvex-windows-x86_64.exe
```

## Execution evidence

The exact files in this directory were executed before commit.

### Linux (glibc, native)

- `--version`: `carvex 0.3.1`
- `info bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `scan bench/images/mixed_fat32.img -o <tmp>/out --force --json-report <tmp>/report.json`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=1307adcaadb6`

### Windows (PE32+, Wine)

- `wine carvex-windows-x86_64.exe --version`: `carvex 0.3.1`
- `wine carvex-windows-x86_64.exe info Z:/home/kali/carvex/bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `wine carvex-windows-x86_64.exe scan ... --json-report ...`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=1307adcaadb6`

## X23 / R5 / G8 (shipped Linux glibc binary)

Measured on 2026-11-23, host Linux x86_64, median of 3 runs.

| Metric | Result | Target | Status |
|---|---:|---:|---|
| X23 `--threads 1` | 0.527s | - | measured |
| X23 default threads | 0.146s | <= 1.10x threads 1 | PASS (0.28x) |
| R5 (64 MiB fake JPEG headers) | 0.288s | <= 1.0s | PASS |

### G8 perf gate (`scripts/perf_gate.py --carvex dist/carvex-linux-x86_64`)

| Scenario | Median | Baseline ratio | Status |
|---|---:|---:|---|
| `s9_4m` | 0.155s | 0.395x baseline 0.392s (max 1.10x) | PASS |
| `rand64m` | 0.032s | 1.082x baseline 0.030s (max 1.10x) | PASS |

`PERF GREEN`.

## Not shipped

- `carvex-linux-x86_64-musl` (x86_64-unknown-linux-musl): measured from the
  0.3.1 release line; X23 default 2.452s vs 1t 1.048s (2.34x, FAIL) and R5
  5.020s (FAIL). Not included as a shipped Linux binary because R2-02 requires
  every shipped Linux binary to pass X23 and R5.
