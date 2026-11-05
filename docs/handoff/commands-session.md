# Command / plugin-API session handoff (cloud session -> local)

Branch `main`. Last commits: slot ops (`f9b70bc`), `/place template`, `/ride`.
Build on a non-Windows box: `RUSTUP_TOOLCHAIN=stable cargo ...` (toolchain file pins windows-msvc).
Expected test state: `cargo test --workspace --no-fail-fast` -> only 4 known failures
(`redstone_*_on_fixture_world` x3, `survival_eating_uses_food_component_after_use_duration`).

## Done this session
- `/fillbiome`, `/advancement`, `/recipe`, `/compute`, `/waypoint` (state only), `/loot` (all
  destinations + `kill`), `/item replace|fill|override` (blocks + players, `from`), `/place
  template`, `/ride`.
- New modules (crates/ferrite-server/src/game): `commands_progress`, `commands_late`,
  `commands_loot`, `commands_item`, `slot_ops` (Action::Slots/DropItems executor),
  `structure_template` (reads extracted `.nbt`), `ride` (global ride table, per-tick follow in the
  tick branch of mod.rs, `SetPassengers` packet id 109 in ferrite-protocol).
- `tools/fetch_vanilla.py` now extracts structure templates into
  `vanilla/26.3/generated/data/minecraft/structure/` (already done locally for this checkout;
  re-run extract_structures or the whole script on your machine).
- `docs/COMMANDS-STATUS.md` lists every limit; read it first.

## Not done (needs your machine)
1. `/place feature|jigsaw|structure` and plugin API `getStructures`/`generateTree`: need new ops in
   the worldgen host (`java/ferrite-worldgen-host`, wire protocol in docs/worldgen-host.md) and
   Java 25 to run it. Cloud box only had Java 21, nothing Java-side was run.
2. Client packets for `/advancement` (update_advancements 133), `/recipe` (recipe_book_add/remove
   75/76), `/waypoint` (waypoint 141): ids known, field layouts unverified. Capture from a real
   client/server or the decompiled jar before encoding. Advancement/recipe state is not persisted.
3. `/item modify` and `<modifier>`: no item modifiers in vanilla data -> always
   `argument.item_modifier.unknown`.
4. Java change `FerriteWorld.isChunkGenerated/loadChunk` (commit 5bb5db8) never compiled (no Gradle/Java 25).

## Unverified / guessed
- Feedback texts in feedback.rs were written from memory; `reports/lang_en_us.json` exists after
  fetch_vanilla, so replace guesses with real keys/text.
- `/item fill|override` semantics inferred; `/place template` ignores block-entity NBT, entities,
  `strict`, stair shapes; `/ride` seat offset fixed at +0.6, no steering, never tried with a real
  client (sneak = input bit 0x20 -> dismount).
- Non-player entities have no inventory, so `/item ... entity` only writes players.
