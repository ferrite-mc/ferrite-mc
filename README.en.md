[한국어](README.md) · **English**

# Ferrite

A **Minecraft Java server** written in Rust. It uses Folia-style regionized multithreading and aims to run Bukkit/Spigot/Paper/Folia plugins on an embedded JVM.

- **Target**: Minecraft Java **26.3**, protocol **777**
- **Status (2026-11-08)**: Survival and game systems have been verified with protocol bots and Rust/Java tests. The default world is now saved as Anvil files under `world/` and persists across restarts. The full Rust test suite, clippy, fmt, Java Gradle tests, and Linux/Windows CI pass. **A real 26.3 client connection is still unverified.**
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

The plugin API handles only part of Paper's `Server`/`Player`. Unhandled calls surface as `UnsupportedOperationException("Ferrite: not implemented: …")`. Large plugins such as EssentialsX and plugins that depend on NMS do not work across all paths yet.

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

Server behavior is verified by protocol bot sessions in `crates/ferrite-server/tests`. As of 2026-11-08, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `./gradlew test` in `java/ferrite-api-impl` pass. [main CI run 42398148054](https://github.com/yxshadxw/ferrite-mc/actions/runs/42398148054) also passed fmt, clippy, and the full Rust test suite on Linux and Windows. Bot tests passing does not mean real Minecraft client visuals were verified.

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

- Remaining game/plugin work: vanilla dragon strafe phases and gateway beams, plugin block-entity access across multiple worlds, and broader Paper/Bukkit and NMS compatibility.
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
