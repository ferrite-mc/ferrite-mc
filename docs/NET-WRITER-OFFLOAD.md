# Compression and encryption off the connection tasks: measurements and plan

Status: **deferred**. Nothing in the server changed for this item. This file records what was
measured on 2026-11-12, what it does and does not show, and the plan if the work is picked up.

## Where the work happens today

- Outbound: `write_loop` (`crates/ferrite-net/src/lib.rs`) runs in one tokio task per connection.
  `queue_batch` frames and compresses each queued packet (`FrameEncoder::encode`), then
  `WireWriter::write_batch` (`crates/ferrite-net/src/wire.rs`) CFB8-encrypts the whole batch in
  place before the socket write. Both run synchronously on a tokio worker.
- Inbound: `WireReader::poll_read` decrypts and `FrameDecoder::next_frame` decompresses inside the
  connection's read loop, also on a tokio worker. Serverbound traffic is small packets.
- Game ticks do **not** share these workers: region workers are dedicated `std::thread`s
  (`crates/ferrite-region/src/lib.rs`, `thread::spawn(... worker ...)`). The tick only pushes
  already-encoded bytes into the bounded per-connection queue. So inline compression and
  encryption can delay only socket-side tasks (readers, keep-alive answers, other writers), not
  the tick.

## Measurements

Machine: Intel Xeon Platinum 8358 @ 2.60 GHz, 12 CPUs, Linux, release profile, `aes-ni` CFB8
backend. Raw output is not committed; the commands below regenerate it.

### Per-operation cost

`cargo bench -p ferrite-net --bench cfb8 -- current` (median):

| operation | 64 KiB | 1 MiB |
|---|---:|---:|
| CFB8 encrypt | 832 µs (75.1 MiB/s) | 13.30 ms (75.2 MiB/s) |
| CFB8 decrypt | 153 µs (407 MiB/s) | 3.08 ms (325 MiB/s, noisy) |

`cargo bench -p ferrite-protocol --bench protocol -- 'compression_256KiB|frame_64KiB'` (median):

| operation | time |
|---|---:|
| frame_64KiB encode_plain / decode_plain | 1.57 µs / 3.22 µs |
| frame_64KiB encode_compressed | 1.048 ms (59.6 MiB/s) |
| frame_64KiB decode_compressed | 74.8 µs |
| compression_256KiB libdeflater compress / decompress | 4.58 ms / 279 µs |
| compression_256KiB flate2 compress / decompress | 16.1 ms / 562 µs |

CFB8 encryption is a serial dependency per stream, so one connection's encryption cannot be spread
over cores; compression of different packets could be.

### Does inline work disturb neighbouring tasks?

New bench: `cargo bench -p ferrite-net --bench writer_stage -- [workers] [seconds]`
(`crates/ferrite-net/benches/writer_stage.rs`). N simulated connections each compress (level 6,
threshold 256) and encrypt 4 x 64 KiB chunk-shaped packets every 50 ms, either `inline` in the
connection task (as `write_loop` does) or on `spawn_blocking`. Eight probe tasks sleep 1 ms and
record how late they wake; the table is that lateness in µs. The ~1.07 ms p50 floor is tokio's
1 ms timer granularity, not load. One run per row except where two runs are shown; 5 s per row.

12 workers, 12 CPUs (run 1 / run 2):

| conns | mode | p50 | p99 | max | batches |
|---:|---|---:|---:|---:|---:|
| 8 | inline | 1071 / 1074 | 5755 / 6075 | 8069 / 8401 | 703 / 701 |
| 8 | offload | 1070 / 1070 | 1593 / 1581 | 4241 / 1995 | 706 / 704 |
| 32 | inline | 1094 / 1099 | 4791 / 5581 | 6437 / 7769 | 2785 / 2775 |
| 32 | offload | 1080 / 1082 | 1867 / 1874 | 4365 / 2055 | 2791 / 2791 |
| 64 | inline | 1183 / 1261 | 5868 / 7271 | 18639 / 17596 | 5391 / 5289 |
| 64 | offload | 1096 / 1139 | 3388 / 5164 | 8040 / 11832 | 5433 / 5083 |
| 128 | inline | 32012 / 36069 | 75944 / 86411 | 87732 / 111183 | 6990 / 6639 |
| 128 | offload | 2219 / 2515 | 35950 / 57119 | 55558 / 158280 | 6632 / 6088 |

4 workers pinned to 4 CPUs (`taskset -c 0-3 ... writer_stage 4 5`), one run:

| conns | mode | p50 | p99 | max | batches |
|---:|---|---:|---:|---:|---:|
| 8 | inline | 1075 | 5816 | 8392 | 704 |
| 8 | offload | 1072 | 1764 | 4337 | 697 |
| 32 | inline | 7142 | 15223 | 18017 | 2507 |
| 32 | offload | 1131 | 6135 | 10915 | 2384 |
| 64 | inline | 70724 | 145848 | 170703 | 2488 |
| 64 | offload | 1076 | 69405 | 103946 | 2384 |
| 128 | inline | 188069 | 339676 | 369309 | 2505 |
| 128 | offload | 1122 | 160803 | 280024 | 2446 |

