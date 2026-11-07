# Next session — where the work stands (2026-11-05)

> Newer note: `docs/handoff/2026-11-06-commands-worldgen-api.md` (read it first).

Branch `yxshadxw/plan1104` (worktree `plan1104`). `origin/main` is at `4feaf42`; later commits are
on the branch only.

## Done this session

- Merged the piston work, finished End milestone 1 (frames, travel, platform, host terrain, void).
- The End fight: dragon mob (`ferrite-entity/src/dragon.rs`, `MobKind::EnderDragon`), boss bar,
  crystals on the spikes (heal, break by melee/arrow/explosion with chain blasts), contact hits,
  periodic dives, exit portal + egg + xp (12000 first kill, 500 later), gateways opened per kill
  and a gateway teleport to 1024 blocks out. Details and gaps: `docs/end-dimension.md`.
- Pistons: extending pistons carry mobs/items; nothing is pushed outside the build height.
  Details and gaps: `docs/pistons.md`.
- Removed multi-version support (`ferrite-translate` crate, `supported-versions`): only 26.3 / 777.
- README and `docs/COMMANDS-STATUS.md` refreshed.

## Test flakiness seen in CI today (not root-caused)

- `chest_contents_are_shared_saved_and_spilled_when_broken`: fails on both Linux and Windows CI with
  only 26-32 of 40 stone picked up, even with the 90 s deadline; passes locally every time
  (3 of 3 alone). Look at spilled-item pickup/merge when two players stand on the chest.
- `an_extending_piston_carries_the_mob_in_the_way`: timed out on Windows CI (15 s wait for the
  cow's teleport to x 4.5); passes locally. Probably timing under load, but unverified.
- The plugin worker saw the chest test fail once in a full workspace run and not alone.

## To do next

1. **Merge/push to main** is being done by the other worker (plugin session); do not duplicate it.
   If it is still open: **push to main.** Push up to `b14c18d` once its CI is green (`git push origin b14c18d:main`).
   Commit `a13cd9b` (piston height) also carries another session's half-finished `region.rs`
   change (`Command::ItemMeta`, `ItemMetaFields`); its matching files (`game/mod.rs`,
   `plugins/inventory.rs`, `ferrite-plugin/src/lib.rs`, Java `FerritePlayerInventory`) were still
   uncommitted. Push `a13cd9b` and later only after that session's commit is in and CI is green.
2. **Check CI** of the last pushed commits (`gh run list --branch yxshadxw/plan1104`). A red run on
   `444a3c3` came from the other session's partial commit (`arrow::Kind` missing), not from this work.
3. **End, still missing:** dragon phases (strafe, perch, breath, fireballs), block destruction,
   respawning the dragon with four crystals, iron-bar cages, healing beam, credits, advancement,
   saving the fight state, gateway exit search for the nearest island and beam animation.
   Not verified live: needs the user's EULA for the worldgen host and a real client.
4. **Pistons, still missing:** moving-piston block entity and animation timing, retraction/sticky
   pulls carrying entities, pushing players, interrupted movement, cross-region behaviour,
   resolver parity tests.
5. Other known gaps: `/place template` block entities, `/test` (no game-test framework) and
   `/jfr` (not a JVM) -- `/damage` and `/debug function` are already implemented
   (see `docs/COMMANDS-STATUS.md`). The plugin Structure API is done (2026-11-07 below).

## Gotchas

- Another Claude session edits the same worktree. Before `git add`, run `git diff <file>` and
  stage only your own hunks; never revert their edits.
- Use a separate `CARGO_TARGET_DIR=target-tree` and delete it afterwards (it is untracked).
  `rm -rf` is blocked by the ECC GateGuard hook unless the destructive-command facts are stated.
- ECC (`ecc@ecc`) is enabled globally; its GateGuard asks for facts before the first Bash/Write.
  Disable with `ECC_GATEGUARD=off` or `ECC_DISABLED_HOOKS=pre:bash:gateguard-fact-force`.
- Heredocs with apostrophes break the Bash tool: write scripts with the Write tool.
- Never accept the Minecraft EULA for the user; host integration tests are run by them.
- Trail-Code (`@trailhq/trail-code`) would not install: node-gyp needs Visual Studio C++ build tools.

## 2026-11-07 update

- Done since the list above: End (credits, advancement, saving, dragon attacks, block smashing),
  tridents, wolf pack anger, worldgen host default 8 threads, Paper/Folia comparisons
  (`docs/handoff/2026-11-07-paper-comparison.md`; its 200-bot tables were measured with bots that
  did not send a view distance, so read them as rough).
- Vaults: a trial key opens a vault once per player and ejects the reward table (`game/vault.rs`; no state check or animation).
- Still open: tamed wolves attacking for the owner
  (mobs only target players today), other tameable kinds, piston timing and edge-case tests,
  Moonrise-style worldgen ports (profile is flat, see `docs/worldgen-host.md`), real-client checks.
- Horses, donkeys, mules and llamas: taming by riding (temper roll), saddle, steering by the
  client (`move_vehicle`), rolled speed/jump/health, horse armour absorbing damage, llama carpets,
  treats, chest on donkey/mule/llama, llama spit when hurt. Shortcuts: the chest menu is 18 slots
  (vanilla 15 or 3 x strength) and a dead animal drops chest, items and saddle; no llama caravans or trader llama;
  horse inventory uses the generic chest screen.
- Weather cycle (clear/rain/thunder, `advance_weather` rule, game_event packets), trident riptide /
  loyalty / channeling, piston block updates delayed two ticks. Shortcuts: single weather timer,
  no riptide spin pose, vault state/animation ignored.
- 26.3 `PlayerCommand` ids were wrong (wake/sprint) and are fixed: 0 stop sleeping, 1/2 sprint,
  5 open inventory.
- Plugin Structure API (`java/ferrite-api-impl`): `createStructure()` + `fill`, `place` with
  rotation/mirror/integrity/palette, `getPalettes`, `saveStructure`/`loadStructure` (file, stream,
  key), `deleteStructure`, `getStructureFile`, `copy`. Shortcuts: no entities or block entity data;
  only `facing`/`axis` turn with rotation; the Java world now carries full block states (host
  sends `name[props]`, `setBlockData` keeps them); templates loaded by key still place through the
  server (`/place template`) and cannot be `fill`ed.
- Not verifiable here: any of the above against a real client.
- Order the user gave: Rust worldgen last; Moonrise-style optimisations are done while writing the
  Rust version, not as a Java patch port.
