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

### Profile of the host (JFR, 8 request threads, view 12, 625 chunks, 106 chunks/s)

Share of execution samples whose stack contains the area (areas overlap):
noise router / `doFill` 31%, surface (`MaterialSystem`) 22%, chunk serialisation 11%,
features 9%, biomes 7%, lighting 4%, structures 3%. The hottest single frame is
`GradientNoise.permute` at 6%, then `Aquifer.computeSubstance` 4%, `Climate$RTree.search` 3%.

The profile is flat. 26.3 already restructured noise (`GradientNoise`, `DensityBuffer`,
`MaterialSystem`), so Moonrise's 1.21 patches do not apply as they are, and a faster Perlin
permutation table would win a few percent at most. Threads were the large lever
(4 -> 8 -> 16 = 60 -> 102 -> 153 chunks/s).

## Rust worldgen (planned, requested 2026-11-08)

**Order: last, after the remaining gameplay/system work. This section records the plan only; do not
start implementation early. When that phase begins, the coordinator may split independent
algorithms into parallel workers with separate file ownership and integration tests.**

Goal: move the hot parts of terrain generation out of the JVM host into an independently written
Rust implementation (`ferrite-world` / `ferrite-server::game::vanilla_gen`), one piece at a time,
until the host is only needed for what is not yet ported. Parallelize by algorithm/data boundary
(noise and density evaluation, surface rules, biome lookup, features/structures, lighting and chunk
serialization), not by letting multiple workers edit the same core files. Each piece is specified
from observable inputs and outputs measured against the user's local reference jar; do not copy or
closely translate Mojang or third-party implementation code. Review licenses before reusing any
external code. Performance work informed by C2ME- and Moonrise-style techniques happens in this same
final Rust phase, alongside each corresponding algorithm, rather than as a later Java patch port.
Use those projects to identify optimization targets (parallel chunk stages, scheduling, caches and
I/O), check the version and license of each reference, and implement independently against
Ferrite's ownership model and measured behavior.

Order (by the JFR profile above, biggest first):

1. Noise router / `doFill` (31%) and surface rules (22%): density-function evaluation on the data
   pack's `noise_settings`, `density_function` and `noise` JSON (already extracted under `vanilla/`).
2. Biome lookup (7%): parameter-space search over `multi_noise` biome sources.
3. Features (9%) and structures (3%): placed features and structure starts from the data pack.
4. Lighting (4%) and chunk serialisation (11%): ferrite-world already owns the chunk format.

Use [Cubiomes](https://github.com/Cubitect/cubiomes) as a research and comparison reference for
biome/feature-generation behavior, version coverage, and test vectors. It is a C library focused on
biome and feature generation, not a complete block-level terrain generator; check its current
version coverage and license before relying on it. Do not copy its code into Ferrite without a
separate license and provenance review.

Per piece, the definition of done is the same as for patches: (1) a test compares the Rust output
with the unmodified jar over many positions and seeds and finds **no difference**, (2) a
measurement shows it is faster, (3) it runs behind a `[worldgen]` switch that falls back to the host.
The host stays the reference and the fallback; nothing of Mojang's is copied into the repository.
Needs the user's `eula=true` on the dev machine to produce the reference output.

### Storage idea for the Rust world: compressed world blob and in-memory chunk access (note only, not implemented)

Requested 2026-11-07 as a **note to remember** for the new Rust worldgen. Nothing here is built, and
**no database is being added** (no MySQL/Mongo/Redis); like the rest of this section it is the user's
local work.

The desired model is informed by ASWM / Slime-style compressed world storage:

- A world is **one compact, compressed blob** (chunks, block entities, entities and world data
  together, e.g. zstd) instead of a directory of per-chunk Anvil region files.
- The blob is **read once and kept whole in memory**. Chunk access is a lookup in that in-memory
  structure: no per-chunk file open/seek/decompress, no region-file locking.
- Saving writes the blob back as a unit (or only the changed parts, then re-packs). That keeps
  start-up and world cloning cheap: copy or load one buffer, e.g. one world per minigame or per
  instance, spawned in milliseconds.
- Best for small or bounded worlds. A huge, ever-growing survival map does not fit in memory, so
  Anvil stays for those and the blob format is for generated/bounded worlds.

Plan for Ferrite:

1. Put it behind the existing chunk-source seam (`ChunkSource`, today `AnvilChunkSource`, with
   `new_memory()` for tests), as another source, e.g. `BlobChunkSource`. The world code does not
   change, only where chunks come from and go to.
2. The **place the blob is stored must stay swappable**: a local file first; a different backend
   (network, object store, a database) can be added later behind a small `load()` / `store()` trait
   without touching the format or the world code.
3. The Rust worldgen writes its output straight into the in-memory blob, so freshly generated
   chunks never go through region files.
4. Define the blob layout with its own version number, and test round trip (write, read, equal) and
   that a clone is independent of the original.

What the speed-up really is: the format and keeping it in RAM, not the choice of Redis. A networked
store would only add latency; Ferrite's in-process memory mode (`new_memory()`) is the closest
equivalent and is what this idea builds on.
