package dev.ferrite.api.world;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.block.FerriteBlock;
import dev.ferrite.api.entity.FerriteEntitySnapshot;
import dev.ferrite.api.internal.Unsupported;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.host.Bootstrap;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.util.ArrayList;
import java.util.Collection;
import java.util.EnumMap;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Predicate;
import org.bukkit.Bukkit;
import org.bukkit.Chunk;
import org.bukkit.GameRule;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.WorldBorder;
import org.bukkit.block.Biome;
import org.bukkit.block.Block;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Player;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.util.BoundingBox;

/**
 * First {@link World} slice backed by an in-process {@link BlockStore}.
 *
 * <p>Time/weather/gamerule reads are facade state. Game-rule writes and world creation are
 * refused until Lane 1 provides the owner-region op; they never pretend to succeed.
 */
public final class FerriteWorld extends UnsupportedWorldBase {

    private static final int DEFAULT_VIEW_DISTANCE = 10;

    private final FerriteRuntime runtime;
    private final WorldSpec spec;
    private final BlockStore blocks;
    private final FerriteWorldBorder worldBorder;
    private final Map<String, Object> gameRules = new HashMap<>();
    private final Set<Long> loadedChunks = ConcurrentHashMap.newKeySet();
    private final Set<Long> generatedChunks = ConcurrentHashMap.newKeySet();
    private final Set<Long> forceLoadedChunks = ConcurrentHashMap.newKeySet();
    private final Map<Long, Set<org.bukkit.plugin.Plugin>> chunkTickets = new ConcurrentHashMap<>();
    private final Map<Long, Long> chunkInhabitedTime = new ConcurrentHashMap<>();
    private final Map<Long, dev.ferrite.api.item.FerritePersistentDataContainer> chunkPdcs =
            new ConcurrentHashMap<>();
    private final Map<UUID, Entity> worldEntities = new LinkedHashMap<>();
    private volatile Set<String> negotiatedV3Ops;

    private long time;
    private boolean storm;
    private boolean thundering;
    private int weatherDuration = 6000;
    private int thunderDuration = 6000;
    private volatile boolean pvp = true;
    private volatile org.bukkit.Difficulty difficulty = org.bukkit.Difficulty.NORMAL;

    public FerriteWorld(FerriteRuntime runtime, WorldSpec spec) {
        this.runtime = runtime;
        this.spec = spec;
        this.blocks = new InMemoryBlockStore(spec.defaultBlock());
        this.worldBorder = new FerriteWorldBorder(this);
        this.time = spec.time();
        // String keys on purpose: the Paper API GameRule constants initialize
        // RegistryAccess, which only a CraftBukkit server installs. The facade must not
        // trip that initializer while building its own worlds.
        this.gameRules.put("doDaylightCycle", Boolean.TRUE);
        this.gameRules.put("doWeatherCycle", Boolean.TRUE);
        this.gameRules.put("doMobSpawning", Boolean.TRUE);
        this.gameRules.put("keepInventory", Boolean.FALSE);
    }

    /** The block backend; the host replaces it with an adapter over its region ops. */
    public BlockStore blockStore() {
        return blocks;
    }

    long worldSeed() {
        return spec.uniqueId().getMostSignificantBits() ^ spec.uniqueId().getLeastSignificantBits();
    }

    @Override
    public boolean isChunkForceLoaded(int cx, int cz) {
        return forceLoadedChunks.contains(Chunk.getChunkKey(cx, cz));
    }

    @Override
    public void setChunkForceLoaded(int cx, int cz, boolean forced) {
        long key = Chunk.getChunkKey(cx, cz);
        if (forced) {
            forceLoadedChunks.add(key);
        } else {
            forceLoadedChunks.remove(key);
        }
    }

    boolean addChunkTicket(int cx, int cz, org.bukkit.plugin.Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        return chunkTickets
                .computeIfAbsent(Chunk.getChunkKey(cx, cz), ignored -> ConcurrentHashMap.newKeySet())
                .add(plugin);
    }

    boolean removeChunkTicket(int cx, int cz, org.bukkit.plugin.Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        Set<org.bukkit.plugin.Plugin> set = chunkTickets.get(Chunk.getChunkKey(cx, cz));
        return set != null && set.remove(plugin);
    }

    Collection<org.bukkit.plugin.Plugin> chunkTickets(int cx, int cz) {
        Set<org.bukkit.plugin.Plugin> set = chunkTickets.get(Chunk.getChunkKey(cx, cz));
        return set == null ? List.of() : List.copyOf(set);
    }

    long chunkInhabitedTime(int cx, int cz) {
        return chunkInhabitedTime.getOrDefault(Chunk.getChunkKey(cx, cz), 0L);
    }

    void setChunkInhabitedTime(int cx, int cz, long ticks) {
        chunkInhabitedTime.put(Chunk.getChunkKey(cx, cz), Math.max(0L, ticks));
    }

    org.bukkit.persistence.PersistentDataContainer chunkPdc(int cx, int cz) {
        return chunkPdcs.computeIfAbsent(
                Chunk.getChunkKey(cx, cz),
                ignored -> new dev.ferrite.api.item.FerritePersistentDataContainer());
    }

    @Override
    public String getName() {
        return spec.name();
    }

    @Override
    public UUID getUID() {
        return spec.uniqueId();
    }

