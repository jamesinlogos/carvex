# CarveX prebuilt binaries (stale)

These binaries were built at commit `cbee72d26634be041e48f9256198c1ff344df82f`
(pre-B-04/B-06, pre-R-01/R-02, pre-harness remediation) and report `carvex 0.2.0`.
They **predate** the current source tree and **must not be used as evidence** for
HEAD behaviour.

They will be replaced at the 0.3.0 release from the tagged release commit, after
being rebuilt and executed (§7 of `harness.md`, ADR-H9). Do not delete them.

| File | Platform | Notes |
|---|---|---|
| `carvex-linux-x86_64` | Linux x86_64 | static PIE (musl), stripped |
| `carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain, stripped |
| `SHA256SUMS` | — | SHA-256 checksums for the stale binaries above |

Verify on Linux/macOS:

```bash
sha256sum -c SHA256SUMS
```
