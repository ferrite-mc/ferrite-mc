# Hand-off 2026-11-06 — commands, worldgen API, async

Branch `main` (all pushed). Read this first when continuing locally; `next-session.md` is the older
piston/End note and still applies for those areas.

## Read first

- Folia reference (not readable from the cloud session): <https://docs.papermc.io/folia/reference/overview/#_top>
  — diff its scheduler/ownership/region rules against `ferrite-region` and `docs/BACKLOG.md`.
- `docs/COMMANDS-STATUS.md` — what each command really does, and what it does not.
- `docs/worldgen-host.md` — host protocol (ops 1-10), patcher, open problems.
- `docs/plugin-stubs-status.md` — Bukkit/Paper API coverage and the gaps.
- `docs/BACKLOG.md` — "Performance and operations" now has the threading picture.

## Done this session (newest last)

- Locator bar (`game/waypoints.rs`): waypoint packet 141, azimuth/position/chunk, styles/colors,
  spectators do not transmit.
- Advancements (`advancement_packets.rs`, `recipe_unlock.rs`): tabs/toasts, saved progress,
  completion by `inventory_changed`, melee kills of a plain entity type, `consume_item`,
  `changed_dimension`, `placed_block` (plain block); chat announcement; recipe and XP rewards.
- Recipe book (`recipe_packets.rs`): update_recipes, add/remove, unlock from play, the client's
  open/filter settings are stored (`playerdata/<uuid>.recipebook.json`).
- `/ride`, `/spectate` follow, `/debug function`, inline `/dialog show`, `/attribute max_health`,
  `/place template` entities and block entities.
- Worldgen API: `World#getStructures` (host op 10), `Server#createExplorerMap` (3 overloads,
  Java side: nearest structure -> `MapView` centred there -> FILLED_MAP with `MapMeta`),
  host threads clamped to 4, jar patcher on by default with fallback to the unmodified jar.
- Threading: the locator bar and the advancement/recipe book sync run on their own threads
  (`game/offload.rs`, `Latest<T>` one-slot mailbox; completions return over an mpsc channel). Folia's Region/Entity/Global/Async schedulers and `isOwnedByCurrentRegion` exist.

## Not verified — needs you locally

- Nothing worldgen was run against the real jar: `eula.txt` has no `eula=true`, and Ferrite never
  accepts it for you. Put `eula=true` in `eula.txt` yourself, then run
  `cargo test -p ferrite-server --test vanilla_host` and a real server with `[worldgen]` defaults
  (patcher is now on; set `patch = false` to compare).
- No real client was used: check the locator bar, advancement tabs/toasts and the recipe book. A
  client-side decode failure would disconnect.
- `createExplorerMap`: the map link lives only in the Java `MapMeta`; Rust items are material +
  amount, so a real client sees a plain map without the structure. Needs a `map_id` item component
  and map data/decorations to be useful.

## CI state

- Last green run: #177 (before the retracting-sticky-piston test was added). Every run since fails
  in `cargo test` with the single test
  `game::integration::a_retracting_sticky_piston_carries_the_mob_standing_on_the_block`
  (Linux and Windows). fmt, clippy and everything else pass.
- Real cause (from run #205's log: no teleports seen at all): the test decoded `EntityTeleport` (id 35,
  the position-sync packet) but a piston carry sends `TeleportEntity` (id 128). Test now listens for
  `TeleportEntity`. `NoAI` (`/summon ... {NoAI:1b}`) stays, it keeps the cow still. Confirm in the next run.
- Locally in the cloud container the 9 `game::integration::*` fixture-world tests always fail
  (no `vanilla/26.3/fixture-world`); that is expected there, not a regression.

## Still open

- Commands: `/test` (GameTest, no equivalent) and `/jfr` stay failing on purpose; other
  `/attribute` simulation; advancement triggers other than the ones above (placed_block,
  recipe_crafted, bred_animals, tame_animal, item_used_on_block, location, ...); loot-table and
  function rewards; `seen_recipe`; waypoint invisible/`waypoint_transmit_range` filters and
  plugin-set spectators.
- Async: still serialized on the one coordinator task in `game/mod.rs`: join/leave, packet
  routing (cheap), the clock tick, `Signal::Update` per player per tick, the `command_state` mutex. Next
  candidates are listed in `docs/BACKLOG.md`. Re-run the 200-bot load test to measure.
- Worldgen: host stalls with 8 request threads (likely requests blocking each other on the
  server main thread; unverified), `call:` hooks, `process`/`remote` modes (remote needs an
  authenticated channel).

## Gotchas

- Rust: `RUSTUP_TOOLCHAIN=stable` (the pinned MSVC toolchain does not exist on Linux).
- Java tests need JDK 25: `gradle --offline -Porg.gradle.java.installations.paths=<jdk25> test`
  from `java/ferrite-api-impl`; the host builds with `FERRITE_JAVA_HOME=<jdk25> python3
  tools/build_worldgen_host.py`.
- `tools/make_fixture_world.py` writes `eula=true` into the fixture world; only run it if you
  accept the EULA yourself.
- Do not commit Mojang code or data; extractor output stays under `vanilla/26.3/generated/ferrite/`.
