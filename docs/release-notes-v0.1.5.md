# Ferrite v0.1.5

## Highlights

- Replaced the Windows console window with a Tauri 2 desktop console for server start/stop,
  command input, and log viewing. The existing `--headless` path remains available; Linux stays
  headless.
- Added the runtime-world storage catalog groundwork, including stable world IDs and separate
  Anvil-backed storage. Runtime worlds are not yet registered with the region scheduler, so
  Bukkit `Server#createWorld` and `unloadWorld` remain unsupported.

## Requirements

- Windows GUI builds require the Microsoft Edge WebView2 Runtime. The portable executable does
  not bundle WebView2 or vanilla data.
- Prepare `vanilla/26.3/generated/` in the Ferrite working directory before starting the server.
- Use `--headless` for the terminal console on Windows; Linux always uses the terminal console.

## Known limitations

- There is no auto-updater or installer in the portable ZIP. The NSIS bundle configured for Tauri
  is not included in that ZIP.
- A real Minecraft 26.3 client has not been verified. Protocol-bot tests do not validate client
  rendering or animations.
- Bukkit/Paper support remains partial. NMS-typed CraftBukkit methods intentionally fail with
  `NoSuchMethodError`.
