# Handoff: WorldEdit In-Game Smoke (`yxshadxw/plugin-b-worldedit-ingame`)

- **Branch**: `yxshadxw/plugin-b-worldedit-ingame` (branched from `origin/main` at `acc7338f`)
- **Scope**: Move WorldEdit (`compat/worldedit-bukkit-7.4.6-dist.jar`, gitignored) from headless boot to real console and player protocol command execution, and identify the exact boundary for an in-game region edit (`//set minecraft:stone`).

## What Changed

1. **Dynamic plugin `CommandMap` fallback in `ferrite-server`** ([crates/ferrite-server/src/game/mod.rs](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/crates/ferrite-server/src/game/mod.rs))
   - When `command_tree.execute(...)` fails for `ServerboundPlay::ChatCommand` or `GameMessage::Console`, fall back to `plugins.run_plugin_command(...)` (instead of only checking `plugin.yml` commands).
   - Preserves double-slash commands (`//pos1`, `//pos2`, `//set`) by prepending a single `/` to `ServerboundChatCommand.command`.
   - Fixed `PluginHooks::player_command` in [crates/ferrite-server/src/plugin_host/hooks.rs](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/crates/ferrite-server/src/plugin_host/hooks.rs) from `Ok(false)` to `Ok(true)` (`false` means cancelled in `PluginHooks::player_command`, which was dropping all player commands when plugins were enabled).
   - Synced server operator status (`set_player_op`) on join and exposed `"op"` in `player_state_get` ([crates/ferrite-server/src/plugin_host/state.rs](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/crates/ferrite-server/src/plugin_host/state.rs)).

2. **Bukkit/Paper player command feedback & locale** ([FerritePlayer.java](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/java/ferrite-api-impl/src/main/java/dev/ferrite/api/entity/FerritePlayer.java), [FerriteHostApi.java](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/java/ferrite-api-impl/src/main/java/dev/ferrite/api/FerriteHostApi.java))
   - Implemented `FerritePlayer#sendMessage(Component)` using `LegacyComponentSerializer.legacySection().serialize(message)` so Adventure `Component` responses from WorldEdit reach `state.messages()` and `SystemChat`.
   - Implemented `FerritePlayer#locale()` (`Locale.US`) and `FerritePlayer#getLocale()` (`"en_us"`), which `BukkitPlayer#print` calls when formatting localized messages.
   - Synced `player.setOp(...)` from `Bootstrap.playerStateGet` in `FerriteHostApi#runCommand`.

3. **Tests**
   - Added [crates/ferrite-server/tests/worldedit_ingame_smoke.rs](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/crates/ferrite-server/tests/worldedit_ingame_smoke.rs) (skips cleanly if `compat/worldedit-bukkit-7.4.6-dist.jar` is absent):
     - Executes `/we version` via server console and asserts `WorldEdit version 7.4.6`.
     - Joins `WorldEditBot`, OPs it, and sends `//pos1 0,65,0` and `//pos2 1,65,1` via `ServerboundChatCommand`, verifying `SystemChat` replies `"First position set to (0, 65, 0)"` and `"Second position set to (1, 65, 1) (4)"`.
     - Sends `//set minecraft:stone` and asserts the reproducible NMS adapter boundary below.
   - Added `CommandTest#doubleSlashCommandAdventureMessageAndLocale` in [CommandTest.java](file:///home/ubuntu/orca/workspaces/ferrite-mc/plugin-b-bench/java/ferrite-api-impl/src/test/java/dev/ferrite/api/CommandTest.java).

## Exact Boundary for `//set minecraft:stone`

`//set minecraft:stone` reaches `DefaultBlockParser.parseLogic` -> `BlockType.computeProperties(BlockType)` and fails with:
```text
java.lang.NullPointerException: Cannot invoke "java.util.Map.keySet()" because "propertiesMap" is null
	at com.sk89q.worldedit.world.block.BlockType.computeProperties(BlockType.java:52)
```
Root cause (verified with `javap` on `compat/worldedit-bukkit-7.4.6-dist.jar`):
- `BukkitBlockRegistry#getProperties(BlockType)` returns `null` when `WorldEditPlugin.getInstance().getBukkitImplAdapter()` is `null`.
- `BukkitImplLoader#loadAdapter()` fails to load `com.sk89q.worldedit.bukkit.adapter.impl.v26_3.PaperweightAdapter` because its constructor immediately casts `Bukkit.getServer()` to `org.bukkit.craftbukkit.CraftServer`, reflects on `CraftServer.class.getDeclaredField("worlds")`, calls `net.minecraft.SharedConstants.getCurrentVersion().dataVersion().version()`, and depends on full CraftBukkit/NMS internals (`MinecraftServer`, `DedicatedServer`, `ServerLevel`, `LevelChunk`, `LevelChunkSection`, `PalettedContainer`, `PaperweightDataConverters`, `CraftBlockData`, etc.).
- Per project scope, broad CraftBukkit/NMS adapter implementation was not invented; the refusal/boundary is preserved and tested.
