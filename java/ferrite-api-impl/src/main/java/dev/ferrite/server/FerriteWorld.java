package dev.ferrite.server;

/** World identity and explicit block snapshots; unknown coordinates fail rather than inventing air. */
final class FerriteWorld implements java.lang.reflect.InvocationHandler {
    private record Pos(int x, int y, int z) {}
    private record Column(int x, int z) {}
    private final FerriteServer server;
    private final java.util.UUID id;
    private final String name;
    private final long time;
    /** The world seed as the host reported it when the world was opened; the host's live answer wins. */
    private volatile long seed;
    /** The spawn as registered, then as {@code setSpawnLocation} last moved it; the host has no spawn getter. */
    private volatile Pos spawn;
    private volatile float spawnAngle;
    private final java.util.Map<Pos, org.bukkit.Material> blocks = new java.util.concurrent.ConcurrentHashMap<>();
    /** Full state strings for blocks a plugin placed with {@code setBlockData}; {@link #blocks} holds their material. */
    private final java.util.Map<Pos, String> states = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<Column, Integer> heights = new java.util.concurrent.ConcurrentHashMap<>();
    private org.bukkit.World proxy;
    private final WorldEntities entities = new WorldEntities();
    // Remembered for the plugin; no server rule reads these (players cannot hurt each other yet, hardcore needs a respawn rule).
    private volatile boolean hardcore, pvp = true, autoSave = true;
    /** The dimension this handle was opened in; the name suffix is only the bootstrap default. */
    private volatile org.bukkit.World.Environment environment;
    private FerriteWorld(FerriteServer server, java.util.UUID id, String name, long time, org.bukkit.World.Environment environment, Pos spawn) { this.server = server; this.id = id; this.name = name; this.time = time; this.environment = java.util.Objects.requireNonNull(environment, "environment"); this.spawn = spawn; }
    static org.bukkit.World create(FerriteServer server, java.util.UUID id, String name, long time, int x, int y, int z) {
        return create(server, id, name, time, environmentOf(name), x, y, z);
    }
    static org.bukkit.World create(FerriteServer server, java.util.UUID id, String name, long time, org.bukkit.World.Environment environment, int x, int y, int z) {
        var world = new FerriteWorld(server, id, name, time, environment, new Pos(x, y, z));
        world.proxy = (org.bukkit.World) java.lang.reflect.Proxy.newProxyInstance(FerriteWorld.class.getClassLoader(), new Class<?>[]{org.bukkit.World.class}, world);
        return world.proxy;
    }
    /** The environment a world opened without an explicit one stands for: the Nether and End are suffixed. */
    static org.bukkit.World.Environment environmentOf(String name) {
        if (name.endsWith("_nether")) return org.bukkit.World.Environment.NETHER;
        if (name.endsWith("_the_end")) return org.bukkit.World.Environment.THE_END;
        return org.bukkit.World.Environment.NORMAL;
    }
    /** The `minecraft:*` dimension key a host reply or `level.dat` records. */
    static org.bukkit.World.Environment environmentOfKey(String key) {
        return switch (key) {
            case "minecraft:the_nether" -> org.bukkit.World.Environment.NETHER;
            case "minecraft:the_end" -> org.bukkit.World.Environment.THE_END;
            default -> org.bukkit.World.Environment.NORMAL;
        };
    }
    static void setSeed(org.bukkit.World world, long seed) { ((FerriteWorld) java.lang.reflect.Proxy.getInvocationHandler(java.util.Objects.requireNonNull(world))).seed = seed; }
    static void setEnvironment(org.bukkit.World world, org.bukkit.World.Environment environment) {
        ((FerriteWorld) java.lang.reflect.Proxy.getInvocationHandler(java.util.Objects.requireNonNull(world))).environment = java.util.Objects.requireNonNull(environment, "environment");
    }
    static void setHeight(org.bukkit.World world, int x, int z, int y) { ((FerriteWorld) java.lang.reflect.Proxy.getInvocationHandler(java.util.Objects.requireNonNull(world))).heights.put(new Column(x, z), y); }
    static void setSnapshot(org.bukkit.World world, int x, int y, int z, String material) {
        var type = org.bukkit.Material.matchMaterial(material);
        if (type == null) throw new IllegalArgumentException("Unknown material " + material);
        setSnapshot(world, x, y, z, type);
    }
    static void setSnapshot(org.bukkit.World world, int x, int y, int z, org.bukkit.Material type) {
        var handler = (FerriteWorld) java.lang.reflect.Proxy.getInvocationHandler(java.util.Objects.requireNonNull(world));
        handler.blocks.put(new Pos(x, y, z), java.util.Objects.requireNonNull(type, "type"));
    }
    /** A chunk proxy for hosts that only publish block snapshots, never entity lists. */
    static org.bukkit.Chunk chunkAt(org.bukkit.World world, int x, int z) {
        var handler = (FerriteWorld) java.lang.reflect.Proxy.getInvocationHandler(java.util.Objects.requireNonNull(world));
        return handler.chunk(x, z);
    }
    private org.bukkit.Location location(Pos pos) { return new org.bukkit.Location(proxy, pos.x(), pos.y(), pos.z()); }
    private org.bukkit.Material typeAt(Pos pos) {
        var material = blocks.get(pos);
        if (material != null) return material;
        var key = Native.blockType(id.toString(), pos.x(), pos.y(), pos.z());
        if (key == null) throw new UnsupportedOperationException("Ferrite: no block data at " + pos);
        if (key.indexOf('[') >= 0) key = key.substring(0, key.indexOf('['));
        var resolved = org.bukkit.Material.matchMaterial(key);
        if (resolved == null) throw new IllegalStateException("Unknown block material " + key);
        return resolved;
    }
    /** The vanilla state string at {@code pos}, with its properties when the host reports them. */
    private String stateAt(Pos pos) {
        if (blocks.containsKey(pos)) return states.getOrDefault(pos, blocks.get(pos).getKey().toString());
        var state = Native.blockType(id.toString(), pos.x(), pos.y(), pos.z());
        return state != null ? state : typeAt(pos).getKey().toString();
    }
    private void setState(Pos pos, org.bukkit.block.data.BlockData data) {
        var state = data.getAsString();
        if (!Native.worldSetBlock(id.toString(), pos.x(), pos.y(), pos.z(), state)) throw new IllegalStateException("No world command handler");
        blocks.put(pos, data.getMaterial());
        if (state.indexOf('[') >= 0) states.put(pos, state); else states.remove(pos);
        heights.remove(new Column(pos.x(), pos.z()));
    }
    private void setType(Pos pos, org.bukkit.Material material) {
        if (!Native.worldSetBlock(id.toString(), pos.x(), pos.y(), pos.z(), material.getKey().toString())) throw new IllegalStateException("No world command handler");
        blocks.put(pos, material);
        states.remove(pos);
        heights.remove(new Column(pos.x(), pos.z()));
    }
    private org.bukkit.block.Block relative(Pos pos, org.bukkit.block.BlockFace face) {
        return block(new Pos(pos.x() + face.getModX(), pos.y() + face.getModY(), pos.z() + face.getModZ()));
    }
    private org.bukkit.block.Block block(Pos pos) {
        return (org.bukkit.block.Block) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.block.Block.class}, (self, method, args) -> switch (method.getName()) {
            case "getX" -> pos.x(); case "getY" -> pos.y(); case "getZ" -> pos.z();
            case "getWorld" -> proxy;
            case "getLocation" -> location(pos);
            case "getChunk" -> chunk(pos.x() >> 4, pos.z() >> 4);
            case "getType" -> typeAt(pos);
            case "getTemperature" -> climate(true, pos.x(), pos.y(), pos.z());
            case "getHumidity" -> climate(false, pos.x(), pos.y(), pos.z());
            case "getLightLevel" -> {
                var reply = (String) HostBridge.call("World", id, "getLightLevel(int,int,int)", String.class, pos.x(), pos.y(), pos.z());
                if (reply == null) throw new UnsupportedOperationException("Ferrite: no light data at " + pos);
                yield (byte) Integer.parseInt(reply);
            }
            case "getLightFromSky" -> { int level = Native.skyLight(id.toString(), pos.x(), pos.y(), pos.z()); if (level < 0) throw new UnsupportedOperationException("Ferrite: no light data at " + pos); yield (byte) level; }
            case "setType" -> { setType(pos, java.util.Objects.requireNonNull((org.bukkit.Material) args[0])); yield null; }
            case "getBlockData" -> FerriteBlockData.create(stateAt(pos));
            case "setBlockData" -> { setState(pos, (org.bukkit.block.data.BlockData) args[0]); yield null; }
            case "isEmpty" -> typeAt(pos).isAir();
            case "isLiquid" -> { var type = typeAt(pos); yield type == org.bukkit.Material.WATER || type == org.bukkit.Material.LAVA; }
            case "getRelative" -> relative(pos, (org.bukkit.block.BlockFace) args[0]);
            case "hashCode" -> java.util.Objects.hash(id, pos);
            case "equals" -> self == args[0];
            case "toString" -> "Block[" + id + ":" + pos + "]";
            default -> HostBridge.dispatch("Block", "", method, args);
        });
    }
    private boolean chunkLoaded(int x, int z) { return (boolean) HostBridge.call("World", id, "isChunkLoaded(int,int)", Boolean.class, x, z); }
    private org.bukkit.Chunk chunk(int x, int z) {
        return (org.bukkit.Chunk) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.Chunk.class}, (self, method, args) -> switch (method.getName()) {
            case "getX" -> x; case "getZ" -> z; case "getWorld" -> proxy;
            case "isLoaded" -> chunkLoaded(x, z);
            case "getBlock" -> block(new Pos((x << 4) + (int) args[0], (int) args[1], (z << 4) + (int) args[2]));
            case "getChunkSnapshot" -> {
                var world = (org.bukkit.World) proxy;
                var key = world.getKey();
                // Bukkit's three flags control optional snapshot data; the host-backed snapshot
                // exposes biome data only, so its block and light queries are refused with that reason.
                yield FerriteChunkSnapshot.ofChunk(
                    x, z, name, id.toString(),
                    net.kyori.adventure.key.Key.key(key.getNamespace(), key.getKey()),
                    world.getMinHeight(), world.getMaxHeight(),
                    hostLong("getFullTime()", time), (boolean) args[1]
                );
            }
            case "hashCode" -> java.util.Objects.hash(id, x, z);
            case "equals" -> self == args[0];
            case "toString" -> "Chunk[" + id + ":" + x + "," + z + "]";
            default -> HostBridge.dispatch("Chunk", "", method, args);
        });
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getUID" -> id; case "getName" -> name; case "getTime", "getFullTime" -> hostLong(method.getName() + "()", time);
            case "getGameTime", "hasStorm", "isThundering", "isClearWeather", "getWeatherDuration", "getThunderDuration", "getClearWeatherDuration", "getDifficulty" -> hostValue(method);
            case "getMoonPhase" -> moonPhase(method);
            case "getWorldBorder" -> border();
            case "spigot" -> spigot();
            case "getSeed" -> hostLong("getSeed()", seed);
            case "isDayTime" -> { long t = hostLong("getTime()", time) % 24_000; yield t < 12_000 || t > 23_960; }
            case "isHardcore" -> hardcore;
            case "setHardcore" -> { hardcore = (boolean) args[0]; yield null; }
            case "getPVP" -> pvp;
            case "setPVP" -> { pvp = (boolean) args[0]; yield null; }
            case "isAutoSave" -> autoSave;
            case "setAutoSave" -> { autoSave = (boolean) args[0]; try { HostBridge.callVoid("World", id, "setAutoSave(boolean)", autoSave); } catch (UnsupportedOperationException noHost) { /* remembered only */ } yield null; }
            case "getSpawnLocation" -> { var at = spawn; yield new org.bukkit.Location(proxy, at.x(), at.y(), at.z(), spawnAngle, 0f); }
            case "getHighestBlockYAt" -> {
                if (args.length == 3 || (args.length == 2 && args[1] instanceof org.bukkit.HeightMap)) {
                    // The host reads the chunk's stored heightmap, loading the chunk first when nobody holds it.
                    var map = (org.bukkit.HeightMap) args[args.length - 1];
                    int mx, mz;
                    if (args[0] instanceof org.bukkit.Location l) { mx = l.getBlockX(); mz = l.getBlockZ(); } else { mx = (int) args[0]; mz = (int) args[1]; }
                    var reply = (String) HostBridge.call("World", id, "getHighestBlockYAt(int,int,HeightMap)", String.class, mx, mz, map);
                    if (reply == null) throw new UnsupportedOperationException("Ferrite: no " + map + " height data at " + mx + "," + mz);
                    yield Integer.parseInt(reply);
                }
                if (args.length > 2) throw new UnsupportedOperationException("Ferrite: unsupported getHighestBlockYAt overload"); int x, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); z = l.getBlockZ(); } else { x = (int) args[0]; z = (int) args[1]; } var height = heights.get(new Column(x, z)); if (height == null) { int value = Native.highestBlockY(id.toString(), x, z); if (value == Integer.MIN_VALUE) throw new UnsupportedOperationException("Ferrite: no height data at " + x + "," + z); height = value; heights.put(new Column(x, z), height); } yield height; }
            case "getBiome", "getComputedBiome" -> { int x, y, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); y = l.getBlockY(); z = l.getBlockZ(); } else { x = (int) args[0]; y = (int) args[1]; z = (int) args[2]; } var key = Native.biomeAt(id.toString(), x, y, z); if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at " + x + "," + y + "," + z); var biome = org.bukkit.Registry.BIOME.get(org.bukkit.NamespacedKey.fromString(key)); if (biome == null) throw new IllegalStateException("Unknown biome " + key); yield biome; }
            case "getFluidData" -> {
                // Paper's default is the fluid state of the block there; Ferrite derives it from the vanilla state string.
                if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield FerriteFluidData.of(stateAt(new Pos(l.getBlockX(), l.getBlockY(), l.getBlockZ())));
                yield FerriteFluidData.of(stateAt(new Pos((int) args[0], (int) args[1], (int) args[2])));
            }
            case "getBlockAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield block(new Pos(l.getBlockX(), l.getBlockY(), l.getBlockZ())); yield block(new Pos((int) args[0], (int) args[1], (int) args[2])); }
            case "getEmptyChunkSnapshot" -> {
                var world = (org.bukkit.World) proxy;
                var key = world.getKey();
                yield new FerriteChunkSnapshot((int) args[0], (int) args[1], name, id.toString(), net.kyori.adventure.key.Key.key(key.getNamespace(), key.getKey()), world.getMinHeight(), world.getMaxHeight(), hostLong("getFullTime()", time), (boolean) args[3]);
            }
            case "getChunkAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield chunk(l.getBlockX() >> 4, l.getBlockZ() >> 4); yield chunk((int) args[0], (int) args[1]); }
            case "getPlayers" -> playersHere();
            case "getEntities" -> entitiesHere();
            case "getLivingEntities" -> livingHere();
            case "hashCode" -> id.hashCode();
            case "equals" -> self == args[0];
            case "toString" -> "World[" + id + "]";
            default -> {
                Object answer = dimension(method.getName(), args);
                if (answer == NONE) answer = more(method.getName(), args == null ? new Object[0] : args);
                yield answer != NONE ? answer : HostBridge.dispatch("World", id, method, args);
            }
        };
    }

    private final java.util.Set<java.util.UUID> created = java.util.concurrent.ConcurrentHashMap.newKeySet();
    /** The one entity type whose API class is {@code api}; a class several types share (Monster, Mob, ...) cannot be spawned. */
    static org.bukkit.entity.EntityType entityTypeOf(Class<?> api) {
        java.util.Objects.requireNonNull(api, "class");
        org.bukkit.entity.EntityType found = null;
        for (var type : org.bukkit.entity.EntityType.values()) {
            if (type == org.bukkit.entity.EntityType.UNKNOWN || type == org.bukkit.entity.EntityType.PLAYER || type.getEntityClass() != api) continue;
            if (found != null) throw new IllegalArgumentException("Ferrite: " + api.getName() + " is the class of both " + found + " and " + type);
            found = type;
        }
        if (found == null) throw new IllegalArgumentException("Ferrite: cannot spawn " + api.getName() + ": no entity type has exactly that class");
        return found;
    }
    private static final Object NONE = new Object();
    private final java.util.Map<String, java.util.Map<org.bukkit.plugin.Plugin, org.bukkit.metadata.MetadataValue>> metadata = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<org.bukkit.entity.SpawnCategory, Integer> spawnLimits = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<org.bukkit.entity.SpawnCategory, Integer> ticksPerSpawns = new java.util.concurrent.ConcurrentHashMap<>();
    private volatile boolean allowAnimals = true, allowMonsters = true;
    // Pushed to the host on every change; the game loop applies them below the world.
    private volatile boolean voidDamageEnabled = true;
    private volatile float voidDamageAmount = 4.0f;
    private volatile double voidDamageOffset = -64.0;
    private final java.util.Map<Long, java.util.Set<org.bukkit.plugin.Plugin>> chunkTickets = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<Long> forceLoaded = java.util.concurrent.ConcurrentHashMap.newKeySet();
    /** Chunk keys the host currently keeps loaded on this world's behalf (tickets + forceload). */
    private final java.util.Set<Long> hostHeld = java.util.concurrent.ConcurrentHashMap.newKeySet();

    /** Tells the host the final union of this chunk's plugin holds, so two plugins can share one. */
    private void refreshHold(int x, int z) {
        long key = chunkKey(x, z);
        boolean held = forceLoaded.contains(key)
            || chunkTickets.getOrDefault(key, java.util.Set.of()).size() > 0;
        boolean known = hostHeld.contains(key);
        if (held == known) return;
        if (held) hostHeld.add(key); else hostHeld.remove(key);
        try { HostBridge.callBoolean("World", id, "holdChunk(int,int,boolean)", x, z, held); }
        catch (UnsupportedOperationException noHost) { /* remembered only; the chunk list still reflects it */ }
    }
    /** Block entity types that run a server-side ticker in vanilla (a `ticker` in their block's `getTicker`). */
    static final java.util.Set<String> TICKING_BLOCK_ENTITIES = java.util.Set.of(
        "minecraft:furnace", "minecraft:blast_furnace", "minecraft:smoker", "minecraft:hopper", "minecraft:brewing_stand",
        "minecraft:beacon", "minecraft:conduit", "minecraft:campfire", "minecraft:bell", "minecraft:beehive",
        "minecraft:mob_spawner", "minecraft:trial_spawner", "minecraft:vault", "minecraft:jukebox",
        // `ShulkerBoxBlock`, `SignBlock`/`HangingSignBlock` and `CrafterBlock` return a server
        // ticker unconditionally; `DaylightDetectorBlock` only in a sky-light dimension.
        "minecraft:shulker_box", "minecraft:sign", "minecraft:hanging_sign", "minecraft:daylight_detector",
        "minecraft:sculk_sensor", "minecraft:calibrated_sculk_sensor", "minecraft:sculk_shrieker",
        "minecraft:sculk_catalyst", "minecraft:end_gateway", "minecraft:piston", "minecraft:crafter",
        "minecraft:creaking_heart", "minecraft:potent_sulfur");

    private record PoiHit(io.papermc.paper.entity.poi.PoiType poiType, org.bukkit.Location location)
        implements io.papermc.paper.entity.poi.PoiSearchResult {}

    /**
     * Points of interest around a location, nearest first. Ferrite has no `PoiManager` storage to query, so this scans
     * the loaded chunks within the radius. The scan matches the full block state the host reports (a bed's head half is
     * a POI, its foot half is not), not just the block name. Occupancy is the live state: nothing in Ferrite claims POI
     * tickets yet, so `HAS_SPACE` is every type that accepts occupants and `IS_OCCUPIED` is empty.
     */
    @SuppressWarnings("unchecked")
    private java.util.List<PoiHit> poiSearch(Object[] args, boolean nearestOnly) {
        var origin = (org.bukkit.Location) args[0];
        int radius = (int) args[2];
        if (radius < 0) throw new IllegalArgumentException("radius must be >= 0");
        var occupancy = args.length > 3 ? (io.papermc.paper.entity.poi.PoiType.Occupancy) args[3] : io.papermc.paper.entity.poi.PoiType.Occupancy.ANY;
        if (occupancy == io.papermc.paper.entity.poi.PoiType.Occupancy.IS_OCCUPIED) return java.util.List.of();
        java.util.List<FerritePoiType> types = new java.util.ArrayList<>();
        if (args[1] instanceof FerritePoiType one) types.add(one);
        else if (args[1] instanceof java.util.function.Predicate<?> filter) {
            for (var key : FerriteRegistryAccess.poiTypeKeys()) {
                var type = new FerritePoiType(org.bukkit.NamespacedKey.fromString(key));
                if (((java.util.function.Predicate<io.papermc.paper.entity.poi.PoiType>) filter).test(type)) types.add(type);
            }
        } else throw new IllegalArgumentException("Unsupported PoiType " + args[1]);
        if (occupancy == io.papermc.paper.entity.poi.PoiType.Occupancy.HAS_SPACE) types.removeIf(type -> !type.hasOccupants());
        var names = new java.util.HashSet<String>();
        for (var type : types) names.addAll(type.blockNames());
        var hits = new java.util.ArrayList<PoiHit>();
        if (names.isEmpty() || radius > 256) {
            if (radius > 256) throw new IllegalArgumentException("radius must be <= 256");
            return hits;
        }
        int ox = origin.getBlockX(), oy = origin.getBlockY(), oz = origin.getBlockZ();
        var loaded = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
        if (loaded == null || loaded.isEmpty()) return hits;
        int low = Math.max(minHeight(), oy - radius), high = Math.min(minHeight() + height() - 1, oy + radius);
        long limit = (long) radius * radius;
        for (var pair : loaded.split(String.valueOf(HostBridge.ITEM))) {
            var xz = pair.split(",");
            int cx = Integer.parseInt(xz[0]) << 4, cz = Integer.parseInt(xz[1]) << 4;
            if (cx + 15 < ox - radius || cx > ox + radius || cz + 15 < oz - radius || cz > oz + radius) continue;
            for (int x = Math.max(cx, ox - radius); x <= Math.min(cx + 15, ox + radius); x++)
                for (int z = Math.max(cz, oz - radius); z <= Math.min(cz + 15, oz + radius); z++)
                    for (int y = low; y <= high; y++) {
                        long dx = x - ox, dy = y - oy, dz = z - oz;
                        if (dx * dx + dy * dy + dz * dz > limit) continue;
                        String state;
                        try { state = stateAt(new Pos(x, y, z)); } catch (UnsupportedOperationException unknown) { continue; }
                        int bracket = state.indexOf('[');
                        if (!names.contains(bracket < 0 ? state : state.substring(0, bracket))) continue;
                        var data = FerriteBlockData.create(state);
                        for (var type : types) {
                            if (type.is(data)) { hits.add(new PoiHit(type, new org.bukkit.Location((org.bukkit.World) proxy, x, y, z))); break; }
                        }
                    }
        }
        hits.sort(java.util.Comparator.comparingDouble((PoiHit hit) -> hit.location().distanceSquared(origin))
            .thenComparingInt(hit -> hit.location().getBlockX())
            .thenComparingInt(hit -> hit.location().getBlockY())
            .thenComparingInt(hit -> hit.location().getBlockZ()));
        return nearestOnly && hits.size() > 1 ? new java.util.ArrayList<>(hits.subList(0, 1)) : hits;
    }

    /** Counts the block entities of the loaded chunks, one host query per chunk. */
    private int tileEntities(boolean tickingOnly) {
        var text = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
        if (text == null || text.isEmpty()) return 0;
        int total = 0;
        for (var pair : text.split(String.valueOf(HostBridge.ITEM))) {
            var xz = pair.split(",");
            int x = Integer.parseInt(xz[0]) << 4, z = Integer.parseInt(xz[1]) << 4;
            @SuppressWarnings("unchecked") var reply = (java.util.List<String>) HostBridge.call("World", id,
                "ferriteBlockEntities(int,int,int,int,int,int)", java.util.List.class, x, minHeight(), z, x + 15, minHeight() + height() - 1, z + 15);
            if (reply == null) throw new UnsupportedOperationException("Ferrite: block entity data is unavailable for world " + name);
            for (var item : reply) {
                if (item.isEmpty()) continue;
                if (!tickingOnly) { total++; continue; }
                var parts = item.split(",", 4);
                if (parts.length != 4) continue;
                var nbt = StructureNbt.compound(parts[3]);
                if (!(nbt != null && nbt.get("id") instanceof String type && TICKING_BLOCK_ENTITIES.contains(type))) continue;
                // Vanilla's ticker is conditional for these blocks (their `getTicker` returns
                // null for the other states), so count only the states that really tick.
                if (type.equals("minecraft:jukebox")) {
                    if (!stateAt(new Pos(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]))).contains("has_record=true")) continue;
                } else if (type.equals("minecraft:creaking_heart")) {
                    if (stateAt(new Pos(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]))).contains("state=uprooted")) continue;
                } else if (type.equals("minecraft:daylight_detector") && environment() != org.bukkit.World.Environment.NORMAL) {
                    // Its ticker only exists in dimensions with sky light; neither the nether nor
                    // the end does.
                    continue;
                }
                total++;
            }
        }
        return total;
    }
    private FerriteDragonBattle dragonBattle;
    private synchronized FerriteDragonBattle dragonBattle() {
        if (dragonBattle == null) dragonBattle = new FerriteDragonBattle(proxy);
        return dragonBattle;
    }
    private void pushVoidDamage() {
        HostBridge.call("World", id, "setVoidDamage(boolean,float,double)", String.class,
            voidDamageEnabled, voidDamageAmount, voidDamageOffset);
    }
    private static long chunkKey(int x, int z) { return ((long) x & 0xffffffffL) | (((long) z & 0xffffffffL) << 32); }
    private static int keyX(long key) { return (int) key; }
    private static int keyZ(long key) { return (int) (key >> 32); }

    private volatile org.bukkit.WorldBorder worldBorder;
    private org.bukkit.WorldBorder border() {
        var b = worldBorder;
        if (b == null) worldBorder = b = new FerriteHostBorder(proxy, id);
        return b;
    }
    private volatile org.bukkit.World.Spigot spigot;
    private org.bukkit.World.Spigot spigot() {
        var facade = spigot;
        if (facade == null) spigot = facade = new FerriteWorldSpigot(proxy);
        return facade;
    }
    private long hostLong(String method, long fallback) {
        try { return Long.parseLong((String) HostBridge.call("World", id, method, String.class)); }
        catch (UnsupportedOperationException offline) { return fallback; }
    }
    /**
     * A world query the host answers from the live state, converted to the method's return type. With no host and
     * no value remembered it fails with the "not implemented" error, since a made-up weather or difficulty would be
     * wrong (and {@code null} would be a {@link NullPointerException} for the boolean and numeric queries).
     */
    private Object hostValue(java.lang.reflect.Method method) {
        return HostBridge.dispatch("World", id, method, new Object[0]);
    }
    private Object moonPhase(java.lang.reflect.Method method) {
        // The host reports it; without one it is the day count of the time snapshot mod 8, as in vanilla.
        long reported = hostLong("getMoonPhase()", -1);
        int phase = (int) (reported >= 0 ? reported % 8 : Math.floorMod(hostLong("getFullTime()", time) / 24_000L, 8L));
        Class<?> type = method.getReturnType();
        return type.isEnum() ? type.getEnumConstants()[phase] : (Object) Integer.valueOf(phase);
    }

    private Object ruleValue(org.bukkit.GameRule<?> rule, String method) {
        var text = (String) HostBridge.call("World", id, method, String.class, rule.getName());
        if (text == null) return null;
        return rule.getType() == Boolean.class ? (Object) Boolean.valueOf(text) : (Object) Integer.valueOf(text);
    }
    private boolean setRule(String name, String value) {
        try { return HostBridge.callBoolean("World", id, "setGameRuleValue(String,String)", name, value); }
        catch (UnsupportedOperationException invalidOrOffline) { return false; }
    }

    private java.util.List<org.bukkit.entity.Player> playersHere() {
        return server.getOnlinePlayers().stream().filter(p -> { try { return p.getWorld() == proxy; } catch (UnsupportedOperationException | IllegalStateException | IllegalArgumentException e) { return false; } }).map(p -> (org.bukkit.entity.Player) p).toList();
    }

    private java.util.List<org.bukkit.entity.Entity> hostEntities() {
        String reply;
        try { reply = (String) HostBridge.call("World", id, "getEntities()", String.class); }
        catch (UnsupportedOperationException | IllegalStateException noHost) { return java.util.List.of(); }
        return WorldEntities.decode(reply, proxy, entities);
    }

    private java.util.List<org.bukkit.entity.Entity> entitiesHere() {
        var out = new java.util.LinkedHashMap<java.util.UUID, org.bukkit.entity.Entity>();
        for (var player : playersHere()) out.put(player.getUniqueId(), player);
        for (var entity : hostEntities()) out.putIfAbsent(entity.getUniqueId(), entity);
        return java.util.List.copyOf(out.values());
    }

    private java.util.List<org.bukkit.entity.LivingEntity> livingHere() {
        var out = new java.util.ArrayList<org.bukkit.entity.LivingEntity>();
        for (var entity : entitiesHere()) if (entity instanceof org.bukkit.entity.LivingEntity living) out.add(living);
        return out;
    }

    private static boolean inBox(org.bukkit.Location at, double cx, double cy, double cz, double dx, double dy, double dz) {
        return Math.abs(at.getX() - cx) <= dx && Math.abs(at.getY() - cy) <= dy && Math.abs(at.getZ() - cz) <= dz;
    }

    @SuppressWarnings("unchecked")
    private static java.util.function.Predicate<org.bukkit.entity.Entity> filterOf(Object[] args) {
        return args.length > 0 && args[args.length - 1] instanceof java.util.function.Predicate<?> p ? (java.util.function.Predicate<org.bukkit.entity.Entity>) p : null;
    }

    /**
     * Walks the ray cell by cell and returns the first block it enters. Solid blocks count as whole
     * cubes (partial shapes such as slabs are not modelled); fluids are hit unless the mode is NEVER.
     */
    private org.bukkit.util.RayTraceResult traceBlocks(org.bukkit.Location start, org.bukkit.util.Vector direction, double max, org.bukkit.FluidCollisionMode mode, boolean ignorePassable) {
        java.util.Objects.requireNonNull(start, "start");
        java.util.Objects.requireNonNull(direction, "direction");
        if (direction.lengthSquared() == 0) throw new IllegalArgumentException("Direction's magnitude (0.0) must be greater than 0");
        if (max < 0) return null;
        var dir = direction.clone().normalize();
        double ox = start.getX(), oy = start.getY(), oz = start.getZ();
        int x = (int) Math.floor(ox), y = (int) Math.floor(oy), z = (int) Math.floor(oz);
        int sx = dir.getX() > 0 ? 1 : -1, sy = dir.getY() > 0 ? 1 : -1, sz = dir.getZ() > 0 ? 1 : -1;
        double tx = dir.getX() == 0 ? Double.MAX_VALUE : ((sx > 0 ? x + 1 : x) - ox) / dir.getX();
        double ty = dir.getY() == 0 ? Double.MAX_VALUE : ((sy > 0 ? y + 1 : y) - oy) / dir.getY();
        double tz = dir.getZ() == 0 ? Double.MAX_VALUE : ((sz > 0 ? z + 1 : z) - oz) / dir.getZ();
        double dx = dir.getX() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getX());
        double dy = dir.getY() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getY());
        double dz = dir.getZ() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getZ());
        org.bukkit.block.BlockFace face = null;
        double t = 0;
        while (true) {
            var type = typeAt(new Pos(x, y, z));
            boolean fluid = type == org.bukkit.Material.WATER || type == org.bukkit.Material.LAVA;
            boolean hit = !type.isAir() && (fluid ? mode != org.bukkit.FluidCollisionMode.NEVER : !(ignorePassable && !type.isSolid()));
            if (hit) return new org.bukkit.util.RayTraceResult(new org.bukkit.util.Vector(ox + dir.getX() * t, oy + dir.getY() * t, oz + dir.getZ() * t), proxy.getBlockAt(x, y, z), face);
            if (tx <= ty && tx <= tz) { t = tx; tx += dx; x += sx; face = sx > 0 ? org.bukkit.block.BlockFace.WEST : org.bukkit.block.BlockFace.EAST; }
            else if (ty <= tz) { t = ty; ty += dy; y += sy; face = sy > 0 ? org.bukkit.block.BlockFace.DOWN : org.bukkit.block.BlockFace.UP; }
            else { t = tz; tz += dz; z += sz; face = sz > 0 ? org.bukkit.block.BlockFace.NORTH : org.bukkit.block.BlockFace.SOUTH; }
            if (t > max) return null;
        }
    }

    /** The nearest player the ray hits (the only entities this world exposes). */
    private org.bukkit.util.RayTraceResult traceEntities(org.bukkit.Location start, org.bukkit.util.Vector direction, double max, double raySize, java.util.function.Predicate<org.bukkit.entity.Entity> filter, Object unused) {
        if (direction.lengthSquared() == 0) throw new IllegalArgumentException("Direction's magnitude (0.0) must be greater than 0");
        if (max < 0) return null;
        var origin = start.toVector();
        var dir = direction.clone().normalize();
        org.bukkit.util.RayTraceResult best = null;
        double bestDistance = max;
        for (var player : playersHere()) {
            if (filter != null && !filter.test(player)) continue;
            var result = player.getBoundingBox().expand(Math.max(0, raySize)).rayTrace(origin, dir, max);
            if (result == null) continue;
            double distance = result.getHitPosition().distance(origin);
            if (distance <= bestDistance) {
                bestDistance = distance;
                best = new org.bukkit.util.RayTraceResult(result.getHitPosition(), player, result.getHitBlockFace());
            }
        }
        return best;
    }

    private boolean solid(int x, int y, int z) {
        var type = typeAt(new Pos(x, y, z));
        return !type.isAir() && type != org.bukkit.Material.WATER && type != org.bukkit.Material.LAVA && type.isSolid();
    }

    /** World queries answered from Java-side state and the player list. */
    /**
     * `ServerLevel.findLightningRod`: the block above the nearest lightning rod whose column top is
     * the rod itself, within 128 blocks and only in chunks the server has loaded. The host walks its
     * own chunk data, so this works for the overworld, nether, end and runtime worlds alike.
     */
    private org.bukkit.Location findRod(org.bukkit.Location from) {
        return hostPosition("findLightningRod(int,int,int)", from);
    }
    /** `ServerLevel.findLightningTargetAround`: the rod strike point, else the column's MOTION_BLOCKING top. */
    private org.bukkit.Location findTarget(org.bukkit.Location from) {
        return hostPosition("findLightningTarget(int,int,int)", from);
    }
    private org.bukkit.Location hostPosition(String method, org.bukkit.Location from) {
        var reply = (String) HostBridge.call("World", id, method, String.class, from.getBlockX(), from.getBlockY(), from.getBlockZ());
        if (reply == null) throw new UnsupportedOperationException("Ferrite: " + method.substring(0, method.indexOf('(')) + " needs the world host");
        if (reply.isEmpty()) return null;
        var parts = reply.split(" ");
        return new org.bukkit.Location((org.bukkit.World) proxy, Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]));
    }
    /** Biome temperature (cooler above y=81, noise ignored) or humidity at a position. */
    private double climate(boolean temperature, int x, int y, int z) {
        var key = Native.biomeAt(id.toString(), x, y, z);
        if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at " + x + "," + y + "," + z);
        var text = (String) HostBridge.call("World", id, "biomeClimate(String)", String.class, key);
        var parts = text.split(String.valueOf(HostBridge.ITEM));
        double value = Double.parseDouble(parts[temperature ? 0 : 1]);
        if (temperature && y > 81) value -= (y - 81) * 0.05 / 40;
        return value;
    }
    private Object more(String method, Object[] args) {
        switch (method) {
            case "getTemperature", "getHumidity" -> {
                int x, y, z;
                if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); y = l.getBlockY(); z = l.getBlockZ(); } else { x = (int) args[0]; y = args.length > 3 ? (int) args[1] : 64; z = (int) args[args.length - 1]; }
                return climate(method.equals("getTemperature"), x, y, z);
            }
            case "setSpawnLocation" -> {
                // (int x, int y, int z[, float angle]) or (Location): the host moves the world spawn, then it is read back here.
                int x, y, z;
                float angle = 0f;
                if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); y = l.getBlockY(); z = l.getBlockZ(); angle = l.getYaw(); }
                else { x = (int) args[0]; y = (int) args[1]; z = (int) args[2]; if (args.length > 3) angle = (float) args[3]; }
                boolean moved = HostBridge.callBoolean("World", id, "setSpawnLocation(int,int,int,float)", x, y, z, angle);
                if (moved) { spawnAngle = angle; spawn = new Pos(x, y, z); }
                return moved;
            }
            case "isGameRule" -> { return (boolean) HostBridge.call("World", id, "isGameRule(String)", Boolean.class, (String) args[0]); }
            case "getGameRules" -> {
                var names = (java.util.List<?>) HostBridge.call("World", id, "getGameRules()", java.util.List.class);
                return names.toArray(new String[0]);
            }
            case "getGameRuleValue" -> {
                if (args[0] instanceof org.bukkit.GameRule<?> rule) return ruleValue(rule, "getGameRuleValue(String)");
                return HostBridge.call("World", id, "getGameRuleValue(String)", String.class, (String) args[0]);
            }
            case "getGameRuleDefault" -> { return ruleValue((org.bukkit.GameRule<?>) args[0], "getGameRuleDefault(String)"); }
            case "setGameRule" -> { return setRule(((org.bukkit.GameRule<?>) args[0]).getName(), String.valueOf(args[1])); }
            case "setGameRuleValue" -> { return setRule((String) args[0], (String) args[1]); }
            case "getMetadata" -> {
                var values = metadata.get((String) args[0]);
                return values == null ? java.util.List.<org.bukkit.metadata.MetadataValue>of() : java.util.List.copyOf(values.values());
            }
            case "hasMetadata" -> { return metadata.containsKey((String) args[0]); }
            case "setMetadata" -> {
                var plugin = ((org.bukkit.metadata.MetadataValue) args[1]).getOwningPlugin();
                if (plugin == null) throw new IllegalArgumentException("Plugin cannot be null");
                metadata.computeIfAbsent((String) args[0], k -> new java.util.concurrent.ConcurrentHashMap<>()).put(plugin, (org.bukkit.metadata.MetadataValue) args[1]);
                return null;
            }
            case "removeMetadata" -> {
                var values = metadata.get((String) args[0]);
                if (values != null) { values.remove((org.bukkit.plugin.Plugin) args[1]); if (values.isEmpty()) metadata.remove((String) args[0]); }
                return null;
            }
            case "playEffect" -> {
                // (Location, Effect, data[, radius]): shown to every player of this world within the radius.
                var at = (org.bukkit.Location) args[0];
                var effect = (org.bukkit.Effect) args[1];
                int radius = args.length > 3 ? (int) args[3] : 64;
                for (var viewer : server.getOnlinePlayers()) {
                    org.bukkit.Location there;
                    try { there = viewer.getLocation(); } catch (UnsupportedOperationException | IllegalStateException unknown) { continue; }
                    if (there.getWorld() != proxy || there.distanceSquared(at) > (double) radius * radius) continue;
                    viewer.playEffect(at, effect, args[2]);
                }
                return null;
            }
            case "isChunkInUse" -> { return HostBridge.callBoolean("World", id, "isChunkInUse(int,int)", (int) args[0], (int) args[1]); }
            case "getChunkAtAsync" -> {
                // The chunk handle is a view that needs no loading step, so the callback runs at once.
                var done = new java.util.concurrent.CompletableFuture<org.bukkit.Chunk>();
                int cx, cz;
                if (args[0] instanceof org.bukkit.Location l) { cx = l.getBlockX() >> 4; cz = l.getBlockZ() >> 4; }
                else if (args[0] instanceof org.bukkit.block.Block b) { cx = b.getX() >> 4; cz = b.getZ() >> 4; }
                else { cx = (int) args[0]; cz = (int) args[1]; }
                var found = chunk(cx, cz);
                Object last = args[args.length - 1];
                if (last instanceof java.util.function.Consumer<?> consumer) { @SuppressWarnings("unchecked") var typed = (java.util.function.Consumer<org.bukkit.Chunk>) consumer; typed.accept(found); return null; }
                if (last instanceof org.bukkit.World.ChunkLoadCallback callback) { callback.onLoad(found); return null; }
                done.complete(found);
                return done;
            }
            case "getChunksAtAsync" -> {
                // Same contract as getChunkAtAsync: chunk handles are views, so every chunk of the rectangle
                // (chunk coordinates, inclusive) is available at once and the callback runs right away.
                int minX = (int) args[0], minZ = (int) args[1], maxX = (int) args[2], maxZ = (int) args[3];
                if (minX > maxX || minZ > maxZ) throw new IllegalArgumentException("minimum chunk coordinate is greater than the maximum");
                for (int cx = minX; cx <= maxX; cx++) for (int cz = minZ; cz <= maxZ; cz++) chunk(cx, cz);
                ((Runnable) java.util.Objects.requireNonNull(args[5], "post")).run();
                return null;
            }
            case "isChunkLoaded" -> {
                int cx = args[0] instanceof org.bukkit.Chunk c ? c.getX() : (int) args[0];
                int cz = args[0] instanceof org.bukkit.Chunk c ? c.getZ() : (int) args[1];
                return chunkLoaded(cx, cz);
            }
            case "isChunkGenerated" -> {
                int cx = args[0] instanceof org.bukkit.Chunk c ? c.getX() : (int) args[0];
                int cz = args[0] instanceof org.bukkit.Chunk c ? c.getZ() : (int) args[1];
                return (boolean) HostBridge.call("World", id, "isChunkGenerated(int,int)", Boolean.class, cx, cz);
            }
            case "getChunkGenerationState", "getChunkGenerationStateAsync" -> {
                int cx = (int) args[0], cz = (int) args[1];
                boolean generated = (boolean) HostBridge.call("World", id, "isChunkGenerated(int,int)", Boolean.class, cx, cz);
                // Paper's states: a saved but non-resident chunk is PARTIAL, a resident one is
                // GENERATED. OUTDATED needs a saved DataVersion check Ferrite does not keep.
                var state = !generated ? io.papermc.paper.world.chunk.ChunkGenerationState.NOT_GENERATED
                    : chunkLoaded(cx, cz) ? io.papermc.paper.world.chunk.ChunkGenerationState.GENERATED
                    : io.papermc.paper.world.chunk.ChunkGenerationState.PARTIAL;
                return method.equals("getChunkGenerationState") ? state : java.util.concurrent.CompletableFuture.completedFuture(state);
            }
            case "unloadChunk", "unloadChunkRequest" -> {
                // The host drops the plugin hold; a chunk a player still holds (or a source that
                // keeps it resident) refuses, exactly like Bukkit's "cannot unload now".
                int cx = args[0] instanceof org.bukkit.Chunk c ? c.getX() : (int) args[0];
                int cz = args[0] instanceof org.bukkit.Chunk c ? c.getZ() : (int) args[1];
                return HostBridge.callBoolean("World", id, "unloadChunk(int,int)", cx, cz);
            }
            case "loadChunk" -> {
                // The host loads the chunk (generating it only when `generate` is set) and
                // answers whether it is now loaded or generated.
                int cx, cz; boolean generate = true;
                if (args[0] instanceof org.bukkit.Chunk c) { cx = c.getX(); cz = c.getZ(); if (args.length > 1) generate = (boolean) args[1]; }
                else { cx = (int) args[0]; cz = (int) args[1]; if (args.length > 2) generate = (boolean) args[2]; }
                if (!generate && !(boolean) HostBridge.call("World", id, "isChunkGenerated(int,int)", Boolean.class, cx, cz)) return false;
                return (boolean) HostBridge.call("World", id, "loadChunk(int,int)", Boolean.class, cx, cz);
            }
            case "findLightningRod" -> { return findRod((org.bukkit.Location) args[0]); }
            case "findLightningTarget" -> { return findTarget((org.bukkit.Location) args[0]); }
            case "getLoadedChunks" -> {
                var text = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
                var chunks = new java.util.ArrayList<org.bukkit.Chunk>();
                if (text != null && !text.isEmpty()) for (var pair : text.split(String.valueOf(HostBridge.ITEM))) { var parts = pair.split(","); chunks.add(chunk(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]))); }
                return chunks.toArray(new org.bukkit.Chunk[0]);
            }
            case "getType" -> { if (args.length == 3) return typeAt(new Pos((int) args[0], (int) args[1], (int) args[2])); }
            case "getBlockData" -> { if (args.length == 3) return FerriteBlockData.create(stateAt(new Pos((int) args[0], (int) args[1], (int) args[2]))); }
            case "setBlockData" -> {
                if (args.length == 4) { setState(new Pos((int) args[0], (int) args[1], (int) args[2]), (org.bukkit.block.data.BlockData) args[3]); return null; }
            }
            case "getWorldType" -> { return org.bukkit.WorldType.NORMAL; }
            case "setSpawnFlags" -> { allowMonsters = (boolean) args[0]; allowAnimals = (boolean) args[1]; return null; }
            case "getAllowAnimals" -> { return allowAnimals; }
            case "getAllowMonsters" -> { return allowMonsters; }
            case "getSpawnLimit" -> { return spawnLimits.getOrDefault((org.bukkit.entity.SpawnCategory) args[0], -1); }
            case "setSpawnLimit" -> { spawnLimits.put((org.bukkit.entity.SpawnCategory) args[0], (int) args[1]); return null; }
            case "getTicksPerSpawns" -> { return ticksPerSpawns.getOrDefault((org.bukkit.entity.SpawnCategory) args[0], 1); }
            case "setTicksPerSpawns" -> { ticksPerSpawns.put((org.bukkit.entity.SpawnCategory) args[0], (int) args[1]); return null; }
            case "save" -> { if (args.length == 1) { HostBridge.callVoid("World", id, "save()"); return null; } }
            case "sendPluginMessage" -> {
                for (var player : playersHere()) player.sendPluginMessage((org.bukkit.plugin.Plugin) args[0], (String) args[1], (byte[]) args[2]);
                return null;
            }
            case "getEntity" -> { for (var entity : entitiesHere()) if (entity.getUniqueId().equals(args[0])) return entity; return null; }
            case "getEntitiesByClass" -> {
                var types = args.length == 1 && args[0] instanceof Class<?>[] array ? array : new Class<?>[]{(Class<?>) args[0]};
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                for (var entity : entitiesHere()) for (var type : types) if (type.isInstance(entity)) { out.add(entity); break; }
                return out;
            }
            case "getEntitiesByClasses" -> {
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                for (var entity : entitiesHere()) for (var type : (Class<?>[]) args[0]) if (type.isInstance(entity)) { out.add(entity); break; }
                return out;
            }
            case "getNearbyEntities" -> {
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                java.util.function.Predicate<org.bukkit.entity.Entity> filter = args[args.length - 1] instanceof java.util.function.Predicate<?> f ? (java.util.function.Predicate<org.bukkit.entity.Entity>) f : e -> true;
                for (var entity : entitiesHere()) {
                    var at = entity.getLocation();
                    boolean hit;
                    if (args[0] instanceof org.bukkit.util.BoundingBox box) hit = box.contains(at.getX(), at.getY(), at.getZ());
                    else { var c = (org.bukkit.Location) args[0]; hit = inBox(at, c.getX(), c.getY(), c.getZ(), (double) args[1], (double) args[2], (double) args[3]); }
                    if (hit && filter.test(entity)) out.add(entity);
                }
                return out;
            }
            case "getPlayersSeeingChunk" -> {
                int cx, cz;
                if (args[0] instanceof org.bukkit.Chunk chunk) { cx = chunk.getX(); cz = chunk.getZ(); } else { cx = (int) args[0]; cz = (int) args[1]; }
                int range = server.getViewDistance();
                var out = new java.util.ArrayList<org.bukkit.entity.Player>();
                for (var player : playersHere()) {
                    var at = player.getLocation();
                    if (Math.abs((at.getBlockX() >> 4) - cx) <= range && Math.abs((at.getBlockZ() >> 4) - cz) <= range) out.add(player);
                }
                return out;
            }
            case "getIntersectingChunks" -> {
                var box = (org.bukkit.util.BoundingBox) args[0];
                var out = new java.util.ArrayList<org.bukkit.Chunk>();
                for (int x = (int) Math.floor(box.getMinX()) >> 4; x <= (int) Math.floor(box.getMaxX()) >> 4; x++)
                    for (int z = (int) Math.floor(box.getMinZ()) >> 4; z <= (int) Math.floor(box.getMaxZ()) >> 4; z++) out.add(chunk(x, z));
                return out;
            }
            case "hasCollisionsIn" -> {
                var box = (org.bukkit.util.BoundingBox) args[0];
                for (int x = (int) Math.floor(box.getMinX()); x < Math.ceil(box.getMaxX()); x++)
                    for (int y = (int) Math.floor(box.getMinY()); y < Math.ceil(box.getMaxY()); y++)
                        for (int z = (int) Math.floor(box.getMinZ()); z < Math.ceil(box.getMaxZ()); z++)
                            if (solid(x, y, z)) return true;
                return false;
            }
            // Game events only matter to listeners such as sculk sensors; the game has none, so nobody hears them.
            case "sendGameEvent" -> { return null; }
            case "rayTraceBlocks" -> { return traceBlocks((org.bukkit.Location) args[0], (org.bukkit.util.Vector) args[1], ((Number) args[2]).doubleValue(),
                args.length > 3 ? (org.bukkit.FluidCollisionMode) args[3] : org.bukkit.FluidCollisionMode.NEVER, args.length > 4 && (boolean) args[4]); }
            case "rayTraceEntities" -> {
                double size = args.length > 3 && args[3] instanceof Number n ? n.doubleValue() : 0.0;
                return traceEntities((org.bukkit.Location) args[0], (org.bukkit.util.Vector) args[1], ((Number) args[2]).doubleValue(), size, filterOf(args), null);
            }
            case "rayTrace" -> {
                var start = (org.bukkit.Location) args[0];
                var direction = (org.bukkit.util.Vector) args[1];
                double max = ((Number) args[2]).doubleValue();
                var block = traceBlocks(start, direction, max, (org.bukkit.FluidCollisionMode) args[3], (boolean) args[4]);
                double limit = block == null ? max : block.getHitPosition().distance(start.toVector());
                var entity = traceEntities(start, direction, limit, ((Number) args[5]).doubleValue(), filterOf(args), null);
                return entity != null ? entity : block;
            }
            case "lineOfSightExists" -> {
                var from = (org.bukkit.Location) args[0];
                var to = (org.bukkit.Location) args[1];
                var delta = to.toVector().subtract(from.toVector());
                double length = delta.length();
                if (length == 0) return true;
                var step = delta.clone().normalize().multiply(0.25);
                var at = from.toVector();
                for (double travelled = 0; travelled < length; travelled += 0.25) {
                    if (solid(at.getBlockX(), at.getBlockY(), at.getBlockZ()) && typeAt(new Pos(at.getBlockX(), at.getBlockY(), at.getBlockZ())).isOccluding()) return false;
                    at.add(step);
                }
                return true;
            }
            default -> { }
        }
        return NONE;
    }
    private final org.bukkit.persistence.PersistentDataContainer data = new FerriteDataContainer();

    /** The dimension this handle was opened in (the host reports the stored one for runtime worlds). */
    private org.bukkit.World.Environment environment() {
        return environment;
    }

    private int minHeight() { return environment() == org.bukkit.World.Environment.NORMAL ? -64 : 0; }
    private int height() { return environment() == org.bukkit.World.Environment.NORMAL ? 384 : 256; }

    /** Constants of the three vanilla dimension types (min_y, height, logical_height, coordinate_scale, flags). */
    private Object dimension(String method, Object[] args) {
        var env = environment();
        boolean nether = env == org.bukkit.World.Environment.NETHER, end = env == org.bukkit.World.Environment.THE_END;
        return switch (method) {
            case "getEnvironment" -> env;
            case "getMinHeight" -> minHeight();
            case "getMaxHeight" -> minHeight() + height();
            case "getLogicalHeight" -> nether ? 128 : height();
            case "getSeaLevel" -> nether ? 32 : 63;
            case "getCoordinateScale" -> nether ? 8.0 : 1.0;
            case "hasSkyLight" -> env == org.bukkit.World.Environment.NORMAL;
            case "hasCeiling" -> nether;
            case "isNatural", "isBedWorks" -> env == org.bukkit.World.Environment.NORMAL;
            case "isUltraWarm", "isPiglinSafe", "isRespawnAnchorWorks" -> nether;
            case "hasRaids" -> !nether;
            case "isFixedTime" -> nether || end;
            case "hasBonusChest" -> false;
            case "key" -> net.kyori.adventure.key.Key.key("minecraft", nether ? "the_nether" : end ? "the_end" : "overworld");
            case "getKey" -> new org.bukkit.NamespacedKey("minecraft", nether ? "the_nether" : end ? "the_end" : "overworld");
            case "getPlayerCount" -> playersHere().size();
            case "getEntityCount" -> entitiesHere().size();
            case "getViewDistance", "getSendViewDistance" -> server.getViewDistance();
            case "getSimulationDistance" -> server.getSimulationDistance();
            case "getPersistentDataContainer" -> data;
            case "getPluginChunkTickets" -> {
                var tickets = new java.util.LinkedHashMap<org.bukkit.plugin.Plugin, java.util.Collection<org.bukkit.Chunk>>();
                chunkTickets.forEach((key, plugins) -> { for (var plugin : plugins) tickets.computeIfAbsent(plugin, ignored -> new java.util.ArrayList<>()).add(chunk(keyX(key), keyZ(key))); });
                yield tickets;
            }
            case "addPluginChunkTicket" -> {
                int x = (int) args[0], z = (int) args[1];
                boolean added = chunkTickets.computeIfAbsent(chunkKey(x, z), ignored -> java.util.concurrent.ConcurrentHashMap.newKeySet()).add(java.util.Objects.requireNonNull((org.bukkit.plugin.Plugin) args[2], "plugin"));
                refreshHold(x, z);
                yield added;
            }
            case "removePluginChunkTicket" -> {
                int x = (int) args[0], z = (int) args[1];
                var plugins = chunkTickets.get(chunkKey(x, z));
                boolean removed = plugins != null && plugins.remove((org.bukkit.plugin.Plugin) args[2]);
                refreshHold(x, z);
                yield removed;
            }
            case "removePluginChunkTickets" -> {
                for (var entry : chunkTickets.entrySet()) entry.getValue().remove((org.bukkit.plugin.Plugin) args[0]);
                for (long key : hostHeld) refreshHold(keyX(key), keyZ(key));
                yield null;
            }
            case "isChunkForceLoaded" -> forceLoaded.contains(chunkKey((int) args[0], (int) args[1]));
            case "setChunkForceLoaded" -> {
                int x = (int) args[0], z = (int) args[1];
                if ((boolean) args[2]) forceLoaded.add(chunkKey(x, z)); else forceLoaded.remove(chunkKey(x, z));
                refreshHold(x, z);
                yield null;
            }
            case "getForceLoadedChunks" -> { var chunks = new java.util.ArrayList<org.bukkit.Chunk>(); for (long key : forceLoaded) chunks.add(chunk(keyX(key), keyZ(key))); yield chunks; }
            case "getRaids", "getPopulators" -> java.util.List.of();
            case "getRaid", "locateNearestRaid" -> null;
            case "dropItem", "dropItemNaturally" -> {
                var at = ((org.bukkit.Location) args[0]).clone();
                var stack = ((org.bukkit.inventory.ItemStack) args[1]).clone();
                if (stack.getType().isAir() || stack.getAmount() <= 0) throw new IllegalArgumentException("Cannot drop air");
                var uuid = java.util.UUID.randomUUID();
                var handle = DroppedItem.of(at, stack, proxy, uuid);
                if (args.length > 2 && args[2] instanceof java.util.function.Consumer<?> consumer) { @SuppressWarnings("unchecked") var typed = (java.util.function.Consumer<org.bukkit.entity.Item>) consumer; typed.accept(handle); }
                HostBridge.callVoid("World", id, method + "(Location,ItemStack,String)", at, stack, uuid.toString());
                yield handle;
            }
            case "createExplosion" -> {
                // Every overload becomes (Location, power, fire, breakBlocks): the source entity only decides the
                // centre when there is no location, and the host applies plain vanilla blast rules to everything.
                int i = 0;
                org.bukkit.entity.Entity source = args[i] instanceof org.bukkit.entity.Entity e ? e : null;
                if (source != null) i++;
                org.bukkit.Location at;
                if (args[i] instanceof org.bukkit.Location l) { at = l.clone(); i++; }
                else if (source != null) at = source.getLocation();
                else if (args[i] instanceof Number) { at = new org.bukkit.Location(proxy, ((Number) args[i]).doubleValue(), ((Number) args[i + 1]).doubleValue(), ((Number) args[i + 2]).doubleValue()); i += 3; }
                else throw new IllegalArgumentException("Ferrite: World#createExplosion needs a location or a source entity");
                if (at.getWorld() != null && !at.getWorld().equals(proxy)) throw new IllegalArgumentException("Ferrite: World#createExplosion location is in another world: " + at.getWorld().getName());
                float power = ((Number) args[i++]).floatValue();
                boolean fire = i < args.length && args[i] instanceof Boolean b0 && b0; if (i < args.length && args[i] instanceof Boolean) i++;
                boolean breaks = !(i < args.length && args[i] instanceof Boolean b1) || b1; if (i < args.length && args[i] instanceof Boolean) i++;
                at.setWorld(proxy);
                yield HostBridge.callBoolean("World", id, "createExplosion(Location,float,boolean,boolean)", at, power, fire, breaks);
            }
            case "createEntity" -> {
                // An entity of the class that is not in the world yet; addEntity puts it there.
                var at = ((org.bukkit.Location) args[0]).clone();
                var type = entityTypeOf((Class<?>) args[1]);
                var uuid = java.util.UUID.randomUUID();
                created.add(uuid);
                yield SpawnedEntity.of(type, at, proxy, uuid);
            }
            case "addEntity" -> {
                var entity = (org.bukkit.entity.Entity) java.util.Objects.requireNonNull(args[0], "entity");
                if (!created.remove(entity.getUniqueId()))
                    throw new IllegalArgumentException("Ferrite: World#addEntity only accepts an entity from World#createEntity that is not in a world yet: " + entity);
                var at = entity.getLocation();
                var type = entity.getType();
                HostBridge.callVoid("World", id, "spawnEntity(Location,EntityType,String)", at, type, entity.getUniqueId().toString());
                yield entity;
            }
            case "spawn" -> {
                // (Location, Class, ...): any of the RegionAccessor overloads. The entity is spawned first and the
                // consumer sees the live entity, where Bukkit calls it just before adding; the spawn reason and
                // randomizeData have no host effect (no creature spawn event is fired for plugin spawns).
                var at = ((org.bukkit.Location) args[0]).clone();
                var type = entityTypeOf((Class<?>) args[1]);
                java.util.function.Consumer<?> consumer = null;
                for (int i = 2; i < args.length; i++) if (args[i] instanceof java.util.function.Consumer<?> c) consumer = c;
                var uuid = java.util.UUID.randomUUID();
                var handle = SpawnedEntity.of(type, at, proxy, uuid);
                HostBridge.callVoid("World", id, "spawnEntity(Location,EntityType,String)", at, type, uuid.toString());
                if (consumer != null) { @SuppressWarnings("unchecked") var typed = (java.util.function.Consumer<org.bukkit.entity.Entity>) consumer; typed.accept(handle); }
                yield handle;
            }
            case "spawnEntity" -> {
                var at = ((org.bukkit.Location) args[0]).clone();
                var type = (org.bukkit.entity.EntityType) args[1];
                var uuid = java.util.UUID.randomUUID();
                var handle = SpawnedEntity.of(type, at, proxy, uuid);
                HostBridge.callVoid("World", id, "spawnEntity(Location,EntityType,String)", at, type, uuid.toString());
                yield handle;
            }
            case "strikeLightning", "strikeLightningEffect" -> {
                var at = ((org.bukkit.Location) args[0]).clone();
                var uuid = java.util.UUID.randomUUID();
                var handle = SpawnedEntity.of(org.bukkit.entity.EntityType.LIGHTNING_BOLT, at, proxy, uuid);
                HostBridge.callVoid("World", id, method + "(Location)", at);
                yield handle;
            }
            case "spawnFallingBlock" -> {
                var at = ((org.bukkit.Location) args[0]).clone();
                org.bukkit.Material material;
                if (args[1] instanceof org.bukkit.block.data.BlockData data) material = data.getMaterial();
                else if (args[1] instanceof org.bukkit.Material m) material = m;
                else material = ((org.bukkit.material.MaterialData) args[1]).getItemType();
                if (!material.isBlock()) throw new IllegalArgumentException("Material must be a block");
                var uuid = java.util.UUID.randomUUID();
                var handle = SpawnedEntity.of(org.bukkit.entity.EntityType.FALLING_BLOCK, at, proxy, uuid);
                HostBridge.callVoid("World", id, "spawnFallingBlock(Location,Material,String)", at, material, uuid.toString());
                yield handle;
            }
            case "spawnArrow" -> {
                // (Location, Vector, float speed, float spread, Class): vanilla Projectile#shoot with the arrow type.
                if (args.length != 5) yield NONE;
                var at = ((org.bukkit.Location) args[0]).clone();
                var direction = ((org.bukkit.util.Vector) args[1]).clone();
                float speed = (Float) args[2], spread = (Float) args[3];
                var kind = (Class<?>) args[4];
                if (!kind.isAssignableFrom(org.bukkit.entity.Arrow.class))
                    throw new UnsupportedOperationException("Ferrite: World#spawnArrow(" + kind.getSimpleName() + ") is not available: the host only spawns plain arrows");
                if (direction.lengthSquared() == 0) throw new IllegalArgumentException("direction must not be zero");
                direction.normalize();
                var random = java.util.concurrent.ThreadLocalRandom.current();
                var velocity = new org.bukkit.util.Vector(
                    direction.getX() + random.nextGaussian() * 0.0075 * spread,
                    direction.getY() + random.nextGaussian() * 0.0075 * spread,
                    direction.getZ() + random.nextGaussian() * 0.0075 * spread).multiply(speed);
                var uuid = java.util.UUID.randomUUID();
                var handle = SpawnedEntity.of(org.bukkit.entity.EntityType.ARROW, at, proxy, uuid);
                HostBridge.callVoid("World", id, "spawnArrow(Location,double,double,double,String)", at, velocity.getX(), velocity.getY(), velocity.getZ(), uuid.toString());
                yield handle;
            }
            case "getInfiniburn" -> {
                // The dimension type's `infiniburn` block tag: overworld, nether and end each name their own.
                var tag = switch (env) {
                    case NETHER -> "infiniburn_nether";
                    case THE_END -> "infiniburn_end";
                    default -> "infiniburn_overworld";
                };
                for (var candidate : FerriteTags.tags("blocks", org.bukkit.Material.class))
                    if (candidate.getKey().getKey().equals(tag)) yield java.util.Set.copyOf(candidate.getValues());
                throw new UnsupportedOperationException("Ferrite: block tag minecraft:" + tag + " is not in the generated data");
            }
            case "locateNearestPoi" -> {
                var hits = poiSearch(args, true);
                yield hits.isEmpty() ? null : hits.get(0).location();
            }
            case "locateAllPoiInRange" -> poiSearch(args, false);
            case "getTileEntityCount" -> tileEntities(false);
            case "getTickableTileEntityCount" -> tileEntities(true);
            case "getWorldPath" -> {
                var dir = (String) HostBridge.call("World", id, "getWorldPath()", String.class);
                if (dir == null || dir.isEmpty()) throw new UnsupportedOperationException("Ferrite: this world is not saved to disk");
                yield java.nio.file.Path.of(dir);
            }
            case "canGenerateStructures" -> true;
            case "generateTree" -> {
                // (Location, TreeType) | (Location, Random, TreeType) | (... , BlockChangeDelegate).
                if (args.length < 2 || !(args[0] instanceof org.bukkit.Location at)) yield NONE;
                org.bukkit.TreeType type = null;
                for (var arg : args) if (arg instanceof org.bukkit.TreeType found) type = found;
                if (type == null) yield NONE;
                var feature = treeFeature(type);
                if (feature == null) yield false;
                org.bukkit.BlockChangeDelegate delegate = null;
                for (var arg : args) if (arg instanceof org.bukkit.BlockChangeDelegate d) delegate = d;
                if (delegate != null) {
                    // The delegate owns the changes; the host only reports the feature's blocks.
                    var reply = (String) HostBridge.call("World", id, "featureBlocks(Location,String)", String.class, at, feature);
                    if (reply == null) throw new UnsupportedOperationException("Ferrite: generateTree with a BlockChangeDelegate needs the world host");
                    boolean applied = true;
                    for (var line : reply.split("\n")) {
                        if (line.isEmpty()) continue;
                        var fields = line.split(",", 4);
                        if (fields.length != 4) continue;
                        applied &= delegate.setBlockData(Integer.parseInt(fields[0]), Integer.parseInt(fields[1]), Integer.parseInt(fields[2]), FerriteBlockData.create(fields[3]));
                    }
                    yield applied && !reply.isEmpty();
                }
                yield Boolean.parseBoolean((String) HostBridge.call("World", id, "placeFeature(Location,String)", String.class, at, feature));
            }
            case "getChunkCount" -> {
                yield Integer.parseInt((String) HostBridge.call("World", id, "getChunkCount()", String.class));
            }
            case "isVoidDamageEnabled" -> voidDamageEnabled;
            case "setVoidDamageEnabled" -> { voidDamageEnabled = (boolean) args[0]; pushVoidDamage(); yield null; }
            case "getVoidDamageAmount" -> voidDamageAmount;
            case "setVoidDamageAmount" -> {
                float amount = (float) args[0];
                if (!(amount >= 0f) || Float.isInfinite(amount)) throw new IllegalArgumentException("void damage amount must be finite and >= 0");
                voidDamageAmount = amount;
                pushVoidDamage();
                yield null;
            }
            case "getVoidDamageMinBuildHeightOffset" -> voidDamageOffset;
            case "setVoidDamageMinBuildHeightOffset" -> {
                double offset = (double) args[0];
                if (Double.isNaN(offset) || Double.isInfinite(offset)) throw new IllegalArgumentException("void damage offset must be finite");
                voidDamageOffset = offset;
                pushVoidDamage();
                yield null;
            }
            case "getFeatureFlags", "getListeningPluginChannels" -> java.util.Set.of();
            case "getGenerator", "getBiomeProvider" -> null;
            case "getEnderDragonBattle" -> environment() == org.bukkit.World.Environment.THE_END ? dragonBattle() : null;
            case "locateNearestBiome", "locateNearestStructure", "hasStructureAt" -> locate(method, args);
            case "getStructures" -> structures(args);
            default -> NONE;
        };
    }

    /** The vanilla configured feature that grows a tree type, or null when there is none. */
    static String treeFeature(org.bukkit.TreeType type) {
        return switch (type) {
            case TREE -> "minecraft:oak";
            case BIG_TREE -> "minecraft:fancy_oak";
            case REDWOOD -> "minecraft:spruce";
            case TALL_REDWOOD -> "minecraft:pine";
            case MEGA_REDWOOD -> "minecraft:mega_spruce";
            case BIRCH -> "minecraft:birch";
            case TALL_BIRCH -> "minecraft:birch_tall";
            case JUNGLE -> "minecraft:mega_jungle_tree";
            case SMALL_JUNGLE -> "minecraft:jungle_tree_no_vine";
            case JUNGLE_BUSH -> "minecraft:jungle_bush";
            case SWAMP -> "minecraft:swamp_oak";
            case ACACIA -> "minecraft:acacia";
            case DARK_OAK -> "minecraft:dark_oak";
            case MANGROVE -> "minecraft:mangrove";
            case TALL_MANGROVE -> "minecraft:tall_mangrove";
            case CHERRY -> "minecraft:cherry";
            case PALE_OAK -> "minecraft:pale_oak";
            case PALE_OAK_CREAKING -> "minecraft:pale_oak_creaking";
            case RED_POPLAR -> "minecraft:red_poplar";
            case ORANGE_POPLAR -> "minecraft:orange_poplar";
            case YELLOW_POPLAR -> "minecraft:yellow_poplar";
            case COCOA_TREE -> "minecraft:jungle_tree";
            case MEGA_PINE -> "minecraft:mega_pine";
            case CRIMSON_FUNGUS -> "minecraft:crimson_fungus";
            case WARPED_FUNGUS -> "minecraft:warped_fungus";
            case AZALEA -> "minecraft:azalea_tree";
            case RED_MUSHROOM -> "minecraft:huge_red_mushroom";
            case BROWN_MUSHROOM -> "minecraft:huge_brown_mushroom";
            case CHORUS_PLANT -> "minecraft:chorus_plant";
            default -> null;
        };
    }

    /** `getStructures(chunkX, chunkZ[, Structure])`: the structure starts the host reports for a chunk. */
    private Object structures(Object[] args) {
        if (args.length < 2 || !(args[0] instanceof Integer cx) || !(args[1] instanceof Integer cz)) return NONE;
        var only = args.length == 3 && args[2] instanceof org.bukkit.generator.structure.Structure s ? s : null;
        if (args.length == 3 && only == null) return NONE;
        var reply = (String) HostBridge.call("World", id, "getStructureStarts(int,int)", String.class, cx, cz);
        var found = new java.util.ArrayList<org.bukkit.generator.structure.GeneratedStructure>();
        if (reply == null || reply.isEmpty()) return found;
        for (var line : reply.split("\n")) {
            var f = line.split(" ");
            if (f.length != 7) continue;
            var key = org.bukkit.NamespacedKey.fromString(f[0]);
            var structure = key == null ? null : org.bukkit.Registry.STRUCTURE.get(key);
            if (structure == null || (only != null && !only.equals(structure))) continue;
            var box = new org.bukkit.util.BoundingBox(Integer.parseInt(f[1]), Integer.parseInt(f[2]), Integer.parseInt(f[3]),
                Integer.parseInt(f[4]) + 1, Integer.parseInt(f[5]) + 1, Integer.parseInt(f[6]) + 1);
            found.add(new Generated(structure, box));
        }
        return found;
    }

    /** A structure start; the individual pieces are not reported by the host. */
    private record Generated(org.bukkit.generator.structure.Structure structure, org.bukkit.util.BoundingBox box)
            implements org.bukkit.generator.structure.GeneratedStructure {
        @Override public org.bukkit.generator.structure.Structure getStructure() { return structure; }
        @Override public org.bukkit.util.BoundingBox getBoundingBox() { return box; }
        /**
         * The host reports a structure start's bounding box only (`getStructureStarts(int,int)` answers
         * `id minX minY minZ maxX maxY maxZ`); the jar's `StructureStart#getPieces` is not carried over the bridge.
         * An empty list would claim the structure has no pieces, so this refuses instead.
         */
        @Override public java.util.Collection<org.bukkit.generator.structure.StructurePiece> getPieces() {
            throw new UnsupportedOperationException("Ferrite: GeneratedStructure#getPieces is not available: the world generation host reports a structure start's bounding box, not its pieces");
        }
        @Override public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() { return new FerriteDataContainer(); }
    }

    /** The sampling intervals of the host's biome search (`Host.locateBiome`), equal to Paper's defaults. */
    private static final int BIOME_HORIZONTAL_INTERVAL = 32, BIOME_VERTICAL_INTERVAL = 64;

    /** Biome and structure searches, answered by the vanilla world generation host. */
    private Object locate(String name, Object[] args) {
        if (name.equals("hasStructureAt")) {
            if (args.length != 2 || !(args[0] instanceof io.papermc.paper.math.Position at) || !(args[1] instanceof org.bukkit.generator.structure.Structure structure)) return NONE;
            return Boolean.parseBoolean((String) HostBridge.call("World", id, "hasStructureAt(String,int,int,int)", String.class,
                structure.getKey().toString(), at.blockX(), at.blockY(), at.blockZ()));
        }
        if (!(args[0] instanceof org.bukkit.Location origin)) return NONE;
        if (name.equals("locateNearestStructure")) {
            if (args.length != 4) return NONE;
            int radius = (int) args[2];
            if (args[1] instanceof org.bukkit.generator.structure.Structure structure) {
                var at = search(true, structure.getKey().toString(), radius, origin);
                return at == null ? null : new FoundStructure(structure, at);
            }
            // `StructureType` groups several registry structures (all jigsaw villages, all ocean
            // ruins); search each and keep the nearest, like vanilla's `findNearestMapStructure`
            // over a holder set.
            if (args[1] instanceof org.bukkit.generator.structure.StructureType type) {
                org.bukkit.generator.structure.Structure bestStructure = null;
                org.bukkit.Location bestAt = null;
                for (var candidate : org.bukkit.Registry.STRUCTURE) {
                    org.bukkit.generator.structure.StructureType candidateType;
                    try { candidateType = candidate.getStructureType(); }
                    // A structure whose type is missing from the generated data cannot match any
                    // requested type; asking that structure for its type directly still fails.
                    catch (UnsupportedOperationException missingType) { continue; }
                    if (!type.equals(candidateType)) continue;
                    var at = search(true, candidate.getKey().toString(), radius, origin);
                    if (at != null && (bestAt == null || at.distanceSquared(origin) < bestAt.distanceSquared(origin))) {
                        bestStructure = candidate;
                        bestAt = at;
                    }
                }
                return bestAt == null ? null : new FoundStructure(bestStructure, bestAt);
            }
            // The deprecated org.bukkit.StructureType overload returns a plain Location and
            // matches by the legacy name prefix (`village` covers `village_plains`, ...).
            if (args[1] instanceof org.bukkit.StructureType type) {
                org.bukkit.Location best = null;
                for (var candidate : org.bukkit.Registry.STRUCTURE) {
                    var path = candidate.getKey().getKey();
                    if (!path.equals(type.getName()) && !path.startsWith(type.getName() + "_")) continue;
                    var at = search(true, candidate.getKey().toString(), radius, origin);
                    if (at != null && (best == null || at.distanceSquared(origin) < best.distanceSquared(origin))) best = at;
                }
                return best;
            }
            return NONE;
        }
        // Biomes: (Location, int radius, Biome...) | (Location, int, int, int, Biome...) | (Location, Biome, int[, int]).
        int radius;
        java.util.List<org.bukkit.block.Biome> wanted = new java.util.ArrayList<>();
        boolean legacy = args[1] instanceof org.bukkit.block.Biome;
        if (legacy) { wanted.add((org.bukkit.block.Biome) args[1]); radius = (int) args[2]; }
        else {
            radius = (int) args[1];
            // The host searches with the vanilla host's fixed 32-block horizontal and 64-block vertical
            // sampling (`ServerLevel#findClosestBiome3d(..., 32, 64)`), which is also what Paper's
            // `locateNearestBiome(Location, int, Biome...)` uses. Other explicit intervals would be
            // silently ignored, so they are refused.
            if (args.length >= 4 && args[2] instanceof Integer horizontal && args[3] instanceof Integer vertical
                    && (horizontal != BIOME_HORIZONTAL_INTERVAL || vertical != BIOME_VERTICAL_INTERVAL)) {
                throw new UnsupportedOperationException("Ferrite: World#locateNearestBiome samples at the host's fixed "
                    + BIOME_HORIZONTAL_INTERVAL + "-block horizontal and " + BIOME_VERTICAL_INTERVAL
                    + "-block vertical intervals; horizontalInterval=" + horizontal + ", verticalInterval=" + vertical + " cannot be honoured");
            }
            for (var arg : args) {
                if (arg instanceof org.bukkit.block.Biome[] many) wanted.addAll(java.util.List.of(many));
                else if (arg instanceof org.bukkit.block.Biome one) wanted.add(one);
            }
        }
        org.bukkit.block.Biome best = null;
        org.bukkit.Location bestAt = null;
        for (var biome : wanted) {
            var at = search(false, biome.getKey().toString(), radius, origin);
            if (at != null && (bestAt == null || at.distanceSquared(origin) < bestAt.distanceSquared(origin))) { best = biome; bestAt = at; }
        }
        if (legacy) return bestAt;
        return bestAt == null ? null : new FoundBiome(best, bestAt);
    }

    private org.bukkit.Location search(boolean structure, String target, int radius, org.bukkit.Location origin) {
        var method = structure ? "locateStructure(String,int,int,int)" : "locateBiome(String,int,int,int)";
        var reply = (String) HostBridge.call("World", id, method, String.class, target, radius, origin.getBlockX(), origin.getBlockZ());
        if (reply == null || reply.isEmpty()) return null;
        var parts = reply.split(" ");
        return new org.bukkit.Location((org.bukkit.World) proxy, Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]));
    }

    private record FoundBiome(org.bukkit.block.Biome biome, org.bukkit.Location location) implements org.bukkit.util.BiomeSearchResult {
        @Override public org.bukkit.block.Biome getBiome() { return biome; }
        @Override public org.bukkit.Location getLocation() { return location; }
    }

    private record FoundStructure(org.bukkit.generator.structure.Structure structure, org.bukkit.Location location) implements org.bukkit.util.StructureSearchResult {
        @Override public org.bukkit.generator.structure.Structure getStructure() { return structure; }
        @Override public org.bukkit.Location getLocation() { return location; }
    }
}
