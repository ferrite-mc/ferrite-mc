# Handoff: Plugin B End-to-End Plugin Messaging

- **Branch**: `yxshadxw/plugin-b-plugin-messaging` (branched from `origin/main` at `873d173e`)
- **Date**: 2026-11-18

## Summary

Implemented end-to-end Play-state plugin messaging across `ferrite-protocol`, `ferrite-net`, `ferrite-plugin-host`, `ferrite-server`, `java/ferrite-host`, and `java/ferrite-api-impl`:

1. **Protocol (`ferrite-protocol` + `ferrite-net`)**:
   - Added `ServerboundCustomPayload` (Play state, serverbound ID `22` per `vanilla/26.3/generated/reports/packets.json`) with round-trip unit test in `crates/ferrite-protocol/src/packets/play.rs`.
   - Wired `ServerboundPlay::CustomPayload` decoding in `crates/ferrite-net/src/game.rs`.
2. **Contract Event & Ops (`ferrite-plugin-host` + `java/ferrite-host`)**:
   - Event `plugin_message`: `{"player_uuid","player_name","channel","data_base64"}` (added to `protocol::EVENTS` and `HostRuntime.EVENTS`).
   - Ops `plugin_message.register`, `plugin_message.unregister`, `plugin_message.send` (added to `Bootstrap.java` and `native.rs`):
     - `plugin_message.register`: validates known plugin + channel length (`1..=256`), tracks per-plugin incoming/outgoing channel ownership in `NativeState`.
     - `plugin_message.unregister`: removes a specific channel or all channels for a plugin on disable; `HostMessengerBridge` treats `shutdown` as idempotent during `onHostStopping` after `NativeState::shutdown_provider` has already cleared all channels.
     - `plugin_message.send`: verifies the sending plugin owns the outgoing channel (`channel_not_registered` otherwise), enforces `MAX_PLUGIN_MESSAGE_BYTES = 1_048_576` (`payload_too_large` otherwise), and dispatches `HostStateProvider::send_plugin_message` (targeted player UUID or all online players when `player_uuid` is omitted).
3. **Server & Facade Routing (`ferrite-server` + `java/ferrite-api-impl`)**:
   - Inbound: `region.rs` forwards `ServerboundPlay::CustomPayload` to `ServerHooks::plugin_message_received`. `HostHooks` rejects payloads `> MAX_SERVERBOUND_PLUGIN_MESSAGE_BYTES` (`32_767` bytes) or unregistered channels before queuing `plugin_message` for the host tick; `FerriteHostApi` decodes `data_base64` and dispatches via `FerriteMessenger#dispatchIncomingMessage` (copying the byte array).
   - Outbound: `FerriteServer#sendPluginMessage` and `FerritePlayer#sendPluginMessage` validate via `StandardMessenger.validatePluginMessage` (ownership, enabled plugin, size) and forward through `WorldStateProvider::send_plugin_message` -> `PlayerCommand::Packet(PluginPacket::PluginMessage)` -> `container.rs::send_plugin_packet` (`play::CustomPayload`, clientbound ID `24`).
4. **WorldEdit `worldedit:cui` Check**:
   - Verified `worldedit:cui` channel registration/sending in `CommandMapAndMessengerTest` and booted `worldedit-bukkit-7.4.6-dist.jar` headlessly on `ferrite-server`.

## Tests

- `java`: `:ferrite-api-impl:test` (`HelpMapTest`, `CommandMapAndMessengerTest`, `TypicalFixtureIntegrationTest`, `FerriteHostApiTest`, etc.) and `:ferrite-host:test` passed.
- `ferrite-protocol`: 62 tests passed (including `ServerboundCustomPayload` ID `22` round-trip).
- `ferrite-plugin-host`: 19 tests passed (including `plugin_message_channels_enforce_ownership_size_and_shutdown_cleanup` + `jvm_smoke`).
- `ferrite-server --test plugin_b_server_acceptance`: passed end-to-end (unregistered channel rejection, oversized inbound `32,768`-byte payload rejection, bidirectional `ferrite:echo` via `Player#sendPluginMessage`, and `Server#sendPluginMessage`).

## Limitations

- Configuration-state `ServerboundCustomPayload` (ID `2`, used for `minecraft:brand`) remains separate and handled in `ferrite-net`; Bukkit `Messenger` routing applies to Play state only.
- `World#sendPluginMessage` remains unimplemented (`UnsupportedWorldBase`).
- WorldEdit headless boot only verifies `worldedit:cui` registration and clean enable/disable; full in-game WorldEdit compatibility and real Minecraft 26.3 client testing are not claimed.
