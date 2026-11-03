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
