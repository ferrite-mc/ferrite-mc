# Hand-off 2026-11-06 (branch yxshadxw/plan1104)

Last pushed commit: 7de43d8. Everything after it (15 commits) is local and **not pushed**; push to main only when the user says so.
Workspace tests, clippy and fmt were green before the final tracker commit; `ferrite-entity` (93) and `ferrite-server --lib` (538) were green after it.

## Done this session
- Chunk stream: tracking lock held briefly, prefetch only new chunks (`prefetched`/`prefetch_wait`), O(1) pending count in the Anvil cache.
- Right-click no longer attacks.
- Smithing: `minecraft:trim` component, SmithingTrim, `recipe_crafted` for smithing.
- Recipe book: `place_recipe` (packet 39), ghost recipe (64), unlock check; `recipes.json` now carries a `display` hex (regenerate with `tools/extractor/RecipeExtractor.java`).
- Entity tracker: viewer grid (cells of 64 blocks, used from 16 viewers) in `ferrite-entity/src/tracker.rs`, with a test.
- `examples/bots.rs`: `BOTS_ADDR` external mode, keep-alive replies, join report.

## Measured (200 bots, noisy machine)
superflat avg 6.6 ms / p99 32 ms; fixture avg 1.15 ms / p99 6.9 ms. Results vary 2-3x with other load.

## Not done
- Paper comparison: bots get kicked for "moved too quickly" on Paper (unresolved); abandoned at the user's request.
- Worldgen scheduler replacement (C2ME-style; plateau ~32 chunks/s is the vanilla chunk system). 1-2 days. Do not rewrite worldgen (license); only the scheduling/hosting approach.
- Breeding and wolf taming are implemented (`ferrite-entity` `breed.rs`/`tame.rs`, right-click in `region.rs` `resolve_uses`) and unit-tested, but not tried with a real client. Hearts/smoke (entity events 18/7/6), baby half-size hitbox and tamed-wolf breeding with meat are in. Left out: wolves that fight (anger), natural wolf spawning (the spawner has no biome rules at all yet), other tameable kinds (cats, horses, parrots), `bred_all_animals` for kinds not modelled.
- `item_used_on_block`, `seen_recipe`, shutdown pending-teleport completion, weighted-fair region scheduling, region redstone-time/global-time copy per Folia.
- Fixture join-storm tail (800 bots: p99 join ~116-235 ms, max 600-800 ms). The cache eviction min-scan is now batched (LRU eighth per pass); not re-measured.
- Weighted-fair region scheduling: the scheduler is already earliest-deadline-first with lag capped (`ferrite-region` module docs); only the TPS jitter of merging/splitting regions under saturation remains. Same as Folia, so left alone.
- `item_used_on_block`: needs waxing, lodestone, jukebox, respawn anchor and vault mechanics first.
