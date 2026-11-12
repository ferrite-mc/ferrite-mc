# The End

What works, how it is built, and what is still missing.

## Server-side behaviour (covered by tests unless noted)

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
  - When the holding window ends the dragon rolls a landing with probability
    `1 / (alive crystals + 3)`; otherwise it strafes a nearby player, or keeps circling when
    there is none. A strafe locks onto that player, orbits at radius 32, charges for five ticks,
    fires one fireball and returns to the holding pattern; losing the target ends it. `DragonPhase`
    names `Holding`, `Strafe`, `LandingApproach`, `Perched` and `Takeoff`, and each transition has
    a unit test.
  - The landing cycle is `Holding` -> `LandingApproach` -> perched -> `Takeoff` -> `Holding`.
    The perch has three subphases (`DragonPhase::Perched` is the scanning one, so older saves
    still load): **scanning** takes off after 100 ticks when no player is within 20 blocks, and
    starts the attack once a player has been in range for more than 25 ticks; **attacking** roars
    for 40 ticks; **flaming** lasts 200 ticks with four breath clouds, on ticks 50, 100, 150 and
    200, then goes back to scanning. Every transition tick is pinned by a unit test in
    `dragon.rs`, and each subphase and its timer survive a save.
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
- **Gateway beam.** A gateway gets the vanilla block entity (`minecraft:end_gateway`, `Age` as a
  long). The server sends the existing `ClientboundBlockEntityDataPacket` (`BlockEntityData`, id
  6) with `Age = 0` to the chunk's viewers. The client's block-entity ticker advances its local
  `Age`, so that one packet renders the full 200-tick magenta creation beam. No block event or
  new protocol structure is involved. `Age = 200` is used when only the
  block survived an older save, so the creation beam is not replayed; a gateway whose block is
  missing but whose chunk is now loaded is written with `Age = 0`. While a gateway's chunk is held
  its stored `Age` is advanced one per region tick up to 200 (`end_fight::advance_gateway_age`, no
  packet: the client ticks its own copy), so a viewer who arrives later, or the chunk after a
  restart, receives the age already reached instead of 0. A gateway saved at 50 resumes at 50;
  a finished one is never rewritten. Covered by
  `a_reloaded_end_gateway_keeps_its_age_and_never_replays_the_creation_beam`
  (`persistence_tests.rs`). The 200 ticks of re-encoding the chunk packet are the only cost.

- **Purple beam.** Each gateway the region holds has a runtime clock
  (`end_fight::GatewayBeam`) that follows vanilla's `serverTick`: the age counts on past the
  200-tick creation beam, and whenever it reaches a multiple of 2400 with no beam running the
  region sends block event 1 (`BlockEvent`, param A 1) for the gateway block to the chunk's
  viewers; the client draws the purple beam for the next 40 ticks. A player teleporting through a
  gateway starts the same beam (the player sets `gateway_used`, the region triggers the clock),
  and a beam that is already running is not restarted. The clock lives in memory because the
  stored `Age` stays capped at 200 (rewriting it every tick would resave the chunk forever), so a
  restart resumes counting from the stored age, and it only ticks while a player is in the End.
  Covered by the timer unit tests in `end_fight.rs`, two region tests (the event reaches the
  viewers once; the clock follows region split and merge) and the integration test
  `a_new_end_gateway_sends_the_block_entity_age_for_its_beam`, which waits for the automatic
  event at `/tick rate 10000`.

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
  credits flag lives in the player data. A test writes the file through one `AnvilChunkSource`
  and reopens it through a second one, and the stage timer is pinned at its boundary ticks (50,
  39/40 and 100) so a restart cannot skip or repeat an action.

## Remaining gaps

- **Perched subphase timings are from memory.** Scanning 100/25 ticks, attacking 40, flaming
  200 with four clouds, and the 20 block scan range were written from recollection of vanilla's
  sitting phases and are not checked against the 26.3 jar. Vanilla's scanning dragon may also
  charge a player when it gives up instead of always taking off, and its attack turns toward the
  player; neither is modelled. The dragon smashes every block in its body except
  `#minecraft:dragon_immune`.
- **Gateway beam.** The purple beam is not gated like vanilla: vanilla refuses to teleport an
  entity while the beam cooldown runs, here a second player can still step through. The beam
  clock restarts from the stored age after a restart and is idle when nobody is in the End. The
  event id and the 2400 and 40 tick values are from memory of `TheEndGatewayBlockEntity`; the
  client rendering of the purple beam is unverified (see below).
- **Outer islands.** A gateway landing searches up to 16 blocks around the point 1024 blocks out
  for the nearest loaded column with ground and two air above; an all-void 3x3 chunk area cancels
  the trip instead of dropping the player into the void. The search radius is smaller than
  vanilla's.
- **Guarded-spike choice.** The cage pair follows the two shortest loaded spike tops. On a host
  terrain without vanilla's spike podiums the choice can differ from the generated world's
  shuffled sizes.
## Client-only behaviour (not verified here)

The step-by-step checks for these are in [`manual-checklist.md`](manual-checklist.md).

The packets and state above are covered by protocol bots and unit tests. These effects still need
a real 26.3 client:

- The gateway beam renderer. The tests decode the `Age` packet and the beam block event; they do
  not prove how the client draws the creation beam or the purple beam.
- The dragon's circle, strafe orbit, landing/perch/takeoff motion, the perched scan/attack/flame sequence, breath radius, boss bar and the
  obsidian arrival platform.
- The credits screen and the advancement toast after the first exit-portal trip.

The End terrain through the real host generator also needs the user's EULA; without the host the
tests use the flat generator.
