# Local plugin API queue

> **2026-11-17 — historical.** This queue belonged to the removed bridge. It is not the Plugin B
> work queue; see [`../BACKLOG.md`](../BACKLOG.md) §3 for current open work.

Finite workstream owned by the local Codex worker. Coordinator merges all PRs; no local self-merge or releases. Remote lanes own commands/datapacks/Player/entities/metadata/attributes, maps/AI/vehicles, ItemMeta/PDC/books/loot/world physics, and networking/performance/release. Do not edit those paths.

**Status 2026-11-15: all entries below are delivered and merged into `main`; no pending item remains.**

| Priority/state | Concrete API gap and evidence | Source/ownership | Delivery/validation |
|---|---|---|---|
| Delivered | Registry properties: Trim, JukeboxSong, Art, instruments and Cat/Frog/Art OldEnum | Local registry-only lane; extracted data and installed Paper API | The registry-property and legacy-identity paths are in `main`; `FerriteRegistryValueProperties` and `FerriteLegacyRegistryValues` serve them. |
| Delivered | Real `Registry.GAME_RULE`: all 59 typed definitions, official defaults/features, known aliases | Local extractor/data/value helpers; narrow RegistryAccess/InternalBridge hunks and registry pre-initialization in Bootstrap. No world values/get-set | `FerriteGameRuleData` and `gameRuleRegistry` publish the 59 rules; `FerriteRegistryBackingTest.gameRulesAndMemoryKeysResolveWithoutInventingValues` checks count/lookup/unknown key. |
| Delivered | `DamageType.getDamageScaling/getDamageEffect/getDeathMessageType/getExhaustion` | Data-backed read-only registry properties from generated `damage_type/*.json` | `FerriteDamageData` feeds `FerriteDamageType`; `DamageTypeRegistryTest` checks `FALL` scaling, exhaustion, death message and effect against the generated data. |
| Delivered | Sound and Villager Profession/Type `OldEnum` ordinal/compareTo | Local typed/legacy registry lane; built-in registry report IDs | `FerriteLegacyRegistryValues` implements ordinal/compareTo and default methods; `LegacyRegistryValuesTest` checks report IDs, comparisons and translation keys. |
| Delivered | `Registry.MEMORY_MODULE_TYPE`: the 45 typed `MemoryKey` constants Bukkit exposes are published; the 70 internal modules with no Bukkit value type are excluded rather than fabricated | Installed Paper API + generated registry; no entity memory mutation claim | `FerriteRegistryBackingTest` checks both the `RegistryAccess` and Paper static paths for count, `HOME`, and unknown-key null. |

The queue is closed. After these finite items, audit data-backed Server/Bukkit utilities for a concrete unowned gap with actual API callers/tests. Already-correct behavior is not a task; unsupported methods without canonical data remain documented. Do not expand into new subsystems.

Each feature: failing API regression, complete real implementation, focused/full applicable Java test/build on Java 25, diff/security/correctness review, atomic commit, own pushed branch/PR, update this queue. CI failures are repaired on the owning PR without weakening tests. No real-client/world behavior claims from registry tests.
