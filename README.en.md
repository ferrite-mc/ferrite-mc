[한국어](README.md) · **English**

# Ferrite

A **Minecraft Java server** written in Rust. It uses Folia-style regionized multithreading and targets the vanilla server core.

- **Target**: Minecraft Java **26.3**, protocol **777**
- **Status (2026-11-18)**: Survival/game systems have been verified with protocol bots and Rust tests. The default world is saved as Anvil files under `world/` and persists across restarts. **A real 26.3 client connection remains unverified.**
- **License**: GPL-3.0-or-later

> **한국어** — Ferrite는 Rust로 작성한 Minecraft Java 서버(26.3, 프로토콜 777)로, Folia 스타일 리전 멀티스레딩을 사용합니다. 봇으로 검증한 생존 서버를 실행하며 **실제 26.3 클라이언트로는 아직 테스트하지 않았습니다.** Mojang 코드나 에셋을 배포하지 않고, 바닐라 데이터는 공식 jar에서 로컬로 생성합니다.

## Status

| Area | Details |
|---|---|
| ✅ Working (bot-verified) | Login, encryption, signed chat, server list, region multithreading, world IO/lighting and default world saving (`world/`), blocks/items, inventory/crafting/recipe book/containers, survival/combat/mobs (biome-based natural spawning, breeding, wolf taming, daylight burning and zombie door breaking, enderman stare anger/water damage, charged creepers, sheep shearing/grass eating, spider climbing/leaps/skeleton jockeys), spawn eggs, mob/item tracking, villager trading, wandering trader spawn rules, lifetime while trading, and llama caravan lifecycle (server regression tests), block interactions such as jukeboxes, waxing, and lodestones plus their advancements, redstone/hoppers/command blocks, brewing/beacons, the Nether and portals, fluids/fire, most commands (`/give`, `/tick`, `/forceload`, `/place`, `/damage`, etc.; see [`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 Partial | Pistons (moving blocks and entity movement plus the client overlay; animation is unverified with a real client, [`docs/pistons.md`](docs/pistons.md)), End combat (below, [`docs/end-dimension.md`](docs/end-dimension.md)) |
| ⛔ Not yet | Real-client verification |

**The End**: Portal activation and dimension travel, the arrival platform, the vanilla worldgen host, void detection, the dragon boss bar/combat/crystal healing and destruction, the exit portal and gateways, perched scan/attack/flame subphases, and the purple/2400-tick gateway beams are implemented server-side. Credits, the first-return advancement, four-crystal staged resummoning, and combat/phase save and restore are also wired. Remaining work is the charge and fireball-breath approximations (speed, collision exits, cloud shape) and real-client visuals.

The Java worldgen host (`java/ferrite-worldgen-host`, `ferrite-jvm`) remains because vanilla
world generation needs it.

## Build and run

Requires: **Rust stable** (`rust-toolchain.toml` currently pins the Linux GNU toolchain; Windows/macOS should override with `RUSTUP_TOOLCHAIN=stable`), **Java 25**, **Python 3**. The Windows GUI also needs the **Microsoft Edge WebView2 Runtime** (included with Windows 10/11).

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

Server behavior is verified by protocol bot sessions in `crates/ferrite-server/tests`. On
2026-11-14, local `main` passed `cargo +stable test --workspace`,
`cargo +stable clippy --workspace --all-targets -- -D warnings`,
`cargo +stable fmt --all -- --check`, and the Java Gradle tests. Check
[GitHub Actions](https://github.com/ferrite-mc/ferrite-mc/actions) for the latest CI state.
Bot tests passing does not mean real Minecraft client visuals were verified.

## Roadmap

| Stage | Scope | Status |
|---|---|---|
| 1 | Foundation: protocol codecs, NBT, networking, regionization | Done |
| 2 | World IO, movement, blocks, chat | Done (bot-verified) |
| 3 | Regionizer benchmarks | In progress |
| 4 | Gameplay: inventory, entities, combat, crafting | In progress |
| 5 | Vanilla parity | Continue closing vanilla behavior gaps and unimplemented systems |

## Crates

| Crate | Responsibility |
|---|---|
| `ferrite-protocol`, `ferrite-net` | Packet codecs, connections, encryption, login |
| `ferrite-region` | Folia-style regionizer and schedulers |
| `ferrite-world`, `ferrite-light`, `ferrite-nbt` | Chunks, Anvil IO, lighting, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | Entities/mobs, fluids, menus, items, recipes, loot |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | Command dispatcher, game rules/permissions, `/tps`, `/profile` |
| `ferrite-jvm`, `java/ferrite-worldgen-host` | Java worldgen host |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | Vanilla data, text, math, config, console |
| `ferrite-server` | Binary: config, console, wiring |

## Legal notice

This repository **does not distribute Mojang code or assets.** Vanilla data is generated from the official server jar on the user's machine and stored in `vanilla/`, which is excluded from git. Minecraft is a trademark of Mojang Studios; this project is unofficial and not affiliated with Mojang or Microsoft.

## License

GPL-3.0-or-later ([`LICENSE`](LICENSE)). The project moved from MIT/Apache to GPLv3 to port GPL/LGPL code such as Paper, Folia, and Lithium; sources are credited in [`NOTICE.md`](NOTICE.md) and in comments in the relevant source files.

## To do

- Remaining game work: dragon charge/breath approximations, mob AI/metadata and vanilla
  behavior differences, and gameplay verification with a real client.
- Real-client verification: dimension travel/arrival platform/void death, the End boss bar/dragon/gateways, piston animation, animal interactions, and vault/trade UI on an integrated server.
- Optimization references: [server optimization mods](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods),
  [optimization modpack](https://modrinth.com/modpack/server-optimizationn),
  [optimization guide](https://github.com/YouHaveTrouble/minecraft-optimization),
  [Pumpkin Rust server](https://github.com/Pumpkin-MC/Pumpkin).
  Research notes: [`OPTIMIZATIONS.md`](docs/OPTIMIZATIONS.md).
  Research these as references for optimization design; do not assume their implementation or compatibility carries over.

## Follow-up roadmap

- **Final stage:** implement a Rust worldgen that replaces the Java host. Do this after completing the vanilla 26.3 comparison and running the existing user worldgen host in parallel. For performance research, reference worldgen optimization projects such as [C2ME](https://github.com/RelativityMC/C2ME-fabric),
  [Moonrise](https://github.com/Tuinity/Moonrise), [Noisium](https://github.com/Steveplays28/noisium),
  and C2ME's experimental [OpenCL acceleration module](https://github.com/RelativityMC/C2ME-fabric/tree/dev/26.3.0/c2me-opts-accel-opencl).
  Research techniques and bottlenecks, implement them independently in Ferrite, and verify with vanilla comparison and performance measurements. Evaluate OpenCL optionally after checking result parity with the CPU path and device compatibility.

Work other than Rust worldgen is parallelized with independent file ownership and explicit acceptance criteria. [`docs/BACKLOG.md`](docs/BACKLOG.md) is the single list of remaining work.
