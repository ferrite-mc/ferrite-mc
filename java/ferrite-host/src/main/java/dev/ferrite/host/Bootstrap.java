package dev.ferrite.host;

/**
 * The Rust→Java entry points fixed by contract v1, plus the v2 state transport helpers.
 *
 * <p>Every method returns a JSON string and converts exceptions to an {@code {ok:false,...}}
 * envelope, so a plugin error never escapes through JNI as an unhandled Java exception.
 */
public final class Bootstrap {
    /** Minimum supported contract version in {@code docs/plugin-b-host-contract.md}. */
    public static final long MIN_CONTRACT_VERSION = 2;
    /** Contract version in {@code docs/plugin-b-host-contract.md}. */
    public static final long CONTRACT_VERSION = 3;

    private Bootstrap() {
    }

    public static String start(String configJson) {
        return guarded(() -> HostRuntime.start(configJson));
    }

    public static String loadPlugin(String pluginJson) {
        return guarded(() -> HostRuntime.instance().loadPlugin(pluginJson));
    }

    /** Runtime hot-unload of a single loaded JVM plugin by name. */
    public static String unloadPlugin(String pluginName) {
        return guarded(() -> HostRuntime.instance().unloadPlugin(pluginName));
    }

    public static String fireEvent(String name, String payloadJson) {
        return guarded(() -> HostRuntime.instance().fireEvent(name, payloadJson));
    }

    public static String runTask(long handle) {
        return guarded(() -> HostRuntime.instance().runTask(handle));
    }

    public static String runTaskAsync(long handle) {
        return guarded(() -> HostRuntime.instance().runTaskAsync(handle));
    }

    public static String forgetTask(long handle) {
        return guarded(() -> HostRuntime.instance().forgetTask(handle));
    }

    public static String runCommand(String originJson, String commandLine) {
        return guarded(() -> HostRuntime.instance().runCommand(originJson, commandLine));
    }

    public static String shutdown(String reasonJson) {
        return guarded(() -> HostRuntime.shutdown(reasonJson));
    }

    public static String status() {
        return guarded(HostRuntime::status);
    }

    /** v2 transport helper: sends one {@code world.block_get} request envelope. */
    public static String worldBlockGet(String requestJson) {
        return guarded(() -> HostBridge.call("world.block_get", requestJson));
    }

