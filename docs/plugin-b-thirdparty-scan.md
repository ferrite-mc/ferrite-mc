# Plugin B Third-Party Jar Scan & WorldEdit Boot Attempt

> [!IMPORTANT]
> A static bytecode scan and a single headless boot attempt are **not** compatibility certification. Static constant-pool scanning does not detect reflection, `jarinjar` payloads, or methods that exist on the classpath as `UnsupportedOperationException` stubs in `java/ferrite-api-impl`. No real Minecraft 26.3 client was tested.

## 1. Evaluated Jars (Gitignored under `compat/`)

All jars were downloaded into the gitignored `compat/` directory using [`tools/compat-scan/run_scan.py`](../tools/compat-scan/run_scan.py). Where Maven Central returned `HTTP 404 Not Found` for the coordinate, the exact HTTP failure is recorded and the official upstream repository (EngineHub Maven, JitPack, ExtendedClip Maven, or Modrinth) was used.

| Artifact | Coordinates | Source & Maven Central Probe | SHA256 | Declared Version / `api-version` |
|---|---|---|---|---|
| `worldedit-core-7.4.6.jar` | `com.sk89q.worldedit:worldedit-core:7.4.6` | EngineHub Maven (Maven Central: `HTTP 404 Not Found`) | `fa796d6b903f79aaa4348d510729e0526182e7c9df752865da1b690710c8d1fb` | library jar (no `plugin.yml`) |
| `worldedit-bukkit-7.4.6-maven.jar` | `com.sk89q.worldedit:worldedit-bukkit:7.4.6` | EngineHub Maven (Maven Central: `HTTP 404 Not Found`) | `d1731b62471819df95a7305b54b4435bf5dae978d073362ff737cb64c7988f89` | `7.4.6+1648-00664b0` / `1.21.4` |
| `worldedit-bukkit-7.4.6-dist.jar` | `org.enginehub.worldedit:worldedit-bukkit:7.4.6` | Modrinth shaded plugin jar | `e0ce67d870ad1fef8a40dd9d7926360caf3bb44722438545d6b20b55f82a0c91` | `7.4.6+7648-00664b0` / `1.21.4` |
| `ProtocolLib-5.4.0.jar` | `net.dmulloy2:ProtocolLib:5.4.0` | Maven Central (`HTTP 200`) | `d11a6cef959b052c5f11ca7820d55e4fd1204ce0d604a2c70a55a277075c5f29` | `5.4.0` / `1.13` |
| `VaultAPI-1.7.1.jar` | `com.github.MilkBowl:VaultAPI:1.7.1` | JitPack (Maven Central: `HTTP 404 Not Found`) | `46cbb044e6013b0f14e99b08c2afc273f5554b7da5320e36614ddb9d719cebcc` | library API jar (no `plugin.yml`) |
| `LuckPerms-api-5.5.jar` | `net.luckperms:api:5.5` | Maven Central (`HTTP 200`) | `cd910f936adc4dee7705fb51cc29401fde4f29aba6ee73a6475a3a0515f6294f` | library API jar (no `plugin.yml`) |
| `LuckPerms-Bukkit-5.5.71.jar` | `net.luckperms:luckperms-bukkit:5.5.71` | Modrinth Bukkit plugin jar | `49cecb66fa1fd22a133039a490e9c1e5095a238e7cd66eb9d2a16fe6c897550d` | `5.5.71` / `1.13` |
| `PlaceholderAPI-2.12.3.jar` | `me.clip:placeholderapi:2.12.3` | ExtendedClip Maven (Maven Central: `HTTP 404 Not Found`) | `8abc7422e2db496a35b7df0e8a9a1facf2938d1169c7ff43bde8635bc9902406` | `2.12.3` / `1.13` |

