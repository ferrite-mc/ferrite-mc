# Vanilla world generation host

Ferrite contains **no world generator**. New chunks are produced by Mojang's own generator, which
runs inside a separate JVM process (the *host*) from the server jar the user downloads
themselves. Ferrite only asks the host for chunks and reads the NBT it returns.

This is the same legal shape as Paper/Spigot (BuildTools, Paperclip): nothing of Mojang's is
distributed. What ships here is our own host code and a set of *rules* that describe how to
disable parts of the user's jar on the user's machine.

## Status

Done: the host (`Host.java`), the Rust client (`vanilla_gen.rs`), the shared `eula.txt` gate,
`tools/build_worldgen_host.py`, `[worldgen]` settings and an end-to-end test
(`crates/ferrite-server/tests/vanilla_host.rs`). The host runs the user's jar **unmodified**.
The patcher (`Patcher.java`, rules in `rules/worldgen.rules`, `[worldgen] patch = true`) removes the
network listener, gameplay ticking, player and function ticks; it is on by default (see Measuring). Ops 5-10 (locate biome/structure/poi,
structure_at, place, structure starts) are done. Not done: `call:` hooks for optimisations (no
measured hotspot to target), the `process`/`remote` modes (`remote` would need an authenticated,
non-loopback channel and cannot be tested on one machine).

## Rules

- Never commit or distribute Mojang code, jars, classes or dumps. `vanilla/` and `worldgen/` are
  git-ignored. A patched jar is only ever created on the user's machine.
- Never write generation logic. The host calls `ServerChunkCache.getChunk(x, z, FULL, true)` and
  serializes the result with Mojang's own `SerializableChunkData`.
- Ferrite does not accept the Mojang EULA for anyone. Ferrite and the host read the **same**
  `eula.txt`; the user sets `eula=true` themselves.

## Pieces

| Piece | Where | Role |
|---|---|---|
| `ChunkGenerator` trait | `crates/ferrite-server/src/game/generator.rs` | The only seam the server knows. |
| Process client | `crates/ferrite-server/src/game/vanilla_gen.rs` | Starts the host JVM, speaks the wire protocol, implements `ChunkGenerator`. |
| Host | `java/ferrite-worldgen-host/` | Our Java code: boots the user's jar, answers chunk requests. |
| Patcher | `java/ferrite-worldgen-host/.../Patcher.java` | Rewrites the user's jar on their machine (Java `java.lang.classfile`, no extra dependency) so only world generation remains. |
| Build script | `tools/build_worldgen_host.py` | Compiles the host against the local jar into `target/worldgen-host/`. |

## Process model

Ferrite binds `127.0.0.1:<ephemeral>`, starts `java ... Host --port N`, and the host connects
back. Nothing is reachable from outside the machine. One host process per Ferrite process by
default; `worldgen.mode` can later grow `remote` for large deployments because the trait is the
only coupling.

## Wire protocol (loopback TCP, big-endian)

Request: `u32 id, u8 op, u8 dimension, i32 x, i32 z, u16 argument length, argument bytes (UTF-8)`

| op | meaning | ok payload |
|---|---|---|
| 1 | generate chunk column | uncompressed named NBT, as stored in an Anvil chunk |
| 2 | spawn height at block x, z | `i32` |
| 3 | shutdown | none |
| 4 | biome at block x, y, z (argument `y`) | UTF-8 biome id |
| 5 | locate biome (argument `spec;radius;x;y;z`, spec is an id or `#tag`) | `x y z`, or empty when not found |
| 6 | locate structure (argument `spec;radius;x;z`, radius in chunks) | `x y z`, or empty when not found |
| 7 | is block inside a structure (argument `id x y z`) | `1` or `0` |
| 8 | place a feature/template (see `plugins/worldgen.rs`) | `ok` + changed blocks, or `fail <text>` |
| 9 | locate point of interest (argument `spec;radius;x;y;z`) | `x y z`, or empty |
| 10 | structure starts of chunk x, z | lines `id minX minY minZ maxX maxY maxZ` |

