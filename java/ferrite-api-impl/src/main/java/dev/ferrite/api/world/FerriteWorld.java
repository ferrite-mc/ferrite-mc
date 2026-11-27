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
    private final Map<Long, Map<String, List<org.bukkit.metadata.MetadataValue>>> blockMetadata =
            new ConcurrentHashMap<>();
    private final Map<String, List<org.bukkit.metadata.MetadataValue>> worldMetadata =
            new ConcurrentHashMap<>();
    private final dev.ferrite.api.item.FerritePersistentDataContainer persistentData =
            new dev.ferrite.api.item.FerritePersistentDataContainer();
    private final EnumMap<org.bukkit.entity.SpawnCategory, Long> ticksPerSpawns =
            new EnumMap<>(org.bukkit.entity.SpawnCategory.class);
    private final EnumMap<org.bukkit.entity.SpawnCategory, Integer> spawnLimits =
            new EnumMap<>(org.bukkit.entity.SpawnCategory.class);
    private volatile Set<String> negotiatedV3Ops;

    private double spawnX;
    private double spawnY;
    private double spawnZ;
    private float spawnYaw;
    private float spawnPitch;
    private long time;
    private boolean storm;
    private boolean thundering;
    private int weatherDuration = 6000;
    private int thunderDuration = 6000;
    private int clearWeatherDuration;
    private volatile boolean pvp = true;
    private volatile org.bukkit.Difficulty difficulty = org.bukkit.Difficulty.NORMAL;
    private volatile boolean autoSave = true;
    private volatile boolean hardcore;
    private volatile boolean allowAnimals = true;
    private volatile boolean allowMonsters = true;
    private volatile int viewDistance = DEFAULT_VIEW_DISTANCE;
    private volatile int simulationDistance = DEFAULT_VIEW_DISTANCE;
    private volatile int sendViewDistance = DEFAULT_VIEW_DISTANCE;
    private volatile boolean voidDamageEnabled;
    private volatile float voidDamageAmount = 4.0f;
    private volatile double voidDamageMinBuildHeightOffset = -64.0;

    public FerriteWorld(FerriteRuntime runtime, WorldSpec spec) {
        this.runtime = runtime;
        this.spec = spec;
        this.blocks = new InMemoryBlockStore(spec.defaultBlock());
        this.worldBorder = new FerriteWorldBorder(this);
        this.spawnX = spec.spawnX();
        this.spawnY = spec.spawnY();
        this.spawnZ = spec.spawnZ();
        this.time = spec.time();
        this.voidDamageEnabled = spec.environment() != World.Environment.NETHER;
        this.ticksPerSpawns.put(org.bukkit.entity.SpawnCategory.ANIMAL, 400L);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.MONSTER, 70);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.ANIMAL, 10);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.WATER_ANIMAL, 5);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.WATER_AMBIENT, 20);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.WATER_UNDERGROUND_CREATURE, 5);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.AMBIENT, 15);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.AXOLOTL, 5);
        this.spawnLimits.put(org.bukkit.entity.SpawnCategory.MISC, 0);
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
        return new Location(this, spawnX, spawnY, spawnZ, spawnYaw, spawnPitch);
    }

    @Override
    public boolean setSpawnLocation(Location location) {
        Objects.requireNonNull(location, "location");
        if (location.getWorld() != null && !equals(location.getWorld())) {
            return false;
        }
        this.spawnX = location.getX();
        this.spawnY = location.getY();
        this.spawnZ = location.getZ();
        this.spawnYaw = location.getYaw();
        this.spawnPitch = location.getPitch();
        return true;
    }

    @Override
    public boolean setSpawnLocation(int x, int y, int z, float angle) {
        this.spawnX = x;
        this.spawnY = y;
        this.spawnZ = z;
        this.spawnYaw = angle;
        return true;
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
        if (this.storm != storm && runtime.isPrimaryThread()) {
            org.bukkit.event.weather.WeatherChangeEvent event =
                    new org.bukkit.event.weather.WeatherChangeEvent(this, storm);
            runtime.plugins().callEvent(event);
            if (event.isCancelled()) {
                return;
            }
        }
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
    public Entity spawnEntity(Location loc, EntityType type) {
        return spawnEntity(loc, type, true);
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
        if (entity instanceof org.bukkit.entity.LivingEntity living && runtime.isPrimaryThread()) {
            org.bukkit.event.entity.CreatureSpawnEvent spawnEvent =
                    new org.bukkit.event.entity.CreatureSpawnEvent(
                            living, org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason.CUSTOM);
            runtime.plugins().callEvent(spawnEvent);
            if (spawnEvent.isCancelled()) {
                return entity;
            }
        }
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

    private static long packBlockCoord(int x, int y, int z) {
        return ((long) (x & 0x3FFFFFF) << 38) | ((long) (z & 0x3FFFFFF) << 12) | (y & 0xFFFL);
    }

    public void setBlockMetadata(
            int x, int y, int z, String key, org.bukkit.metadata.MetadataValue value) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(value, "value");
        long coord = packBlockCoord(x, y, z);
        Map<String, List<org.bukkit.metadata.MetadataValue>> byKey =
                blockMetadata.computeIfAbsent(coord, ignored -> new ConcurrentHashMap<>());
        byKey.compute(key, (k, existing) -> {
            List<org.bukkit.metadata.MetadataValue> updated = existing == null
                    ? new ArrayList<>()
                    : new ArrayList<>(existing);
            updated.removeIf(v -> v.getOwningPlugin() == value.getOwningPlugin());
            updated.add(value);
            return updated;
        });
    }

    public List<org.bukkit.metadata.MetadataValue> getBlockMetadata(int x, int y, int z, String key) {
        Objects.requireNonNull(key, "key");
        Map<String, List<org.bukkit.metadata.MetadataValue>> byKey =
                blockMetadata.get(packBlockCoord(x, y, z));
        if (byKey == null) {
            return List.of();
        }
        List<org.bukkit.metadata.MetadataValue> values = byKey.get(key);
        return values == null ? List.of() : List.copyOf(values);
    }

    public boolean hasBlockMetadata(int x, int y, int z, String key) {
        Objects.requireNonNull(key, "key");
        Map<String, List<org.bukkit.metadata.MetadataValue>> byKey =
                blockMetadata.get(packBlockCoord(x, y, z));
        if (byKey == null) {
            return false;
        }
        List<org.bukkit.metadata.MetadataValue> values = byKey.get(key);
        return values != null && !values.isEmpty();
    }

    public void removeBlockMetadata(int x, int y, int z, String key, org.bukkit.plugin.Plugin owner) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(owner, "owner");
        Map<String, List<org.bukkit.metadata.MetadataValue>> byKey =
                blockMetadata.get(packBlockCoord(x, y, z));
        if (byKey == null) {
            return;
        }
        byKey.computeIfPresent(key, (k, existing) -> {
            List<org.bukkit.metadata.MetadataValue> updated = new ArrayList<>(existing);
            updated.removeIf(v -> v.getOwningPlugin() == owner);
            return updated.isEmpty() ? null : updated;
        });
    }

    @Override
    public int getEntityCount() {
        return getEntities().size();
    }

    @Override
    public int getPlayerCount() {
        return getPlayers().size();
    }

    @Override
    public int getChunkCount() {
        return loadedChunks.size();
    }

    @Override
    public int getTileEntityCount() {
        return 0;
    }

    @Override
    public int getTickableTileEntityCount() {
        return 0;
    }

    @Override
    public long getSeed() {
        return worldSeed();
    }

    @Override
    public NamespacedKey getKey() {
        String path = spec.name().toLowerCase(java.util.Locale.ROOT).replaceAll("[^a-z0-9/._-]", "_");
        return NamespacedKey.minecraft(path);
    }

    @Override
    public java.nio.file.Path getWorldPath() {
        return java.nio.file.Path.of(spec.name()).toAbsolutePath();
    }

    @Override
    public org.bukkit.WorldType getWorldType() {
        return org.bukkit.WorldType.NORMAL;
    }

    @Override
    public boolean canGenerateStructures() {
        return true;
    }

    @Override
    public boolean hasBonusChest() {
        return false;
    }

    @Override
    public int getLogicalHeight() {
        return getMaxHeight();
    }

    @Override
    public int getSeaLevel() {
        return switch (spec.environment()) {
            case NETHER -> 32;
            case THE_END -> 0;
            default -> 63;
        };
    }

    @Override
    public boolean isNatural() {
        return spec.environment() == World.Environment.NORMAL;
    }

    @Override
    public boolean isBedWorks() {
        return spec.environment() == World.Environment.NORMAL;
    }

    @Override
    public boolean hasSkyLight() {
        return spec.environment() == World.Environment.NORMAL;
    }

    @Override
    public boolean hasCeiling() {
        return spec.environment() == World.Environment.NETHER;
    }

    @Override
    public boolean isPiglinSafe() {
        return spec.environment() == World.Environment.NETHER;
    }

    @Override
    public boolean isRespawnAnchorWorks() {
        return spec.environment() == World.Environment.NETHER;
    }

    @Override
    public boolean hasRaids() {
        return spec.environment() == World.Environment.NORMAL;
    }

    @Override
    public boolean isUltraWarm() {
        return spec.environment() == World.Environment.NETHER;
    }

    @Override
    public double getCoordinateScale() {
        return spec.environment() == World.Environment.NETHER ? 8.0 : 1.0;
    }

    @Override
    public boolean isFixedTime() {
        return spec.environment() != World.Environment.NORMAL;
    }

    @Override
    public boolean isDayTime() {
        long dayTime = Math.floorMod(getTime(), 24000L);
        return dayTime < 12300L || dayTime > 23850L;
    }

    @Override
    public long getGameTime() {
        return runtime.currentTick();
    }

    @Override
    public io.papermc.paper.world.MoonPhase getMoonPhase() {
        return io.papermc.paper.world.MoonPhase.getPhase(Math.floorDiv(getFullTime(), 24000L));
    }

    @Override
    public boolean isClearWeather() {
        return !storm && !thundering;
    }

    @Override
    public int getClearWeatherDuration() {
        return clearWeatherDuration;
    }

    @Override
    public void setClearWeatherDuration(int duration) {
        if (duration < 0) {
            throw new IllegalArgumentException("clearWeatherDuration must be >= 0, was " + duration);
        }
        this.clearWeatherDuration = duration;
        if (duration > 0) {
            this.storm = false;
            this.thundering = false;
        }
    }

    @Override
    public boolean isAutoSave() {
        return autoSave;
    }

    @Override
    public void setAutoSave(boolean value) {
        this.autoSave = value;
    }

    @Override
    public void save(boolean flush) {
        // Facade state is already in-memory; save succeeds without native disk wait.
    }

    @Override
    public boolean isHardcore() {
        return hardcore;
    }

    @Override
    public void setHardcore(boolean hardcore) {
        this.hardcore = hardcore;
    }

    @Override
    public boolean getAllowAnimals() {
        return allowAnimals;
    }

    @Override
    public boolean getAllowMonsters() {
        return allowMonsters;
    }

    @Override
    public void setAllowMonsterSpawning(boolean allowMonsters) {
        this.allowMonsters = allowMonsters;
    }

    @Override
    public void setSpawnFlags(boolean allowMonsters, boolean allowAnimals) {
        this.allowMonsters = allowMonsters;
        this.allowAnimals = allowAnimals;
    }

    @Override
    public int getViewDistance() {
        return viewDistance;
    }

    @Override
    public void setViewDistance(int viewDistance) {
        if (viewDistance < 2 || viewDistance > 32) {
            throw new IllegalArgumentException("viewDistance out of range: " + viewDistance);
        }
        this.viewDistance = viewDistance;
    }

    @Override
    public int getSimulationDistance() {
        return simulationDistance;
    }

    @Override
    public void setSimulationDistance(int simulationDistance) {
        if (simulationDistance < 2 || simulationDistance > 32) {
            throw new IllegalArgumentException("simulationDistance out of range: " + simulationDistance);
        }
        this.simulationDistance = simulationDistance;
    }

    @Override
    public int getSendViewDistance() {
        return sendViewDistance;
    }

    @Override
    public void setSendViewDistance(int viewDistance) {
        if (viewDistance < -1 || viewDistance > 32) {
            throw new IllegalArgumentException("sendViewDistance out of range: " + viewDistance);
        }
        this.sendViewDistance = viewDistance < 0 ? this.viewDistance : viewDistance;
    }

    @Override
    public long getTicksPerSpawns(org.bukkit.entity.SpawnCategory spawnCategory) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        return ticksPerSpawns.getOrDefault(spawnCategory, 1L);
    }

    @Override
    public void setTicksPerSpawns(org.bukkit.entity.SpawnCategory spawnCategory, int ticksPerSpawns) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        this.ticksPerSpawns.put(spawnCategory, (long) ticksPerSpawns);
    }

    @Override
    public int getSpawnLimit(org.bukkit.entity.SpawnCategory spawnCategory) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        return spawnLimits.getOrDefault(spawnCategory, 0);
    }

    @Override
    public void setSpawnLimit(org.bukkit.entity.SpawnCategory spawnCategory, int limit) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        this.spawnLimits.put(spawnCategory, Math.max(0, limit));
    }

    @Override
    public boolean isGameRule(String rule) {
        if (rule == null || rule.isEmpty()) {
            return false;
        }
        if (gameRules.containsKey(rule)) {
            return true;
        }
        return GameRule.getByName(rule) != null;
    }

    @Override
    public double getTemperature(int x, int y, int z) {
        return switch (spec.environment()) {
            case NETHER -> 2.0;
            case THE_END -> 0.5;
            default -> 0.8;
        };
    }

    @Override
    public double getHumidity(int x, int y, int z) {
        return spec.environment() == World.Environment.NORMAL ? 0.4 : 0.0;
    }

    @Override
    public boolean isVoidDamageEnabled() {
        return voidDamageEnabled;
    }

    @Override
    public void setVoidDamageEnabled(boolean enabled) {
        this.voidDamageEnabled = enabled;
    }

    @Override
    public float getVoidDamageAmount() {
        return voidDamageAmount;
    }

    @Override
    public void setVoidDamageAmount(float voidDamageAmount) {
        this.voidDamageAmount = voidDamageAmount;
    }

    @Override
    public double getVoidDamageMinBuildHeightOffset() {
        return voidDamageMinBuildHeightOffset;
    }

    @Override
    public void setVoidDamageMinBuildHeightOffset(double offset) {
        this.voidDamageMinBuildHeightOffset = offset;
    }

    @Override
    public org.bukkit.block.BlockState getBlockState(int x, int y, int z) {
        return getBlockAt(x, y, z).getState();
    }

    @Override
    public org.bukkit.block.data.BlockData getBlockData(int x, int y, int z) {
        return getBlockAt(x, y, z).getBlockData();
    }

    @Override
    public Material getType(int x, int y, int z) {
        return blocks.getType(x, y, z);
    }

    @Override
    public void setBlockData(int x, int y, int z, org.bukkit.block.data.BlockData blockData) {
        Objects.requireNonNull(blockData, "blockData");
        getBlockAt(x, y, z).setBlockData(blockData);
    }

    @Override
    public int getHighestBlockYAt(int x, int z, org.bukkit.HeightMap heightMap) {
        Objects.requireNonNull(heightMap, "heightMap");
        for (int y = getMaxHeight() - 1; y >= getMinHeight(); y--) {
            Block block = getBlockAt(x, y, z);
            Material mat = block.getType();
            if (mat.isAir()) {
                continue;
            }
            boolean match = switch (heightMap) {
                case WORLD_SURFACE, WORLD_SURFACE_WG -> true;
                case OCEAN_FLOOR, OCEAN_FLOOR_WG -> block.isSolid() && !block.isLiquid();
                case MOTION_BLOCKING -> block.isSolid() || block.isLiquid();
                case MOTION_BLOCKING_NO_LEAVES ->
                    (block.isSolid() || block.isLiquid()) && !mat.name().endsWith("_LEAVES");
            };
            if (match) {
                return y;
            }
        }
        return getMinHeight();
    }

    @Override
    public int getHighestBlockYAt(Location location, org.bukkit.HeightMap heightMap) {
        Objects.requireNonNull(location, "location");
        return getHighestBlockYAt(location.getBlockX(), location.getBlockZ(), heightMap);
    }

    @Override
    public Collection<Chunk> getForceLoadedChunks() {
        List<Chunk> result = new ArrayList<>(forceLoadedChunks.size());
        for (long key : forceLoadedChunks) {
            result.add(new FerriteChunk(this, (int) key, (int) (key >> 32)));
        }
        return Set.copyOf(result);
    }

    @Override
    public boolean addPluginChunkTicket(int x, int z, org.bukkit.plugin.Plugin plugin) {
        return addChunkTicket(x, z, plugin);
    }

    @Override
    public boolean removePluginChunkTicket(int x, int z, org.bukkit.plugin.Plugin plugin) {
        return removeChunkTicket(x, z, plugin);
    }

    @Override
    public void removePluginChunkTickets(org.bukkit.plugin.Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        for (Set<org.bukkit.plugin.Plugin> holders : chunkTickets.values()) {
            holders.remove(plugin);
        }
    }

    @Override
    public Collection<org.bukkit.plugin.Plugin> getPluginChunkTickets(int x, int z) {
        return chunkTickets(x, z);
    }

    @Override
    public Map<org.bukkit.plugin.Plugin, Collection<Chunk>> getPluginChunkTickets() {
        Map<org.bukkit.plugin.Plugin, Collection<Chunk>> byPlugin = new LinkedHashMap<>();
        for (Map.Entry<Long, Set<org.bukkit.plugin.Plugin>> entry : chunkTickets.entrySet()) {
            long key = entry.getKey();
            Chunk chunk = new FerriteChunk(this, (int) key, (int) (key >> 32));
            for (org.bukkit.plugin.Plugin plugin : entry.getValue()) {
                byPlugin.computeIfAbsent(plugin, ignored -> new ArrayList<>()).add(chunk);
            }
        }
        return byPlugin;
    }

    @Override
    public Collection<Chunk> getIntersectingChunks(BoundingBox box) {
        Objects.requireNonNull(box, "box");
        int minCx = Math.floorDiv((int) Math.floor(box.getMinX()), 16);
        int maxCx = Math.floorDiv((int) Math.floor(box.getMaxX()), 16);
        int minCz = Math.floorDiv((int) Math.floor(box.getMinZ()), 16);
        int maxCz = Math.floorDiv((int) Math.floor(box.getMaxZ()), 16);
        List<Chunk> chunks = new ArrayList<>();
        for (int cx = minCx; cx <= maxCx; cx++) {
            for (int cz = minCz; cz <= maxCz; cz++) {
                chunks.add(getChunkAt(cx, cz, false));
            }
        }
        return List.copyOf(chunks);
    }

    @Override
    public io.papermc.paper.world.chunk.ChunkGenerationState getChunkGenerationState(int x, int z) {
        return isChunkGenerated(x, z)
                ? io.papermc.paper.world.chunk.ChunkGenerationState.GENERATED
                : io.papermc.paper.world.chunk.ChunkGenerationState.NOT_GENERATED;
    }

    @Override
    public java.util.concurrent.CompletableFuture<io.papermc.paper.world.chunk.ChunkGenerationState>
            getChunkGenerationStateAsync(int x, int z) {
        return java.util.concurrent.CompletableFuture.completedFuture(getChunkGenerationState(x, z));
    }

    @Override
    public void getChunkAtAsync(
            int x,
            int z,
            boolean gen,
            boolean urgent,
            java.util.function.Consumer<? super Chunk> cb) {
        Objects.requireNonNull(cb, "cb");
        cb.accept(getChunkAt(x, z, gen));
    }

    @Override
    public void getChunksAtAsync(
            int minX, int minZ, int maxX, int maxZ, boolean urgent, Runnable cb) {
        for (int cx = minX; cx <= maxX; cx++) {
            for (int cz = minZ; cz <= maxZ; cz++) {
                loadChunk(cx, cz, true);
            }
        }
        if (cb != null) {
            cb.run();
        }
    }

    @Override
    public boolean refreshChunk(int x, int z) {
        return isChunkLoaded(x, z);
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceEntities(
            io.papermc.paper.math.Position start,
            org.bukkit.util.Vector direction,
            double maxDistance,
            double raySize,
            Predicate<? super Entity> filter) {
        Objects.requireNonNull(start, "start");
        Objects.requireNonNull(direction, "direction");
        if (maxDistance <= 0.0 || direction.lengthSquared() == 0.0) {
            return null;
        }
        org.bukkit.util.Vector dir = direction.clone().normalize();
        org.bukkit.util.Vector origin = start.toVector();
        Entity closest = null;
        double closestDist = maxDistance;
        org.bukkit.util.Vector closestHit = null;
        for (Entity entity : getEntities()) {
            if (filter != null && !filter.test(entity)) {
                continue;
            }
            BoundingBox box;
            try {
                box = entity.getBoundingBox();
            } catch (UnsupportedOperationException ignored) {
                Location loc = entity.getLocation();
                box = BoundingBox.of(new org.bukkit.util.Vector(loc.getX(), loc.getY() + 0.9, loc.getZ()), 0.4, 0.9, 0.4);
            }
            if (raySize > 0.0) {
                box = box.clone().expand(raySize);
            }
            org.bukkit.util.RayTraceResult hit = box.rayTrace(origin, dir, closestDist);
            if (hit != null) {
                double dist = origin.distance(hit.getHitPosition());
                if (dist < closestDist) {
                    closestDist = dist;
                    closest = entity;
                    closestHit = hit.getHitPosition();
                }
            }
        }
        return closest == null ? null : new org.bukkit.util.RayTraceResult(closestHit, closest);
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceBlocks(
            io.papermc.paper.math.Position start,
            org.bukkit.util.Vector direction,
            double maxDistance,
            org.bukkit.FluidCollisionMode fluidCollisionMode,
            boolean ignorePassableBlocks,
            Predicate<? super Block> canCollide) {
        Objects.requireNonNull(start, "start");
        Objects.requireNonNull(direction, "direction");
        if (canCollide == null) {
            return rayTraceBlocks(start.toLocation(this), direction, maxDistance, fluidCollisionMode, ignorePassableBlocks);
        }
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
                            || (mat != Material.WATER && mat != Material.LAVA))
                    && canCollide.test(block);
            if (hit) {
                org.bukkit.block.BlockFace face = org.bukkit.block.BlockFace.UP;
                if (bx > prevX) face = org.bukkit.block.BlockFace.WEST;
                else if (bx < prevX) face = org.bukkit.block.BlockFace.EAST;
                else if (by > prevY) face = org.bukkit.block.BlockFace.DOWN;
                else if (by < prevY) face = org.bukkit.block.BlockFace.UP;
                else if (bz > prevZ) face = org.bukkit.block.BlockFace.NORTH;
                else if (bz < prevZ) face = org.bukkit.block.BlockFace.SOUTH;
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
    public org.bukkit.util.RayTraceResult rayTrace(
            io.papermc.paper.math.Position start,
            org.bukkit.util.Vector direction,
            double maxDistance,
            org.bukkit.FluidCollisionMode fluidCollisionMode,
            boolean ignorePassableBlocks,
            double raySize,
            Predicate<? super Entity> entityFilter,
            Predicate<? super Block> blockFilter) {
        org.bukkit.util.RayTraceResult blockHit = rayTraceBlocks(
                start, direction, maxDistance, fluidCollisionMode, ignorePassableBlocks, blockFilter);
        double limit = blockHit == null ? maxDistance : start.toVector().distance(blockHit.getHitPosition());
        org.bukkit.util.RayTraceResult entityHit =
                rayTraceEntities(start, direction, limit, raySize, entityFilter);
        return entityHit != null ? entityHit : blockHit;
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTrace(
            java.util.function.Consumer<io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder>
                    builderConsumer) {
        Objects.requireNonNull(builderConsumer, "builderConsumer");
        var cfg = new io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder() {
            Location start;
            org.bukkit.util.Vector direction;
            double maxDistance = 120.0;
            org.bukkit.FluidCollisionMode fluidMode = org.bukkit.FluidCollisionMode.NEVER;
            boolean ignorePassable = true;
            double raySize = 0.0;
            Predicate<? super Entity> entityFilter;
            Predicate<? super Block> blockFilter;
            boolean blocksTarget = true;
            boolean entitiesTarget = true;

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder start(Location s) {
                this.start = s;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder direction(
                    org.bukkit.util.Vector d) {
                this.direction = d;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder maxDistance(double dist) {
                this.maxDistance = dist;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder fluidCollisionMode(
                    org.bukkit.FluidCollisionMode mode) {
                this.fluidMode = mode;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder blockCollisionMode(
                    io.papermc.paper.raytracing.BlockCollisionMode mode) {
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder ignorePassableBlocks(
                    boolean ignore) {
                this.ignorePassable = ignore;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder raySize(double size) {
                this.raySize = size;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder entityFilter(
                    Predicate<? super Entity> filter) {
                this.entityFilter = filter;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder blockFilter(
                    Predicate<? super Block> filter) {
                this.blockFilter = filter;
                return this;
            }

            @Override
            public io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder targets(
                    io.papermc.paper.raytracing.RayTraceTarget first,
                    io.papermc.paper.raytracing.RayTraceTarget... others) {
                this.blocksTarget = first == io.papermc.paper.raytracing.RayTraceTarget.BLOCK;
                this.entitiesTarget = first == io.papermc.paper.raytracing.RayTraceTarget.ENTITY;
                if (others != null) {
                    for (io.papermc.paper.raytracing.RayTraceTarget target : others) {
                        if (target == io.papermc.paper.raytracing.RayTraceTarget.BLOCK) {
                            this.blocksTarget = true;
                        } else if (target == io.papermc.paper.raytracing.RayTraceTarget.ENTITY) {
                            this.entitiesTarget = true;
                        }
                    }
                }
                return this;
            }
        };
        builderConsumer.accept(cfg);
        Objects.requireNonNull(cfg.start, "start");
        Objects.requireNonNull(cfg.direction, "direction");
        if (cfg.blocksTarget && cfg.entitiesTarget) {
            return rayTrace(
                    cfg.start,
                    cfg.direction,
                    cfg.maxDistance,
                    cfg.fluidMode,
                    cfg.ignorePassable,
                    cfg.raySize,
                    cfg.entityFilter,
                    cfg.blockFilter);
        }
        if (cfg.blocksTarget) {
            return rayTraceBlocks(
                    cfg.start, cfg.direction, cfg.maxDistance, cfg.fluidMode, cfg.ignorePassable, cfg.blockFilter);
        }
        return rayTraceEntities(cfg.start, cfg.direction, cfg.maxDistance, cfg.raySize, cfg.entityFilter);
    }

    @Override
    public boolean hasCollisionsIn(BoundingBox box) {
        Objects.requireNonNull(box, "box");
        int minX = (int) Math.floor(box.getMinX());
        int maxX = (int) Math.floor(box.getMaxX());
        int minY = (int) Math.floor(box.getMinY());
        int maxY = (int) Math.floor(box.getMaxY());
        int minZ = (int) Math.floor(box.getMinZ());
        int maxZ = (int) Math.floor(box.getMaxZ());
        for (int bx = minX; bx <= maxX; bx++) {
            for (int by = minY; by <= maxY; by++) {
                for (int bz = minZ; bz <= maxZ; bz++) {
                    if (!getBlockAt(bx, by, bz).isPassable()) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    @Override
    public boolean lineOfSightExists(Location from, Location to) {
        Objects.requireNonNull(from, "from");
        Objects.requireNonNull(to, "to");
        if ((from.getWorld() != null && !equals(from.getWorld()))
                || (to.getWorld() != null && !equals(to.getWorld()))) {
            return false;
        }
        org.bukkit.util.Vector delta = to.toVector().subtract(from.toVector());
        double dist = delta.length();
        if (dist <= 1e-6) {
            return true;
        }
        return rayTraceBlocks(from, delta.normalize(), dist, org.bukkit.FluidCollisionMode.NEVER, true) == null;
    }

    @Override
    public void sendGameEvent(
            Entity sourceEntity, org.bukkit.GameEvent gameEvent, org.bukkit.util.Vector position) {
        Objects.requireNonNull(gameEvent, "gameEvent");
        Objects.requireNonNull(position, "position");
    }

    @Override
    @SuppressWarnings("deprecation")
    public void playEffect(Location location, org.bukkit.Effect effect, int data, int radius) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(effect, "effect");
    }

    @Override
    @SuppressWarnings("deprecation")
    public <T> void playEffect(Location location, org.bukkit.Effect effect, T data, int radius) {
        Objects.requireNonNull(location, "location");
        Objects.requireNonNull(effect, "effect");
    }

    @Override
    public Collection<org.bukkit.generator.structure.GeneratedStructure> getStructures(int x, int z) {
        return List.of();
    }

    @Override
    public Collection<org.bukkit.generator.structure.GeneratedStructure> getStructures(
            int x, int z, org.bukkit.generator.structure.Structure structure) {
        return List.of();
    }

    @Override
    public void sendPluginMessage(org.bukkit.plugin.Plugin source, String channel, byte[] message) {
        runtime.messenger().sendPluginMessage(source, null, channel, message);
    }

    @Override
    public Set<String> getListeningPluginChannels() {
        return runtime.messenger().getIncomingChannels();
    }

    @Override
    public void setMetadata(String metadataKey, org.bukkit.metadata.MetadataValue newMetadataValue) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(newMetadataValue, "newMetadataValue");
        worldMetadata.compute(metadataKey, (k, existing) -> {
            List<org.bukkit.metadata.MetadataValue> updated = existing == null
                    ? new ArrayList<>()
                    : new ArrayList<>(existing);
            updated.removeIf(v -> v.getOwningPlugin() == newMetadataValue.getOwningPlugin());
            updated.add(newMetadataValue);
            return updated;
        });
    }

    @Override
    public List<org.bukkit.metadata.MetadataValue> getMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        List<org.bukkit.metadata.MetadataValue> values = worldMetadata.get(metadataKey);
        return values == null ? List.of() : List.copyOf(values);
    }

    @Override
    public boolean hasMetadata(String metadataKey) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        List<org.bukkit.metadata.MetadataValue> values = worldMetadata.get(metadataKey);
        return values != null && !values.isEmpty();
    }

    @Override
    public void removeMetadata(String metadataKey, org.bukkit.plugin.Plugin owningPlugin) {
        Objects.requireNonNull(metadataKey, "metadataKey");
        Objects.requireNonNull(owningPlugin, "owningPlugin");
        worldMetadata.computeIfPresent(metadataKey, (k, existing) -> {
            List<org.bukkit.metadata.MetadataValue> updated = new ArrayList<>(existing);
            updated.removeIf(v -> v.getOwningPlugin() == owningPlugin);
            return updated.isEmpty() ? null : updated;
        });
    }

    @Override
    public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() {
        return persistentData;
    }

    private final World.Spigot worldSpigot = new World.Spigot() {
        @Override
        public org.bukkit.entity.LightningStrike strikeLightning(Location loc, boolean isSilent) {
            return FerriteWorld.this.strikeLightning(loc);
        }

        @Override
        public org.bukkit.entity.LightningStrike strikeLightningEffect(Location loc, boolean isSilent) {
            return FerriteWorld.this.strikeLightningEffect(loc);
        }
    };

    @Override
    public World.Spigot spigot() {
        return worldSpigot;
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
