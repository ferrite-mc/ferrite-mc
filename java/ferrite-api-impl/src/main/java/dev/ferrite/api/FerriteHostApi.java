package dev.ferrite.api;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.inventory.FerriteHostInventoryBridge;
import dev.ferrite.api.internal.FerriteMainThread;
import dev.ferrite.api.plugin.FerritePluginClassLoader;
import dev.ferrite.api.world.FerriteWorld;
import dev.ferrite.api.world.WorldSpec;
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
import dev.ferrite.nms.host.HostVersion;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.WorldSnapshot;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.Callable;
import java.util.concurrent.CompletableFuture;
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
import org.bukkit.event.player.PlayerInteractEvent;
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

        @Override
        public PlayerSnapshot player(UUID playerId) {
            Player found = runtime.player(playerId);
            if (!(found instanceof FerritePlayer player)) {
                throw new NmsHostException(
                        "unknown_player", "player " + playerId + " is not online");
            }
            Location loc = player.getLocation();
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
                    player.getFoodLevel());
        }

        @Override
        public EntitySnapshot entity(UUID entityId) {
            throw new NmsHostException("unknown_entity", "entity " + entityId + " is not loaded");
        }

        @Override
        public CompletableFuture<Void> teleport(
                UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch) {
            player(entityId);
            world(worldId);
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setGameMode(UUID playerId, String gameMode) {
            player(playerId);
            return CompletableFuture.completedFuture(null);
        }

        @Override
        public CompletableFuture<Void> setHealth(UUID playerId, float health) {
            player(playerId);
            return CompletableFuture.completedFuture(null);
        }
    }
}
