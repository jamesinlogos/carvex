#!/usr/bin/env python3
"""Write the no-root synthetic FAT32 image used by the benchmark smoke test.

The generated image is the same fixture used by ``bench/run_benchmark.py
--smoke``.  CI uses it for packaged-binary checks because the large
``bench/images/*.img`` fixtures are deliberately git-ignored and are absent on
a clean checkout.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "bench"))

from run_benchmark import build_smoke_image  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=ROOT / "bench" / "corpus")
    parser.add_argument("--out", type=Path, required=True, help="output directory")
    args = parser.parse_args()

    image = build_smoke_image(args.corpus, args.out)
    print(image)
    return 0


if __name__ == "__main__":
    sys.exit(main())
