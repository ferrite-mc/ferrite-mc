# Vanilla command coverage (2026-11-04)

Goal: every vanilla command works like vanilla. The list of root commands comes from the
dispatcher compared with `vanilla/26.3/generated/reports/commands.json` (94 roots).

## Done on 2026-11-04 (all in `commands_more.rs` unless noted)

version, random, setidletimeout, teammsg/tm, transfer, stopwatch, publish, unpublish, tick,
rotate, forceload, damage, debug, perf, jfr, test, fetchprofile, spreadplayers, swing, attribute, dialog, posteffect, spectate.

## Not done: 9 commands are still missing

advancement, compute, fillbiome, item, loot, place, recipe, ride, waypoint.

Notes for whoever picks these up:

- `ride`: needs passenger plumbing in the region (no vehicle model yet).
- `item`, `loot`: need slot addressing and loot tables from the extracted vanilla data.
- `advancement`, `recipe`: need per-player progress state, packets and persistence.
- `compute`: needs the context int/float provider registries.
- `place`, `fillbiome`: need new requests in the worldgen host protocol (Mojang's code stays
  in the host process; Ferrite must not carry it).
- `waypoint`: needs the clientbound `waypoint` packet and entity waypoint state.

## Partly done (works, but not like vanilla yet)

- `/attribute` stores base values and modifiers per entity and computes vanilla's formula, but
  nothing in the simulation reads them yet (only generic defaults plus the player overrides for
  attack damage and movement speed; per-mob defaults and `failed.entity` are missing).
- `/dialog show` takes registry ids only (no inline dialog). `/spectate` sends `set_camera` but
  the server does not move the spectator with the target. Feedback texts for the new commands
  were written from memory of en_us (argument order not verified against a lang file).

- `/swing` sends the arm-swing animation to viewers; the animation type and duration are
  validated but not used, and non-living entities are not rejected (`failed.notliving`).
- `/tick sprint` is missing; `/tick query` has no percentile line. Freeze stops game time,
  fluid ticks and random ticks, not mob or entity movement.
- `/forceload` marks are not saved across restarts.
- `/debug function` always fails; `/test` accepts all syntax but never runs a test (no game
  test framework); `/jfr` always fails (Ferrite is not a JVM process); `/perf` and `/debug`
  report time and ticks but write no report file.
- `/fetchprofile` only knows players who are online (no Mojang API lookup).
- `/rotate` assumes the target is in the command's dimension.
- `/damage` maps the damage type to the nearest survival kind; mobs take the plain amount
  without a hurt animation, and `at`/`by`/`from` are parsed but not used.
- `/locate biome|structure` ask the vanilla host (ops 5 and 6, needs `generator = "vanilla"`).
  `/locate poi` still returns not found: the host has no POI request yet.

## Merge state

Nothing was pushed to the remote on 2026-11-04 after the worldgen/README push.