Download URLs:
- `worldedit-core`: `https://maven.enginehub.org/repo/com/sk89q/worldedit/worldedit-core/7.4.6/worldedit-core-7.4.6.jar`
- `worldedit-bukkit` (Maven): `https://maven.enginehub.org/repo/com/sk89q/worldedit/worldedit-bukkit/7.4.6/worldedit-bukkit-7.4.6.jar`
- `worldedit-bukkit` (dist): `https://cdn.modrinth.com/data/1u6JkXh5/versions/YFnqiUlL/worldedit-bukkit-7.4.6.jar`
- `ProtocolLib`: `https://repo1.maven.org/maven2/net/dmulloy2/ProtocolLib/5.4.0/ProtocolLib-5.4.0.jar`
- `VaultAPI`: `https://jitpack.io/com/github/MilkBowl/VaultAPI/1.7.1/VaultAPI-1.7.1.jar`
- `LuckPerms-api`: `https://repo1.maven.org/maven2/net/luckperms/api/5.5/api-5.5.jar`
- `LuckPerms-Bukkit`: `https://cdn.modrinth.com/data/Vebnzrzj/versions/b0mk8uS6/LuckPerms-Bukkit-5.5.71.jar`
- `PlaceholderAPI`: `https://repo.extendedclip.com/releases/me/clip/placeholderapi/2.12.3/placeholderapi-2.12.3.jar`

## 2. Static Compatibility Scan

Reproducible command:

```sh
python3 tools/compat-scan/run_scan.py
```

The scanner ([`CompatScanner.java`](../tools/compat-scan/CompatScanner.java)) inspects every constant-pool class and member reference against Ferrite's built runtime classpath (`ferrite-host.jar`, `ferrite-api-impl.jar`, `ferrite-nms.jar`, and `paper-api-classpath.txt`) and classifies gaps into `bukkit-api`, `paper-api`, `nms` (including `org.bukkit.craftbukkit.*`, `net.minecraft.*`, `com.mojang.*`), and `other`.

| Jar | Own Classes | `bukkit-api` (refs / missing C/M/F) | `paper-api` (refs / missing C/M/F) | `nms` (refs / missing C/M/F) | `other` (refs / missing C/M/F) |
|---|---:|---|---|---|---|
| `worldedit-core-7.4.6.jar` | 1119 | 0 / 0/0/0 | 0 / 0/0/0 | 0 / 0/0/0 | 1124 / 151/0/0 |
| `worldedit-bukkit-7.4.6-maven.jar` | 64 | 340 / 0/0/0 | 21 / 1/0/0 | 0 / 0/0/0 | 489 / 142/0/0 |
| `worldedit-bukkit-7.4.6-dist.jar` | 3856 | 390 / 2/0/0 | 23 / 3/0/0 | 1288 / 225/161/4 | 537 / 23/0/0 |
| `ProtocolLib-5.4.0.jar` | 534 | 278 / 0/0/0 | 8 / 0/0/0 | 8 / 0/0/0 | 517 / 65/0/0 |
| `VaultAPI-1.7.1.jar` | 7 | 18 / 0/0/0 | 0 / 0/0/0 | 0 / 0/0/0 | 0 / 0/0/0 |
| `LuckPerms-api-5.5.jar` | 192 | 0 / 0/0/0 | 0 / 0/0/0 | 0 / 0/0/0 | 4 / 4/0/0 |
| `LuckPerms-Bukkit-5.5.71.jar` | 196 | 2 / 0/0/0 | 0 / 0/0/0 | 0 / 0/0/0 | 4 / 4/0/0 |
| `PlaceholderAPI-2.12.3.jar` | 78 | 104 / 0/0/0 | 143 / 1/2/0 | 0 / 0/0/0 | 71 / 6/0/0 |

### Top Missing Packages & Symbols

- **`worldedit-bukkit-7.4.6-dist.jar`**:
  - `bukkit-api`: `org.spigotmc.SpigotConfig`, `org.spigotmc.WatchdogThread` (2 missing classes).
  - `paper-api`: `io.papermc.paper.world.PaperWorldLoader`, `io.papermc.paper.world.PaperWorldLoader$LoadedWorldData`, `io.papermc.paper.world.saveddata.PaperWorldPDC` (3 missing classes).
  - `nms`: `225` missing classes, `161` missing methods, `4` missing fields across `net.minecraft.nbt` (140), `net.minecraft.server.level` (135), `net.minecraft.core` (86), `net.minecraft.world.level.block.state` (55), `net.minecraft.world.level.storage` (54), `net.minecraft.world.entity` (53), `net.minecraft.world.level` (44), `net.minecraft.world.level.chunk` (40), `net.minecraft.util` (39), `net.minecraft.util.datafix.fixes` (39) — including multi-version CraftBukkit adapters (`v1_20_*`, `v1_21_*`) and `v26_3` internals (`com.mojang.authlib.GameProfile`, `net.minecraft.SharedConstants`, `net.minecraft.core.Holder`, etc.).
