package dev.ferrite.api;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.inventory.FerriteHostInventoryBridge;
import dev.ferrite.api.internal.FerriteMainThread;
import dev.ferrite.api.plugin.FerriteMessenger;
import dev.ferrite.api.plugin.FerritePluginClassLoader;
import dev.ferrite.api.world.FerriteWorld;
import dev.ferrite.api.world.WorldSpec;
import dev.ferrite.host.Bootstrap;
import dev.ferrite.host.CommandMessage;
import dev.ferrite.host.CommandOrigin;
import dev.ferrite.host.CommandResult;
import dev.ferrite.host.EventResult;
import dev.ferrite.host.HostApi;
import dev.ferrite.host.HostContext;
import dev.ferrite.host.HostException;
import dev.ferrite.host.HostLogger;
import dev.ferrite.host.HostPlugin;
import dev.ferrite.host.LoadedHostPlugin;
import dev.ferrite.host.PluginSpec;
import dev.ferrite.nms.FerriteNmsVersion;
import dev.ferrite.nms.bridge.NmsBridge;
import dev.ferrite.nms.bridge.NmsBridgeException;
import dev.ferrite.nms.bridge.NmsHandleProvider;
import dev.ferrite.nms.host.BlockStateSnapshot;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.HostVersion;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.PlayerState;
import dev.ferrite.nms.host.WorldSnapshot;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.Callable;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Future;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.BlockFace;
import org.bukkit.entity.Player;
import org.bukkit.event.Event;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.block.BlockPlaceEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.player.AsyncPlayerChatEvent;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerMoveEvent;
import org.bukkit.event.player.PlayerRespawnEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Lane 2 {@link HostApi} implementation: wires the embedded Java host to {@link FerriteRuntime},
 * initializes Paper {@link JavaPlugin} instances inside isolated per-plugin classloaders, and
 * installs the NMS snapshot port and handle bridge.
 */
public final class FerriteHostApi implements HostApi {
    private HostContext context;
    private FerriteRuntime runtime;
    private NmsHost nmsHost;
    private NmsHandleProvider nmsProvider;
    private boolean tickPumpScheduled;

    @Override
    public void onHostStarted(HostContext context) {
        Objects.requireNonNull(context, "context");
        if (runtime != null) {
            return;
        }
        this.context = context;
        FerriteRuntime created = FerriteRuntime.builder()
                .serverName(context.server().name())
                .version(context.server().version())
                .minecraftVersion(context.apiVersion())
                .maxPlayers(context.server().maxPlayers())
                .mainThread(new HostMainThread())
                .world(WorldSpec.of("world", World.Environment.NORMAL))
                .build();
        FerriteApi.install(created);
        created.world("world").blockStore().setType(0, 65, 0, Material.STONE);
        created.messenger().setBridge(new HostMessengerBridge());
        this.runtime = created;
        try {
            this.nmsHost = new RuntimeNmsHost(created);
            NmsHostBinding.install(this.nmsHost);
            this.nmsProvider = new RuntimeNmsHandleProvider();
            NmsBridge.install(this.nmsProvider);
        } catch (RuntimeException error) {
            cleanupBindings(created);
            throw error;
        }
    }

    @Override
    public LoadedHostPlugin loadPlugin(HostContext context, PluginSpec spec) throws Exception {
        Objects.requireNonNull(context, "context");
        Objects.requireNonNull(spec, "spec");
        FerriteRuntime activeRuntime = requireRuntime();
        PluginDescriptionFile description = readDescription(spec);
        FerritePluginClassLoader classLoader = new FerritePluginClassLoader(
                new URL[] {spec.jar().toUri().toURL()},
                FerriteHostApi.class.getClassLoader(),
                description);
        try {
            Class<?> mainClass = Class.forName(spec.main(), true, classLoader);
            if (!JavaPlugin.class.isAssignableFrom(mainClass)) {
                throw new HostException(
                        spec.main()
                                + " does not extend "
                                + JavaPlugin.class.getName()
                                + " (or "
                                + dev.ferrite.host.JavaPlugin.class.getName()
                                + ")");
            }
            JavaPlugin bukkitPlugin = (JavaPlugin) mainClass.getDeclaredConstructor().newInstance();
            activeRuntime.attachPlugin(
                    bukkitPlugin,
                    description,
                    spec.jar().toFile(),
                    spec.dataFolder().toFile(),
                    classLoader);
            return new PaperLoadedHostPlugin(activeRuntime, context, spec, bukkitPlugin, classLoader);
        } catch (Throwable error) {
            try {
                classLoader.close();
            } catch (IOException closeError) {
                error.addSuppressed(closeError);
            }
            if (error instanceof Exception exception) {
                throw exception;
            }
            throw new HostException("failed to load plugin " + spec.name(), error);
        }
    }

