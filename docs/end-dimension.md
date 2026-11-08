# The End

What works, how it is built, and what is still missing.

## Works (covered by tests unless noted)

- **Frames and portal.** Eyes of ender fill portal frames; a complete 12-frame ring lights the
  3x3 `end_portal` (`end_frame.rs`, integration test `the_last_eye_of_ender_lights_the_end_portal`).
- **Travel.** Overworld to End lands on the 5x5 obsidian platform at x 98..102, z -2..2, y 48
  (`portal.rs`; the platform spans two chunks, both are loaded first). From the End an `end_portal`
  block leads back to the player's bed, or the world spawn when there is none.
- **Terrain.** The End uses the host terrain (`worldgen-host.md`); without the host it falls back
  to the flat generator. Void damage starts 64 blocks below the dimension floor (y -64).
- **The fight** (`end_fight.rs`, `region.rs::tick_end_fight`):
  - The first time a player is in the End the dragon spawns (200 health) and the inactive
    bedrock podium is placed at the world origin. A portal already on the podium means an earlier
    dragon died, so nothing spawns.
  - Ten crystals stand on the bedrock tops of the obsidian spikes (found by scanning the ten
    columns on the radius-42 ring, not by recomputing the spike layout).
  - Crystals within 32 blocks heal the dragon 1 health every 10 ticks. Hitting a crystal breaks it
    and explodes it with power 6.
  - A boss bar (pink, progress, darken sky + music + fog flags) is shown to every player in the
    dimension and removed when the dragon dies.
  - The dragon flies a circle (radius 40, around y 90) and hurts players within 5 blocks for 6
    every second.
  - On death the podium turns into the exit portal, a dragon egg appears on the pillar on the
    first kill only, and 12000 experience drops.
- **Respawning the dragon.** Four end crystals in the cells three blocks from the centre start
  vanilla's staged sequence: the crystals' beam targets, the roars, ten spike crystals restored
  one every 40 ticks, and one dragon after 604 ticks. At completion the four summon crystals
  explode with power 6 without breaking blocks, then are removed. Breaking a crystal or losing the
  origin chunk aborts the sequence and puts the exit portal back. The stage and timer are saved and
  a restart resumes at the same stage. `/summon minecraft:end_crystal` creates the crystals for
  this path. The full 604-tick sequence is covered by an integration test using
  `/tick rate 10000`.
- **Repair after a crash.** The exit portal and every opened gateway are part of the saved fight;
  the region holding the origin puts a missing portal back once the dragon is gone, and any End
  region restores a gateway whose chunk it holds.

- **Dragon body and breath.** Melee and arrows resolve the eight vanilla sub-hitboxes; only the
  head takes full damage, every other part uses `damage / 4 + min(1, damage)`. Breath clouds use
  vanilla's shape: radius 3 growing to 5 over 600 ticks, a 20-tick fireball wait, and 3 damage
  every 20 ticks.
- **Guarded spikes.** The two shortest spikes (the vanilla guarded pair) get a 73-block iron-bar
  cage when their crystal is placed; the cage is restored with the spike on a respawn.
- **Credits and advancement.** The first trip home through the exit portal sends `WIN_GAME` and
  waits for the client's respawn request before moving. The seen flag is saved in
  `playerdata/<uuid>.json`, so a relog or restart does not replay the credits. Paper's
  `misc.disable-end-credits` in `config/paper-world-defaults.yml` suppresses the screen while the
  return trip still happens. The "Free the End" advancement credits melee, arrow and explosion
  kills by recording the last attacker on the mob and resolving it in the death pass. (Later kills
  give 500 xp, first kills 12000; the first-kill flag is remembered from the exit portal.)
- **Persistence.** The fight (started, killed before, dragon alive, gateways, placed spike
  crystals, respawn stage/timer, exit portal level) is written to
  `DIM1/data/minecraft/ender_dragon_fight.dat` whenever it changes and read on the first tick in
  the end. The dragon, its landing phase and the crystals are saved as ordinary entities; the
  credits flag lives in the player data.

## Remaining gaps

- **Dragon phase machine.** The landing cycle is modelled as `Holding` -> `LandingApproach` ->
  `Perched` -> `Takeoff` with vanilla-length perch and holding decisions, but strafing, the full
  per-phase attack schedule and exact perched timings remain approximations. The dragon smashes
  every block in its body except `#minecraft:dragon_immune`.
- **Outer islands.** A gateway landing searches up to 16 blocks around the point 1024 blocks out
  for the nearest loaded column with ground and two air above; an all-void 3x3 chunk area cancels
  the trip instead of dropping the player into the void. The search radius is smaller than
  vanilla's and the client-side gateway beam animation is not modelled.
- **Guarded-spike choice.** The cage pair follows the two shortest loaded spike tops. On a host
  terrain without vanilla's spike podiums the choice can differ from the generated world's
  shuffled sizes.
- **Not verified with a real client.** The boss bar, dragon movement, breath radius and the
  platform were only exercised with protocol bots and unit tests, and the End terrain through the
  real host needs the user's EULA.