    @Override
    public World.Environment getEnvironment() {
        return spec.environment();
    }

    @Override
    public Location getSpawnLocation() {
        return new Location(this, spec.spawnX(), spec.spawnY(), spec.spawnZ());
    }

    @Override
    public Block getBlockAt(int x, int y, int z) {
        return new FerriteBlock(this, x, y, z);
    }

    @Override
    public int getMinHeight() {
        return spec.environment() == World.Environment.NORMAL ? -64 : 0;
    }

    @Override
    public int getMaxHeight() {
        return spec.environment() == World.Environment.NORMAL ? 320 : 256;
    }

    @Override
    public int getHighestBlockYAt(int x, int z) {
        for (int y = getMaxHeight() - 1; y >= getMinHeight(); y--) {
            if (!getBlockAt(x, y, z).getType().isAir()) {
                return y;
            }
        }
        return getMinHeight();
    }

    @Override
    public int getHighestBlockYAt(Location location) {
        Objects.requireNonNull(location, "location");
        return getHighestBlockYAt(location.getBlockX(), location.getBlockZ());
    }

    @Override
    public Block getHighestBlockAt(int x, int z) {
        return getBlockAt(x, getHighestBlockYAt(x, z), z);
    }

    @Override
    public Block getHighestBlockAt(Location location) {
        Objects.requireNonNull(location, "location");
        return getHighestBlockAt(location.getBlockX(), location.getBlockZ());
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceBlocks(
            Location start, org.bukkit.util.Vector direction, double maxDistance) {
        return rayTraceBlocks(start, direction, maxDistance, org.bukkit.FluidCollisionMode.NEVER, false);
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceBlocks(
            Location start,
            org.bukkit.util.Vector direction,
            double maxDistance,
            org.bukkit.FluidCollisionMode fluidCollisionMode,
            boolean ignorePassableBlocks) {
        Objects.requireNonNull(start, "start");
        Objects.requireNonNull(direction, "direction");
        if (maxDistance <= 0.0 || direction.lengthSquared() == 0.0) {
            return null;
        }
        org.bukkit.util.Vector step = direction.clone().normalize().multiply(0.25);
        int steps = (int) Math.ceil(maxDistance / 0.25);
        org.bukkit.util.Vector cur = start.toVector();
        int prevX = cur.getBlockX();
        int prevY = cur.getBlockY();
        int prevZ = cur.getBlockZ();
        for (int i = 0; i <= steps; i++) {
            int bx = cur.getBlockX();
            int by = cur.getBlockY();
            int bz = cur.getBlockZ();
            Block block = getBlockAt(bx, by, bz);
            Material mat = block.getType();
            boolean hit = !mat.isAir()
                    && (!ignorePassableBlocks || mat.isSolid())
                    && (fluidCollisionMode != org.bukkit.FluidCollisionMode.NEVER
                            || (mat != Material.WATER && mat != Material.LAVA));
            if (hit) {
                org.bukkit.block.BlockFace face = org.bukkit.block.BlockFace.UP;
                if (bx > prevX) {
                    face = org.bukkit.block.BlockFace.WEST;
                } else if (bx < prevX) {
                    face = org.bukkit.block.BlockFace.EAST;
                } else if (by > prevY) {
                    face = org.bukkit.block.BlockFace.DOWN;
                } else if (by < prevY) {
                    face = org.bukkit.block.BlockFace.UP;
                } else if (bz > prevZ) {
                    face = org.bukkit.block.BlockFace.NORTH;
                } else if (bz < prevZ) {
                    face = org.bukkit.block.BlockFace.SOUTH;
                }
                return new org.bukkit.util.RayTraceResult(cur.clone(), block, face);
            }
            prevX = bx;
            prevY = by;
            prevZ = bz;
            cur.add(step);
        }
        return null;
    }

    @Override
    public Biome getBiome(int x, int z) {
        return getBiome(x, 64, z);
    }

    @Override
    public Biome getBiome(int x, int y, int z) {
        if (supportsV3Op("world.biome_get")) {
            return queryHostBiome(x, y, z);
        }
        if (queryHostChunkLoaded(0, 0) != null) {
            throw Unsupported.exception("World#getBiome");
        }
        int cx = Math.floorDiv(x, 16);
        int cz = Math.floorDiv(z, 16);
        if (!isChunkLoaded(cx, cz)) {
            throw new IllegalStateException("chunk (" + cx + ", " + cz + ") is not loaded");
        }
        return switch (spec.environment()) {
            case NETHER -> resolveBiome("minecraft:nether_wastes");
            case THE_END -> resolveBiome("minecraft:the_end");
            default -> resolveBiome("minecraft:plains");
        };
    }

    @Override
    public Biome getComputedBiome(int x, int y, int z) {
        return getBiome(x, y, z);
    }

    /** Resolves a namespaced biome id (for example {@code minecraft:plains}) via Paper's registry. */
    public static Biome resolveBiome(String biomeId) {
        Objects.requireNonNull(biomeId, "biomeId");
        NamespacedKey key = NamespacedKey.fromString(biomeId);
        if (key == null) {
            throw new IllegalStateException("invalid biome id: " + biomeId);
        }
        Biome biome = RegistryAccess.registryAccess().getRegistry(RegistryKey.BIOME).get(key);
        if (biome == null) {
            throw new IllegalStateException("unknown biome id in 26.3 registry: " + biomeId);
        }
        return biome;
    }

    @Override
    public Chunk getChunkAt(int x, int z) {
        return getChunkAt(x, z, true);
    }

    @Override
    public Chunk getChunkAt(int x, int z, boolean generate) {
        if (generate && queryHostChunkLoaded(x, z) == null) {
            loadChunk(x, z, true);
        }
        return new FerriteChunk(this, x, z);
    }

    @Override
    public Chunk getChunkAt(Block block) {
        Objects.requireNonNull(block, "block");
        if (!equals(block.getWorld())) {
            throw new IllegalArgumentException("block belongs to another world");
        }
        return getChunkAt(Math.floorDiv(block.getX(), 16), Math.floorDiv(block.getZ(), 16));
    }

    @Override
    public boolean isChunkLoaded(Chunk chunk) {
        Objects.requireNonNull(chunk, "chunk");
        return equals(chunk.getWorld()) && isChunkLoaded(chunk.getX(), chunk.getZ());
    }

    @Override
    public boolean isChunkLoaded(int x, int z) {
        Boolean hostLoaded = queryHostChunkLoaded(x, z);
        if (hostLoaded != null) {
            return hostLoaded;
        }
        return loadedChunks.contains(Chunk.getChunkKey(x, z));
    }

    @Override
    public Chunk[] getLoadedChunks() {
        if (supportsV3Op("world.loaded_chunks")) {
            List<Chunk> chunks = queryHostLoadedChunks();
            if (chunks != null) {
                return chunks.toArray(Chunk[]::new);
            }
        }
        if (queryHostChunkLoaded(0, 0) != null) {
            throw Unsupported.exception("World#getLoadedChunks");
        }
        List<Chunk> result = new ArrayList<>(loadedChunks.size());
        for (long key : loadedChunks) {
            int cx = (int) key;
            int cz = (int) (key >> 32);
            result.add(new FerriteChunk(this, cx, cz));
        }
        return result.toArray(Chunk[]::new);
    }

    @Override
    public void loadChunk(Chunk chunk) {
        Objects.requireNonNull(chunk, "chunk");
        if (!equals(chunk.getWorld())) {
            throw new IllegalArgumentException("chunk belongs to another world");
        }
        loadChunk(chunk.getX(), chunk.getZ(), true);
    }

    @Override
    public boolean loadChunk(int x, int z, boolean generate) {
        Boolean hostLoaded = queryHostChunkLoaded(x, z);
        if (Boolean.TRUE.equals(hostLoaded)) {
            return true;
        }
        if (Boolean.FALSE.equals(hostLoaded)) {
            throw Unsupported.exception("World#loadChunk");
        }
        long key = Chunk.getChunkKey(x, z);
        if (!generate && !generatedChunks.contains(key) && !loadedChunks.contains(key)) {
            return false;
        }
        generatedChunks.add(key);
        loadedChunks.add(key);
        return true;
    }

    @Override
    public boolean isChunkGenerated(int x, int z) {
        if (supportsV3Op("world.chunk_status")) {
            ChunkStatus status = queryHostChunkStatus(x, z);
            if (status != null) {
                return status.generated();
            }
        }
        Boolean hostLoaded = queryHostChunkLoaded(x, z);
        if (Boolean.TRUE.equals(hostLoaded)) {
            return true;
        }
        if (Boolean.FALSE.equals(hostLoaded)) {
            throw Unsupported.exception("World#isChunkGenerated");
        }
        long key = Chunk.getChunkKey(x, z);
        return generatedChunks.contains(key) || loadedChunks.contains(key);
    }

    @Override
    public boolean isChunkInUse(int x, int z) {
        if (supportsV3Op("world.chunk_status")) {
            ChunkStatus status = queryHostChunkStatus(x, z);
            if (status != null) {
                return status.inUse();
            }
        }
        return isChunkLoaded(x, z) && !getPlayersSeeingChunk(x, z).isEmpty();
    }

    @Override
    public Collection<Player> getPlayersSeeingChunk(Chunk chunk) {
        Objects.requireNonNull(chunk, "chunk");
        if (!equals(chunk.getWorld())) {
            return List.of();
        }
        return getPlayersSeeingChunk(chunk.getX(), chunk.getZ());
    }

    @Override
    public Collection<Player> getPlayersSeeingChunk(int x, int z) {
        if (!isChunkLoaded(x, z)) {
            return List.of();
        }
        int viewDistance = effectiveViewDistance();
        List<Player> seeing = new ArrayList<>();
        for (Player player : getPlayers()) {
            Location loc = player.getLocation();
            int pcx = Math.floorDiv((int) Math.floor(loc.getX()), 16);
            int pcz = Math.floorDiv((int) Math.floor(loc.getZ()), 16);
            if (Math.max(Math.abs(pcx - x), Math.abs(pcz - z)) <= viewDistance) {
                seeing.add(player);
            }
        }
        return List.copyOf(seeing);
    }

    /** Registers a non-player entity snapshot in this world (for facade/test use). */
    public void registerEntity(Entity entity) {
        Objects.requireNonNull(entity, "entity");
        if (!equals(entity.getWorld())) {
            throw new IllegalArgumentException("entity belongs to another world");
        }
        synchronized (worldEntities) {
            worldEntities.put(entity.getUniqueId(), entity);
        }
    }

    /** Removes a registered non-player entity snapshot from this world. */
    public boolean unregisterEntity(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        synchronized (worldEntities) {
            return worldEntities.remove(uuid) != null;
        }
    }

    @Override
    public List<Player> getPlayers() {
        return List.copyOf(runtime.onlinePlayersIn(this));
    }

    @Override
    public List<Entity> getEntities() {
        Map<UUID, Entity> combined = new LinkedHashMap<>();
        for (Player player : runtime.onlinePlayersIn(this)) {
            combined.put(player.getUniqueId(), player);
        }
        if (supportsV3Op("world.entities_snapshot")) {
            List<Entity> hostEntities = queryHostEntitiesSnapshot();
            if (hostEntities != null) {
                for (Entity entity : hostEntities) {
                    combined.putIfAbsent(entity.getUniqueId(), entity);
                }
            }
        }
        synchronized (worldEntities) {
            for (Entity entity : worldEntities.values()) {
                combined.putIfAbsent(entity.getUniqueId(), entity);
            }
        }
        return List.copyOf(combined.values());
    }

    @Override
    public List<LivingEntity> getLivingEntities() {
        List<LivingEntity> result = new ArrayList<>();
        for (Entity entity : getEntities()) {
            if (entity instanceof LivingEntity living) {
                result.add(living);
            }
        }
        return List.copyOf(result);
    }

    @Override
    @SafeVarargs
    public final <T extends Entity> Collection<T> getEntitiesByClass(Class<T>... classes) {
        Objects.requireNonNull(classes, "classes");
        Set<T> matched = new LinkedHashSet<>();
        for (Entity entity : getEntities()) {
            for (Class<T> cls : classes) {
                if (cls != null && cls.isInstance(entity)) {
                    matched.add(cls.cast(entity));
                    break;
                }
            }
        }
        return List.copyOf(matched);
    }

    @Override
    public <T extends Entity> Collection<T> getEntitiesByClass(Class<T> cls) {
        Objects.requireNonNull(cls, "cls");
        List<T> matched = new ArrayList<>();
        for (Entity entity : getEntities()) {
            if (cls.isInstance(entity)) {
                matched.add(cls.cast(entity));
            }
        }
        return List.copyOf(matched);
    }

    @Override
    public Collection<Entity> getEntitiesByClasses(Class<?>... classes) {
        Objects.requireNonNull(classes, "classes");
        Set<Entity> matched = new LinkedHashSet<>();
        for (Entity entity : getEntities()) {
            for (Class<?> cls : classes) {
                if (cls != null && cls.isInstance(entity)) {
                    matched.add(entity);
                    break;
                }
            }
        }
        return List.copyOf(matched);
    }

    @Override
    public Entity getEntity(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        Player player = runtime.player(uuid);
        if (player != null && equals(player.getWorld())) {
            return player;
        }
        synchronized (worldEntities) {
            Entity local = worldEntities.get(uuid);
            if (local != null) {
                return local;
            }
        }
        return queryHostEntity(uuid);
    }

    @Override
    public Collection<Entity> getNearbyEntities(
            Location location,
            double x,
            double y,
            double z,
            Predicate<? super Entity> filter) {
        Objects.requireNonNull(location, "location");
        if (location.getWorld() != null && !equals(location.getWorld())) {
            throw new IllegalArgumentException("location belongs to another world");
        }
        BoundingBox box = BoundingBox.of(location, Math.abs(x), Math.abs(y), Math.abs(z));
        return getNearbyEntities(box, filter);
    }

    @Override
    public Collection<Entity> getNearbyEntities(
            BoundingBox boundingBox, Predicate<? super Entity> filter) {
        Objects.requireNonNull(boundingBox, "boundingBox");
        List<Entity> nearby = new ArrayList<>();
        for (Entity entity : getEntities()) {
            Location loc = entity.getLocation();
            if (boundingBox.contains(loc.getX(), loc.getY(), loc.getZ())
                    && (filter == null || filter.test(entity))) {
                nearby.add(entity);
            }
        }
        return List.copyOf(nearby);
    }

    private int effectiveViewDistance() {
        if (Bukkit.getServer() == null) {
            return DEFAULT_VIEW_DISTANCE;
        }
        try {
            return Math.max(0, Bukkit.getViewDistance());
        } catch (UnsupportedOperationException ignored) {
            return DEFAULT_VIEW_DISTANCE;
        }
    }

    private String hostWorldKey() {
        return switch (spec.environment()) {
            case NETHER -> "minecraft:the_nether";
            case THE_END -> "minecraft:the_end";
            default -> "minecraft:overworld";
        };
    }

    private Boolean queryHostChunkLoaded(int x, int z) {
        try {
            String req = "{\"world\":\"" + hostWorldKey() + "\",\"x\":" + x + ",\"z\":" + z + "}";
            String raw = Bootstrap.worldChunkLoaded(req);
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (envelope.get("ok").getAsBoolean()) {
                return envelope.getAsJsonObject("result").get("loaded").getAsBoolean();
            }
            var errElem = envelope.get("error");
            String code = envelope.has("code") && envelope.get("code").isJsonPrimitive()
                    ? envelope.get("code").getAsString()
                    : errElem != null && errElem.isJsonObject() && errElem.getAsJsonObject().has("code")
                            ? errElem.getAsJsonObject().get("code").getAsString()
                            : "host_error";
            if ("state_unavailable".equals(code)
                    || "unsupported".equals(code)
                    || "java_exception".equals(code)) {
                return null;
            }
            throw new IllegalStateException("world.chunk_loaded failed (" + code + "): " + raw);
        } catch (UnsatisfiedLinkError | NoClassDefFoundError unavailable) {
            return null;
        }
    }

    private record ChunkStatus(boolean loaded, boolean generated, boolean inUse) {}

    private boolean supportsV3Op(String op) {
        Set<String> cached = negotiatedV3Ops;
        if (cached == null) {
            cached = queryNegotiatedV3Ops();
            if (cached == null) {
                return false;
            }
            negotiatedV3Ops = cached;
        }
        return cached.contains(op);
    }

    private Set<String> queryNegotiatedV3Ops() {
        try {
            String raw = Bootstrap.hostInfo();
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (!envelope.has("ok") || !envelope.get("ok").getAsBoolean()) {
                return null;
            }
            JsonObject result = envelope.getAsJsonObject("result");
            if (result == null || !result.has("capabilities")) {
                return Set.of();
            }
            JsonObject caps = result.getAsJsonObject("capabilities");
            if (caps == null || !caps.has("v3_ops") || !caps.get("v3_ops").isJsonArray()) {
                return Set.of();
            }
            Set<String> ops = new LinkedHashSet<>();
            for (var elem : caps.getAsJsonArray("v3_ops")) {
                if (elem.isJsonPrimitive()) {
                    ops.add(elem.getAsString());
                }
            }
            return Set.copyOf(ops);
        } catch (UnsatisfiedLinkError | NoClassDefFoundError | RuntimeException unavailable) {
            return null;
        }
    }

    private Biome queryHostBiome(int x, int y, int z) {
        String req = "{\"world\":\"" + hostWorldKey() + "\",\"x\":" + x + ",\"y\":" + y + ",\"z\":" + z + "}";
        String raw = Bootstrap.worldBiomeGet(req);
        JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
        if (envelope.has("ok") && envelope.get("ok").getAsBoolean()) {
            JsonObject result = envelope.getAsJsonObject("result");
            if (result == null || !result.has("biome")) {
                throw new IllegalStateException("world.biome_get returned no biome field: " + raw);
            }
            return resolveBiome(result.get("biome").getAsString());
        }
        var errElem = envelope.get("error");
        String code = envelope.has("code") && envelope.get("code").isJsonPrimitive()
                ? envelope.get("code").getAsString()
                : errElem != null && errElem.isJsonObject() && errElem.getAsJsonObject().has("code")
                        ? errElem.getAsJsonObject().get("code").getAsString()
                        : "host_error";
        throw new IllegalStateException("world.biome_get failed (" + code + "): " + raw);
    }

    private ChunkStatus queryHostChunkStatus(int x, int z) {
        try {
            String req = "{\"world\":\"" + hostWorldKey() + "\",\"x\":" + x + ",\"z\":" + z + "}";
            String raw = Bootstrap.worldChunkStatus(req);
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (!envelope.has("ok") || !envelope.get("ok").getAsBoolean()) {
                return null;
            }
            JsonObject result = envelope.getAsJsonObject("result");
            if (result == null) {
                return null;
            }
            return new ChunkStatus(
                    result.get("loaded").getAsBoolean(),
                    result.get("generated").getAsBoolean(),
                    result.get("in_use").getAsBoolean());
        } catch (UnsatisfiedLinkError | NoClassDefFoundError | RuntimeException unavailable) {
            return null;
        }
    }

    private List<Chunk> queryHostLoadedChunks() {
        try {
            String req = "{\"world\":\"" + hostWorldKey() + "\"}";
            String raw = Bootstrap.worldLoadedChunks(req);
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (!envelope.has("ok") || !envelope.get("ok").getAsBoolean()) {
                return null;
            }
            JsonObject result = envelope.getAsJsonObject("result");
            if (result == null || !result.has("chunks") || !result.get("chunks").isJsonArray()) {
                return null;
            }
            var arr = result.getAsJsonArray("chunks");
            List<Chunk> chunks = new ArrayList<>(arr.size());
            for (var elem : arr) {
                JsonObject obj = elem.getAsJsonObject();
                chunks.add(new FerriteChunk(this, obj.get("x").getAsInt(), obj.get("z").getAsInt()));
            }
            return chunks;
        } catch (UnsatisfiedLinkError | NoClassDefFoundError | RuntimeException unavailable) {
            return null;
        }
    }

    private List<Entity> queryHostEntitiesSnapshot() {
        try {
            String req = "{\"world\":\"" + hostWorldKey() + "\"}";
            String raw = Bootstrap.worldEntitiesSnapshot(req);
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (!envelope.has("ok") || !envelope.get("ok").getAsBoolean()) {
                return null;
            }
            JsonObject result = envelope.getAsJsonObject("result");
            if (result == null || !result.has("entities") || !result.get("entities").isJsonArray()) {
                return null;
            }
            var arr = result.getAsJsonArray("entities");
            List<Entity> entities = new ArrayList<>(arr.size());
            for (var elem : arr) {
                Entity parsed = entityFromJson(elem.getAsJsonObject());
                if (parsed != null) {
                    entities.add(parsed);
                }
            }
            return entities;
        } catch (UnsatisfiedLinkError | NoClassDefFoundError | RuntimeException unavailable) {
            return null;
        }
    }

    private Entity entityFromJson(JsonObject result) {
        if (result == null) {
            return null;
        }
        String worldKey = result.get("world").getAsString();
        if (!hostWorldKey().equals(worldKey)) {
            return null;
        }
        UUID uuid = UUID.fromString(result.get("uuid").getAsString());
        Player online = runtime.player(uuid);
        if (online != null && equals(online.getWorld())) {
            return online;
        }
        String typeKey = result.get("type").getAsString();
        EntityType type = resolveEntityType(typeKey);
        String name = result.has("name") && !result.get("name").isJsonNull()
                ? result.get("name").getAsString()
                : typeKey;
        String customName = result.has("custom_name") && !result.get("custom_name").isJsonNull()
                ? result.get("custom_name").getAsString()
                : null;
        double x = result.get("x").getAsDouble();
        double y = result.get("y").getAsDouble();
        double z = result.get("z").getAsDouble();
        float yaw = result.get("yaw").getAsFloat();
        float pitch = result.get("pitch").getAsFloat();
        double vx = result.has("vx") ? result.get("vx").getAsDouble() : 0.0;
        double vy = result.has("vy") ? result.get("vy").getAsDouble() : 0.0;
        double vz = result.has("vz") ? result.get("vz").getAsDouble() : 0.0;
        Double health = result.has("health") && !result.get("health").isJsonNull()
                ? result.get("health").getAsDouble()
                : null;
        int networkId = result.has("network_id") && !result.get("network_id").isJsonNull()
                ? result.get("network_id").getAsInt()
                : (result.has("id") && !result.get("id").isJsonNull() ? result.get("id").getAsInt() : 0);
        Map<EquipmentSlot, ItemStack> equipment = parseEquipment(result);
        Map<EquipmentSlot, Float> dropChances = parseDropChances(result);
        return FerriteEntitySnapshot.create(
                this, uuid, type, name, customName, x, y, z, yaw, pitch, vx, vy, vz, health,
                networkId, equipment, dropChances);
    }

    private static EquipmentSlot parseEquipmentSlot(String key) {
        return switch (key.toUpperCase(java.util.Locale.ROOT)) {
            case "HAND", "MAIN_HAND", "MAINHAND" -> EquipmentSlot.HAND;
            case "OFF_HAND", "OFFHAND" -> EquipmentSlot.OFF_HAND;
            case "FEET", "BOOTS" -> EquipmentSlot.FEET;
            case "LEGS", "LEGGINGS" -> EquipmentSlot.LEGS;
            case "CHEST", "CHESTPLATE" -> EquipmentSlot.CHEST;
            case "HEAD", "HELMET" -> EquipmentSlot.HEAD;
            case "BODY" -> EquipmentSlot.BODY;
            case "SADDLE" -> EquipmentSlot.SADDLE;
            default -> null;
        };
    }

    private static Map<EquipmentSlot, ItemStack> parseEquipment(JsonObject result) {
        if (!result.has("equipment") || !result.get("equipment").isJsonObject()) {
            return Map.of();
        }
        JsonObject eqObj = result.getAsJsonObject("equipment");
        EnumMap<EquipmentSlot, ItemStack> map = new EnumMap<>(EquipmentSlot.class);
        for (String key : eqObj.keySet()) {
            EquipmentSlot slot = parseEquipmentSlot(key);
            if (slot == null || !eqObj.get(key).isJsonObject()) {
                continue;
            }
            JsonObject itemObj = eqObj.getAsJsonObject(key);
            if (!itemObj.has("id") || !itemObj.get("id").isJsonPrimitive()) {
                continue;
            }
            String id = itemObj.get("id").getAsString();
            Material mat = Material.matchMaterial(id);
            if (mat == null || mat.isAir()) {
                continue;
            }
            int count = itemObj.has("count") && itemObj.get("count").isJsonPrimitive()
                    ? Math.max(1, itemObj.get("count").getAsInt())
                    : 1;
            if (itemObj.has("nbt_base64") && !itemObj.get("nbt_base64").isJsonNull()) {
                String nbtBase64 = itemObj.get("nbt_base64").getAsString();
                map.put(slot, FerriteItemNbt.decodeItemBase64(nbtBase64, mat, count));
            } else {
                map.put(slot, new ItemStack(mat, count));
            }
        }
        return map.isEmpty() ? Map.of() : map;
    }

    private static Map<EquipmentSlot, Float> parseDropChances(JsonObject result) {
        if (!result.has("drop_chances") || !result.get("drop_chances").isJsonObject()) {
            return Map.of();
        }
        JsonObject dcObj = result.getAsJsonObject("drop_chances");
        EnumMap<EquipmentSlot, Float> map = new EnumMap<>(EquipmentSlot.class);
        for (String key : dcObj.keySet()) {
            EquipmentSlot slot = parseEquipmentSlot(key);
            if (slot != null && dcObj.get(key).isJsonPrimitive()) {
                float chance = dcObj.get(key).getAsFloat();
                if (Float.isFinite(chance)) {
                    map.put(slot, chance);
                }
            }
        }
        return map.isEmpty() ? Map.of() : map;
    }

    private Entity queryHostEntity(UUID uuid) {
        try {
            String raw = Bootstrap.entityStateGet("{\"uuid\":\"" + uuid + "\"}");
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (!envelope.get("ok").getAsBoolean()) {
                return null;
            }
            return entityFromJson(envelope.getAsJsonObject("result"));
        } catch (UnsatisfiedLinkError | NoClassDefFoundError | RuntimeException unavailable) {
            return null;
        }
    }

    private static EntityType resolveEntityType(String typeKey) {
        for (EntityType candidate : EntityType.values()) {
            if (candidate != EntityType.UNKNOWN && candidate.getKey().asString().equals(typeKey)) {
                return candidate;
            }
        }
        return EntityType.UNKNOWN;
    }

    @Override
    public long getTime() {
        return time;
    }

    @Override
    public void setTime(long time) {
        this.time = time;
        forwardTimeToHost(time);
    }

    @Override
    public long getFullTime() {
        return time;
    }

    @Override
    public void setFullTime(long time) {
        this.time = time;
        forwardTimeToHost(time);
    }

    @Override
    public boolean hasStorm() {
        return storm;
    }

    @Override
    public void setStorm(boolean storm) {
        this.storm = storm;
        forwardWeatherToHost(this.storm, this.thundering, this.weatherDuration);
    }

    @Override
    public boolean isThundering() {
        return thundering;
    }

    @Override
    public void setThundering(boolean thundering) {
        this.thundering = thundering;
        forwardWeatherToHost(this.storm, this.thundering, this.weatherDuration);
    }

    @Override
    public int getWeatherDuration() {
        return weatherDuration;
    }

    @Override
    public void setWeatherDuration(int duration) {
        if (duration < 0) {
            throw new IllegalArgumentException("weather duration must be >= 0");
        }
        this.weatherDuration = duration;
    }

    @Override
    public int getThunderDuration() {
        return thunderDuration;
    }

    @Override
    public void setThunderDuration(int duration) {
        if (duration < 0) {
            throw new IllegalArgumentException("thunder duration must be >= 0");
        }
        this.thunderDuration = duration;
    }

    @Override
    public String[] getGameRules() {
        return gameRules.keySet().toArray(String[]::new);
    }

    @Override
    public String getGameRuleValue(String rule) {
        Object value = gameRules.get(rule);
        return value == null ? null : value.toString();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> T getGameRuleValue(GameRule<T> rule) {
        Object value = gameRules.get(rule.getName());
        return value == null ? rule.getDefaultValue() : (T) value;
    }

    @Override
    public boolean setGameRuleValue(String rule, String value) {
        throw Unsupported.exception("World#setGameRuleValue");
    }

    @Override
    public <T> boolean setGameRule(GameRule<T> rule, T newValue) {
        Objects.requireNonNull(rule, "rule");
        Objects.requireNonNull(newValue, "newValue");
        gameRules.put(rule.getName(), newValue);
        return true;
    }

    @Override
    public boolean getPVP() {
        return pvp;
    }

    @Override
    public void setPVP(boolean pvp) {
        this.pvp = pvp;
    }

    @Override
    public org.bukkit.Difficulty getDifficulty() {
        return difficulty;
    }

    @Override
    public void setDifficulty(org.bukkit.Difficulty difficulty) {
        this.difficulty = Objects.requireNonNull(difficulty, "difficulty");
    }

    @Override
    public void playSound(
            Location location,
            org.bukkit.Sound sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch) {
        playSound(location, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Location location,
            String sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch) {
        playSound(location, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Location location,
            org.bukkit.Sound sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch,
            long seed) {
        if (location == null || sound == null) {
            return;
        }
        for (Player player : getPlayers()) {
            player.playSound(location, sound, category, volume, pitch, seed);
        }
    }

    @Override
    public void playSound(
            Location location,
            String sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch,
            long seed) {
        if (location == null || sound == null) {
            return;
        }
        for (Player player : getPlayers()) {
            player.playSound(location, sound, category, volume, pitch, seed);
        }
    }

    @Override
    public void playSound(
            Entity entity,
            org.bukkit.Sound sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch) {
        playSound(entity, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Entity entity,
            String sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch) {
        playSound(entity, sound, category, volume, pitch, 0L);
    }

    @Override
    public void playSound(
            Entity entity,
            org.bukkit.Sound sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch,
            long seed) {
        if (entity == null || sound == null) {
            return;
        }
        for (Player player : getPlayers()) {
            player.playSound(entity, sound, category, volume, pitch, seed);
        }
    }

    @Override
    public void playSound(
            Entity entity,
            String sound,
            org.bukkit.SoundCategory category,
            float volume,
            float pitch,
            long seed) {
        if (entity == null || sound == null) {
            return;
        }
        for (Player player : getPlayers()) {
            player.playSound(entity, sound, category, volume, pitch, seed);
        }
    }

    @Override
    public <T> void spawnParticle(
            org.bukkit.Particle particle,
            List<Player> receivers,
            Player source,
            double x,
            double y,
            double z,
            int count,
            double offsetX,
            double offsetY,
            double offsetZ,
            double extraX,
            double extraY,
            double extraZ,
            T data,
            boolean force,
            org.bukkit.Particle.RandomizationType randomizationType) {
        Objects.requireNonNull(particle, "particle");
        List<Player> targets = receivers != null ? receivers : getPlayers();
        for (Player target : targets) {
            if (target != null && (source == null || target.canSee(source))) {
                target.spawnParticle(
                        particle,
                        x,
                        y,
                        z,
                        count,
                        offsetX,
                        offsetY,
                        offsetZ,
                        extraX,
                        extraY,
                        extraZ,
                        data,
                        force,
                        randomizationType);
            }
        }
    }

    @Override
    public WorldBorder getWorldBorder() {
        return worldBorder;
    }

    @Override
    public org.bukkit.entity.LightningStrike strikeLightning(Location loc) {
        Objects.requireNonNull(loc, "loc");
        forwardLightningToHost(loc.getX(), loc.getY(), loc.getZ());
        return null;
    }

    @Override
    public org.bukkit.entity.LightningStrike strikeLightningEffect(Location loc) {
        Objects.requireNonNull(loc, "loc");
        forwardLightningToHost(loc.getX(), loc.getY(), loc.getZ());
        return null;
    }

    @Override
    public boolean createExplosion(
            double x,
            double y,
            double z,
            float power,
            boolean setFire,
            boolean breakBlocks,
            Entity source) {
        forwardExplosionToHost(x, y, z, power, setFire, breakBlocks);
        return true;
    }

    @Override
    public boolean createExplosion(
            Entity source,
            Location loc,
            float power,
            boolean setFire,
            boolean breakBlocks,
            boolean excludeSourceFromDamage) {
        Objects.requireNonNull(loc, "loc");
        return createExplosion(loc.getX(), loc.getY(), loc.getZ(), power, setFire, breakBlocks, source);
    }

    @Override
    public boolean createExplosion(
            Location loc,
            float power,
            boolean setFire,
            boolean breakBlocks,
            Entity source) {
        Objects.requireNonNull(loc, "loc");
        return createExplosion(loc.getX(), loc.getY(), loc.getZ(), power, setFire, breakBlocks, source);
    }

    @Override
    public Entity spawnEntity(Location loc, EntityType type, boolean randomizeData) {
        Objects.requireNonNull(loc, "loc");
        Objects.requireNonNull(type, "type");
        UUID uuid = UUID.randomUUID();
        String key = type.getKey().asString();
        Entity entity = FerriteEntitySnapshot.create(
                this,
                uuid,
                type,
                type.name(),
                null,
                loc.getX(),
                loc.getY(),
                loc.getZ(),
                loc.getYaw(),
                loc.getPitch(),
                0.0,
                0.0,
                0.0,
                type.isAlive() ? 20.0 : null,
                Math.abs(uuid.hashCode()));
        worldEntities.put(uuid, entity);
        forwardSpawnEntityToHost(key, loc.getX(), loc.getY(), loc.getZ(), uuid);
        return entity;
    }

    @Override
    public org.bukkit.entity.Item dropItem(
            Location location,
            ItemStack item,
            java.util.function.Consumer<? super org.bukkit.entity.Item> function) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(item, "item");
        forwardDropItemToHost(
                item.getType().getKey().asString(),
                Math.max(1, Math.min(99, item.getAmount())),
                location.getX(),
                location.getY(),
                location.getZ(),
                false,
                UUID.randomUUID());
        return null;
    }

    @Override
    public org.bukkit.entity.Item dropItemNaturally(
            Location location,
            ItemStack item,
            java.util.function.Consumer<? super org.bukkit.entity.Item> function) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(item, "item");
        forwardDropItemToHost(
                item.getType().getKey().asString(),
                Math.max(1, Math.min(99, item.getAmount())),
                location.getX(),
                location.getY(),
                location.getZ(),
                true,
                UUID.randomUUID());
        return null;
    }

    private void forwardSpawnEntityToHost(String kind, double x, double y, double z, UUID uuid) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("kind", kind);
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("uuid", uuid.toString());
            Bootstrap.worldSpawnEntity(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardDropItemToHost(
            String item, int amount, double x, double y, double z, boolean natural, UUID uuid) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("item", item);
            req.put("amount", amount);
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("natural", natural);
            req.put("uuid", uuid.toString());
            Bootstrap.worldDropItem(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardLightningToHost(double x, double y, double z) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            Bootstrap.worldLightningStrike(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardExplosionToHost(
            double x, double y, double z, float power, boolean fire, boolean breaks) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("x", x);
            req.put("y", y);
            req.put("z", z);
            req.put("power", power);
            req.put("fire", fire);
            req.put("breaks", breaks);
            Bootstrap.worldExplosionCreate(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardTimeToHost(long ticks) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("time", ticks);
            Bootstrap.worldTimeSet(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private void forwardWeatherToHost(boolean hasStorm, boolean isThunder, int duration) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("world", spec.name());
            req.put("storm", hasStorm);
            req.put("thunder", isThunder);
            req.put("duration", Math.max(1, duration));
            Bootstrap.worldWeatherSet(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteWorld other && other.spec.uniqueId().equals(spec.uniqueId());
    }

    @Override
    public int hashCode() {
        return spec.uniqueId().hashCode();
    }

    @Override
    public String toString() {
        return "FerriteWorld{" + spec.name() + "}";
    }
}
