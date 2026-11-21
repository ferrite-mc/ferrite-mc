# Light pool — vanilla fixture parity (2026-11-21)

Goal: prove or disprove exact parity between `LightPool` and the serial `LightEngine` on the
real vanilla fixture. Base branch `yxshadxw/lighting-pool` at `2ac1b115`; work branch
`yxshadxw/lighting-fixture`.

**Result: both engines are bit-identical on every compared block, at 1, 2, 4 and 8 workers.
No pool defect was found and `crates/ferrite-light/src/pool.rs` is unchanged.**

## Fixture

Gitignored local data, not committed:

| Data | In-worktree path | Copied from |
|---|---|---|
| Generated vanilla data | `vanilla/26.3/generated/` (95 MB) | `/home/ubuntu/orca/workspaces/ferrite-mc/lighting-pool/vanilla/26.3/generated` |
| Fixture world | `vanilla/26.3/fixture-world/` (81 MB) | `/home/ubuntu/orca/workspaces/ferrite-mc/larvacean/vanilla/26.3/fixture-world` |

Region files:
`vanilla/26.3/fixture-world/world/dimensions/minecraft/overworld/region/{r.-1.-1,r.-1.0,r.0.-1,r.0.0}.mca`

The region headers index 2,603 chunks; 677 are ready (`Status = minecraft:full` and
`isLightOn = 1`) and are parsed. The compared set is the fixture's inner 16x16 chunks,
`0..=15` in both axes, each with a fully loaded and lit 7x7 halo: **256 comparable centre
chunks**. The buildable range is sections `-4..=19` (y `-64..=319`, 384 blocks). The 3x3
neighbourhood of every submitted centre is asserted loaded with `chunk_neighbourhood`.

## Test change

`crates/ferrite-light/tests/vanilla.rs` (only source file changed):

- Fixture chunks are held in `Arc` instead of `Rc`, so `Fixture` is `Sync` and satisfies
  `LightPool::relight`'s block-view bound. Chunk parsing, readiness checks and the stored
  vanilla light comparison are unchanged.
- After the fixture scan, all 256 centres are submitted as one `LightJob` batch per worker
  count: `LightPoolConfig::new(workers, -4, 19, true)` for workers `1, 2, 4, 8`. Each
  returned `LightSnapshot` is also checked against `pool.snapshot(...)`.
- For every centre, every pooled snapshot is compared with the serial `LightEngine` built
  for that centre, for every block in `y -64..=319`, all 16x16 columns, and both
  `LightKind::Block` and `LightKind::Sky`. Any difference fails the test with the worker
  count, kind and coordinates; no mismatch counter is allowed to be non-zero.
- The existing early return still prints `SKIP vanilla light differential: ... absent` and
  passes when the fixture is missing, so CI without `vanilla/` still succeeds.

Comparison volume per run:
`256 snapshots x 4 worker counts x 2 kinds x 384 y x 256 columns = 201,326,592`
block/kind comparisons.

## Commands and results

```sh
source tools/env.sh
cargo +stable fmt --all
cargo +stable clippy -p ferrite-light --all-targets -- -D warnings
cargo +stable test -p ferrite-light
```

Clippy: clean (`CLIPPY_OK`); the only warning is the pre-existing workspace
`rust-version` manifest warning.

Full suite, all passing:

```text
unittests src/lib.rs:  3 passed; 0 failed
tests/light.rs:       14 passed; 0 failed
tests/pool.rs:         5 passed; 0 failed
tests/vanilla.rs:      1 passed; 0 failed (finished in 99.30s)
doc-tests:             0 passed; 0 failed
```

Fixture test with output (`cargo +stable test -p ferrite-light --test vanilla -- --nocapture`):

```text
vanilla light differential: 256 comparable chunk(s) with a 7x7 full + lit halo, 677 full lit chunks in 4 region file(s)
pool differential: 1 worker(s) produced 256 snapshots
pool differential: 2 worker(s) produced 256 snapshots
pool differential: 4 worker(s) produced 256 snapshots
pool differential: 8 worker(s) produced 256 snapshots
pool differential total: 201326592 block/kind comparisons, 0 mismatches
vanilla differential total: block 0/3866624 (0.00000%), sky 0/3112960 (0.00000%)
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 96.37s
```

The last two totals cover the existing serial-versus-stored-vanilla check: 3,866,624 stored
block-light values and 3,112,960 stored sky-light values, all equal. The pool totals are over
the serial engine on the same block view, so pool parity and vanilla parity are established
in the same pass.

Skip behavior was re-verified by temporarily renaming the fixture directory and running the
test binary, then restoring it:

```text
SKIP vanilla light differential: .../vanilla/26.3/fixture-world/world/dimensions/minecraft/overworld/region/r.0.0.mca absent
test result: ok. 1 passed; 0 failed; ... finished in 0.00s
```

## Mismatch and fix

None. The pool produced zero differences at all four worker counts, so there was no
pool-or-harness discrepancy to resolve. `LightEngine::recenter` (used to reuse one engine per
worker) also matches a fresh engine on this fixture. No regression test was added because no
defect was found; the fixture differential itself is the regression test.

## Limitations

- Only this fixture is covered: the four listed region files and the 256 interior centres.
  Light at the fixture margin and in unloaded chunks is not compared.
- The pool comparison range is the buildable range, y `-64..=319`. The two extra propagation
  sections the engine stores (y `-80..=-65` and `320..=335`) are captured in the snapshot but
  not compared, matching the range used by `tests/pool.rs`.
- The fixture test is a debug build of 256 full-height jobs and takes about 96-99 s here
  (12 logical CPUs, 62 GiB); this is a correctness check, not a performance measurement.
- The pool runs one batch per worker count over a fixed block view. Incremental
  `block_changed` interaction, overlapping resubmission after edits, panics and deadlock
  ordering are covered by `tests/pool.rs`, not by the fixture.
- The fixture is gitignored. CI without `vanilla/26.3/fixture-world` skips this test and
  therefore does not verify parity.
- `LightPool` is still not wired into `ferrite-server`; no server tick or client-visible
  behaviour is exercised here.
