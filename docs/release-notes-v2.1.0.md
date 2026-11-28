# Ferrite 2.1.0

> Minecraft Java **26.3 (protocol 777)** server release: patrol captain AI with vanilla local
> difficulty, server-wide spawn protection, and Paper lifecycle events.

## Highlights

- **Patrols and local difficulty**
  - Pillager patrol members follow their banner captain with vanilla's 8-block follow distance and
    0.7 speed; the captain UUID is runtime state and is not written to save NBT.
  - Patrol and phantom group sizes read a vanilla `DifficultyInstance` built from world age,
    per-chunk inhabited time and the Hard moon phase, so long-settled chunks can field larger
    groups (up to eight pillagers).
  - Region split and merge carry each chunk's inhabited time, so local difficulty stays attached to
    the terrain rather than the scheduler.
- **Spawn protection**
  - Breaking, placing and using blocks are all rejected for survival/adventure non-operators inside
    the Overworld spawn-protection radius, not only projectile block breaks.
  - Creative, spectator and operator players remain exempt, matching vanilla behavior.
- **Java Paper API**
  - Lifecycle event registration is implemented through the `COMMANDS` registrar for
    `BasicCommand` and Brigadier literals, with priority/monitor ordering, the enable-window check,
    handler isolation, `INITIAL` firing after the last plugin enables, `RELOAD` refiring and
    per-plugin cleanup.

## Packages

- `Ferrite-v2.1.0-ubuntu-x86_64.tar.gz`
- `Ferrite-v2.1.0-windows-x86_64.zip`
- `ferrite-convert-v2.1.0-ubuntu-x86_64` / `ferrite-convert-v2.1.0-windows-x86_64.exe`

The portable packages contain the server and converter binaries, `LICENSE`, `NOTICE.md`, and
`START-HERE.txt`. Vanilla-generated data is prepared locally from the official server jar.
