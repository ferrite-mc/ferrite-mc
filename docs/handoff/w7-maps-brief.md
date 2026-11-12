# W7-2: Persistent map system and complete cartography flow (brief)

Worker brief as assigned (saved verbatim in substance). Branch `w7-maps` from origin/main 236f0e2.
No PR required; push the branch and report completion. Shared host: cap build jobs, no simultaneous
heavy benchmarks, never kill others' processes. Do not spawn agents, merge/push main, tag, release.

## Problem
plugins/inventory.rs refuses openCartographyTable: no map store, ID allocator or typed map_id component.
Explorer-map Java metadata does not survive the Java-to-Rust item path. Build the model and coherent behaviour.

1. Typed map id + saved map model with vanilla 26.3 NBT shape (dimension/world identity, center, scale,
   locked, colors, decorations/tracking). Inspect local extracted vanilla data; no guessed IDs, no Mojang
   sources in git. Stable noncolliding allocation; persisted idcounts / map_<id>.dat equivalents in the
   right world data dir; IDs survive restart; no silent overwrite; concurrent allocation safe. Specify and
   test default/nether/end/runtime-world namespace behaviour.
2. Cartography menu from player interaction and plugin openCartographyTable: clone (empty map),
   scale-out (paper), lock (glass pane) with costs, result components, max-scale / already-locked
   rejection, output taking, shift-click, close/cancel. Preserve source data; never consume inputs twice.
   Unsupported state is explicit; no placeholder success.
3. Wire map_id through item encoding, slot updates, persistence and Java MapMeta/explorer-map paths so a
   returned filled map keeps identity across inventory ops and restart. Map packet sending against the real
   model; bounded terrain-colour init/update from owned chunk data if feasible; do not claim terrain
   rendering if only stored colours are sent; report remaining gaps.
4. Tests exercise the real menu and bridge: each recipe, invalid/max-scale, conservation, full
   inventory/shift-click, cancellation, restart, malformed data, allocation concurrency, runtime-world
   isolation; codec round-trips for map_id and saved map NBT. Cross-region requests go through
   ownership-safe scheduling; no global live-world lock or synchronous cross-region wait.

## Overlap contract
Own game/map*.rs, map item component representation, cartography container logic, plugins/inventory.rs
cartography path, map Java classes/tests. Others own generic ItemMeta/PDC, shulker/loot, API breadth,
networking. Keep hunks in shared files (game/mod.rs, region.rs) limited to module registration, dispatch
and persistence hooks. Preserve host operation numbers; explain new ones. Lane-specific handoff only.

## Working contract
Test-first with failing regression tests; never weaken/skip/delete tests; no fabricated data; no no-op APIs;
no Mojang assets. Linux host: RUSTUP_TOOLCHAIN=stable, Java 25, Gradle wrapper; fixture via
tools/make_fixture_world.py; build Java jars before FERRITE_JAVA_PREBUILT=1. Final: map/item/menu Java and
Rust tests, affected plugin/integration tests, cargo fmt --all --check, affected clippy -D warnings.
Deliver atomic commits, pushed branch, docs/handoff/w7-maps.md (base/head, architecture, ownership,
verified recipes/persistence, test evidence, limitations, integration steps).
