[한국어](README.md) · **English**

# Ferrite

A **Minecraft Java server** written in Rust. It uses Folia-style regionized multithreading and aims to run Bukkit/Spigot/Paper/Folia plugins on an embedded JVM.

- **Target**: Minecraft Java **26.3**, protocol **777**
- **Status (2026-11-12)**: Survival/game systems and part of the plugin API have been verified with protocol bots and Rust/Java tests. The default world is saved as Anvil files under `world/` and persists across restarts. **A real 26.3 client connection and broad plugin ecosystem compatibility remain unverified.**
- **License**: GPL-3.0-or-later

> **한국어** — Ferrite는 Rust로 작성한 Minecraft Java 서버(26.3, 프로토콜 777)로, Folia 스타일 리전 멀티스레딩과 Bukkit/Paper 플러그인을 위한 내장 JVM을 갖추고 있습니다. 봇으로 검증한 생존 서버를 실행하며 **실제 26.3 클라이언트로는 아직 테스트하지 않았습니다.** Mojang 코드나 에셋을 배포하지 않고, 바닐라 데이터는 공식 jar에서 로컬로 생성합니다.

## Status

| Area | Details |
|---|---|
| ✅ Working (bot-verified) | Login, encryption, signed chat, server list, region multithreading, world IO/lighting and default world saving (`world/`), blocks/items, inventory/crafting/recipe book/containers, survival/combat/mobs (biome-based natural spawning, breeding, wolf taming), spawn eggs, mob/item tracking, villager trading, wandering trader spawn rules, lifetime while trading, and llama caravan lifecycle (server regression tests), block interactions such as jukeboxes, waxing, and lodestones plus their advancements, redstone/hoppers/command blocks, brewing/beacons, the Nether and portals, fluids/fire, most commands (`/give`, `/tick`, `/forceload`, `/place`, `/damage`, etc.; see [`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 Partial | Pistons (moving blocks and entity movement plus the client overlay; animation is unverified with a real client, [`docs/pistons.md`](docs/pistons.md)), End combat (below, [`docs/end-dimension.md`](docs/end-dimension.md)) |
| 🔧 In progress | Plugin compatibility (part of the Paper/Bukkit API and part of NMS; unsupported calls fail explicitly) |
| ⛔ Not yet | Real-client verification |

**The End**: Portal activation and dimension travel, the arrival platform, the vanilla worldgen host, void detection, the dragon boss bar/combat/crystal healing and destruction, and the exit portal and gateways are implemented. Simplified dragon strafing/dashing/breath/fireball behavior, four-crystal staged resummoning, and combat/phase save and restore are also in. Credits and full vanilla combat behavior are missing, and real-client visuals have not been checked.

The plugin API handles only part of Paper's `Server`/`Player`. Unhandled calls surface as `UnsupportedOperationException("Ferrite: not implemented: …")` or an explicit `NoSuchMethodError`. Large plugins such as EssentialsX and plugins that depend on NMS do not work across all paths yet.

### Plugin API status (code cross-check: 2026-11-14)

This status compares the Java implementations, Rust host dispatchers, and regression tests on
`main`. Hostless JUnit probe failures are not equivalent to server-side missing implementations:
the report distinguishes real host paths from deliberate rejection paths. Item-level detail is in
[`docs/plugin-api-completion-directive.md`](docs/plugin-api-completion-directive.md) §10.

**Newly implemented since 2026-11-12** (details in the directive §10.1 and §10.5)

- Enchanting table follows vanilla: bookshelf count, per-player seed (saved with player data),
  button cost, weighted choice with exclusions, clue and material cost. Random numbers differ from
  vanilla's source, so the same seed does not necessarily produce the same buttons.
- Anvil: same-item merge bonus (12% of max durability), rename-only cost, creative cap.
- Gravity blocks and fluids: `dragon_egg`, concrete powder solidification, landing in water mid-fall,
  lava spread delay; pressure plates sense every entity with vanilla re-trigger delay and weighted output.
- Chunks: clamp of the client's requested view distance; out-of-view chunks saved and unloaded at once.
- Plugin loading: one failing plugin no longer stops the others.
- `Player#startRiptideAttack`, `sleep`, `fireworkBoost`, `canCriticalAttack`, `getSentChunkKeys`,
  `launchProjectile` for trident, snowball, egg, pearl, bottle, potion, wind charge, fireball,
  firework, wither skull, shulker bullet, spectral arrow, and fishing bobber; eye of ender via item use.
- `World#createExplosion` (all overloads), `spawn`/`createEntity`/`addEntity`, `getChunksAtAsync`,
  `DATAPACK_DISCOVERY` bootstrap, `help.yml`, recipe add/remove (menu only, not the client recipe book).
- Dragon battle `resetCrystals` and `spawnNewGateway(Position)`.

**Remaining gaps** (directive §10.3)

- Firework-specific handle methods, firework color/shape display, approximate firework acceleration.
- POI search scans loaded chunks in every dimension. With no AI that claims POI tickets,
  `IS_OCCUPIED` is always empty and `HAS_SPACE` filters by type. `locateNearestBiome` uses the
  host's fixed y=64 and 8-block-step approximation and ignores the interval arguments; structure
  pieces (`getPieces`) are empty. `getWorldPath` is default-world only; `getFluidData`, `spawnAt`,
  and `vanillaBiomeProvider` are absent from the compiled Paper API.
- Runtime worlds accept `NORMAL`, `NETHER`, and `THE_END`, and persist the environment and seed
  in `level.dat` across unload/reopen. Custom `WorldCreator#generator`, `generateStructures(false)`,
  non-`NORMAL` `WorldType`, `biomeProvider`, and `CUSTOM` are rejected explicitly.
- Cooking, smithing, and stonecutting recipes cannot be added at runtime; no result metadata,
  `ExactChoice`, or client recipe-book display.
- Some NMS menus are deliberately refused (hopper, dispenser, furnace, brewing stand, and others).
- `loadData` does not restore health, food, or experience.
- Real 26.3 client visuals and large third-party plugin behavior remain unverified.

**Earlier items (still valid)**

**Implemented and covered by tests**

- Runtime `Server#createWorld`/`unloadWorld`, persistence, unload/reopen, and player rejoin.
- Worldgen: POI search in all dimensions, structure starts and `StructureType`/deprecated
  `locateNearestStructure` overloads, `setBiome` in the Nether and End, server-wide game rules,
  and runtime-world environment persistence.
- `Player#breakBlock`, player-permission `Player#chat("/command")`, `Player#performCommand`,
  slot-break/hurt/pickup animations.
- 41 registry views, `PotionEffectType`, `playEffect`, and all six player equipment slots.
  Only `GameRule` and `MemoryKey` registries are rejected because generated data lacks the
  required value-type/default metadata.
- Shulker-box menus (27 slots, no nesting, contents spill on break), anvil enchantment merging,
  `World#spawnArrow`, and `World#getInfiniburn`.
- Structure block/block-entity capture and placement, transforms, and runtime-world non-player
  entity snapshots with `EntityTransformer`; `EntityFactory#createEntitySnapshot` has an SNBT
  host path.

**Explicitly unsupported or constrained in code**

- Projectile types unknown to the host. Potion callback content mutation is also unsupported.
  (Trident launches are now implemented; see above.)
- `Player#openCartographyTable`: there is no map store, `idcounts` allocator, or typed map-data
  component, so the table is refused rather than faked.
- Some `MapCanvas` methods, styled/translated map cursor captions, and hidden cursor transfer.
- Entity snapshots for structure capture provide NBT only for runtime worlds; default Overworld,
  Nether, and End non-player entities are excluded from NBT-based capture/recreation.
- Unsupported item metadata/PDC container types, non-sign block-entity updates, some `BlockData`
  and chunk-snapshot methods, and NMS/Unsafe APIs.

See [`docs/plugin-api-stubs.md`](docs/plugin-api-stubs.md), [`docs/plugin-stubs-status.md`](docs/plugin-stubs-status.md),
and [`docs/plugin-api-audit.md`](docs/plugin-api-audit.md) for method-level ownership, evidence,
and tests. Real client visuals and booting large third-party plugins remain unverified.

## Build and run

Requires: **Rust stable** (`rust-toolchain.toml` pins Windows MSVC; Linux/macOS use `RUSTUP_TOOLCHAIN=stable`), **Java 25**, **Python 3**. The Windows GUI also needs the **Microsoft Edge WebView2 Runtime** (included with Windows 10/11).

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # generate vanilla data (first run only, network + Java 25)
cargo run --release -p ferrite-server      # local server in offline mode
```

On first run it creates `eula.txt` and exits. Read the [Minecraft EULA](https://aka.ms/MinecraftEULA), change it to `eula=true` yourself, then run again. Ferrite does not accept it on your behalf.

`vanilla/` is not committed to git. Do not guess packet IDs, blocks, or registry values; read the reports under `vanilla/<version>/generated/`. Config keys are in [`docs/CONFIG.md`](docs/CONFIG.md).

The default world directory is `world/` in the server working directory. Chunks stay in an in-memory cache during play, are saved every 60 seconds, and are written in Anvil format on unload and graceful shutdown. Entities are saved as per-loaded-chunk snapshots, and online player state is saved every 60 seconds as well. Changes made within the autosave interval before a hard kill may be lost. On Windows, running `ferrite-server.exe` directly opens a Tauri 2 GUI for server start/stop, command input, and log viewing. Keep the executable in a Ferrite working directory with `vanilla/26.3/generated/` prepared. Add `--headless` to use the terminal console, and note that Linux always runs headless and reads stdin. Read the EULA before the first run and edit `eula.txt` yourself only if you agree. See [`docs/windows-console.md`](docs/windows-console.md) for the GUI behavior and limitations.

## Verification

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Server behavior is verified by protocol bot sessions in `crates/ferrite-server/tests`. The existing
CI run [42398148054](https://github.com/yxshadxw/ferrite-mc/actions/runs/42398148054) passed fmt,
clippy, and the full Rust test suite on Linux and Windows. Bot tests passing does not mean real
Minecraft client visuals were verified.

## Roadmap

| Stage | Scope | Status |
|---|---|---|
| 1 | Foundation: protocol codecs, NBT, networking, regionization | Done |
| 2 | World IO, movement, blocks, chat | Done (bot-verified) |
| 3 | Regionizer benchmarks | In progress |
| 4 | Plugin bridge (Bukkit events, commands, scheduler) | In progress, top priority |
| 5 | Gameplay: inventory, entities, combat, crafting | In progress |
| 6 | Paper feature parity | Planned |
| 7 | Broader plugin ecosystem compatibility | Planned |

## Crates

| Crate | Responsibility |
|---|---|
| `ferrite-protocol`, `ferrite-net` | Packet codecs, connections, encryption, login |
| `ferrite-region` | Folia-style regionizer and schedulers |
| `ferrite-world`, `ferrite-light`, `ferrite-nbt` | Chunks, Anvil IO, lighting, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | Entities/mobs, fluids, menus, items, recipes, loot |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | Command dispatcher, game rules/permissions, `/tps`, `/profile` |
| `ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm` | Plugin loading, `plugin.yml`, embedded JVM |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | Vanilla data, text, math, config, console |
| `ferrite-server` | Binary: config, console, wiring |

## Legal notice

This repository **does not distribute Mojang code or assets.** Vanilla data is generated from the official server jar on the user's machine and stored in `vanilla/`, which is excluded from git. Minecraft is a trademark of Mojang Studios; this project is unofficial and not affiliated with Mojang or Microsoft.

## License

GPL-3.0-or-later ([`LICENSE`](LICENSE)). The project moved from MIT/Apache to GPLv3 to port GPL/LGPL code such as Paper, Folia, and Lithium; sources are credited in [`NOTICE.md`](NOTICE.md) and in comments in the relevant source files.

## To do

- Remaining game/plugin work: vanilla dragon phase timing and gateway beams, non-player entity NBT
  boundaries for the default/Nether/End worlds, cartography/tridents, and broader Paper/Bukkit/NMS compatibility.
- Real-client verification: dimension travel/arrival platform/void death, the End boss bar/dragon/gateways, piston animation, animal interactions, vault/trade UI, and plugin behavior on an integrated server.
- Optimization references: [server optimization mods](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods),
  [optimization modpack](https://modrinth.com/modpack/server-optimizationn),
  [optimization guide](https://github.com/YouHaveTrouble/minecraft-optimization),
  [Pumpkin Rust server](https://github.com/Pumpkin-MC/Pumpkin),
  [Pumpkin PatchBukkit bridge](https://github.com/Pumpkin-MC/PatchBukkit).
  Research notes: [`OPTIMIZATIONS.md`](docs/OPTIMIZATIONS.md).
  Research these as references for optimization and bridge design; do not assume their implementation or compatibility carries over.

## Follow-up roadmap

- Implement Slime Region Format world load/save/switch and plugin integration, referencing
  [Advanced Slime World Manager (ASWM)](https://github.com/Paul19988/Advanced-Slime-World-Manager) /
  [Advanced Slime Paper](https://github.com/InfernalSuite/AdvancedSlimePaper).
- **Final stage:** implement a Rust worldgen that replaces the Java host. Do this after completing the vanilla 26.3 comparison and running the existing user worldgen host in parallel. For performance research, reference worldgen optimization projects such as [C2ME](https://github.com/RelativityMC/C2ME-fabric),
  [Moonrise](https://github.com/Tuinity/Moonrise), [Noisium](https://github.com/Steveplays28/noisium),
  and C2ME's experimental [OpenCL acceleration module](https://github.com/RelativityMC/C2ME-fabric/tree/dev/26.3.0/c2me-opts-accel-opencl).
  Research techniques and bottlenecks, implement them independently in Ferrite, and verify with vanilla comparison and performance measurements. Evaluate OpenCL optionally after checking result parity with the CPU path and device compatibility.

Work other than Rust worldgen is parallelized with independent file ownership and explicit acceptance criteria. [`docs/BACKLOG.md`](docs/BACKLOG.md) is the single list of remaining work.
