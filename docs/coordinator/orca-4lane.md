# Orca four-lane runbook — 2026-11-11

> **Historical runbook (2026-11-11).** The W5 lanes are complete. See `docs/BACKLOG.md` and
> `docs/README.md` for the current work and documentation map.

How the W5 lanes are started and driven. Everything marked **verified** was read from `orca … --help`
or `orca terminal list --json` on this host on 2026-11-11. Anything I could not verify is listed under
"Not verified" instead of being guessed.

## Lanes

| Lane | Branch | Worktree (`/home/ubuntu/orca/workspaces/ferrite-mc/…`) | Terminal handle | Target dir |
|---|---|---|---|---|
| L1 | `w5-world-plugin` | `rw-chunk-api` | `term_dffde10b-314c-45ce-9d55-931262ad2160` | `target-rw2` |
| L2 | `w5-ai-gameplay` | `rw-player-world` | `term_a452282d-cc9d-4ff7-b20d-9f2d47f4344b` | `target-rw1` |
| L3 | `w5-perf-docs` | `rw-plugin-cmd-thread` | `term_ec631ec3-5c7c-4a33-aab0-ba3d3cfb4f44` | `target-rw3` |
| L4 | `w5-residual` | `w5-residual` | `term_3ec5966a-ba99-4bb6-824f-dad90012328a` (agent) | `target-rw4` |

The briefs name the first three handles in short form (`term_dffde10b`, …); `orca terminal` takes the full handle.
`w5-residual` also has a plain-shell terminal, `term_8a4bf284-3d58-4a2c-b87e-99b6a3ae6c8e` (no agent).
Briefs: `/home/ubuntu/ferrite/briefs/w5-{world-plugin,ai-gameplay,perf-docs,residual}.md`.

## Commands (verified against `--help`)

```sh
# 1. worktree (optionally with an agent in its first terminal)
orca worktree create --name w5-residual --base-branch origin/main \
    [--agent claude --model sonnet --prompt "<text>"] --json

# 2. a terminal in that worktree (visible tab, focus unchanged unless --focus)
orca terminal create --worktree branch:w5-residual --title "L4" [--command "<text>"] --json

# 3. start the agent in it, pinned to sonnet
orca terminal send --terminal <handle> --text "claude --model sonnet --dangerously-skip-permissions" --enter --json

# 4. give it the brief
orca terminal send --terminal <handle> \
    --text "Read /home/ubuntu/ferrite/briefs/w5-residual.md in full, then execute it exactly." \
    --enter --wait-submit 20 --json

# 5. inspect
orca terminal list --json          # handles, worktreePath, branch, agentIdentity
orca terminal read --terminal <handle>
orca terminal wait --terminal <handle> --for tui-idle --timeout-ms 600000
```

Model rule for every W5 lane: `claude --model sonnet` only.

## Checking `input_accepted -> turn_started`

`orca terminal send --text … --enter` returns a receipt that separates input acceptance from observed
submission and turn start. With `--json`, require all three:

1. `input_accepted` — the text reached the PTY;
2. the prompt was observed as submitted (use `--wait-submit <seconds>`);
3. `turn_started` — the agent began a turn.

If `--wait-submit` times out, the CLI returns the queued/input-accepted receipt and **never resends**. After an
ambiguous transport failure, re-run the exact same command with the `--retry-request <id>` from the receipt; the
id is bound to the prompt payload and the terminal's process incarnation, so it cannot double-submit. Do not
re-send by hand. Confirm with `orca terminal read --terminal <handle>` that the agent shows the brief.
(The wave 3 and wave 4 queue entries record "confirmed `input_accepted -> turn_started`" for each lane.)

## `MSYS_NO_PATHCONV=1` and `/clear`

On the Windows/Git Bash coordinator, MSYS rewrites any argument that looks like a POSIX path into a Windows
path. A bare slash command sent as `--text /clear` would arrive as `C:/Program Files/Git/clear`; that is exactly
what an earlier lane session recorded as its third user message. Therefore:

- Export `MSYS_NO_PATHCONV=1` for every `orca terminal send` and `orca worktree create` issued from Git Bash.
- **`/clear` must never be path-converted.** Send it as
  `MSYS_NO_PATHCONV=1 orca terminal send --terminal <h> --text "/clear" --enter`, then
  `orca terminal read` to confirm the literal `/clear` was accepted.
- Brief paths such as `/home/ubuntu/ferrite/briefs/…` in `--text` are subject to the same conversion on a
  Windows host. This Linux host does not convert them; the variable is harmless here.

## Visibility

`orca terminal create` "creates a visible terminal tab without switching focus when possible; falls back to a
background handle if the UI cannot adopt it. Pass `--focus` to switch to it." A background handle still accepts
`send` and `read`. `orca terminal list --json` reports `connected` and `writable` per handle. Per the L3 brief,
Orca app visibility is coordinator-owned; lanes must not create terminals or change it.

## Not verified

- The exact Orca application "visibility" setting the coordinator changed: it is not recorded in the repo, the
  briefs or `orca … --help`. Only the `--focus` / background-handle behaviour above is.
- The literal JSON key names of the `send` receipt beyond the three stage names. Run once with `--json` and read it.
- Whether `MSYS_NO_PATHCONV` is still needed: no Windows host is available here.