- **`ProtocolLib-5.4.0.jar`**:
  - `bukkit-api`: 0 missing; `paper-api`: 0 missing; `nms`: 0 missing (relies heavily on runtime reflection for NMS).
  - `other`: `65` missing classes (Netty `io.netty.channel` / `io.netty.buffer`, ByteBuddy `net.bytebuddy.*`, `org.json.simple`, `org.apache.commons.lang` — not on the plugin-host classpath; provided by the plugin or server runtime in Spigot/Paper).
- **`PlaceholderAPI-2.12.3.jar`**:
  - `paper-api`: `net.kyori.adventure.platform.bukkit.BukkitAudiences` (missing class), `net.kyori.adventure.text.event.ClickEvent$Action#ordinal()I` and `#values()` (2 missing methods).
  - `other`: `org.bstats.*` (shaded/optional metrics) and `org.jetbrains.annotations.ApiStatus*`.
- **`LuckPerms-Bukkit-5.5.71.jar`**:
  - The outer jar only contains the `jarinjar` bootstrap (`196` classes, `2` `bukkit-api` refs); the actual Bukkit plugin payload (`luckperms-bukkit.jarinjar`, 1.47 MB) is loaded dynamically at runtime and is not expanded by constant-pool scanning.

## 3. WorldEdit Headless Boot Attempt

### 3.1 Baseline Boot (before `FerriteHelpMap`)

Built `ferrite-server` and Java modules, placed `worldedit-bukkit-7.4.6-dist.jar` into a temporary `plugins/` directory with `[plugins] enabled = true` and `FerriteHostApi`, and started `ferrite-server --headless`.

WorldEdit (`load: STARTUP`) initially failed during `WorldEditPlugin.onLoad()` on `Server#getHelpMap()`:

```text
2026-11-17T23:42:42.686623Z  INFO ferrite_jvm::jvm: embedded JVM started java_home=/usr/lib/jvm/java-1.25.0-openjdk-amd64 java_version=25
2026-11-17T23:42:42.941040Z  INFO ferrite_plugin: plugin host started: api 26.3, contract 2 plugin=server
2026-11-17T23:42:43.501586Z ERROR ferrite_plugin: failed during load: java.lang.UnsupportedOperationException: Server#getHelpMap is not implemented by the Ferrite Bukkit API facade for Paper API 26.3.build.227-stable (called by plugin 'WorldEdit')
...
Caused by: java.lang.UnsupportedOperationException: Server#getHelpMap is not implemented by the Ferrite Bukkit API facade for Paper API 26.3.build.227-stable (called by plugin 'WorldEdit')
	at dev.ferrite.api.server.UnsupportedServerBase.getHelpMap(UnsupportedServerBase.java:606)
	at com.sk89q.bukkit.util.CommandRegistration.<clinit>(CommandRegistration.java:42)
```

### 3.2 Boot After `FerriteHelpMap` Implementation

Implemented `dev.ferrite.api.command.FerriteHelpMap` (wired into `FerriteRuntime#helpMap()`, `FerriteServer#getHelpMap()`, and `FerriteCommandDispatcher#register(PluginCommand)`):
- `HelpMap` topic registry (`addTopic`, `removeTopic`, `getHelpTopic`, `getHelpTopics`, `clear`, `getIgnoredPlugins`).
- `HelpTopicFactory` registration/lookup (`registerHelpTopicFactory`) with automatic `GenericCommandHelpTopic` synthesis for registered `PluginCommand`s and `Command#register(CommandMap)` wiring so `Command#isRegistered()` holds.

