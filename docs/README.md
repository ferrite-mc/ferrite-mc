# Documentation map

Current as of **2026-11-14**, `main` = `433b907`.

## Current references

- [`../README.md`](../README.md) / [`../README.en.md`](../README.en.md) — project status, build,
  verification and roadmap.
- [`BACKLOG.md`](BACKLOG.md) — the single list of open work.
- [`COMMANDS-STATUS.md`](COMMANDS-STATUS.md) — command-tree parity and remaining command blockers.
- [`claude-lanes-2026-11-14.md`](claude-lanes-2026-11-14.md) — ownership split for the two
  Claude lanes (scope and acceptance; the items themselves stay in `BACKLOG.md`).
- Plugin API:
  - [`plugin-api-stubs.md`](plugin-api-stubs.md) — method-level evidence and explicit refusals.
  - [`plugin-stubs-status.md`](plugin-stubs-status.md) — session-by-session plugin API status.
  - [`plugin-api-audit.md`](plugin-api-audit.md) — broader API audit.
  - [`plugin-compat-plan.md`](plugin-compat-plan.md) — compatibility plan and working rules.
- Gameplay and world:
  - [`end-dimension.md`](end-dimension.md) — End fight, gateways, credits and remaining gaps.
  - [`world-interaction-audit.md`](world-interaction-audit.md) — fluids, gravity, neighbors and crops.
  - [`pistons.md`](pistons.md) — piston behavior and client-only checks.
  - [`worldgen-host.md`](worldgen-host.md) — Java worldgen host and the deferred Rust worldgen plan.
- Operations:
  - [`CONFIG.md`](CONFIG.md), [`CONFIG-PAPER.md`](CONFIG-PAPER.md), [`windows-console.md`](windows-console.md)
  - [`manual-checklist.md`](manual-checklist.md) — checks that require a real Minecraft 26.3 client.
  - [`NET-WRITER-OFFLOAD.md`](NET-WRITER-OFFLOAD.md), [`OPTIMIZATIONS.md`](OPTIMIZATIONS.md),
    [`perf/README.md`](perf/README.md)

## Historical notes

These are kept for provenance. They describe earlier branches or one-off sessions and are **not**
the current work queue. Check the source and `BACKLOG.md` before acting on them.

- [`coordinator/remaining-2026-11-11.md`](coordinator/remaining-2026-11-11.md)
- [`coordinator/cleanup-2026-11-11.md`](coordinator/cleanup-2026-11-11.md)
- [`coordinator/orca-4lane.md`](coordinator/orca-4lane.md)
- [`handoff-runtime-worlds-2026-11-11.md`](handoff-runtime-worlds-2026-11-11.md)
- [`handoff/`](handoff/) — per-lane briefs, status notes and handoffs.
- [`release-notes-v0.1.6.md`](release-notes-v0.1.6.md),
  [`release-notes-v0.2.1.md`](release-notes-v0.2.1.md) — historical release notes.

## Conventions

- Do not add a new "remaining work" list outside `BACKLOG.md`; link to it instead.
- Mark a handoff or coordinator note historical when a later lane supersedes it.
- Keep real-client-only checks in `manual-checklist.md` and label automated test results separately.
