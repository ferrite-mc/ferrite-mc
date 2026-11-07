# 2026-11-07 hand-off: what is not done

State: branch `yxshadxw/plan1104`, pushed to `main` at `0a85c80`. `cargo test --workspace` (91 result
blocks ok), `cargo clippy --workspace --all-targets -- -D warnings` and `java/ferrite-api-impl`
`gradlew test` are green. Nothing below has been checked with a real client.

Order the user gave: finish the game items below first; **Rust worldgen is last**, and the
Moonrise-style optimisations are done while writing the Rust version (not as a Java patch port).
Rules: no Mojang code or assets in the repo, ask before editing crates outside the task, new
codecs get a round-trip test, bug fixes a regression test, reply to the user in Korean.

## Cannot be done here (do not retry)

- `/test` (needs the vanilla game-test framework) and `/jfr` (Ferrite is not a JVM server).
- Any check with a real Minecraft client. Everything under "unverified" needs the user.

## Unverified with a real client

Horse steering (`move_vehicle`), the mount inventory screen slot layout, weather visuals,
trident riptide/loyalty/channeling, piston animation timing, leads (`set_entity_link`), llama
caravans, wandering trader and the trading screen. Use `docs/handoff/local-verification-checklist.md`.

## Game items still open

| Item | Where | Notes |
|---|---|---|
| Wandering trader vanishing | `game/region.rs` `tick_trader_spawn`, `ferrite-entity/src/ai.rs` `lifetime` | Vanilla also despawns when it drinks invisibility / milk; not modelled. |
| Trader spawn rules | same | Ignores the `spawn_wandering_traders` rule, light level, biome and a proper ground search (first solid block with two air above, 48 blocks below the player's height+16). |
| Lead across regions | `ai.rs` `leash_pull`, `region.rs` leash id refresh | A holder in another region means the lead is neither pulled nor broken. Needs a global holder table like `game/ride.rs`. |
| Caravan details | `ai.rs` `caravan_target` | No speed penalty for long lines, no llama bump, join limit of 10 not enforced. |
| Riptide spin | `region.rs` `throw_trident`, `spin_ticks` | Flag 0x04 only reaches the thrower; other players need the player's tracked metadata. |
| Vault state | `game/vault.rs` | No vault state check or open animation. |
| Weather | `game/commands.rs` | One spell timer; thunder and rain share it. |
| Villager restock/demand | `ferrite-entity/src/villager.rs`, `region.rs` `sync_trades` | Uses and demand are copied back from the open screen; reputation discounts and xp/level-up from trading are not. |
| Horse edge cases | `ferrite-entity/src/horse.rs` | Llama strength-based chest size comes from metadata index 19; baby/foal inheritance of speed/jump is not modelled. |

## Plugin API items still open (`java/ferrite-api-impl`)

- Structures: **no entities** (plugins cannot list non-player entities at all, so
  `World#getEntities` only returns players); rotation turns only `facing`/`axis`, not stair
  shapes, rails or hinges; `place` with `BlockTransformer`/`EntityTransformer` throws.
- Block entity capture goes through `WorldView::block_entities` (`crates/ferrite-plugin`) and
  `plugins/blockdata.rs`; writing back uses `/data merge block` (`plugins/extra.rs`
  `ferriteDataMerge`). Only the default world is served.
- Other stubs: see `docs/plugin-stubs-status.md`.

## Rust worldgen (last)

Plan is in `docs/worldgen-host.md`. The vanilla host (separate JVM, `[worldgen] threads` default
8) stays until the Rust generator matches it. The user writes the Rust generator locally; keep the
plan current and do not start it unprompted. JFR profile of the host is flat (noise router 31%,
surface 22%, chunk serialisation 11%, features 9%); the Moonrise-style optimisations (cached
noise, flat caches, batched chunk status) belong in the Rust implementation.

## Pitfalls learned this round

- `ferrite_item::ItemStack` and `ferrite_protocol::packets::play::ItemStack` are different types;
  dropped items use the protocol one (`count`, `item_id`), inventories the item one. `ferrite-entity`
  needs `ferrite-item` with the `protocol` feature to encode equipment packets.
- 26.3 `PlayerCommand` ids: 0 stop sleeping, 1/2 sprint, 3/4 riding jump, 5 open inventory.
- 26.3 structure templates store palette entries as `id`/`properties` (older: `Name`/`Properties`).
- Wandering trader trade sets in the data are 5 common, 2 uncommon, 2 buying (not 1 uncommon).
- Heredocs with apostrophes break the Bash tool: write edit scripts with the Write tool.
- Java tests share one JVM: a test that registers a world must unregister it in `@AfterAll`, or
  `BootstrapTest` (expects one world) fails depending on order.
- Hex-encode anything binary sent through `HostBridge.call`; arguments are joined with control
  characters, so assert on substrings, not whole lines.