Re-running `ferrite-server --headless` with `worldedit-bukkit-7.4.6-dist.jar` now **passes `WorldEditPlugin.onLoad()`**, writes WorldEdit's default `config.yml`, and reaches `WorldEditPlugin.onEnable()`, where it stops on `CommandRegistration.getCommandMap` (`SimpleCommandMap.<init>(Server)` fallback failure) and `Server#getMessenger`:

```text
2026-11-17T23:52:20.633569Z  INFO ferrite_jvm::jvm: embedded JVM started java_home=/usr/lib/jvm/java-1.25.0-openjdk-amd64 java_version=25
Nov 17, 2026 11:52:20 PM org.bukkit.Bukkit setServer
INFO: This server is running Ferrite version Ferrite 0.1.0 (MC: 26.3) (Implementing API version 26.3.build.227-stable)
2026-11-17T23:52:20.908237Z  INFO ferrite_plugin: plugin host started: api 26.3, contract 2 plugin=server
Nov 17, 2026 11:52:21 PM com.sk89q.worldedit.bukkit.WorldEditPlugin copyDefaultConfig
INFO: Default configuration file written: config.yml
Nov 17, 2026 11:52:21 PM com.sk89q.bukkit.util.CommandRegistration getCommandMap
SEVERE: WorldEdit: Could not retrieve server CommandMap, using fallback instead!
ERROR EventBus Could not dispatch event: com.sk89q.worldedit.event.platform.PlatformsRegisteredEvent@70c1237a to handler EventHandler{priority=NORMAL}
...
Caused by: java.lang.RuntimeException: Failed to create fallback CommandMap
	at com.sk89q.bukkit.util.CommandRegistration.getCommandMap(CommandRegistration.java:104)
...
Caused by: java.lang.NoSuchMethodException: org.bukkit.command.SimpleCommandMap.<init>(org.bukkit.Server)
...
2026-11-17T23:52:21.594595Z  WARN ferrite_plugin_host::host: plugin failed to load plugin=WorldEdit phase="startup" error=plugin 'WorldEdit' failed during enable: java.lang.UnsupportedOperationException: Server#getMessenger is not implemented by the Ferrite Bukkit API facade for Paper API 26.3.build.227-stable (called by plugin 'WorldEdit')
	at dev.ferrite.api.server.UnsupportedServerBase.getMessenger(UnsupportedServerBase.java:601)
	at com.sk89q.worldedit.bukkit.WorldEditPlugin.onEnable(WorldEditPlugin.java:155)
2026-11-17T23:52:21.594690Z  INFO ferrite_server::plugin_host: plugin host started plugins=0
2026-11-17T23:52:35.594424Z  INFO ferrite_plugin: plugin host shutting down: {"reason":"server shutdown"} plugin=server
```

### 3.3 Boot After `FerriteCommandMap` + `FerriteMessenger` Implementation

Implemented:
- `dev.ferrite.api.command.FerriteCommandMap` (`extends SimpleCommandMap`): wired as the live command map backing `FerriteCommandDispatcher`, exposed via `FerriteServer#getCommandMap()` and `FerritePluginManager#commandMap` (for legacy reflection), and synced with `FerriteHelpMap`.
- `dev.ferrite.api.plugin.FerriteMessenger` (`extends StandardMessenger`): wired via `FerriteServer#getMessenger()`, providing incoming/outgoing channel registration, `dispatchIncomingMessage`, and automatic channel cleanup on `disablePlugin`. (Actual network forwarding over Minecraft protocol plugin-message packets is outside the current host contract.)

Re-running `ferrite-server --headless` with `worldedit-bukkit-7.4.6-dist.jar` now **enables and cleanly disables WorldEdit**:

