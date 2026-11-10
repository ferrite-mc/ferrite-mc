# Windows server console

`ferrite-server` opens a Tauri 2 desktop window on Windows. The window is a custom,
frameless shell around a static HTML/CSS/JavaScript frontend; the Rust backend owns the
server and the webview only sends actions and receives events. On non-Windows targets the
GUI is not compiled, and `ferrite-server.exe --headless` keeps the terminal console path.

The GUI needs the Microsoft Edge WebView2 Runtime. It is included with current Windows 10
and Windows 11 installations; a portable build on a machine without it cannot render the
window.

## Layout

The window is 1080 × 720 by default, resizable down to 880 × 600, and centered at launch.
It has no native title bar.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│  F  Ferrite  server console                         Minecraft 26.3  v…  ─ □ × │
├──────────────────────┬───────────────────────────────────────────────────┤
│  ○ 중지됨             │  콘솔                    자동 스크롤  지우기      │
│  서버가 중지되어…     │                                                   │
│  UPTIME              │  2026-… INFO ferrite_server: …                    │
│  00:00:00            │                                                   │
│  [서버 시작] [중지]   │                                                   │
│  주소 / 버전 / 프로토콜│                                                   │
│  EULA 안내            │  › 명령어를 입력하세요                    전송 ↵  │
│                      │  ● 대기 중              Enter 전송 · ↑/↓ 기록    │
├──────────────────────┴───────────────────────────────────────────────────┤
│  FERRITE                                      Minecraft Java 26.3 · 777  │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Title bar** — brand mark, Minecraft target, app version, and minimize/maximize/close.
  Dragging the title bar moves the window; double-clicking it toggles maximize.
- **Status** — current phase, a short explanation, live uptime, and start/stop buttons.
  The address, version, and protocol come from `server_info`; the address is read from
  `ferrite.toml` without creating or logging the file.
- **Console** — the main output surface. `자동 스크롤` is on by default; turning it off
  keeps the reader at the current position while new lines arrive. `지우기` restores the
  empty-console message without affecting server logging.
- **Command bar** — disabled until the runtime reports that the server is ready. Enter
  sends, Up/Down walk the session history, and the current draft is restored when moving
  past the newest entry.
- **Footer** — the protocol hint and a compact keyboard reminder.

## Starting, stopping, and closing

- **Start** moves the UI to `시작 중` immediately. The Rust runtime sends
  `server-started` only after configuration, EULA, world, and plugin setup succeed and the
  listener has been created. A configuration or EULA failure therefore remains in the
  stopped phase and its error is written to the console instead of flashing `실행 중`.
- **Stop** sends the existing `stop` command and shows `종료 중` until `server-stopped`.
  The command input is disabled outside the running phase.
- **Stop during startup** sets a cooperative cancellation flag as well as sending `stop`.
  The runtime checks it after configuration, listener creation, world setup and plugin startup,
  so the window returns to `중지됨` without waiting for the full startup path. A blocking
  initialization step already in progress (for example the registry or plugin host) finishes
  before that checkpoint is reached.
- **Closing while stopped** exits immediately.
- **Closing while running** opens a native Yes/No confirmation. `No` leaves the window and
  server untouched. `Yes` sends the normal save-and-exit path and keeps the window open
  until `server-stopped`. If Stop was already pressed, closing skips the duplicate prompt.
- **Closing while starting** uses the same confirmation and cancellation path; a pending
  startup is not allowed to leave an invisible server behind.
- A shutdown watchdog exits the process after 30 seconds even if the server runtime does
  not report back. Logs still go through the runtime's tracing subscriber.
- The close confirmation uses the Win32 message box (`MB_YESNO`, default `No`), so a
  misplaced Enter does not shut down a live server.

## Keyboard

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab` | Move between focusable controls. |
| `Enter` (command box) | Send the command. |
| `Up` / `Down` (command box) | Previous / next history entry. |
| `Ctrl+L` | Clear the visible console. |
| `Enter` / `Space` | Activate the focused button. |
| `Alt+F4` | Request close (the same save-and-exit path as the close button). |

There are no `Alt+S`/`Alt+T` mnemonics in the Tauri shell.

## Log behavior

- The DOM console keeps at most **2,500 lines**. Once that limit is crossed, the oldest
  rendered line is removed; the server's own log stream is not truncated.
- `server-log` events are classified as `info`, `warn`, `error`, or `command` for
  presentation. Error and warning rows use a subtle white background instead of color.
- `자동 스크롤` is a per-window preference and resets to on when the process restarts.
- Command history is session-only (up to 200 entries) and is not written to disk.

## Theme, sizing, and platform details

- The interface is intentionally **white canvas + black console**. It does not sample the
  Windows theme and does not switch palettes at runtime.
- The custom title bar is part of the webview; only the outer window frame is native.
  Window dragging, minimize, maximize, and close are exposed through Tauri capabilities
  in `crates/ferrite-server/capabilities/default.json`.
- The frontend is embedded from `crates/ferrite-server/ui` through `frontendDist`. There
  is no Node/npm build step; edit the HTML/CSS/JS and rebuild the Rust binary.
- `--headless` bypasses Tauri entirely and reads commands from stdin. Linux always uses
  the terminal path.

## Known limitations

- **No player count.** The runtime does not expose one to the GUI yet.
- **No tab completion or command validation.** The command is forwarded to the existing
  server command path and feedback appears in the log.
- **Plain text logs.** ANSI color codes are stripped; there is no rich-text renderer.
- **Korean labels are built in.** Server log text is unchanged.
- **No auto-update or installer flow in this document.** `tauri.conf.json` describes an
  NSIS bundle, but the release ZIP is a portable executable plus its start notes.
- **WebView2 is a runtime prerequisite.** It is not bundled into the portable ZIP.
- **A wedged shutdown waits up to 30 seconds.** There is no force-kill button in the UI;
  the watchdog is the final fallback.
- **Startup cancellation is cooperative.** A blocking initialization step that is already
  running can delay the transition back to `중지됨`; it is not a hard cancellation.
