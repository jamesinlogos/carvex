# CarveX 0.3.1 prebuilt binaries

These binaries were rebuilt from local annotated tag `v0.3.1`, release commit
`d5be769b93c5a9989cd53a69d7d662c10eb13996`. Both report `carvex 0.3.1` and
write `tool_info.commit = d5be769b93c5` in schema-2 reports.

| File | Platform | Notes |
|---|---|---|
| `carvex-linux-x86_64` | Linux x86_64 | glibc-linked PIE, x86_64-unknown-linux-gnu, stripped; minimum GLIBC 2.34 |
| `carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain (x86_64-pc-windows-gnu) |
| `SHA256SUMS` | — | SHA-256 checksums for the two binaries |

## R2-02 / H-15 allocator note

The 0.3.0 static musl Linux binary failed X23/R5 on this host (default threads
slower than `--threads 1` and R5 exceeded 1s). For 0.3.1, the shipped Linux
binary is glibc-linked (`x86_64-unknown-linux-gnu`) and passes X23/R5. A musl
build from the same commit was measured and **not shipped** because it failed
X23 (default 2.452s vs 1t 1.048s, 2.34x) and R5 (5.020s, target <= 1.0s).
The glibc binary's minimum required GLIBC version is 2.34 (from ELF symbol
versions). No allocator dependency was added; the remedy is the glibc-linked
Linux binary as approved under R2-02.

## Build environment

- Date: 2026-11-23
- Host: Linux x86_64 (Kali container)
- Rust toolchain: rustup 1.100.0-x86_64-unknown-linux-gnu (per `rust-toolchain.toml`)
- Targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`
- Windows linker: `/usr/bin/x86_64-w64-mingw32-gcc`
- Wine: `/usr/bin/wine` (wine64 not installed; PE32+ executed via wine)
- Source: detached worktree at `v0.3.1` / `d5be769b93c5a9989cd53a69d7d662c10eb13996`

## Build commands

```bash
# from detached worktree at v0.3.1
CARGO_NET_OFFLINE=true cargo build --release --locked --target x86_64-unknown-linux-gnu
CARGO_NET_OFFLINE=true cargo build --release --locked --target x86_64-pc-windows-gnu
strip carvex-linux-x86_64
```

## Checksums

```text
c02bbe2d98e34b6fe0c66468c146d2734e541d34423b7cac719beb2d4abdb291  carvex-linux-x86_64
64343c02baf4d4c01f95448269f5b0d081f777c7e53471e6bbdb1be4bb80d2b4  carvex-windows-x86_64.exe
```

## Execution evidence

The exact files in this directory were executed before commit.

### Linux (glibc, native)

- `--version`: `carvex 0.3.1`
- `info bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `scan bench/images/mixed_fat32.img -o /tmp/out --force --json-report /tmp/out/report.json`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=d5be769b93c5`

### Windows (PE32+, Wine)

- `wine carvex-windows-x86_64.exe --version`: `carvex 0.3.1`
- `wine carvex-windows-x86_64.exe info Z:/home/kali/carvex/bench/images/mixed_fat32.img`: identified FAT32 (62.0 MiB), volume CARVEXFAT
- `wine carvex-windows-x86_64.exe scan ... --json-report ...`: carved 20 files; report `tool_info.version=0.3.1`, `tool_info.commit=d5be769b93c5`

## X23 / R5 / G8 (shipped Linux glibc binary)

Measured on 2026-11-23, host Linux x86_64, median of 3 runs.

| Metric | Result | Target | Status |
|---|---:|---:|---|
| X23 `--threads 1` | 0.565s | - | measured |
| X23 default threads | 0.158s | <= 1.10x threads 1 | PASS (0.28x) |
| R5 (64 MiB fake JPEG headers) | 0.310s | <= 1.0s | PASS |

### G8 perf gate (`scripts/perf_gate.py --carvex dist/carvex-linux-x86_64`)

| Scenario | Median | Baseline ratio | Status |
|---|---:|---:|---|
| `s9_4m` | 0.162s | 0.414x baseline 0.392s (max 1.10x) | PASS |
| `rand64m` | 0.028s | 0.929x baseline 0.030s (max 1.10x) | PASS |

`PERF GREEN`.

## Not shipped

- `carvex-linux-x86_64-musl` (x86_64-unknown-linux-musl): measured from same
  commit; X23 default 2.452s vs 1t 1.048s (2.34x, FAIL) and R5 5.020s (FAIL).
  Not included as a shipped Linux binary because R2-02 requires every shipped
  Linux binary to pass X23 and R5.
