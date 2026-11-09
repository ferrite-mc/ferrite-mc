# Windows server console

`ferrite-server` opens a native Windows console window on Windows builds. The window is
implemented in `crates/ferrite-server/src/main.rs` behind `#[cfg(windows)]`; non-Windows
builds never compile it or link `native-windows-gui`. Closing the window is the console's
only job, so starting and stopping the server does not restart the process.

## Layout

```text
┌──────────────────────────────────────────────────────────────────────────┐
│  ● 실행 중             가동 00:04:21              서버 시작  서버 종료   │
│  콘솔 로그                            자동 스크롤  로그 지우기            │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │ [12:00:00 INFO] server listening on 0.0.0.0:25565                 │  │
│  │ ...                                                                │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│  명령  ┌──────────────────────────────────────────┐  보내기             │
│        └──────────────────────────────────────────┘                      │
│  Enter 전송 · ↑/↓ 명령 기록 · Tab 이동 · Alt+S 시작 · Alt+T 종료        │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Status bar** — server state plus a live uptime (`HH:MM:SS`) once the runtime reports
  that it started. The uptime label is hidden, not blank, when there is no uptime to show.
  Labels, the hint, and the read-only log use the console palette (light or dark, sampled at
  launch); buttons keep their native Windows chrome. Player count is intentionally not displayed: the server runtime does not
  expose a player count to the GUI yet, and adding one would require changes outside the
  console module.
- **Log** — read-only console output captured from `tracing`, using a fixed-width font.
  `자동 스크롤` is a toggle button showing `켜짐`/`꺼짐`; it controls whether new lines follow
  the bottom. `로그 지우기` clears the current contents without affecting server logging.
- **Command row** — sends a command to the running server. The input and `보내기` button
  are disabled while no server is running. Enter sends; Up and Down walk the command
  history (the current draft is restored when you walk past the newest entry).
- **Footer** — a short keyboard reminder in a smaller font.

The layout is a manual responsive grid rather than `nwg::GridLayout`: the status bar,
toolbar, command row, and footer keep fixed heights while the log absorbs every remaining
pixel when the window is resized.

## Keyboard

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | Move between controls (Windows' dialog manager handles this). |
| `Enter` (start button focused) | Start the server. The start button is focused at launch. |
| `Alt+S` | Start the server. |
| `Alt+T` | Stop the server and wait for a save-and-exit. |
| `Enter` (command box) | Send the command. |
| `Up` / `Down` (command box) | Previous / next command history entry. |
| `Alt+A` | Toggle auto-scroll (the button shows `켜짐`/`꺼짐`). |
| `Alt+L` | Clear the log. |
| `Alt+D` | Send the command (same as Enter). |

After Start, focus moves to the command box. After a normal stop, focus returns to the
start button so the whole start/stop cycle is reachable without a mouse. Button labels use
standard Windows `&` mnemonics, and the event loop calls `IsDialogMessageW`, so mnemonic
and Tab handling behave like a dialog.

## Starting, stopping, and closing

- **Start** asks the server manager to spawn the runtime thread. The status bar shows
  `● 시작 중…` until `ServerStarted` arrives, then `● 실행 중` with a running uptime.
- **Stop**, `Action::Exit`, and the close confirmation all reuse the existing `stop`
  command path (`server_manager` sends `"stop"` to `run_server`). The UI only changes
  presentation; server startup and shutdown are untouched.
- **Closing while stopped** closes immediately. The manager receives `Exit` and the process
  exits without starting a server.
- **Closing while running** is refused first. A modal Yes/No question asks whether to save
  and exit. `No` leaves the window and the server exactly as they were. `Yes` starts the
  normal shutdown, shows `● 저장 후 종료 중…`, and keeps the window open until the runtime
  reports `ServerStopped`; only then is the window destroyed. If the server is already
  stopping (Stop was clicked first), closing skips the redundant prompt and waits for the
  same shutdown.
- The close prompt is a native message box, so its default button behavior is Windows'
  (Enter activates the focused button); read the prompt before confirming.

## Log retention and auto-scroll

- The log is trimmed after the edit control reports more than **4000 lines**. Instead of
  clearing everything, the oldest block is dropped and **3000 lines** are kept, so a long
  session retains recent context.
- The Win32 edit control has no partial line-deletion message, so trimming reads the text,
  skips the oldest lines, and writes the remainder back with
  `set_text_unix2dos`. This happens only once per ~1000 lines rather than on every append.
- `TextBox::append` always scrolls to the bottom. With auto-scroll off, the console saves
  the first visible line (`EM_GETFIRSTVISIBLELINE`) before appending and restores it
  afterwards, so reading older output is not interrupted by new log lines. Turning
  auto-scroll back on immediately jumps to the newest line.

## Sizing, fonts, and DPI

- All GUI code is inside `#[cfg(windows)]`; Linux and other targets do not build it.
- The process calls `SetProcessDPIAware` before creating any window, then derives a scale
  factor from the system DPI. Window size, minimum size, margins, control sizes, and font
  sizes are all scaled from that factor. Fonts use `Segoe UI` for text and `Consolas` for
  the log and command input.
- The initial client area is **900 × 640** at 96 DPI, centered on the primary screen. The
  minimum window size is **780 × 540**. The window is resizable and maximizable, and the log
  grows or shrinks with it.
- The window is created hidden, laid out, sized, and only then shown to avoid a flash of
  unscaled controls.
- The client surface and label background use a light or dark palette selected from the
  system window colors at launch. A window-level `WM_CTLCOLORSTATIC` hook paints labels and
  the read-only log with the same surface and text colors; buttons keep their native theme.

## Known limitations

- **System DPI only.** The scale is read once at launch with system DPI awareness. Moving
  the window to a monitor with a different scale lets Windows bitmap-stretch it; it does
  not re-layout or rebuild fonts. Restart the server to pick up a different scale.
- **No player count.** See the status bar note above. The UI would need a new runtime data
  feed to show it.
- **Command history is session-only.** Up to 200 entries are kept in memory and are not
  written to disk; starting a new process starts an empty history.
- **No tab completion or command validation.** The console forwards whatever is typed to
  the existing server command path; validation and feedback come from the server log.
- **No ANSI color or rich text.** The log is plain text; `tracing` is initialized with
  `with_ansi(false)` and the edit control has no color parser.
- **Trim is a block operation.** Overflow is checked against the edit control's line count.
  For pathological single lines longer than the control's width, Windows may wrap them; the
  trim then falls back to keeping half the logical lines so the log still shrinks.
- **Theme is sampled at launch.** The console paints its own surface from the system
  window colors at startup. Changing the Windows theme while the console is running is
  picked up on the next launch.
- **Korean labels are built in.** The UI text is Korean; the underlying server log language
  is unchanged.
- **Close waits for the runtime.** If the server hangs during shutdown, the window stays in
  `저장 후 종료 중…` until the runtime returns. There is no forced-kill timer.
