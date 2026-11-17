# Handoff: Plugin B WorldEdit `onLoad` Unblock (`FerriteHelpMap`)

- **Branch**: `yxshadxw/plugin-b-worldedit-boot` (branched from `origin/main` at `ee4d7cf`)
- **Date**: 2026-11-17

## Summary

Implemented a real in-memory `org.bukkit.help.HelpMap` (`dev.ferrite.api.command.FerriteHelpMap`) in `java/ferrite-api-impl` to unblock `WorldEditPlugin.onLoad()` past `Server#getHelpMap`:

- `FerriteHelpMap`: `addTopic`, `removeTopic`, `getHelpTopic`, `getHelpTopics`, `clear`, `registerHelpTopicFactory`, `getIgnoredPlugins`, and `createTopic`/`registerCommandTopic` using `GenericCommandHelpTopic` by default.
- `FerriteCommandDispatcher`: registers each `PluginCommand` with `SimpleCommandMap` (`command.register(commandMap)`) so `Command#isRegistered()` is true (required by `GenericCommandHelpTopic#canSee`) and notifies `runtime.helpMap().registerCommandTopic(command)`.
- `FerriteServer#getHelpMap()` returns `runtime.helpMap()`.
- JUnit tests in `java/ferrite-api-impl/src/test/java/dev/ferrite/api/HelpMapTest.java`.

## WorldEdit Boot Result

Booting `ferrite-server --headless` with gitignored `compat/worldedit-bukkit-7.4.6-dist.jar`:
- `WorldEditPlugin.onLoad()` now **succeeds** and writes WorldEdit's default `config.yml`.
- `WorldEditPlugin.onEnable()` reaches `CommandRegistration.getCommandMap` (fails to create fallback `SimpleCommandMap(Server)`) and then fails on `Server#getMessenger` (`UnsupportedServerBase.getMessenger`).
- Stopped here as `Messenger` (plugin messaging channels) and WorldEdit's broader `CommandMap`/NMS surface are outside this task's scope.

```text
Nov 17, 2026 11:52:21 PM com.sk89q.worldedit.bukkit.WorldEditPlugin copyDefaultConfig
INFO: Default configuration file written: config.yml
Nov 17, 2026 11:52:21 PM com.sk89q.bukkit.util.CommandRegistration getCommandMap
SEVERE: WorldEdit: Could not retrieve server CommandMap, using fallback instead!
...
2026-11-17T23:52:21.594595Z  WARN ferrite_plugin_host::host: plugin failed to load plugin=WorldEdit phase="startup" error=plugin 'WorldEdit' failed during enable: java.lang.UnsupportedOperationException: Server#getMessenger is not implemented by the Ferrite Bukkit API facade for Paper API 26.3.build.227-stable (called by plugin 'WorldEdit')
```

## Limitations

- WorldEdit still does not enable (`Server#getCommandMap`, `Server#getMessenger`, and 225 missing `nms` classes remain).
- No real Minecraft 26.3 client was tested.
