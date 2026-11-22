# G8 scenario definitions

`scripts/perf_gate.py` executes these in-process scenarios. They are described
here so a threshold change can be reviewed against a stable definition; the
Python functions remain the single implementation.

| Name | Fixture | Command shape | Measurement |
|---|---|---|---|
| `s9_4m` | 4 MiB image with a six-byte JPEG header every 512 bytes | `scan --mode raw --no-fs --types jpeg --quiet` | Median of three wall-clock runs |
| `rand64m` | 64 MiB `os.urandom` image | `scan --mode raw --no-fs --quiet` | Median of three wall-clock runs |

Absolute limits and relative baseline ratios live in
`bench/perf/thresholds.toml`; the committed medians are in
`bench/perf/baseline-0.2.0.json`. The default run reports a relative regression
without failing (shared CI runners are noisy); `--strict` makes a regression
above the committed ratio fail the gate. Thresholds are never changed after
seeing a result without a `docs/DECISIONS.md` entry.