    /** v2 transport helper: sends one {@code world.block_set_async} request envelope. */
    public static String worldBlockSetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("world.block_set_async", requestJson));
    }

    /** v2 transport helper: sends one {@code world.chunk_loaded} request envelope. */
    public static String worldChunkLoaded(String requestJson) {
        return guarded(() -> HostBridge.call("world.chunk_loaded", requestJson));
    }

    /** v2 transport helper: sends one {@code player.state_get} request envelope. */
    public static String playerStateGet(String requestJson) {
        return guarded(() -> HostBridge.call("player.state_get", requestJson));
    }

    /** v2 transport helper: sends one {@code player.inventory_get} request envelope. */
    public static String playerInventoryGet(String requestJson) {
        return guarded(() -> HostBridge.call("player.inventory_get", requestJson));
    }

    /** v2 transport helper: sends one {@code player.inventory_set_async} request envelope. */
    public static String playerInventorySetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("player.inventory_set_async", requestJson));
    }

    /** v2 transport helper: sends one {@code entity.state_get} request envelope. */
    public static String entityStateGet(String requestJson) {
        return guarded(() -> HostBridge.call("entity.state_get", requestJson));
    }

    /** v1/v3 capability discovery helper: sends {@code host.info}. */
    public static String hostInfo() {
        return guarded(() -> HostBridge.call("host.info", "{}"));
    }

    /** v3 transport helper: sends one {@code world.chunk_status} request envelope. */
    public static String worldChunkStatus(String requestJson) {
        return guarded(() -> HostBridge.call("world.chunk_status", requestJson));
    }

    /** v3 transport helper: sends one {@code world.loaded_chunks} request envelope. */
    public static String worldLoadedChunks(String requestJson) {
        return guarded(() -> HostBridge.call("world.loaded_chunks", requestJson));
    }

    /** v3 transport helper: sends one {@code world.entities_snapshot} request envelope. */
    public static String worldEntitiesSnapshot(String requestJson) {
        return guarded(() -> HostBridge.call("world.entities_snapshot", requestJson));
    }

    /** v3 transport helper: sends one {@code world.biome_get} request envelope. */
    public static String worldBiomeGet(String requestJson) {
        return guarded(() -> HostBridge.call("world.biome_get", requestJson));
    }

    /** v3 transport helper: sends one {@code entity.equipment_set_async} request envelope. */
    public static String entityEquipmentSetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("entity.equipment_set_async", requestJson));
    }

    /** v3 transport helper: sends one {@code entity.drop_chance_set_async} request envelope. */
    public static String entityDropChanceSetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("entity.drop_chance_set_async", requestJson));
    }

    /** v3 transport helper: registers an incoming/outgoing plugin messaging channel. */
    public static String pluginMessageRegister(String requestJson) {
        return guarded(() -> HostBridge.call("plugin_message.register", requestJson));
    }

    /** v3 transport helper: unregisters plugin messaging channels for a plugin. */
    public static String pluginMessageUnregister(String requestJson) {
        return guarded(() -> HostBridge.call("plugin_message.unregister", requestJson));
    }

    /** v3 transport helper: sends a plugin message on a registered outgoing channel. */
    public static String pluginMessageSend(String requestJson) {
        return guarded(() -> HostBridge.call("plugin_message.send", requestJson));
    }

    /** v3 transport helper: sends a title/subtitle or clear-title packet to a player. */
    public static String playerTitleSend(String requestJson) {
        return guarded(() -> HostBridge.call("player.title_send", requestJson));
    }

    /** v3 transport helper: sends an action-bar message packet to a player. */
    public static String playerActionBarSend(String requestJson) {
        return guarded(() -> HostBridge.call("player.action_bar_send", requestJson));
    }

    /** v3 transport helper: plays a sound packet for a player. */
    public static String playerSoundPlay(String requestJson) {
        return guarded(() -> HostBridge.call("player.sound_play", requestJson));
    }

    /** v3 transport helper: spawns a particle burst packet for a player. */
    public static String playerParticleSpawn(String requestJson) {
        return guarded(() -> HostBridge.call("player.particle_spawn", requestJson));
    }

    /** v3 transport helper: sends a boss bar packet update to a player. */
    public static String playerBossBarUpdate(String requestJson) {
        return guarded(() -> HostBridge.call("player.boss_bar_update", requestJson));
    }

    /** v3 transport helper: updates the world border for online players. */
    public static String worldBorderSet(String requestJson) {
        return guarded(() -> HostBridge.call("world.border_set", requestJson));
    }

    /** v3 transport helper: sends a scoreboard packet update to a player. */
    public static String playerScoreboardUpdate(String requestJson) {
        return guarded(() -> HostBridge.call("player.scoreboard_update", requestJson));
    }

    /** v3 transport helper: updates a player's flight/invulnerability/speed abilities. */
    public static String playerAbilitiesSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.abilities_set", requestJson));
    }

    /** v3 transport helper: updates a player's tab list header and footer. */
    public static String playerListHeaderFooter(String requestJson) {
        return guarded(() -> HostBridge.call("player.list_header_footer", requestJson));
    }

    /** v3 transport helper: updates a player's experience level/progress/total. */
    public static String playerExperienceSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.experience_set", requestJson));
    }

    /** v3 transport helper: teleports a player to a world and coordinates. */
    public static String playerTeleport(String requestJson) {
        return guarded(() -> HostBridge.call("player.teleport", requestJson));
    }

    /** v3 transport helper: sets a player's health. */
    public static String playerHealthSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.health_set", requestJson));
    }

    /** v3 transport helper: sets a player's food level. */
    public static String playerFoodSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.food_set", requestJson));
    }

    /** v3 transport helper: sets a player's game mode. */
    public static String playerGameModeSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.gamemode_set", requestJson));
    }

    /** v3 transport helper: kicks a player with an optional reason. */
    public static String playerKick(String requestJson) {
        return guarded(() -> HostBridge.call("player.kick", requestJson));
    }

    /** v3 transport helper: sends a chat message packet to a player. */
    public static String playerMessageSend(String requestJson) {
        return guarded(() -> HostBridge.call("player.message_send", requestJson));
    }

    /** v3 transport helper: sets a player's velocity vector. */
    public static String playerVelocitySet(String requestJson) {
        return guarded(() -> HostBridge.call("player.velocity_set", requestJson));
    }

    /** v3 transport helper: hides or shows a target player for a viewer player. */
    public static String playerVisibilitySet(String requestJson) {
        return guarded(() -> HostBridge.call("player.visibility_set", requestJson));
    }

    /** v3 transport helper: sets a player's compass target block position. */
    public static String playerCompassTargetSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.compass_target_set", requestJson));
    }

    /** v3 transport helper: applies or removes a potion effect packet on a player. */
    public static String playerPotionEffectUpdate(String requestJson) {
        return guarded(() -> HostBridge.call("player.potion_effect_update", requestJson));
    }

    /** v3 transport helper: strikes lightning at a world location. */
    public static String worldLightningStrike(String requestJson) {
        return guarded(() -> HostBridge.call("world.lightning_strike", requestJson));
    }

    /** v3 transport helper: creates an explosion at a world location. */
    public static String worldExplosionCreate(String requestJson) {
        return guarded(() -> HostBridge.call("world.explosion_create", requestJson));
    }

    /** v3 transport helper: opens a GUI inventory menu for a player. */
    public static String playerOpenInventory(String requestJson) {
        return guarded(() -> HostBridge.call("player.open_inventory", requestJson));
    }

    /** v3 transport helper: closes a player's currently open inventory menu. */
    public static String playerCloseInventory(String requestJson) {
        return guarded(() -> HostBridge.call("player.close_inventory", requestJson));
    }

    /** v3 transport helper: deals generic damage to a player. */
    public static String playerDamage(String requestJson) {
        return guarded(() -> HostBridge.call("player.damage", requestJson));
    }

    /** v3 transport helper: respawns a dead player immediately. */
    public static String playerRespawn(String requestJson) {
        return guarded(() -> HostBridge.call("player.respawn", requestJson));
    }

    /** v3 transport helper: sets or clears a player's display name. */
    public static String playerDisplayNameSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.display_name_set", requestJson));
    }

    /** v3 transport helper: starts or clears an item cooldown for a player. */
    public static String playerCooldownSet(String requestJson) {
        return guarded(() -> HostBridge.call("player.cooldown_set", requestJson));
    }

    /** v3 transport helper: transfers a player to another server host and port. */
    public static String playerTransfer(String requestJson) {
        return guarded(() -> HostBridge.call("player.transfer", requestJson));
    }

    /** v3 transport helper: sends a fake block change packet to one player. */
    public static String playerBlockChangeSend(String requestJson) {
        return guarded(() -> HostBridge.call("player.block_change_send", requestJson));
    }

    /** v3 transport helper: spawns an entity in a world. */
    public static String worldSpawnEntity(String requestJson) {
        return guarded(() -> HostBridge.call("world.spawn_entity", requestJson));
    }

    /** v3 transport helper: drops an item entity in a world. */
    public static String worldDropItem(String requestJson) {
        return guarded(() -> HostBridge.call("world.drop_item", requestJson));
    }

    /** v3 transport helper: sets world clock time in ticks. */
    public static String worldTimeSet(String requestJson) {
        return guarded(() -> HostBridge.call("world.time_set", requestJson));
    }

    /** v3 transport helper: sets world storm/thunder weather state. */
    public static String worldWeatherSet(String requestJson) {
        return guarded(() -> HostBridge.call("world.weather_set", requestJson));
    }

    /** v3 transport helper: broadcasts a plain message to all online players. */
    public static String serverBroadcast(String requestJson) {
        return guarded(() -> HostBridge.call("server.broadcast", requestJson));
    }

    /** v3 transport helper: executes a command on the live server as a player. */
    public static String playerPerformCommand(String requestJson) {
        return guarded(() -> HostBridge.call("player.perform_command", requestJson));
    }

    /** Zero-JSON JNI hot-path block state read (`-1` when unavailable/unloaded). */
    public static int worldBlockGetFast(String world, int x, int y, int z) {
        try {
            return HostBridge.blockGetFast(world, x, y, z);
        } catch (Throwable ignored) {
            return -1;
        }
    }

    /** Zero-JSON JNI hot-path async block state write (`-1` when unavailable/rejected). */
    public static long worldBlockSetFast(String world, int x, int y, int z, int stateId) {
        try {
            return HostBridge.blockSetFast(world, x, y, z, stateId);
        } catch (Throwable ignored) {
            return -1L;
        }
    }

    /** v3 optimization helper: syncs the bitmask of events with active Java listeners to Rust. */
    public static String eventMaskSync(long mask) {
        return guarded(() -> HostBridge.call("host.event_mask_sync", "{\"mask\":" + mask + "}"));
    }

    @FunctionalInterface
    private interface Call {
        String run();
    }

    private static String guarded(Call call) {
        try {
            return call.run();
        } catch (Throwable error) {
            String message = HostRuntime.errorSummary(error);
            String stack = HostRuntime.stackTrace(error);
            try {
                HostBridge.log(4, "server", "host call failed: " + message + "\n" + stack);
            } catch (Throwable ignored) {
                // The native logger is unavailable while the JVM is still starting.
            }
            return HostRuntime.errorResponse("java_exception", message, stack);
        }
    }
}
