# Network-hardening A/B — 2026-11-23

Base is the release `examples/bots` binary built at `093218de`, copied to `/tmp` before the
working tree was rebuilt. The hardened tree adds no tick-path optimization; the bot runs below
are a regression check for the network changes, not a speedup claim.

## Changes under test

- **Play keepalive timeout**: the writer already sent a keepalive every 10 s, but never dropped a
  client that did not answer. `ServerConfig::keepalive_timeout` (default 30 s, vanilla's value)
  now disconnects the connection with the `Timed out` play disconnect once the oldest unanswered
  keepalive is older than the limit. This bounds player slots, writer tasks and send budgets for
  half-open or deliberately mute clients.
- **Configuration read timeout**: the configuration loop sent keepalives but had no read
  deadline, so a client that stopped sending could hold the login connection forever. Each wait
  now uses `ServerConfig::read_timeout` (30 s).
- **Accept-loop resilience**: a transient `accept()` failure (for example `EMFILE` during a
  connection burst) now logs, backs off 20 ms and keeps the listener alive instead of ending
  `serve()` for every connected player.
- **Session authentication client**: Mojang's `hasJoined` call now uses one pooled
  `reqwest::Client` with a 5 s connect timeout and a 10 s total timeout, instead of building a
  client per login. A stalled session server can no longer hold a login task indefinitely.
- **Per-connection config clone**: the server-list favicon is `Arc<str>` rather than `String`, so
  cloning `ServerConfig` for each accepted connection does not copy the (up to tens of KiB)
  data URI.
- **Writer batch sizing**: `flush_bodies_with` computes the work and wire-size totals in one pass
  instead of iterating the batch twice before encoding it.

Regression tests added: `unanswered_keepalive_disconnects_the_client` (asserts a keepalive is
sent and the final frame is `PlayDisconnect("Timed out")`) and
`silent_configuration_client_is_disconnected_by_the_read_timeout` (real TCP login through
configuration, then silence and EOF). `PingProbe::unanswered_for` is covered by the existing
ping test.

## Verification

- `cargo test -p ferrite-net --lib`: **63 passed, 0 failed**.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all --check`: pass.
- `cargo test --workspace --all-features`: **pass**. The first attempt failed while linking
  example binaries with an LLVM `Bus error` because the host filesystem was 100% full (60 MB
  free); after cleaning the regenerable `target/` artifacts the rerun completed with exit 0.

## Workload

Same host and harness as `docs/perf/README.md`:

```sh
ulimit -n 65535
# 200 bots: harness default view=8, probes off
BOTS_STAGGER_MS=2 BOTS_PROBES=0 <binary> 200
# 800 bots: view=2, 30 s walk, no /forceload
BOTS_STAGGER_MS=5 BOTS_VIEW=2 <binary> 800
```

Each invocation runs the superflat world and then the pre-generated `fixture` world. Every run
reported `200/200` or `800/800` bots joined and walked to the deadline. Order was base,
hardened, hardened, base; the host was shared and the 1-minute load at each run start is:

| Batch | base r1 | hardened r1 | hardened r2 | base r2 |
|---|---:|---:|---:|---:|
| 200 bots | 3.52 | 3.72 | 3.85 | 4.87 |
| 800 bots | 9.37 | 23.54 | 29.24 | 20.47 |

The 800-bot process CPU also moved between 528.7% and 823.7% across the four runs, so the host
was not quiet. Compare paired means, not absolute values.

Raw logs: `docs/perf/2026-11-23/bots-200-{base,opt}-harden-r{1,2}.txt` and
`docs/perf/2026-11-23/bots-800-{base,opt}-harden-r{1,2}.txt`.

## Results

### 800 bots (`BOTS_VIEW=2`)

| World | Metric | base r1 | base r2 | hardened r1 | hardened r2 | base mean | hardened mean | Change |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| superflat | avg MSPT | 20.676 ms | 15.402 ms | 23.682 ms | 16.025 ms | 18.039 ms | 19.854 ms | +10.1% |
| superflat | p99 MSPT | 48.797 ms | 42.730 ms | 50.464 ms | 37.581 ms | 45.764 ms | 44.023 ms | -3.8% |
| fixture | avg MSPT | 5.021 ms | 4.320 ms | 4.632 ms | 4.752 ms | 4.671 ms | 4.692 ms | +0.5% |
| fixture | p99 MSPT | 31.798 ms | 23.191 ms | 28.019 ms | 28.519 ms | 27.495 ms | 28.269 ms | +2.8% |
| superflat | join p50 | 47.830 ms | 44.546 ms | 45.419 ms | 44.562 ms | 46.188 ms | 44.991 ms | -2.6% |
| fixture | join p50 | 44.644 ms | 44.370 ms | 45.021 ms | 44.444 ms | 44.507 ms | 44.733 ms | +0.5% |
| superflat | mean RSS | 648.6 MiB | 631.5 MiB | 601.8 MiB | 648.4 MiB | 640.1 MiB | 625.1 MiB | -2.3% |
| fixture | mean RSS | 859.4 MiB | 861.0 MiB | 823.3 MiB | 893.0 MiB | 860.2 MiB | 858.2 MiB | -0.2% |

### 200 bots (harness default `view=8`)

| World | Metric | base r1 | base r2 | hardened r1 | hardened r2 | base mean | hardened mean | Change |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| superflat | avg MSPT | 5.679 ms | 5.992 ms | 5.806 ms | 4.564 ms | 5.836 ms | 5.185 ms | -11.2% |
| superflat | p99 MSPT | 15.440 ms | 16.249 ms | 17.619 ms | 9.981 ms | 15.845 ms | 13.800 ms | -12.9% |
| fixture | avg MSPT | 2.100 ms | 1.905 ms | 2.438 ms | 2.126 ms | 2.003 ms | 2.282 ms | +13.9% |
| fixture | p99 MSPT | 15.488 ms | 13.520 ms | 27.962 ms | 15.815 ms | 14.504 ms | 21.889 ms | +50.9% |
| superflat | join p50 | 44.359 ms | 47.277 ms | 44.381 ms | 44.673 ms | 45.818 ms | 44.527 ms | -2.8% |
| fixture | join p50 | 44.789 ms | 44.328 ms | 44.371 ms | 44.296 ms | 44.559 ms | 44.334 ms | -0.5% |
| superflat | mean RSS | 227.2 MiB | 254.0 MiB | 229.9 MiB | 222.2 MiB | 240.6 MiB | 226.1 MiB | -6.0% |
| fixture | mean RSS | 389.0 MiB | 403.1 MiB | 401.0 MiB | 391.2 MiB | 396.1 MiB | 396.1 MiB | 0.0% |

## Reading

- **No consistent bot-level change.** The 200-bot superflat average moved -11.2% while its
  fixture average moved +13.9%; the 800-bot superflat average moved +10.1% while its p99 moved
  -3.8%. Those directions cancel, and the host load rose from 3.5 to 29.2 over the sequence
  (with in-run CPU varying by ~300 percentage points), so the pair means are inside the noise
  this harness produces. No speedup or regression is claimed for tick time.
- **Connection behavior is unchanged where it should be.** Join p50 stayed at 44–48 ms for both
  binaries and both worlds, and keepalive RTTs stayed in the normal range. The new timeout only
  fires for clients that do not answer at all; every bot answered.
- **The pass is justified by security/availability, not a tick-path speedup.** The timeout and
  accept changes bound resource use for half-open, stalled and abusive connections. The
  favicon/clone and writer-batch changes remove per-connection and per-flush work, but that work
  is below the resolution of a 200/800-bot tick benchmark; they are covered by the compile,
  clippy and unit-test checks rather than by a claimed MSPT delta.