A 4-worker run on the 12-CPU box without pinning (not tabulated) left eight idle cores for the
blocking pool and flattered offload more than a real 4-core host does; the pinned run is the one
to read.

## What this shows, and what it does not

- Moving the work to `spawn_blocking` kept the probe tasks' p50 near the timer floor and cut their
  p99 by roughly 2-4x at 8-32 busy connections on 12 CPUs, and p50 from 7.1 ms (32 connections)
  and 70.7 ms (64 connections) to about 1.1 ms on the pinned 4-CPU host.
- It did **not** raise the number of batches completed. Totals were equal within run-to-run noise
  or slightly lower with offload (for example 2384 vs 2507 at 32 connections, 4 CPUs). The work
  costs the same CPU; offload only changes which threads wait.
- The load is synthetic: every connection streams 256 KiB of chunk data per 50 ms (about 5 MiB/s
  raw each). That resembles many players joining at once, not steady-state play, where serverbound
  and entity traffic is small and mostly below the compression threshold.
- Not measured: any effect on real gameplay latency or MSPT, the real `write_loop` (the bench
  uses the same `FrameEncoder` and `Cfb8` but not the queue, linger window or sockets), the
  reader side, or the 800-bot run (BACKLOG section 6 still lists that re-run as open).
- Each measured effect is a few milliseconds of extra delay for socket tasks during join bursts at
  8 or more concurrent chunk streams. No evidence here that it affects ticks, which run on their
  own threads.

## Update 2026-11-12 (W7): measured on the 800-bot run

The plan's step 1 is done: `examples/bots.rs` now records, on the server's own runtime during
the join storm and the walk, how late four 1 ms timer tasks fire, the round trip of a `/list`
command from a probe bot, the server-measured keepalive RTT, bot join times and received
throughput (full tables and caveats in `docs/perf/README.md`, raw files in `docs/perf/w7/`).

Four runs (two per variant, 800/800 bots joined and walked every time):

- During the join storm, when chunk compression and encryption peak, the timer probes fire at
  the timer floor (p50 1.14-1.18 ms) with p99 4.2-6.8 ms.
- During the walk, with little chunk traffic, they are much later (p50 1.4-1.5 ms, p90 21-31 ms,
  p99 94-115 ms), because the process runs at 900-1000% CPU of 1200% on region ticks and the
  800 in-process bots.
- The `/list` round trip stays at 3-4 ms through the join storm and rises to a 31-127 ms median
  in the walk, following the tick time (superflat p50 MSPT 59-69 ms), not the writer.
- Keepalive RTT p50 is 28-34 ms and join time p50 27-37 ms.

So the socket-task delay that offload would remove is not the dominant latency in this
workload: the inline writer work is a small part of what delays tasks during the storm, and the
long delays occur when there is almost no writer work. The bots share the runtime and join
staggered 5 ms apart over 4 s, a milder storm than 800 simultaneous joins; with a smaller,
more bursty join (or fewer cores) the synthetic `writer_stage` numbers above may apply, and
that was not measured. **Decision unchanged: deferred, with this evidence.** Nothing in
`ferrite-net`'s writer or reader was changed.

## Decision

Deferred. The bench shows a tail-latency isolation benefit for non-writer tasks under heavy
chunk streaming and no throughput benefit, and nothing yet shows that the benefit matters to
players. It also needs a non-trivial rework of `write_loop`'s ownership model. Re-evaluate after
the 800-bot re-run if socket-task latency shows up there.

## Plan if picked up

1. Add a measurement to the 800-bot harness first: record keep-alive round-trip and
   serverbound-packet handling delay during the join burst, inline. This is the "before".
2. In `write_loop`, give the connection a small compression/encryption stage: collect the batch as
   today, then run `FrameEncoder::encode` for every packet at or above the compression threshold
   plus the single `Cfb8::encrypt` pass on one `spawn_blocking` job per batch, moving the cipher
   and output buffer in and back (as the bench does). Keep batches below a size cutoff (start
   around the compression threshold times a few packets) inline, because the handoff costs more
   than the work for small batches; pick the cutoff from a bench, not a guess.
3. Preserve ordering: one job in flight per connection, since CFB8 state is sequential. Keep the
   queue overflow and write-timeout behaviour unchanged, and keep the disconnect packet last.
4. Optional, only if profiling shows single-connection encryption is the limit: compress the
   packets of one batch in parallel and encrypt afterwards in order. Encryption itself cannot be
   split.
5. Leave the reader inline unless the 800-bot data shows otherwise; inbound traffic is small and
   decryption is about 5x faster than encryption.
6. Re-run `writer_stage` and the 800-bot measurement after, and record both here. Do not claim a
   speedup that those runs do not show.
