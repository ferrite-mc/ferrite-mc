# P1 plugin host coverage, tier 1 — 2026-11-11

Branch `plugin-host-coverage`, cut from `main` 41f1579. Not pushed, not merged.

## Finding that shapes the list

The brief's premise ("874 methods, ~290 Rust handlers, so ~650 unanswered") overstates the gap.
Java routing already exists for every generated stub (`PlayerStubs`, `ServerStubs`, ...,
913 distinct owner/method pairs). A stub only reaches the host if no hand-written Java class
overrides it. Re-counting by what actually happens to each call:

| Bucket (913 generated-stub methods) | Before | After |
|---|---:|---:|
| Handled by a Rust handler string | 173 | 174 |
| Overridden in Java (never reaches the host) | 583 | 586 |
| Plain property get/set answered by `HostBridge` local state/defaults | 71 | 71 |
| **Reach the host with no handler (throw "not implemented")** | **86** | **82** |

Plus hand-written classes that call `HostBridge` directly: `FerriteEnchantment` had 10 host calls
with no handler, now 4. Counting method is a regex script (stub call sites vs. Rust string
literals vs. Java override name+arity), so treat it as +-a few; resource-pack methods are
matched by prefix in Rust and still show up in the 82.

Several of the brief's named methods were already fine: `ban(...)` (Java, profile ban list),
`kickPlayer`/`kick` (Java -> `Native.playerVisual`), `clearRecipes` (Java `FerriteRecipes`),
`canBreatheUnderwater` (Java), `broadcast(...)` family (Java, `FerriteServer`). `chat` and
`breakBlock` are genuinely unhandled and stay so (below).

## Ranked list of the truly unhandled (Player, 17 left of 21)

Rank = how likely a normal plugin calls it, then how cleanly vanilla can answer it.

1. `breakBlock(Block)` — common; **not done**: needs block hardness/unbreakable data, gamemode
   rules and a `BlockBreakEvent`; a bare `setblock ... destroy` would answer `true` for air and
   bedrock, which is an invented value.
2. `chat(String)` — common; **not done**: `execute as <p> run` runs with console permission, so
   mapping `/command` would let a non-op run privileged commands. Needs a player-chat injection
   command in the game loop.
3. `banIp(...)` x3 — **done**.
4. `broadcastSlotBreak(EquipmentSlot,Collection)` — **done**; the 1-arg form needs the tracker
   set, which the host does not expose (left).
5. `broadcastHurtAnimation(Collection)`, `playPickupItemAnimation(Item,int)` — need a new
   `PluginPacket` variant in `ferrite-plugin` that addresses another entity id; `HurtAnimation`
   today always uses the recipient's own id. Left.
6. `setResourcePack/addResourcePack(UUID,...)` overloads — handled by prefix in Rust, shown as
   unhandled by the counter only.
7. `spawnParticle(... RandomizationType)`, `spawnAt`, `copy`, `sleep`, `fireworkBoost`,
   `startRiptideAttack`, `canCriticalAttack`, `loadData`, `postEffects` — rarely used, no
   vanilla command mapping.

Enchantment (hand-written class): 6 of 10 host calls closed; `isTradeable`/`isDiscoverable`
(unverified Paper tag mapping) and the two `getDamageIncrease` overloads stay unimplemented.

## Implemented, one family per module

1. **Equipment-break broadcast** — `plugins/broadcast.rs`. `Player#broadcastSlotBreak(EquipmentSlot,Collection)`
   -> one `EntityEvent` (vanilla statuses 47..52) per listed viewer for the subject's entity;
   a malformed viewer list sends nothing. Wired in `plugins.rs` before the network handlers.
   Tests: `plugins::broadcast::tests` (2).
2. **IP bans** — `PlayerMisc.banIp(...)` x3 (Date/Instant/Duration) bans `getAddress()` through
   the server `IpBanList` (which already forwards to `/ban-ip`), optionally kicks.
   Tests: `PlayerBanIpTest` (3, through the real fixture player and `HostBridge` transport).
3. **Enchantment data** — `getWeight`, `getAnvilCost`, `getMin/MaxModifiedCost`, `conflictsWith`
   (vanilla `areCompatible`: an enchantment conflicts with itself), `canEnchantItem`, answered
   from the data-pack definition (`FerriteEnchantmentData.Cost`). Side effect fixed:
   `enchant_randomly` relied on `canEnchantItem` throwing to mean "allow"; it now follows
   vanilla (a plain book accepts anything) in `FerriteLoot`.
   Tests: `FerriteEnchantmentCostsTest` (4); `FerriteLootTableTest` still passes.

## Baseline fix (separate commit cfd3623, drop it if main is repaired first)

`main` 41f1579 did not compile: E0506 at `game/mod.rs`, the runtime-world context closure
borrowed `command_tree` while the plugin-hook attach reassigns it. This lane could not ask the
coordinator (the new brief carried no dispatch ids, `orca orchestration ask` refused), so I made
the smallest fix: the closure reads a shared slot that the attach path refreshes. It does not
change command-tree behaviour.

## Not done / constraints kept

No unload, travel, seed or command-tree logic touched (apart from the compile fix above). No
`#[ignore]`, no weakened assertions, no mocks replacing the fixture. Unanswerable calls still
throw the documented not-implemented error.

## Verification (raw)

```
cargo fmt --all -- --check                               FMT_EXIT=0
cargo clippy --workspace --all-targets -- -D warnings    CLIPPY_EXIT=0
cargo test --workspace                                   EXIT=0   (1600 passed, 0 failed; ferrite-server lib 728 passed in 157.71s)
gradle --offline cleanTest test  (java/ferrite-api-impl)  EXIT=0 x10 consecutive runs
```

Caveat: during editing, three non-clean `gradle test` runs failed once each
(`FerriteLootTableTest` — a real regression, fixed above; then `BootstrapTest` and
`ApiSurfaceTest`, not reproducible). After the fix the unchanged baseline passed 4/4 and this
branch passed 10/10 with `cleanTest`. The suite shares static bootstrap state, so an
order-dependent flake is possible; I could not reproduce it.

Raw cargo logs: scratchpad `p1-test.log`, `p1-clippy.log` (not committed).
