# Freebuff task 30: MapView decoration rendering

Read this task file and `docs/plugin-stubs-status.md` first. Ignore all earlier worker task
instructions; this file is the sole scope for this assignment. Inspect the existing
`FerriteMapView`, `PlayerMisc.sendMap`, inventory map handler, and 26.3 map packet before editing.

## Goal

Finish the missing Bukkit MapView decoration path end to end. Map renderers currently produce
pixels, while `sendMap` sends a map packet without decorations. Preserve the existing map pixel,
scale, and locked behavior while sending renderer-produced cursors and supported player tracking
decorations to the intended viewer.

## Ownership and constraints

Own `java/ferrite-api-impl` map view/rendering code and the narrowly required map packet/server
dispatch files. Inspect the protocol report/generated 26.3 packet data first; do not guess packet
IDs or field order. Do not modify broad plugin host/player APIs, unrelated inventory behavior, or
shared packet codecs outside what this end-to-end feature requires. Report any required wider API
change before expanding scope. No Mojang code/assets, ASWM, or Rust worldgen.

## Acceptance

- Collect cursors created through `MapCanvas#getCursors` and encode their type, position,
  direction, visibility, and supported caption fields using the exact 26.3 wire schema.
- Preserve pixel rendering and full/partial patch behavior; verify scale/lock and decorations
  with deterministic Java and Rust tests. Cover an empty cursor set, multiple cursors, bounds,
  and a player-tracking cursor.
- Ensure a map update is scoped to the requested player and does not leak renderer/viewer state
  between players. Reject or explicitly document unsupported decoration variants.
- Run focused Gradle tests, relevant Rust tests, `cargo fmt --all -- --check`, and scoped clippy.
  Do not claim real-client visual verification unless it was actually run.
- Commit this task. Then clean only this worktree's generated Cargo/Gradle build outputs; do not
  clear shared user caches or another worktree's targets. Report commit, checks, client limits,
  and any unrelated pre-existing changes, then stop.