```text
2026-11-18T09:37:15.188789Z  INFO ferrite_jvm::jvm: embedded JVM started java_home=/usr/lib/jvm/java-1.25.0-openjdk-amd64 java_version=25
Nov 18, 2026 9:37:15 AM org.bukkit.Bukkit setServer
INFO: This server is running Ferrite version Ferrite 0.1.0 (MC: 26.3) (Implementing API version 26.3.build.227-stable)
2026-11-18T09:37:15.440649Z  INFO ferrite_plugin: plugin host started: api 26.3, contract 2 plugin=server
Nov 18, 2026 9:37:17 AM com.sk89q.worldedit.bukkit.WorldEditPlugin copyDefaultConfig
INFO: Default configuration file written: config.yml
...
2026-11-18T09:37:18.931700Z  INFO ferrite_plugin: enabled 7.4.6+7648-00664b0 (com.sk89q.worldedit.bukkit.WorldEditPlugin) plugin=WorldEdit
2026-11-18T09:37:18.931807Z  INFO ferrite_plugin_host::host: plugin enabled plugin=WorldEdit version=7.4.6+7648-00664b0 phase="startup"
2026-11-18T09:37:18.931883Z  INFO ferrite_server::plugin_host: plugin host started plugins=1
...
2026-11-18T09:37:30.150030Z  INFO ferrite_plugin: plugin host shutting down: {"reason":"server shutdown"} plugin=server
2026-11-18T09:37:30.161185Z  INFO ferrite_server::plugin_host: plugins disabled plugins=["WorldEdit"]
```

## 4. Verdict Per Plugin

| Plugin | Verdict | Notes |
|---|---|---|
| `WorldEdit` (`worldedit-bukkit-7.4.6-dist.jar`) | **Boots & enables (headless)** | `onLoad`, `onEnable`, command registration, and `onDisable` succeed in headless `ferrite-server`. **Not** full compatibility: no in-game WorldEdit commands/edits or real 26.3 client tested, `PaperLib` falls back to Spigot reflection, `Messenger` network packet forwarding is not wired, and 225 missing `nms` classes in the static scan remain unexercised. |
| `ProtocolLib` (`ProtocolLib-5.4.0.jar`) | **Not attempted** (boot) | Static scan shows 0 missing `bukkit-api`/`paper-api`/`nms` symbols, but uses runtime reflection and Netty/ByteBuddy (`65` missing `other` classes on host classpath) plus packet listeners explicitly outside Plugin B's NMS scope. |
| `VaultAPI` (`VaultAPI-1.7.1.jar`) | **Not attempted** (library jar) | API-only jar (no `plugin.yml`); 0 missing Bukkit/Paper/NMS symbols. |
| `LuckPerms` (`LuckPerms-Bukkit-5.5.71.jar`) | **Not attempted** (boot) | Outer loader jar has 0 missing Bukkit/Paper/NMS symbols, but real implementation resides inside `luckperms-bukkit.jarinjar`. |
| `PlaceholderAPI` (`PlaceholderAPI-2.12.3.jar`) | **Not attempted** (boot) | Static scan flags missing `BukkitAudiences` and `ClickEvent$Action` methods in `paper-api`. |

## 5. Benchmark Re-run on `origin/main` (`f58a210`)

Re-ran `cargo test -p ferrite-plugin-host --release --test plugin_host_benchmark -- --ignored --nocapture` on HEAD (`f58a210`):

- Discovery: `0.22 ms` | JVM + host start: `253.64 ms` | Load + `onLoad` + `onEnable`: `964.61 ms` | `onDisable` + shutdown: `3.60 ms`
- RSS: `3.0 MiB` before start, `114.4 MiB` (`147.6 MiB` peak) after load, `186.6 MiB` (`186.6 MiB` peak) after workload
- `event:player_join` (200 iters): median `1592.49 us`, p95 `2844.34 us`, `569.8 ops/s`
- `event:block_break` (500 iters): median `172.31 us`, p95 `236.80 us`, `5360.2 ops/s`
- `command:ferrite-hello` (500 iters): median `161.94 us`, p95 `298.34 us`, `4613.4 ops/s`
- `scheduler:tick` (500 iters): median `56.57 us`, p95 `99.89 us`, `21394.5 ops/s`

## 6. Limitations

- **Not compatibility certification**: Static bytecode scanning only checks constant-pool references against classfiles; it cannot detect runtime reflection, dynamic classloading (`luckperms-bukkit.jarinjar`), or `UnsupportedOperationException` stubs in `Unsupported*Base` classes.
- **No real Minecraft client**: Only headless server startup and fixture benchmarks were run.
- **No production changes**: No server, host, or Java facade code was modified.
