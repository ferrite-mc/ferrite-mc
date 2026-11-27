# Ferrite 2.0.0

> Minecraft Java **26.3 (protocol 777)** server release: more vanilla game rules, boat paddling,
> respawn points, command-block feedback, and a working Java service-provider loader.

## Highlights

- **Game rules**
  - `command_block_output` now broadcasts command-block feedback to operators (permission level 2).
  - `show_advancement_messages` gates advancement chat announcements while rewards still run.
  - `respawn_radius` offsets world-spawn respawns; beds and respawn anchors are now used on death.
  - `max_minecart_speed` caps minecart speed in blocks per second after powered-rail and furnace
    acceleration.
- **Vehicles**
  - `PaddleBoat` input is stored per ridden boat and drives the left/right paddle metadata.
- **Java Paper API**
  - `FerriteServicePluginLoader` delegates to the runtime's real `FerritePluginLoader` (plugin
    loading, descriptions, listener registration, enable/disable) instead of throwing for every
    operation.
- **World and command fixes**
  - Bed and respawn-anchor points survive death and are selected over the world spawn.
  - Command-block output is drained once per region tick and only sent when the rule permits it.

## Packages

- `Ferrite-v2.0.0-ubuntu-x86_64.tar.gz`
- `Ferrite-v2.0.0-windows-x86_64.zip`
- `ferrite-convert-v2.0.0-ubuntu-x86_64` / `ferrite-convert-v2.0.0-windows-x86_64.exe`

The portable packages contain the server and converter binaries, `LICENSE`, `NOTICE.md`, and
`START-HERE.txt`. Vanilla-generated data is prepared locally from the official server jar.

## Known limitations

- The crafter screen is not modeled, so `ContainerSlotStateChanged` remains a no-op.
- Patrol/phantom spawning, snow accumulation, command fork/sequence limits, projectile block
  breaking, and spectator chunk generation are still unenforced.
- `glib 0.18.5` is pulled transitively by Tauri 2.12.5 (gtk 0.18); the fixed 0.20 release cannot
  be selected until that desktop stack moves.
- Interactive visual validation with a real 26.3 client remains manual.