Response: `u32 id, u8 status (0 ok, 1 error), u32 length, bytes` (error payload is UTF-8).
Responses may arrive out of order; the id matches them to requests.

## Patcher scope

Goal: keep only what world generation needs, drop everything else.

Remove or neutralise: network listener and connections, status/query/rcon/management servers,
player list and player handling, entity AI and mob spawning ticks, per-tick level processing,
other dimensions' ticking, autosave, console input, watchdog.

Keep: registries and data loading, `ServerLevel`/`ServerChunkCache` (world generation is
coupled to it), chunk generators, biome sources, structure and feature placement, lighting
needed for `FULL` status.

Method: a rule file lists `class -> method -> action` (empty the body, return a constant).
The patcher applies the rules with `java.lang.classfile` when it builds the patched jar from the
user's jar, and refuses to run if a rule's target is missing (a Minecraft update changed it).
The patched jar is cached in `worldgen/` and never leaves the machine.

## Measuring (2026-11-04, noisy shared machine)

- Live heap after load is about 136 MB; the 1 GB working set is JVM heap headroom. `heap-mb` and the
  GC matter more than removing code: 384 MB works.
- Idle CPU: 0.28 s per 15 s unmodified, 0.06 s with ticks removed. Throughput difference between
  the unmodified and the patched jar is inside run-to-run noise (19–39 chunks/s for the same
  settings); idle CPU is lower. The patch is on by default (`[worldgen] patch = true`) and falls back
  to the unmodified jar with a warning when patching fails.
- Vanilla 26.3 already generates on its own worker pool (87% of CPU samples). Top costs: structure
  stages 14%, terrain noise 12%, surface 8%, chunk parse from disk 4%, block counts 4%.
- Unmodified hosts differ from each other block by block, so comparisons use a tolerance.
- `LevelTicks.tick` must stay: scheduled ticks settle leaves and fluids in generated chunks.
- GC: ParallelGC measured about 8% faster than G1 (51.8-53.9 vs 48.8-49.4 chunks/s, 8 request
  threads, 1 GB heap); now the default unless `jvm-args` names a collector. Memory was the same.

### Open problem and dead ends

- An earlier run saw the host stop answering with 8 request threads (1 in 8 runs). 11 later
  stress runs (5 at 8, 4 at 12, 2 at 16 threads, view distance 12) did not hang, and chunk rate
  rose 60 -> 102 -> 153 chunks/s for 4 -> 8 -> 16 threads, so the default is 8 and the clamp 16.
  If a hang returns, lower `threads`.
  Likely cause (unverified): `ServerChunkCache.getChunk` from a non-main thread hops through the server
  main thread, so concurrent requests can block each other there. Reproducing needs a run with the user's `eula=true`.
- Dead end: replacing the blocking `getChunk` with `getChunkFuture(x, z, FULL, true)` made every
  request fail with `Unloaded chunk` on 26.3. Reverted.
- Dead end: emptying `LevelTicks.tick` changes generated blocks (leaf distance). Kept out of the rules.
- Policy (changed 2026-11-07): replacing a hot piece of Mojang's generator with **our own,
  different algorithm that gives the same result** is allowed (a faster noise evaluation, a spatial
  index for biome lookup, fewer allocations), applied to the patched jar on the user's machine.
  Still forbidden: copying or paraphrasing Mojang's code (or decompiled/Paper-patch lines that contain
  it) into the repository, and shipping any of Mojang's classes. Such a replacement is written from
  the behaviour (inputs and outputs), and it is only kept if (1) a test compares its output with the
  unmodified jar over many positions and seeds and finds no difference, and (2) a measurement shows
  it is faster. C2ME/Moonrise-style modules are a reference for *what* to speed up, not code to copy
  (their licences must be checked before any of their code is used).

Each step is kept only if it beats the baseline for resident memory, CPU and chunks per second
(see `tools/` benchmark). Baseline: unpatched jar, 512 MB heap, about 280 MB live after load,
about 10 s start, 100–700 ms per chunk.
