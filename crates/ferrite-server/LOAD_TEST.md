# 200-bot load test

Run from the workspace root on Windows with `cargo run --release -p ferrite-server --example bots -- 200`. The example starts an in-process offline server with view distance 2 and 200 clients; after all clients join, they walk for 30 seconds. It runs once with the in-memory superflat world and once with `vanilla/26.3/fixture-world/world`.

Release run on 2026-10-28, after the fluid-count encoder fix:

| World | Average region tick | p99 region tick | Process RSS | Process CPU |
| --- | ---: | ---: | ---: | ---: |
| Superflat | 0.345 ms | 3.494 ms | 231.0 MiB | 107.9% |
| Fixture | 0.397 ms | 2.655 ms | 282.6 MiB | 123.2% |

Tick times are sampled around region tick callbacks, including the initial join period, and do not include the global-region callback. RSS and CPU cover both server and bots in the same process; CPU is total CPU time divided by wall time (100% means one fully used logical CPU), measured from bot startup through the end of the walk.

The harness also runs on Linux (RSS and CPU come from `/proc/self`) and supports 800 bots, a
connect stagger and a `/forceload` loop; see `docs/perf/README.md` for the command and the
2026-11-11 numbers.
