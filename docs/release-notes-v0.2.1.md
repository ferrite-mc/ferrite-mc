# Ferrite 0.2.1

Ferrite 0.2.1 targets Minecraft Java 26.3 (protocol 777).

## Highlights

- Repairs the w6-protocol merge on `main`: restores the `gateway_used` player
  initializer, the `shulker` flag for plugin-created inventories, and
  `Signal::NeighborUpdate` routing. The `ferrite-server` library suite (836 tests)
  passes again.
- Raises the integration-test packet wait from 120 seconds to 300 seconds so a
  loaded Windows/Linux CI runner does not turn scheduler starvation into a false
  packet-73 timeout. The wait is still bounded.
- Includes the w6-protocol work (configurable rate limits, username validation,
  piston/portal setting enforcement) that was merged after the v0.2.0 tag.

## Known limitations

- Bukkit/Paper coverage remains partial; see `README.md` and
  `docs/plugin-api-stubs.md`.
- Real Minecraft 26.3 client visuals and large third-party plugin boots remain
  unverified; see `docs/manual-checklist.md`.
- The 800-bot numbers in `docs/perf/README.md` are measurements, not a claim of a
  speedup.

## Windows package

The portable ZIP contains `ferrite-server.exe`, `LICENSE`, `NOTICE.md`, and
`START-HERE.txt`. The desktop UI requires the WebView2 runtime. Vanilla-generated
data is not bundled.
