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
  - On death the podium turns into the exit portal, a dragon egg appears on the pillar and 12000
    experience drops.

## Missing

- **Dragon behaviour.** No strafing, charging, perching, breath, fireball or dragon-phase
  machine; it circles and dives at the nearest player every 30 seconds. It does not destroy blocks and has no separate head/wing hitboxes.
- **Crystals.** Arrows, melee and explosions break them (a blast chains to the next crystal). No healing beam. No iron
  bar cages on the guarded spikes unless the terrain provides them.
- **Respawning the dragon** works in a simplified form: end crystals can be placed on obsidian or bedrock, and four of them on the bedrock ring around the exit portal (3 blocks from the centre) bring a new dragon, remove the portal and replace the spike crystals at once. Vanilla's beam animation, the ~10 second delay and the crystal explosions are missing, and this path has only unit tests (the cells are checked against the podium layout), no bot test.
- **Gateways.** Each dragon kill opens the next gateway of the 20-ring (radius 96, y 75), when
  its chunk is loaded. Standing in one sends a player to the top of the ground 1024 blocks out
  in the same direction (or does nothing over empty void); vanilla instead searches for the
  nearest outer island and plays the beam animation, which is not modelled.
- **Credits**: the first trip home through the exit portal sends `WIN_GAME` and waits for the client's
  respawn request before moving (seen-flag is in memory only, not saved). The "Free the End"
  advancement completes on a dragon kill (melee path, covered by a test). (Later kills give 500 xp,
  first kills 12000; the first-kill flag is remembered from the exit portal.)
- **Persistence.** The fight state lives in memory. After a restart the dragon is respawned unless
  the podium already carries a portal; entities are not saved.
- **Not verified with a real client.** The boss bar, dragon movement and the platform were only
  exercised with protocol bots, and the End terrain through the real host needs the user's EULA.
