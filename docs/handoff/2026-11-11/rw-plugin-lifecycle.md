# Runtime-world plugin lifecycle: handoff

Baseline: `b6a7b3e`. Task `task_42e8678d0508`. Result: **all three CI failures fixed**; `plugin_lifecycle` passes 4/4.

## Root cause

`dceb03b` ("wire runtime world lifecycle") made plugin attachment asynchronous. `main.rs` now starts
the game loop with `WorldSettings.plugins = None` (the bridge needs `world.sender`) and attaches the
hooks afterwards through `GameMessage::SetPluginHooks`.

The game loop builds its command tree once at startup in `game/mod.rs`, reading
`settings.plugins.plugin_commands()`. That was always `None` at that point, so no plugin
command was ever registered. The `SetPluginHooks` handler only stored the hooks.

Result: plugin commands such as `ping` fell through to "Unknown or incomplete command", so the
executor never ran. The plugins' later events (`ip ...`, `verify chunk-sent ...`) were produced by
those command flows, which explains the three symptoms. Events fired directly by the loop
(`join Bot`, `chunk-load`, tab completion, async task) kept working.

## Fix

`crates/ferrite-server/src/game/mod.rs`:
- `command_tree` is now `let mut`.
- The `SetPluginHooks` handler rebuilds the dispatcher with `register_plugin_commands` from
  `hooks.plugin_commands()` before storing the hooks.

This runs inside the single game-loop task: no global lock and no cross-region wait. Region
ownership is unchanged. The per-message readers (player execute, console execute, join-time
command serialization) pick up the new tree automatically.

## Verification (raw)

Baseline (before fix):
```
failures:
    plugin_api_calls_reach_the_client_as_packets
    server_loads_plugins_at_startup_and_disables_them_on_stop
    typical_plugin_uses_config_commands_listener_and_repeating_task
test result: FAILED. 1 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 81.48s
```

After fix, `cargo test -p ferrite-server --test plugin_lifecycle`:
```
test bridge_snapshot_survives_a_later_build ... ok
test plugin_api_calls_reach_the_client_as_packets ... ok
test typical_plugin_uses_config_commands_listener_and_repeating_task ... ok
test server_loads_plugins_at_startup_and_disables_them_on_stop ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.72s
exit=0
```
Also clean: `cargo fmt -p ferrite-server -- --check` (0), `cargo clippy -p ferrite-server --all-targets -- -D warnings` (0).
The full workspace suite was not run, per the task constraints.

## Known gap / next owner

Region tick contexts (`TickContext.tree`) capture the tree built at loop start, so **plugin
commands invoked from command blocks / region-driven commands still do not see plugin commands**.
Player chat commands and the console are fixed. No test covers this. Owner: whoever handles
command-block dispatch. Options: hand regions a shared swappable tree read per command, or attach
the plugin hooks before the schedulers are built. Either needs care to avoid a global lock on the
hot path.

The existing integration tests are the regression coverage for this fix (they failed on baseline
and pass now). No unit test was added, as the path needs a live JVM bridge.
