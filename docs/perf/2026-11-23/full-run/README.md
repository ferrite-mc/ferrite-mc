# Full run raw data — 2026-11-23

Artifacts from the sequential Ferrite vs Folia · Leaf · Paper · Purpur comparison. See
[`../server-comparison.md`](../server-comparison.md) for the report.

## Files

- `<server>-<bots>.meta.txt` — runner result line, join distribution, TPS/`mspt` lines and
  (for Ferrite) the 20 s region profile. For JVM runs, the last probe rows are appended.
- `<server>-<bots>.ticks.csv` — JVM probe samples: `epoch_ms,mean_ms,p99_ms,max_ms,tps1,players`
  once per second.
- `schedule.log` — wall-clock start/end and the actual 1-minute load average at each measured
  boundary.
- `parsed.json` — normalized values used for the tables and charts.
- `analyze.py`, `make_charts.py` — parser and SVG generator used for this report.

## Caveat

`LOAD_BEFORE` / `LOAD_AFTER` lines inside the `*.meta.txt` files were produced by
`uptime -p`; they record host **uptime**, not load. The real 1-minute load values are in
`schedule.log`. The scripts in this directory are the exact ones used for the run and are
kept for reproduction, including their `/tmp/ferrite-full-2026-11-23` paths.
