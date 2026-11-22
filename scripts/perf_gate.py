#!/usr/bin/env python3
"""G8 performance gate (minimal Phase 0 scaffold).

Runs small perf scenarios and checks absolute thresholds from
bench/perf/thresholds.toml. Known-red scenarios can be marked xfail in the
thresholds file.
"""
from __future__ import annotations
import argparse, json, os, shutil, statistics, subprocess, tempfile, time
from pathlib import Path

try:
    import tomllib
except Exception:
    tomllib = None


def run(cmd, cwd=None):
    t0 = time.monotonic()
    p = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return time.monotonic() - t0, p.returncode, p.stderr


def scenario_s9_4m(carvex, work):
    img = work / 'flood4m.img'
    data = bytearray(4 * 1024 * 1024)
    hdr = b'\xff\xd8\xff\xe0\x00\x00'
    for off in range(0, len(data) - len(hdr), 512):
        data[off:off + len(hdr)] = hdr
    img.write_bytes(data)
    times = []
    for i in range(3):
        out = work / f'out-s9-{i}'
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir()
        t, rc, err = run([str(carvex), 'scan', str(img), '-o', str(out), '--mode', 'raw', '--no-fs', '--types', 'jpeg', '--quiet'])
        if rc != 0:
            return None, f'exit {rc}: {err[:200]}'
        times.append(t)
    return statistics.median(times), None


def scenario_rand64m(carvex, work):
    img = work / 'rand64.img'
    img.write_bytes(os.urandom(64 * 1024 * 1024))
    times = []
    for i in range(3):
        out = work / f'out-rand-{i}'
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir()
        t, rc, err = run([str(carvex), 'scan', str(img), '-o', str(out), '--mode', 'raw', '--no-fs', '--quiet'])
        if rc != 0:
            return None, f'exit {rc}: {err[:200]}'
        times.append(t)
    return statistics.median(times), None


SCENARIOS = {
    's9_4m': scenario_s9_4m,
    'rand64m': scenario_rand64m,
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--carvex', type=Path, default=Path('target/release/carvex'))
    ap.add_argument('--thresholds', type=Path, default=Path('bench/perf/thresholds.toml'))
    ap.add_argument('--workdir', type=Path, default=None)
    args = ap.parse_args()
    carvex = args.carvex.resolve()
    if not carvex.is_file():
        print(f'SKIPPED perf (carvex missing: {carvex})')
        return 0
    cfg = {}
    if args.thresholds.exists() and tomllib:
        cfg = tomllib.loads(args.thresholds.read_text())
    scenarios_cfg = cfg.get('scenarios', {})

    temp = None
    if args.workdir:
        work = args.workdir.resolve(); work.mkdir(parents=True, exist_ok=True)
    else:
        temp = tempfile.TemporaryDirectory(prefix='carvex-perf-'); work = Path(temp.name)

    failures = 0
    for name, fn in SCENARIOS.items():
        scfg = scenarios_cfg.get(name, {})
        xfail = bool(scfg.get('xfail', False))
        median, err = fn(carvex, work)
        if err:
            print(f'FAIL perf {name}: {err}')
            if not xfail: failures += 1
            continue
        max_s = scfg.get('max_seconds')
        ok = True
        if max_s is not None and median > float(max_s):
            ok = False
        status = 'PASS' if ok else ('XFAIL' if xfail else 'FAIL')
        print(f'{status} perf {name}: median {median:.3f}s' + (f' (max {max_s}s)' if max_s else ''))
        if not ok and not xfail:
            failures += 1
    if temp: temp.cleanup()
    if failures:
        print(f'PERF FAIL ({failures})')
        return 1
    print('PERF GREEN')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
