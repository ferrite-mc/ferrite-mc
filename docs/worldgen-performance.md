# World generation performance experiments

Measurements below use the server `genbench` release example, seed 0, start (0, 0),
and eight worldgen threads on the same shared Windows machine. These numbers include
generation and lighting; they do not measure disk reads or multiplayer performance.

| Experiment | Comparison | Result | Decision |
| --- | --- | --- | --- |
| Memoize individual 2D surface-noise samples in a hash map | 50 chunks: 39.59 baseline versus 37.72 chunks/s in two trials; baseline recheck 39.22 | The extra lookups did not improve throughput. | Reverted. |
| Reuse interpolation corners for surface-height density evaluation within each chunk | 100 chunks, six alternating trials: baseline median 37.33, optimized median 46.30 chunks/s | About 24% higher median; individual samples varied substantially with shared-machine load (baseline 14.56–38.97, optimized 21.27–46.66). | Retained; repeat under stable load before treating the median as a fixed capacity target. |

For the retained change, unit tests compare cached and uncached density results bit for bit
across positive and negative cell boundaries, including the real overworld router at two
seeds. The full `ferrite-worldgen` test suite and Java differential tests passed locally.

After integrating the corner cache with the separate column-noise cache on `oc6gpt`,
and before changing feature-placement RNG ordering,
one release run of `genbench 0 100 0 0` with `FERRITE_WORLDGEN_THREADS=8` measured
51.07 chunks/s. Per-chunk stage times were 74.25 ms noise, 42.82 ms surface,
4.27 ms carvers, 9.27 ms biomes, 13.45 ms features and 0.57 ms light; these are
summed worker times, not wall-clock latency. The packet SHA-256 was
`68702d397903f4fa3849025c425631d96f4bc0958148a0d0ce39f811fd5ace4d`.
This single run is not a controlled before/after comparison.

After the feature-placement and conifer changes, another single run with the same
arguments measured 26.43 chunks/s (noise 143.19 ms, surface 90.01 ms, carvers
8.16 ms, biomes 17.36 ms, features 25.72 ms and light 1.06 ms per chunk of
summed worker time). Its packet hash was
`cae55beb976c4f6f700a128b266e0fdd82b4933e7c073880b65b84f66a64a384`.
The user reported a game running during this period; machine load was uncontrolled,
and the changed generation output also makes this unsuitable as a performance comparison.
