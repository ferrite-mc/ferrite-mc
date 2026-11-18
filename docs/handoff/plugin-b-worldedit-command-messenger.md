# Handoff: Plugin B WorldEdit `CommandMap` & `Messenger`

- **Branch**: `yxshadxw/plugin-b-worldedit-command-messenger` (branched from `origin/main` at `3d7eb846`)
- **Date**: 2026-11-18

## Summary

Implemented real in-memory `CommandMap` and `Messenger` surfaces in `java/ferrite-api-impl`:

- `FerriteCommandMap` (`extends SimpleCommandMap`): backs `FerriteCommandDispatcher`, exposed via `FerriteServer#getCommandMap()` and `FerritePluginManager#commandMap` (for legacy Spigot reflection used by WorldEdit `CommandRegistration`), and registers topics in `FerriteHelpMap`.
- `FerriteMessenger` (`extends StandardMessenger`): exposed via `FerriteServer#getMessenger()`, supporting incoming/outgoing channel registration, `isIncomingChannelRegistered`/`isOutgoingChannelRegistered`, `getIncomingChannels`/`getOutgoingChannels`, `dispatchIncomingMessage`, and cleanup on `disablePlugin`.
- JUnit tests in `java/ferrite-api-impl/src/test/java/dev/ferrite/api/CommandMapAndMessengerTest.java`.

## WorldEdit Boot Result

Booting `ferrite-server --headless` with gitignored `compat/worldedit-bukkit-7.4.6-dist.jar`:
- `WorldEditPlugin` (`7.4.6+7648-00664b0`) now **enables and disables cleanly** (`plugins=1`, `plugins disabled plugins=["WorldEdit"]`).

```text
2026-11-18T09:37:18.931700Z  INFO ferrite_plugin: enabled 7.4.6+7648-00664b0 (com.sk89q.worldedit.bukkit.WorldEditPlugin) plugin=WorldEdit
2026-11-18T09:37:18.931807Z  INFO ferrite_plugin_host::host: plugin enabled plugin=WorldEdit version=7.4.6+7648-00664b0 phase="startup"
2026-11-18T09:37:18.931883Z  INFO ferrite_server::plugin_host: plugin host started plugins=1
...
2026-11-18T09:37:30.161185Z  INFO ferrite_server::plugin_host: plugins disabled plugins=["WorldEdit"]
```

## Limitations

- **Not full WorldEdit compatibility**: only headless boot/enable/disable was tested; in-game WorldEdit commands, region/block edits, and NMS world adapters (225 missing `nms` classes in static scan) were not exercised.
- **No protocol plugin-message forwarding**: `FerriteMessenger` is an in-memory `StandardMessenger` registry/dispatcher; Minecraft protocol `PluginMessage` packet forwarding between client and server is outside the current `ferrite-plugin-host` contract.
- **No real Minecraft 26.3 client** was tested.
