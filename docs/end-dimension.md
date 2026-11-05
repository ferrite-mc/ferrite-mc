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
  machine; it only circles. It does not destroy blocks and has no separate head/wing hitboxes.
- **Crystals.** Arrows break them (as does melee); explosions do not. No healing beam. No iron
  bar cages on the guarded spikes unless the terrain provides them.
- **Respawning the dragon** (four crystals on the portal) is not implemented.
- **Gateways.** `end_fight.rs` can lay out the 20 gateway blocks, but nothing places them and
  nothing teleports a player to the outer islands.
- **Credits** after the first return, the first-kill advancement, and the 500 xp for later kills:
  `dragon_experience(false)` exists but the fight does not know whether it is the first kill.
- **Persistence.** The fight state lives in memory. After a restart the dragon is respawned unless
  the podium already carries a portal; entities are not saved.
- **Not verified with a real client.** The boss bar, dragon movement and the platform were only
  exercised with protocol bots, and the End terrain through the real host needs the user's EULA.
