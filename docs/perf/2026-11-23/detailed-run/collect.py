#!/usr/bin/env python3
"""Copy the detailed benchmark artifacts into the repository.

Only metadata, bot output, JVM probe CSVs, compressed console logs and the summary are
copied; the per-run copied 82 MiB fixture worlds stay in /tmp.
"""
from __future__ import annotations

import argparse
import gzip
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
DEFAULT_SRC = Path("/tmp/ferrite-detailed-2026-11-23")
DEFAULT_DEST = Path(__file__).resolve().parent


def copy_file(src: Path, dest: Path) -> None:
    if src.exists():
        shutil.copy2(src, dest)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--src", type=Path, default=DEFAULT_SRC)
    parser.add_argument("--dest", type=Path, default=DEFAULT_DEST)
    args = parser.parse_args()
    raw = args.dest / "raw"
    raw.mkdir(parents=True, exist_ok=True)
    for run in sorted((args.src / "runs").iterdir()):
        if not run.is_dir():
            continue
        out = raw / run.name
        out.mkdir(parents=True, exist_ok=True)
        for name in ("meta.json", "meta.txt", "bots.log", "ticks.csv"):
            copy_file(run / name, out / name)
        console = run / "console.log"
        if console.exists():
            with open(console, "rb") as source, gzip.open(out / "console.log.gz", "wb", compresslevel=9) as target:
                shutil.copyfileobj(source, target)
    for name in ("schedule.log", "summary.json", "summary.md"):
        copy_file(args.src / name, raw / name)
    copy_file(args.src / "results" / "flush_bench.txt", raw / "flush_bench.txt")
    for name in ("chunk-stream.svg", "fixture-load.svg", "ferrite-800.svg"):
        copy_file(args.src / name, args.dest.parent / name)
    print(f"copied {len(list(raw.glob('*/meta.json')))} run metadata files to {raw}")


if __name__ == "__main__":
    main()
