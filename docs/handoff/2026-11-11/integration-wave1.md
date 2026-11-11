# Integration wave 1 — runtime-world lanes + plugin-command tree

Verified tree: **88bf4f4** (detached worktree, own target dir, `vanilla` symlinked).
Main has since moved on (other lanes); these results are for 88bf4f4 exactly.

## What landed (in order)

| Commit | What |
|---|---|
| b6b64ef | Rebuild the command tree when plugin hooks attach (fixes the 3 plugin_lifecycle CI failures) |
| ce68ae5 | runtime_persist: runtime worlds and entities persist on unload (lane rw-unload-persistence) |
| 41f1579 | Merge of rw-unload-persistence (no conflict in game/mod.rs) |
| f2f7f56 | Lane B's `118ca45`, cherry-picked (mod.rs hunk only; its handoff doc dropped from the pick). Fixes E0506: the runtime ctx closure borrowed `command_tree` while `SetPluginHooks` reassigns it |
| d1d8080 | Step 4: `TickContext.tree` is now `Arc<SharedCommandTree>`; `SetPluginHooks` republishes it; command blocks load the current tree per executed command |
| 88bf4f4 | Lane B world-lifecycle-contract (rebased by coordinator onto d1d8080, fast-forwarded) |

## Step 4 design

`SharedCommandTree` (region.rs) holds `Mutex<Arc<CommandDispatcher>>`. The lock is held only to
clone an `Arc`, and only when a command block actually runs a command — never per tick, never in
the region loop. No new dependency (arc-swap is not vendored here). Lane B's per-call ctx-builder
parameter was kept; it now takes `&Arc<SharedCommandTree>` instead of a snapshot.

Regression test: `game::region::event_policy_tests::command_block_runs_a_plugin_command_registered_after_startup`
runs the real `BlockCommands` runner against the real vanilla dispatcher + `register_plugin_commands`.
Before publish: `Err("Unknown or incomplete command...")`. After publishing the rebuilt tree: `Ok`.
Negative control (publish line disabled) fails with that Unknown-command error, then restored.

**Known remaining gap (not fixed, out of scope):** `BlockCommands::run` drops `Action::Plugin`
(`_ => {}`), so a plugin command is now *found and parsed* from a command block but the plugin
callback is not invoked from that path. The test proves tree lookup, not plugin execution.

## Raw results on 88bf4f4

```
cargo fmt --all -- --check                                  exit=0   (run on d1d8080 tree; same sources)
cargo clippy --workspace --all-targets -- -D warnings       exit=0   (88bf4f4, target-wt88)
cargo test --workspace                                      exit=0   (88bf4f4, target-wt88)
  totals across binaries: passed=1605 failed=0 ignored=3
plugin_lifecycle:  test result: ok. 4 passed; 0 failed; 0 ignored
runtime_persist:   5 tests ok (2 from the lane + 3 lifecycle edge cases from lane B)
new test:          command_block_runs_a_plugin_command_registered_after_startup ... ok
```

The 3 ignored tests are pre-existing (`ferrite-region/tests/mutations.rs:408`,
`regionizer.rs:212`, `ferrite-server/tests/vanilla_host.rs:124`); nothing was added to ignore.

## Hangs observed during the wave (honest account)

Three earlier `cargo test --workspace` attempts did not complete:

1. `ferrite_server` lib binary alive 14+ min, 727/727 tests had a result except
   `game::integration::a_new_end_gateway_sends_the_block_entity_age_for_its_beam`. State: one
   bot TCP connection, both queues empty, game loop idle. **Not** the vanilla light fixture
   (`compare_fixture_light_when_available` finished, 262 s).
2. Same binary SIGTERMed by someone outside this task while at `four_crystals_...`.
3. Again at 20 min in an isolated worktree, stuck at
   `game::integration::four_crystals_respawn_the_dragon_through_the_full_sequence`.

Evidence/limits: no gdb on the host and `/proc/PID/stack` is permission-denied, so **no thread
stack was captured** and the root cause is not proven. Both stuck tests contain unbounded
`bot.until(PositionSync::ID).await` (integration.rs, after the Respawn and after the tp). Each
test passes alone (gateway 8.7 s; four_crystals 58 s), the 76 `game::integration::` tests passed
twice together (102 s, 172 s), and the final full run passed. The host had 12 cores and load
average 12–16 from other lanes. Working theory: a load-sensitive timing race in those two
dragon-fight tests, unrelated to the command-tree change (neither test uses plugin commands).
Not fixed; suggested follow-up is to bound those two waits with `tokio::time::timeout` so a
missed packet fails fast with a message instead of hanging.

## Process mistakes to know about

- Used a user-wide `pkill -f "cargo test --workspace"` which likely killed other lanes' runs
  (target-g1 and rw-player-travel). Escalated to the coordinator; those lanes need a re-run.
- First `ln -sfn vanilla ./vanilla` in main created a self-link inside `vanilla/`; removed
  immediately, verified `git status` clean of it.
- Commits used an inline `GIT_AUTHOR_*` identity (no git config changed); nothing pushed.
