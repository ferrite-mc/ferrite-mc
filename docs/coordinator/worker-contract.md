# Worker contract

What a dispatched worker receives, must do, and must report. Queue state lives in
[task-queue.md](task-queue.md). Commands below use only flags listed by
`orca orchestration <command> --help`.

## Task spec

Every dispatched task states these five sections:

1. **Target** — repo, baseline commit/branch, and the paths in play.
2. **Change** — the exact deliverable (files to create, behavior to alter).
3. **Constraints** — what is forbidden (source edits, installs, external posting, commits/pushes unless allowed).
4. **Ownership** — the only paths the worker may write. Anything outside needs an `ask`, not a guess.
5. **Observable acceptance** — checks a third party can run or see (file exists, command exits 0, links resolve).

A worker that finds the spec ambiguous or the ownership too narrow asks before acting.

## During the task

- Heartbeat about every 5 minutes while working:
  `orca orchestration send --from <handle> --type heartbeat --subject alive --task-id <task> --dispatch-id <dispatch> --phase <text>`
  (not needed while blocked inside `check --wait` or `ask`).
- Poll for coordinator follow-ups with `orca orchestration check --terminal <handle> --json`
  at natural checkpoints and once right before `worker_done`.
- Questions go through `orca orchestration ask --from <handle> --question <text> [--options <csv>] [--timeout-ms <n>]`.
  On timeout, resume with `ask --resume <message_id>` instead of asking again.

## worker_done shape

Sent exactly once, carrying **both** lifecycle IDs and an explicit outcome:

```sh
orca orchestration send --from <handle> --type worker_done \
  --subject "<short status>" \
  --body "<3 sentences: what was done, what was found, what is left>" \
  --task-id <task_id> --dispatch-id <dispatch_id> \
  --outcome succeeded|failed \
  [--files-modified a,b] [--report-path <path>]
```

- `--task-id` and `--dispatch-id` are both required so a late completion from a failed retry
  cannot settle the current dispatch.
- `--outcome` is required (`succeeded` or `failed`); failure is never encoded only in prose.
- `--body` is never empty. Add `--files-modified` only if files changed and `--report-path`
  only for a durable report.
- After sending, the worker idles: no new work, no polling loop.

## Escalation and retry

- **Escalate** when blocked before completion (tool unavailable, ownership conflict, failing
  baseline): `orca orchestration send --from <handle> --type escalation --subject "Blocked: <reason>" --body <details> --task-id <task> --dispatch-id <dispatch>`.
  Then wait for guidance; do not work around the block.
- **Fail honestly**: if the acceptance check cannot be met, send `worker_done --outcome failed`
  with the evidence rather than weakening the check.
- **Retry** is the coordinator's call. It records the failed dispatch id in the queue and runs
  `orca orchestration worker-start --task <task_id> --retry-of <dispatch_id> ...`. A retry reads
  the prior report and starts from the existing working copy, not a clean slate.
- To cancel: `worker-stop` (or `worker-abandon`), then
  `task-update --id <task_id> --status failed --result cancelled`.
- After two failed attempts on the same lane, the coordinator re-scopes or does it directly
  instead of a third blind retry.

## Disk policy

- Workers share one Cargo `target` directory. **Workers MUST NOT run `cargo clean`** (or delete
  `target/`, or point `CARGO_TARGET_DIR` elsewhere) — it destroys other lanes' incremental builds
  and can break a running test.
- If disk is tight, a worker escalates; it does not clean.
- The **coordinator cleans once, at the end**, after all lanes are `done` and reports are collected.

## Consent boundary

Workers do not install MCP servers, register crons, use computer control, post to Slack/GitHub
or other channels, commit, or push unless the task spec explicitly allows it. Results go back
only through the commands above.
