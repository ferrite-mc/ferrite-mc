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
    private final Map<String, Object> gameRules = new HashMap<>();
    private final Set<Long> loadedChunks = ConcurrentHashMap.newKeySet();
    private final Set<Long> generatedChunks = ConcurrentHashMap.newKeySet();
    private final Map<UUID, Entity> worldEntities = new LinkedHashMap<>();
    private volatile Set<String> negotiatedV3Ops;

    private long time;
    private boolean storm;
    private boolean thundering;
    private int weatherDuration = 6000;
    private int thunderDuration = 6000;

    public FerriteWorld(FerriteRuntime runtime, WorldSpec spec) {
        this.runtime = runtime;
        this.spec = spec;
        this.blocks = new InMemoryBlockStore(spec.defaultBlock());
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
    }

    @Override
    public boolean hasStorm() {
        return storm;
    }

    @Override
    public void setStorm(boolean storm) {
        this.storm = storm;
    }

    @Override
    public boolean isThundering() {
        return thundering;
    }

    @Override
    public void setThundering(boolean thundering) {
        this.thundering = thundering;
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
