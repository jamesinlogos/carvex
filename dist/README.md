# CarveX prebuilt binaries

| File | Platform | Notes |
|---|---|---|
| `carvex-linux-x86_64` | Linux x86_64 | static PIE (musl), stripped |
| `carvex-windows-x86_64.exe` | Windows x86_64 | PE32+, GNU toolchain, stripped |
| `SHA256SUMS` | — | SHA-256 checksums |

Verify on Linux/macOS:

```bash
sha256sum -c SHA256SUMS
```

Verify on Windows (PowerShell):

```powershell
Get-FileHash .\carvex-windows-x86_64.exe -Algorithm SHA256
```

Quick test:

```bash
chmod +x carvex-linux-x86_64
./carvex-linux-x86_64 --version
./carvex-linux-x86_64 scan disk.img -o recovered --mode unallocated
```

The Windows binary was cross-compiled and checked as a PE file, but has not
been exercised on a real Windows host in the development environment. See the
project README for usage and limitations.
