# Ferrite 0.1.6

> **Historical release note.** For current status see [`../README.md`](../README.md) and
> [`BACKLOG.md`](BACKLOG.md).

Ferrite 0.1.6 targets Minecraft Java 26.3 (protocol 777).

## Highlights

- Adds a Windows desktop console built with Tauri, with headless operation retained for
  services and tests.
- Adds runtime-world storage catalog groundwork for plugin world lifecycle work.
- Routes an ender pearl landing across region boundaries back to the owning player's region.
- Batches streamed chunk and `/forceload` holder changes so one scheduler lock and wake can cover
  a packet or command batch.

## Known limitations

- Bukkit `Server#createWorld` and `Server#unloadWorld` are implemented for runtime worlds (see
  `docs/BACKLOG.md` section 8). Custom generators, `generateStructures(false)`, non-`NORMAL` world
  types and environments other than `NORMAL`/`NETHER`/`THE_END` are rejected with
  `UnsupportedOperationException`. This corrects the earlier text that listed both calls as
  unsupported.
- `World#unloadChunk` does not force-unload chunks held by a player or `/forceload`.
- `World#sendBlockUpdate` supports signs; other tile states are unsupported.
- The 800-bot baseline is recorded in `docs/perf/README.md` (no optimisation claimed); real-client validation remains open (`docs/manual-checklist.md`).

## Windows package

The portable ZIP contains `ferrite-server.exe`, `LICENSE`, `NOTICE.md`, and `START-HERE.txt`.
The Windows desktop UI requires the WebView2 runtime. Vanilla-generated data is not bundled;
follow `START-HERE.txt` to prepare it locally.