    @Override
    public void onPluginEnabled(HostPlugin plugin) {
        if (context != null && runtime != null && !tickPumpScheduled) {
            tickPumpScheduled = true;
            try {
                context.schedule(plugin, runtime::tick, 1L, 1L, false);
            } catch (UnsatisfiedLinkError ignored) {
                // Pure-Java unit/harness runs have no Rust JNI HostBridge attached.
            }
            runtime.plugins().syncEventMaskToHost();
        }
    }

    @Override
    public EventResult fireEvent(String name, Map<String, Object> payload) {
        FerriteRuntime activeRuntime = requireRuntime();
        return switch (name) {
            case "player_join" -> {
                UUID uuid = UUID.fromString(String.valueOf(payload.get("player_uuid")));
                String playerName = String.valueOf(payload.get("player_name"));
                FerritePlayer player = activeRuntime.player(uuid);
                if (player == null) {
                    FerriteWorld world = activeRuntime.world("world");
                    Location spawn = world.getSpawnLocation();
                    player = activeRuntime.createPlayer(
                            playerName,
                            uuid,
                            false,
                            GameMode.SURVIVAL,
                            world,
                            spawn.getX(),
                            spawn.getY(),
                            spawn.getZ());
                }
                FerriteHostInventoryBridge.attach(player);
                activeRuntime.playerJoined(player);
                yield EventResult.pass();
            }
            case "player_quit" -> {
                UUID uuid = UUID.fromString(String.valueOf(payload.get("player_uuid")));
                activeRuntime.playerQuit(uuid);
                yield EventResult.pass();
            }
            case "block_break" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                int x = intValue(payload.get("x"));
                int y = intValue(payload.get("y"));
                int z = intValue(payload.get("z"));
                syncBlockMaterial(world, x, y, z, payload.get("material"));
                BlockBreakEvent event = new BlockBreakEvent(world.getBlockAt(x, y, z), player);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "player_interact" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                int x = intValue(payload.get("x"));
                int y = intValue(payload.get("y"));
                int z = intValue(payload.get("z"));
                syncBlockMaterial(world, x, y, z, payload.get("material"));
                Action action = "left_click_block".equals(String.valueOf(payload.get("action")))
                        ? Action.LEFT_CLICK_BLOCK
                        : Action.RIGHT_CLICK_BLOCK;
                BlockFace face = blockFace(intValue(payload.get("face")));
                PlayerInteractEvent event = new PlayerInteractEvent(
                        player,
                        action,
                        null,
                        world.getBlockAt(x, y, z),
                        face,
                        EquipmentSlot.HAND);
                event.callEvent();
                boolean cancelled = event.useInteractedBlock() == Event.Result.DENY
                        || event.useItemInHand() == Event.Result.DENY;
                yield cancelled ? EventResult.cancel() : EventResult.pass();
            }
            case "plugin_message" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                String channel = String.valueOf(payload.get("channel"));
                byte[] data = Base64.getDecoder().decode(String.valueOf(payload.get("data_base64")));
                if (!activeRuntime.messenger().getIncomingChannelRegistrations(channel).isEmpty()) {
                    activeRuntime.messenger().dispatchIncomingMessage(player, channel, data);
                }
                yield EventResult.pass();
            }
            case "player_chat" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                String message = String.valueOf(payload.get("message"));
                java.util.Set<Player> recipients = new java.util.HashSet<>(activeRuntime.onlinePlayers());
                AsyncPlayerChatEvent event = new AsyncPlayerChatEvent(false, player, message, recipients);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "player_command" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                String message = String.valueOf(payload.get("message"));
                PlayerCommandPreprocessEvent event = new PlayerCommandPreprocessEvent(player, message);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "block_place" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                int x = intValue(payload.get("x"));
                int y = intValue(payload.get("y"));
                int z = intValue(payload.get("z"));
                syncBlockMaterial(world, x, y, z, payload.get("placed"));
                org.bukkit.block.Block placedBlock = world.getBlockAt(x, y, z);
                BlockFace face = blockFace(intValue(payload.get("face")));
                org.bukkit.block.Block against = placedBlock.getRelative(face.getOppositeFace());
                org.bukkit.block.BlockState replacedState = safeBlockState(placedBlock);
                BlockPlaceEvent event = new BlockPlaceEvent(
                        placedBlock,
                        replacedState,
                        against,
                        player.getInventory().getItemInMainHand(),
                        player,
                        true,
                        EquipmentSlot.HAND);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "player_move" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                float yaw = floatValue(payload.get("yaw"));
                float pitch = floatValue(payload.get("pitch"));
                Location from = new Location(
                        world,
                        doubleValue(payload.get("from_x")),
                        doubleValue(payload.get("from_y")),
                        doubleValue(payload.get("from_z")),
                        yaw,
                        pitch);
                Location to = new Location(
                        world,
                        doubleValue(payload.get("to_x")),
                        doubleValue(payload.get("to_y")),
                        doubleValue(payload.get("to_z")),
                        yaw,
                        pitch);
                player.state().setPosition(to.getX(), to.getY(), to.getZ(), yaw, pitch);
                PlayerMoveEvent event = new PlayerMoveEvent(player, from, to);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "entity_damage" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("entity_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                EntityDamageEvent.DamageCause cause = parseDamageCause(payload.get("cause"));
                double amount = doubleValue(payload.get("amount"));
                EntityDamageEvent event = new EntityDamageEvent(player, cause, amount);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "player_respawn" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                Location spawn = new Location(
                        world,
                        doubleValue(payload.get("x")),
                        doubleValue(payload.get("y")),
                        doubleValue(payload.get("z")),
                        floatValue(payload.get("yaw")),
                        floatValue(payload.get("pitch")));
                PlayerRespawnEvent event = new PlayerRespawnEvent(player, spawn, false);
                event.callEvent();
                yield EventResult.pass();
            }
            case "inventory_click" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                org.bukkit.inventory.InventoryView view = player.getOpenInventory();
                if (!(view instanceof dev.ferrite.api.inventory.FerriteInventoryView ferriteView)) {
                    yield EventResult.pass();
                }
                int slot = intValue(payload.get("slot"));
                org.bukkit.event.inventory.ClickType clickType = parseClickType(payload.get("click"));
                org.bukkit.event.inventory.InventoryClickEvent event =
                        ferriteView.click(slot, clickType, -1);
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "inventory_close" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                player.closeInventory(org.bukkit.event.inventory.InventoryCloseEvent.Reason.PLAYER);
                yield EventResult.pass();
            }
            case "player_death" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                int droppedExp = payload.get("dropped_exp") != null ? intValue(payload.get("dropped_exp")) : 0;
                org.bukkit.damage.DamageSource damageSource = syntheticDamageSource(player);
                org.bukkit.event.entity.PlayerDeathEvent event =
                        new org.bukkit.event.entity.PlayerDeathEvent(
                                player,
                                damageSource,
                                new ArrayList<>(),
                                droppedExp,
                                net.kyori.adventure.text.Component.text(player.getName() + " died"),
                                true);
                if (Boolean.TRUE.equals(payload.get("keep_inventory"))) {
                    event.setKeepInventory(true);
                }
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "chunk_loaded" -> {
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                org.bukkit.Chunk chunk = world.getChunkAt(intValue(payload.get("x")), intValue(payload.get("z")), false);
                new org.bukkit.event.world.ChunkLoadEvent(chunk, Boolean.TRUE.equals(payload.get("new"))).callEvent();
                yield EventResult.pass();
            }
            case "chunk_unloaded" -> {
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                org.bukkit.Chunk chunk = world.getChunkAt(intValue(payload.get("x")), intValue(payload.get("z")), false);
                new org.bukkit.event.world.ChunkUnloadEvent(chunk, true).callEvent();
                yield EventResult.pass();
            }
            case "world_loaded" -> {
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                new org.bukkit.event.world.WorldLoadEvent(world).callEvent();
                yield EventResult.pass();
            }
            case "blocks_exploded" -> {
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                org.bukkit.block.Block block = world.getBlockAt(
                        intValue(payload.get("x")), intValue(payload.get("y")), intValue(payload.get("z")));
                float yieldRate = payload.get("yield") != null ? floatValue(payload.get("yield")) : 1.0f;
                org.bukkit.event.block.BlockExplodeEvent event = new org.bukkit.event.block.BlockExplodeEvent(
                        block,
                        safeBlockState(block),
                        new ArrayList<>(),
                        yieldRate,
                        org.bukkit.ExplosionResult.DESTROY);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "weather_changing" -> {
                FerriteWorld world = resolveWorld(activeRuntime, payload.get("world"));
                boolean toStorm = Boolean.TRUE.equals(payload.get("to_weather_state"));
                org.bukkit.event.weather.WeatherChangeEvent event =
                        new org.bukkit.event.weather.WeatherChangeEvent(world, toStorm);
                event.callEvent();
                yield event.isCancelled() ? EventResult.cancel() : EventResult.pass();
            }
            case "player_level_changed" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                int oldLevel = intValue(payload.get("old_level"));
                int newLevel = intValue(payload.get("new_level"));
                player.state().setLevel(newLevel);
                new org.bukkit.event.player.PlayerLevelChangeEvent(player, oldLevel, newLevel).callEvent();
                yield EventResult.pass();
            }
            case "player_exp_changed" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                int amount = intValue(payload.get("amount"));
                new org.bukkit.event.player.PlayerExpChangeEvent(player, amount).callEvent();
                yield EventResult.pass();
            }
            case "resource_pack_status" -> {
                FerritePlayer player = findPlayer(activeRuntime, payload.get("player_uuid"));
                if (player == null) {
                    yield EventResult.pass();
                }
                UUID packId = payload.get("pack_uuid") != null
                        ? UUID.fromString(String.valueOf(payload.get("pack_uuid")))
                        : UUID.randomUUID();
                int action = payload.get("action") != null ? intValue(payload.get("action")) : 0;
                org.bukkit.event.player.PlayerResourcePackStatusEvent.Status[] values =
                        org.bukkit.event.player.PlayerResourcePackStatusEvent.Status.values();
                org.bukkit.event.player.PlayerResourcePackStatusEvent.Status status =
                        (action >= 0 && action < values.length)
                                ? values[action]
                                : org.bukkit.event.player.PlayerResourcePackStatusEvent.Status.SUCCESSFULLY_LOADED;
                new org.bukkit.event.player.PlayerResourcePackStatusEvent(player, packId, status).callEvent();
                yield EventResult.pass();
            }
            case "entity_died" -> EventResult.pass();
            default -> EventResult.pass();
        };
    }

    @Override
    public CommandResult runCommand(CommandOrigin origin, String line) {
        FerriteRuntime activeRuntime = requireRuntime();
        if ("console".equals(origin.type())) {
            boolean handled = activeRuntime.commandDispatcher()
                    .dispatch(activeRuntime.consoleSender(), line);
            return handled ? CommandResult.handled(List.of()) : CommandResult.notHandled();
        }
        UUID uuid = UUID.fromString(origin.playerUuid());
        FerritePlayer player = activeRuntime.player(uuid);
        if (player == null) {
            return CommandResult.notHandled();
        }
        try {
            String rawState = Bootstrap.playerStateGet("{\"uuid\":\"" + uuid + "\"}");
            if (rawState != null) {
                JsonObject root = JsonParser.parseString(rawState).getAsJsonObject();
                if (root.has("ok") && root.get("ok").getAsBoolean()
                        && root.get("result") instanceof JsonObject res
                        && res.has("op")) {
                    player.setOp(res.get("op").getAsBoolean());
                }
            }
        } catch (RuntimeException ignored) {
        }
        int before = player.state().messages().size();
        boolean handled = activeRuntime.commandDispatcher().dispatch(player, line);
        if (!handled) {
            return CommandResult.notHandled();
        }
        List<String> all = player.state().messages();
        List<CommandMessage> messages = new ArrayList<>();
        for (int i = before; i < all.size(); i++) {
            messages.add(CommandMessage.player(uuid.toString(), player.getName(), all.get(i)));
        }
        return CommandResult.handled(messages);
    }

    private static FerritePlayer findPlayer(FerriteRuntime activeRuntime, Object rawUuid) {
        if (rawUuid == null) {
            return null;
        }
        return activeRuntime.player(UUID.fromString(String.valueOf(rawUuid)));
    }

    private static FerriteWorld resolveWorld(FerriteRuntime activeRuntime, Object rawWorld) {
        if (rawWorld != null) {
            FerriteWorld named = activeRuntime.world(String.valueOf(rawWorld));
            if (named != null) {
                return named;
            }
        }
        return activeRuntime.world("world");
    }

    private static void syncBlockMaterial(
            FerriteWorld world, int x, int y, int z, Object rawMaterial) {
        if (rawMaterial == null) {
            return;
        }
        Material material = Material.matchMaterial(String.valueOf(rawMaterial));
        if (material != null) {
            world.blockStore().setType(x, y, z, material);
        }
    }

    private static int intValue(Object value) {
        return value instanceof Number number ? number.intValue() : Integer.parseInt(String.valueOf(value));
    }

    private static double doubleValue(Object value) {
        return value instanceof Number number ? number.doubleValue() : Double.parseDouble(String.valueOf(value));
    }

    private static float floatValue(Object value) {
        return value instanceof Number number ? number.floatValue() : Float.parseFloat(String.valueOf(value));
    }

    private static org.bukkit.block.BlockState safeBlockState(org.bukkit.block.Block block) {
        try {
            return block.getState();
        } catch (UnsupportedOperationException ignored) {
            return null;
        }
    }

    private static EntityDamageEvent.DamageCause parseDamageCause(Object rawCause) {
        if (rawCause == null) {
            return EntityDamageEvent.DamageCause.CUSTOM;
        }
        try {
            return EntityDamageEvent.DamageCause.valueOf(String.valueOf(rawCause).toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException ignored) {
            return EntityDamageEvent.DamageCause.CUSTOM;
        }
    }

    private static org.bukkit.event.inventory.ClickType parseClickType(Object rawClick) {
        if (rawClick == null) {
            return org.bukkit.event.inventory.ClickType.LEFT;
        }
        try {
            return org.bukkit.event.inventory.ClickType.valueOf(
                    String.valueOf(rawClick).toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException ignored) {
            return org.bukkit.event.inventory.ClickType.LEFT;
        }
    }

    private static org.bukkit.damage.DamageSource syntheticDamageSource(FerritePlayer victim) {
        return (org.bukkit.damage.DamageSource) java.lang.reflect.Proxy.newProxyInstance(
                org.bukkit.damage.DamageSource.class.getClassLoader(),
                new Class<?>[] {org.bukkit.damage.DamageSource.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getDamageLocation", "getSourceLocation" -> victim.getLocation();
                    case "getCausingEntity", "getDirectEntity" -> null;
                    case "getFoodExhaustion" -> 0.0f;
                    case "isIndirect", "scalesWithDifficulty" -> false;
                    case "toString" -> "FerriteDamageSource{}";
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> args != null && args.length == 1 && args[0] == proxy;
                    default -> null;
                });
    }

    private static BlockFace blockFace(int face) {
        return switch (face) {
            case 0 -> BlockFace.DOWN;
            case 1 -> BlockFace.UP;
            case 2 -> BlockFace.NORTH;
            case 3 -> BlockFace.SOUTH;
            case 4 -> BlockFace.WEST;
            case 5 -> BlockFace.EAST;
            default -> BlockFace.UP;
        };
    }

    @Override
    public void onHostStopping() {
        this.tickPumpScheduled = false;
        this.context = null;
        FerriteRuntime activeRuntime = this.runtime;
        this.runtime = null;
        if (activeRuntime != null) {
            cleanupBindings(activeRuntime);
        }
    }

    private void cleanupBindings(FerriteRuntime activeRuntime) {
        RuntimeException failure = null;
        if (this.nmsProvider != null) {
            try {
                NmsBridge.uninstall(this.nmsProvider);
            } catch (RuntimeException error) {
                failure = error;
            } finally {
                this.nmsProvider = null;
            }
        }
        if (this.nmsHost != null) {
            try {
                NmsHostBinding.uninstall(this.nmsHost);
            } catch (RuntimeException error) {
                if (failure == null) {
                    failure = error;
                } else {
                    failure.addSuppressed(error);
                }
            } finally {
                this.nmsHost = null;
            }
        }
        try {
            FerriteApi.uninstall(activeRuntime);
        } catch (RuntimeException error) {
            if (failure == null) {
                failure = error;
            } else {
                failure.addSuppressed(error);
            }
        }
        if (failure != null) {
            throw failure;
        }
    }

    private FerriteRuntime requireRuntime() {
        if (runtime == null) {
            throw new IllegalStateException("FerriteHostApi has not started yet");
        }
        return runtime;
    }

    private static PluginDescriptionFile readDescription(PluginSpec spec) throws Exception {
        try (JarFile jar = new JarFile(spec.jar().toFile())) {
            JarEntry entry = jar.getJarEntry("plugin.yml");
            if (entry != null) {
                try (InputStream input = jar.getInputStream(entry)) {
                    return new PluginDescriptionFile(input);
                }
            }
        }
        return new PluginDescriptionFile(spec.name(), spec.version(), spec.main());
    }

    private static final class HostMessengerBridge implements FerriteMessenger.ChannelBridge {
        private static final Gson GSON = new Gson();

        @Override
        public void register(String plugin, String channel, String direction) {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("plugin", plugin);
            req.put("channel", channel);
            req.put("direction", direction);
            ensureOk(Bootstrap.pluginMessageRegister(GSON.toJson(req)), "plugin_message.register");
        }

        @Override
        public void unregister(String plugin, String channel, String direction) {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("plugin", plugin);
            if (channel != null) {
                req.put("channel", channel);
            }
            req.put("direction", direction);
            ensureOk(
                    Bootstrap.pluginMessageUnregister(GSON.toJson(req)),
                    "plugin_message.unregister",
                    true);
        }

        @Override
        public void send(String plugin, UUID playerUuid, String channel, byte[] data) {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("plugin", plugin);
            if (playerUuid != null) {
                req.put("player_uuid", playerUuid.toString());
            }
            req.put("channel", channel);
            req.put("data_base64", Base64.getEncoder().encodeToString(data));
            ensureOk(Bootstrap.pluginMessageSend(GSON.toJson(req)), "plugin_message.send", false);
        }

        private static void ensureOk(String rawResponse, String op) {
            ensureOk(rawResponse, op, false);
        }

        private static void ensureOk(String rawResponse, String op, boolean allowShutdown) {
            if (rawResponse == null) {
                return;
            }
            JsonObject root = JsonParser.parseString(rawResponse).getAsJsonObject();
            if (!root.has("ok") || !root.get("ok").getAsBoolean()) {
                String errorText = String.valueOf(root.get("error"));
                if (errorText.contains("UnsatisfiedLinkError")) {
                    // Pure-Java unit/harness runs have no Rust JNI HostBridge attached.
                    return;
                }
                if (allowShutdown && root.get("error") instanceof JsonObject errObj
                        && errObj.has("code")
                        && "shutdown".equals(errObj.get("code").getAsString())) {
                    // Host shutdown already cleared all native plugin channels.
                    return;
                }
                throw new IllegalStateException(op + " failed: " + errorText);
            }
        }
    }

    private static final class HostMainThread implements FerriteMainThread {
        @Override
        public boolean isPrimaryThread() {
            return true;
        }

        @Override
        public <T> Future<T> submit(Callable<T> task) {
            try {
                return CompletableFuture.completedFuture(task.call());
            } catch (Exception error) {
                CompletableFuture<T> failed = new CompletableFuture<>();
                failed.completeExceptionally(error);
                return failed;
            }
        }

        @Override
        public void execute(Runnable task) {
            task.run();
        }

        @Override
        public void shutdown() {
        }
    }

    private static final class PaperLoadedHostPlugin implements LoadedHostPlugin {
        private final FerriteRuntime runtime;
        private final HostPlugin hostPlugin;
        private final JavaPlugin bukkitPlugin;
        private final FerritePluginClassLoader classLoader;

        private PaperLoadedHostPlugin(
                FerriteRuntime runtime,
                HostContext context,
                PluginSpec spec,
                JavaPlugin bukkitPlugin,
                FerritePluginClassLoader classLoader) {
            this.runtime = runtime;
            this.bukkitPlugin = bukkitPlugin;
            this.classLoader = classLoader;
            this.hostPlugin = new BukkitHostPluginAdapter(context, spec, bukkitPlugin);
        }

        @Override
        public HostPlugin plugin() {
            return hostPlugin;
        }

        @Override
        public void enable() {
            runtime.enablePlugin(bukkitPlugin);
        }

        @Override
        public void disable() {
            if (bukkitPlugin.isEnabled()) {
                runtime.disablePlugin(bukkitPlugin);
            }
            runtime.plugins().unregisterPlugin(bukkitPlugin);
        }

        @Override
        public AutoCloseable classLoader() {
            return classLoader;
        }
    }

    private static final class BukkitHostPluginAdapter implements HostPlugin {
        private final HostContext context;
        private final PluginSpec spec;
        private final HostLogger logger;
        private final JavaPlugin bukkitPlugin;
        private volatile boolean enabled;

        private BukkitHostPluginAdapter(
                HostContext context, PluginSpec spec, JavaPlugin bukkitPlugin) {
            this.context = context;
            this.spec = spec;
            this.logger = context.serverLogger();
            this.bukkitPlugin = bukkitPlugin;
        }

        @Override
        public String pluginName() {
            return spec.name();
        }

        @Override
        public String pluginVersion() {
            return spec.version();
        }

        @Override
        public PluginSpec pluginSpec() {
            return spec;
        }

        @Override
        public Path pluginDataFolder() {
            return spec.dataFolder();
        }

        @Override
        public HostLogger pluginLogger() {
            return logger;
        }

        @Override
        public HostContext hostContext() {
            return context;
        }

        @Override
        public boolean isPluginEnabled() {
            return enabled || bukkitPlugin.isEnabled();
        }

        @Override
        public void setPluginEnabled(boolean enabled) {
            this.enabled = enabled;
        }

        @Override
        public void onLoad() {
            bukkitPlugin.onLoad();
        }

        @Override
        public void onEnable() {
            bukkitPlugin.onEnable();
        }

        @Override
        public void onDisable() {
            bukkitPlugin.onDisable();
        }
    }

    private static final class RuntimeNmsHandleProvider implements NmsHandleProvider {
        @Override
        public ServerLevel level(World world) {
            if (!(world instanceof FerriteWorld ferriteWorld)) {
                throw new NmsBridgeException(
                        "unknown_world", world.getName() + " is not a Ferrite world");
            }
            return ServerLevel.of(ferriteWorld.getUID());
        }

        @Override
        public ServerPlayer player(Player player) {
            if (!(player instanceof FerritePlayer ferritePlayer)) {
                throw new NmsBridgeException(
                        "unknown_player", player.getName() + " is not a Ferrite player");
            }
            return ServerPlayer.of(ferritePlayer.getUniqueId());
        }

        @Override
        public Entity entity(org.bukkit.entity.Entity entity) {
            if (entity instanceof FerritePlayer ferritePlayer) {
                return ServerPlayer.of(ferritePlayer.getUniqueId());
            }
            throw new NmsBridgeException(
                    "unknown_entity", entity.getUniqueId() + " is not a Ferrite entity");
        }
    }

    private static final class RuntimeNmsHost implements NmsHost {
        private final FerriteRuntime runtime;

        private RuntimeNmsHost(FerriteRuntime runtime) {
            this.runtime = runtime;
        }

        @Override
        public HostVersion version() {
            return FerriteNmsVersion.hostVersion();
        }

        @Override
        public WorldSnapshot world(UUID worldId) {
            World found = runtime.world(worldId);
            if (!(found instanceof FerriteWorld world)) {
                throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
            }
            Location spawn = world.getSpawnLocation();
            String dimensionKey = switch (world.getEnvironment()) {
                case NETHER -> "minecraft:the_nether";
                case THE_END -> "minecraft:the_end";
                default -> "minecraft:overworld";
            };
            int minY = world.getEnvironment() == World.Environment.NORMAL ? -64 : 0;
            int maxY = world.getEnvironment() == World.Environment.NORMAL ? 320 : 256;
            return new WorldSnapshot(
                    world.getUID(),
                    world.getName(),
                    dimensionKey,
                    world.getEnvironment().name(),
                    0L,
                    world.getTime(),
                    spawn.getX(),
                    spawn.getY(),
                    spawn.getZ(),
                    minY,
                    maxY);
        }

        @Override
        public BlockStateSnapshot blockState(UUID worldId, int x, int y, int z) {
            World found = runtime.world(worldId);
            if (!(found instanceof FerriteWorld world)) {
                throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
            }
            Material material = world.blockStore().getType(x, y, z);
            int globalId = material.isAir() ? 0 : Math.max(1, material.ordinal() + 1);
            return new BlockStateSnapshot(
                    globalId, material.getKey().asString(), "", material.isAir());
        }

        @Override
        public boolean isChunkLoaded(UUID worldId, int chunkX, int chunkZ) {
            if (runtime.world(worldId) == null) {
                throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
            }
            return true;
        }

        @Override
        public CompletableFuture<Void> setBlockState(
                UUID worldId, int x, int y, int z, int globalStateId) {
            if (runtime.world(worldId) == null) {
                throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
            }
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public int registryBlockStateId(String blockStateText) {
            if (blockStateText == null || blockStateText.isBlank()) {
                throw new NmsHostException("unknown_block_state", "blank block state");
            }
            String materialName = blockStateText;
            int props = materialName.indexOf('[');
            if (props >= 0) {
                materialName = materialName.substring(0, props);
            }
            Material material = Material.matchMaterial(materialName);
            if (material == null || !material.isBlock()) {
                throw new NmsHostException(
                        "unknown_block_state", "unknown block state: " + blockStateText);
            }
            return material.isAir() ? 0 : Math.max(1, material.ordinal() + 1);
        }

        private final ConcurrentHashMap<UUID, LiveEntityOverlay> overlays = new ConcurrentHashMap<>();

        private static final class LiveEntityOverlay {
            String pose = "STANDING";
            String customName;
            boolean customNameVisible;
            boolean silent;
            boolean noGravity;
            boolean glowing;
            boolean invisible;
            double vx;
            double vy;
            double vz;
            UUID vehicleId;
            final List<UUID> passengers = new ArrayList<>();
            final ConcurrentHashMap<String, String> equipment = new ConcurrentHashMap<>();
        }

        private LiveEntityOverlay overlay(UUID entityId) {
            return overlays.computeIfAbsent(entityId, ignored -> new LiveEntityOverlay());
        }

        @Override
        public PlayerSnapshot player(UUID playerId) {
            Player found = runtime.player(playerId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException(
                        "unknown_player", "player " + playerId + " is not online");
            }
            Location loc = player.getLocation();
            LiveEntityOverlay ov = overlay(playerId);
            EntityState entityState = new EntityState(
                    Math.max(1, playerId.hashCode() & 0x7fffffff),
                    false,
                    1,
                    -1,
                    0.0F,
                    true,
                    0,
                    ov.pose,
                    ov.customName,
                    ov.customNameVisible,
                    ov.silent,
                    ov.noGravity,
                    ov.glowing,
                    ov.invisible,
                    Map.copyOf(ov.equipment));
            PlayerState playerState = new PlayerState(
                    0,
                    "en_us",
                    5.0F,
                    0.0F,
                    0,
                    0.0F,
                    0,
                    0.2F,
                    0.1F,
                    false,
                    false,
                    false,
                    false,
                    300,
                    300,
                    player.getName(),
                    player.getName(),
                    0L,
                    0L,
                    true);
            return new PlayerSnapshot(
                    player.getUniqueId(),
                    player.getName(),
                    player.getWorld().getUID(),
                    loc.getX(),
                    loc.getY(),
                    loc.getZ(),
                    loc.getYaw(),
                    loc.getPitch(),
                    player.getGameMode().name().toUpperCase(Locale.ROOT),
                    (float) player.getHealth(),
                    player.getFoodLevel(),
                    entityState,
                    playerState);
        }

        @Override
        public EntitySnapshot entity(UUID entityId) {
            Player found = runtime.player(entityId);
            if (found instanceof FerritePlayer player) {
                Location loc = player.getLocation();
                LiveEntityOverlay ov = overlay(entityId);
                synchronized (ov) {
                    EntityState state = new EntityState(
                            Math.max(1, entityId.hashCode() & 0x7fffffff),
                            false,
                            1,
                            -1,
                            0.0F,
                            true,
                            0,
                            ov.pose,
                            ov.customName,
                            ov.customNameVisible,
                            ov.silent,
                            ov.noGravity,
                            ov.glowing,
                            ov.invisible,
                            Map.copyOf(ov.equipment));
                    return new EntitySnapshot(
                            player.getUniqueId(),
                            "minecraft:player",
                            player.getWorld().getUID(),
                            loc.getX(),
                            loc.getY(),
                            loc.getZ(),
                            loc.getYaw(),
                            loc.getPitch(),
                            ov.vx,
                            ov.vy,
                            ov.vz,
                            ov.vehicleId,
                            List.copyOf(ov.passengers),
                            state);
                }
            }
            throw new NmsHostException("unknown_entity", "entity " + entityId + " is not loaded");
        }

        @Override
        public CompletableFuture<Void> teleport(
                UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch) {
            Player found = runtime.player(entityId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException("unknown_entity", "entity " + entityId + " is not loaded");
            }
            World target = runtime.world(worldId);
            if (target == null) {
                throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
            }
            player.teleport(new Location(target, x, y, z, yaw, pitch));
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setGameMode(UUID playerId, String gameMode) {
            Player found = runtime.player(playerId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException("unknown_player", "player " + playerId + " is not online");
            }
            player.state().setGameMode(org.bukkit.GameMode.valueOf(gameMode.trim().toUpperCase(Locale.ROOT)));
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setHealth(UUID playerId, float health) {
            Player found = runtime.player(playerId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException("unknown_player", "player " + playerId + " is not online");
            }
            if (health < 0 || health > 20) {
                throw new IllegalArgumentException("health must be between 0 and 20, was " + health);
            }
            player.state().setHealth(health);
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setFoodLevel(UUID playerId, int foodLevel) {
            Player found = runtime.player(playerId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException("unknown_player", "player " + playerId + " is not online");
            }
            player.setFoodLevel(foodLevel);
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setEquipment(UUID entityId, String slot, String itemKey) {
            entity(entityId);
            String normalized = EntityState.normalizeEquipmentSlot(slot);
            LiveEntityOverlay ov = overlay(entityId);
            if (itemKey == null) {
                ov.equipment.remove(normalized);
            } else {
                ov.equipment.put(normalized, itemKey);
            }
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setVelocity(UUID entityId, double vx, double vy, double vz) {
            entity(entityId);
            LiveEntityOverlay ov = overlay(entityId);
            synchronized (ov) {
                ov.vx = vx;
                ov.vy = vy;
                ov.vz = vz;
            }
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setRotation(UUID entityId, float yaw, float pitch) {
            Player found = runtime.player(entityId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException("unknown_entity", "entity " + entityId + " is not loaded");
            }
            Location loc = player.getLocation();
            loc.setYaw(yaw);
            loc.setPitch(pitch);
            player.teleport(loc);
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setPose(UUID entityId, String pose) {
            entity(entityId);
            LiveEntityOverlay ov = overlay(entityId);
            ov.pose = pose;
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setCustomName(
                UUID entityId, String customName, boolean visible) {
            entity(entityId);
            LiveEntityOverlay ov = overlay(entityId);
            ov.customName = customName;
            ov.customNameVisible = visible;
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> addPassenger(UUID vehicleId, UUID passengerId) {
            entity(vehicleId);
            entity(passengerId);
            LiveEntityOverlay vehicle = overlay(vehicleId);
            LiveEntityOverlay passenger = overlay(passengerId);
            synchronized (vehicle) {
                if (!vehicle.passengers.contains(passengerId)) {
                    vehicle.passengers.add(passengerId);
                }
            }
            synchronized (passenger) {
                passenger.vehicleId = vehicleId;
            }
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> removePassenger(UUID vehicleId, UUID passengerId) {
            entity(vehicleId);
            LiveEntityOverlay vehicle = overlay(vehicleId);
            synchronized (vehicle) {
                vehicle.passengers.remove(passengerId);
            }
            LiveEntityOverlay passenger = overlays.get(passengerId);
            if (passenger != null) {
                synchronized (passenger) {
                    if (vehicleId.equals(passenger.vehicleId)) {
                        passenger.vehicleId = null;
                    }
                }
            }
            return CompletableFuture.completedFuture(null);
        }
    }
}
