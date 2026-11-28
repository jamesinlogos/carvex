# CarveX 0.3.1 prebuilt binaries

These binaries were rebuilt from current `main` after the §9 Q13 repair-scoring
change, build commit `cabd89f498932bd5a6e993ae2cff3d28bc7f9c4e`
(descendant of release tag `v0.3.1` at
`d5be769b93c5a9989cd53a69d7d662c10eb13996`). Both report `carvex 0.3.1` and
write `tool_info.commit = cabd89f49893` in schema-2 reports.

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

- Date: 2026-11-28
- Host: Linux x86_64 (Kali container)
- Rust toolchain: rustup 1.100.0-x86_64-unknown-linux-gnu (per `rust-toolchain.toml`)
- Targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`
- Windows linker: `/usr/bin/x86_64-w64-mingw32-gcc`
- Wine: `/usr/bin/wine` (wine64 not installed; PE32+ executed via wine)
- Source: `main` at `cabd89f498932bd5a6e993ae2cff3d28bc7f9c4e`

## Build commands

```bash
# from repository root at cabd89f
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
81cbe1769568ec1d532ab6229d4b98d0fa32ea05109399d8b3a7bee2c7e50f20  carvex-linux-x86_64
f0fa1204d5202ad2109b20904e81638d13bd7f44684f486db6b20801aa418a8c  carvex-windows-x86_64.exe
```

## Execution evidence

The exact files in this directory were executed before commit.

### Linux (glibc, native)

- `--version`: `carvex 0.3.1`
- `info bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `scan bench/images/mixed_fat32.img -o <tmp>/out --force --json-report <tmp>/report.json`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=cabd89f49893`

### Windows (PE32+, Wine)

- `wine carvex-windows-x86_64.exe --version`: `carvex 0.3.1`
- `wine carvex-windows-x86_64.exe info Z:/home/kali/carvex/bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `wine carvex-windows-x86_64.exe scan ... --json-report ...`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=cabd89f49893`

## X23 / R5 / G8 (shipped Linux glibc binary)

X23/R5 were measured on 2026-11-23 for the preceding 0.3.1 build; the current
gate re-checks both. G8 below was re-measured for this rebuild on 2026-11-28.

| Metric | Result | Target | Status |
|---|---:|---:|---|
| X23 `--threads 1` | 0.527s | - | measured |
| X23 default threads | 0.146s | <= 1.10x threads 1 | PASS (0.28x) |
| R5 (64 MiB fake JPEG headers) | 0.288s | <= 1.0s | PASS |

### G8 perf gate (`scripts/perf_gate.py --carvex dist/carvex-linux-x86_64`)

| Scenario | Median | Baseline ratio | Status |
|---|---:|---:|---|
| `s9_4m` | 0.650s | 1.657x committed baseline 0.392s (max 1.10x, informational under Q9) | PASS |
| `rand64m` | 0.084s | 2.809x committed baseline 0.030s (max 1.10x, informational under Q9) | PASS |

`PERF GREEN`.

## Not shipped

- `carvex-linux-x86_64-musl` (x86_64-unknown-linux-musl): measured from the
  0.3.1 release line; X23 default 2.452s vs 1t 1.048s (2.34x, FAIL) and R5
  5.020s (FAIL). Not included as a shipped Linux binary because R2-02 requires
  every shipped Linux binary to pass X23 and R5.
