# CarveX 0.3.0 prebuilt binaries

These binaries were rebuilt from the local annotated tag `v0.3.0`, release
commit `5480f1084f85cc91f68d94c76c703d1348a65b5e`. Both report `carvex 0.3.0`
and write `tool_info.commit = 5480f1084f85` in schema-2 reports.

| File | Platform | Notes |
|---|---|---|
| `carvex-linux-x86_64` | Linux x86_64 | static PIE, musl target, stripped |
| `carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain, stripped to external PDB |
| `SHA256SUMS` | — | SHA-256 checksums for the two binaries |

## Build environment

- Date: 2026-11-22
- Host: Linux x86_64 (Kali container), rustc/cargo 1.100.0
  (`0ef1bc386 2026-11-09`)
- Targets installed with rustup: `x86_64-unknown-linux-musl` and
  `x86_64-pc-windows-gnu`
- Windows linker: `/usr/bin/x86_64-w64-mingw32-gcc`
- Source worktree: detached `v0.3.0` at `5480f1084f85cc91f68d94c76c703d1348a65b5e`
- Commands:

```bash
cargo build --release --locked --target x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-pc-windows-gnu
strip carvex-linux-x86_64
```

## Execution evidence

The exact files in this directory were executed after copying/stripping:

- Linux musl: `--version`, `info` and a small `scan` on
  `bench/images/mixed_fat32.img` succeeded from the tagged source worktree.
- Windows GNU: the same three commands succeeded under Wine 10.0 on Linux
  (`WINEDEBUG=-all`). No Windows runner was available here, so this is **not**
  a Windows-runner execution. An earlier CI Windows packaged smoke job failed
  and logs were unavailable through the public API; later observed run
  `45121233267` at `86ae15f` had green Windows packaged smoke, but that run did
  not execute these committed 0.3.0 dist files as release artifacts.
- Both reports recorded `version = 0.3.0` and
  `commit = 5480f1084f85`.

Release-binary performance and repro controls:

- `python3 scripts/perf_gate.py --carvex dist/carvex-linux-x86_64`:
  **PERF GREEN** (absolute limits), with `s9_4m` median 2.842 s and a reported
  7.251x relative baseline regression.
- The Windows binary through a Wine wrapper: **PERF GREEN** (absolute limits),
  with `s9_4m` median 0.699 s and `rand64m` median 4.752 s; both relative
  regression notices were reported.
- The full repro suite on the musl binary was **not** green: X23 measured
  default 2.80 s versus 1 thread 1.26 s and R5 measured 5.50 s against the
  <= 1.0 s target. Both are the already-recorded musl/release-binary H-15/R5
  behaviour; the glibc source-build gate remains green. No allocator or other
  performance change was made.

## Checksums

```text
953dab643ce6628157b416b3588b1da59b84d760df627f82b14529a2cc3f975e  carvex-linux-x86_64
cbcc5343b63881c6c7decc6b218b99742d0d383c3b6415920ad4dd464758f1fa  carvex-windows-x86_64.exe
```

Verify on Linux/macOS:

```bash
sha256sum -c SHA256SUMS
```
