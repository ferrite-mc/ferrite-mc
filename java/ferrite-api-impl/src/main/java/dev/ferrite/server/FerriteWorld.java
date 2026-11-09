package dev.ferrite.server;

/** World identity and explicit block snapshots; unknown coordinates fail rather than inventing air. */
final class FerriteWorld implements java.lang.reflect.InvocationHandler {
    private record Pos(int x, int y, int z) {}
    private record Column(int x, int z) {}
    private final FerriteServer server;
    private final java.util.UUID id;
    private final String name;
    private final long time;
    private final Pos spawn;
    private final java.util.Map<Pos, org.bukkit.Material> blocks = new java.util.concurrent.ConcurrentHashMap<>();
    /** Full state strings for blocks a plugin placed with {@code setBlockData}; {@link #blocks} holds their material. */
    private final java.util.Map<Pos, String> states = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<Column, Integer> heights = new java.util.concurrent.ConcurrentHashMap<>();
    private org.bukkit.World proxy;
    // Remembered for the plugin; no server rule reads these (players cannot hurt each other yet, hardcore needs a respawn rule).
    private volatile boolean hardcore, pvp = true, autoSave = true;
    private FerriteWorld(FerriteServer server, java.util.UUID id, String name, long time, Pos spawn) { this.server = server; this.id = id; this.name = name; this.time = time; this.spawn = spawn; }
    static org.bukkit.World create(FerriteServer server, java.util.UUID id, String name, long time, int x, int y, int z) {
        var world = new FerriteWorld(server, id, name, time, new Pos(x, y, z));
        world.proxy = (org.bukkit.World) java.lang.reflect.Proxy.newProxyInstance(FerriteWorld.class.getClassLoader(), new Class<?>[]{org.bukkit.World.class}, world);
        return world.proxy;
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
        int x = pos.x(), y = pos.y(), z = pos.z();
        switch (face) {
            case UP -> y++; case DOWN -> y--; case NORTH -> z--; case SOUTH -> z++;
            case EAST -> x++; case WEST -> x--;
            default -> throw new UnsupportedOperationException("Ferrite: relative block face " + face + " is not implemented");
        }
        return block(new Pos(x, y, z));
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
            case "hashCode" -> java.util.Objects.hash(id, x, z);
            case "equals" -> self == args[0];
            case "toString" -> "Chunk[" + id + ":" + x + "," + z + "]";
            default -> HostBridge.dispatch("Chunk", "", method, args);
        });
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getUID" -> id; case "getName" -> name; case "getTime", "getFullTime" -> hostLong(method.getName() + "()", time);
            case "getGameTime", "hasStorm", "isThundering", "isClearWeather", "getWeatherDuration", "getThunderDuration", "getClearWeatherDuration", "getDifficulty" -> hostValue(method, null);
            case "getMoonPhase" -> moonPhase(method);
            case "getWorldBorder" -> border();
            case "spigot" -> spigot();
            case "getSeed" -> hostLong("getSeed()", 0L);
            case "isDayTime" -> { long t = hostLong("getTime()", time) % 24_000; yield t < 12_000 || t > 23_960; }
            case "isHardcore" -> hardcore;
            case "setHardcore" -> { hardcore = (boolean) args[0]; yield null; }
            case "getPVP" -> pvp;
            case "setPVP" -> { pvp = (boolean) args[0]; yield null; }
            case "isAutoSave" -> autoSave;
            case "setAutoSave" -> { autoSave = (boolean) args[0]; try { HostBridge.callVoid("World", id, "setAutoSave(boolean)", autoSave); } catch (UnsupportedOperationException noHost) { /* remembered only */ } yield null; }
            case "getSpawnLocation" -> location(spawn);
            case "getHighestBlockYAt" -> { if (args.length > 2 || (args.length == 2 && args[1] instanceof org.bukkit.HeightMap)) throw new UnsupportedOperationException("Ferrite: heightmap-specific queries are not implemented"); int x, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); z = l.getBlockZ(); } else { x = (int) args[0]; z = (int) args[1]; } var height = heights.get(new Column(x, z)); if (height == null) { int value = Native.highestBlockY(id.toString(), x, z); if (value == Integer.MIN_VALUE) throw new UnsupportedOperationException("Ferrite: no height data at " + x + "," + z); height = value; heights.put(new Column(x, z), height); } yield height; }
            case "getBiome", "getComputedBiome" -> { int x, y, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); y = l.getBlockY(); z = l.getBlockZ(); } else { x = (int) args[0]; y = (int) args[1]; z = (int) args[2]; } var key = Native.biomeAt(id.toString(), x, y, z); if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at " + x + "," + y + "," + z); var biome = org.bukkit.Registry.BIOME.get(org.bukkit.NamespacedKey.fromString(key)); if (biome == null) throw new IllegalStateException("Unknown biome " + key); yield biome; }
            case "getBlockAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield block(new Pos(l.getBlockX(), l.getBlockY(), l.getBlockZ())); yield block(new Pos((int) args[0], (int) args[1], (int) args[2])); }
            case "getEmptyChunkSnapshot" -> {
                var world = (org.bukkit.World) proxy;
                var key = world.getKey();
                yield new FerriteChunkSnapshot((int) args[0], (int) args[1], name, id.toString(), net.kyori.adventure.key.Key.key(key.getNamespace(), key.getKey()), world.getMinHeight(), world.getMaxHeight(), hostLong("getFullTime()", time));
            }
            case "getChunkAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield chunk(l.getBlockX() >> 4, l.getBlockZ() >> 4); yield chunk((int) args[0], (int) args[1]); }
            case "getPlayers", "getEntities", "getLivingEntities" -> server.getOnlinePlayers().stream().filter(p -> { try { return p.getWorld() == proxy; } catch (UnsupportedOperationException | IllegalStateException e) { return false; } }).toList();
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

    private static final Object NONE = new Object();
    private final java.util.Map<String, java.util.Map<org.bukkit.plugin.Plugin, org.bukkit.metadata.MetadataValue>> metadata = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<org.bukkit.entity.SpawnCategory, Integer> spawnLimits = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Map<org.bukkit.entity.SpawnCategory, Integer> ticksPerSpawns = new java.util.concurrent.ConcurrentHashMap<>();
    private volatile boolean allowAnimals = true, allowMonsters = true;
    // Kept so plugins read back what they set; the game does not act on these.
    private volatile float voidDamageAmount = 4.0f;
    private volatile double voidDamageOffset = -64.0;
    private final java.util.Map<Long, java.util.Set<org.bukkit.plugin.Plugin>> chunkTickets = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<Long> forceLoaded = java.util.concurrent.ConcurrentHashMap.newKeySet();
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
    /** A world query the host answers from the live state, converted to the method's return type. */
    private Object hostValue(java.lang.reflect.Method method, Object fallback) {
        try { return HostBridge.dispatch("World", id, method, new Object[0]); }
        catch (UnsupportedOperationException offline) { return fallback; }
    }
    private Object moonPhase(java.lang.reflect.Method method) {
        int phase = (int) (hostLong("getMoonPhase()", 0) % 8);
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
    /** The nearest lightning rod within 128 blocks, looking at the top block of each loaded column. */
    private org.bukkit.Location findRod(org.bukkit.Location from) {
        var text = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
        if (text == null || text.isEmpty()) return null;
        Pos best = null;
        double bestDistance = 128.0 * 128.0;
        for (var pair : text.split(String.valueOf(HostBridge.ITEM))) {
            var parts = pair.split(",");
            int cx = Integer.parseInt(parts[0]), cz = Integer.parseInt(parts[1]);
            if (Math.abs((cx << 4) + 8 - from.getX()) > 144 || Math.abs((cz << 4) + 8 - from.getZ()) > 144) continue;
            for (int dx = 0; dx < 16; dx++) for (int dz = 0; dz < 16; dz++) {
                int x = (cx << 4) + dx, z = (cz << 4) + dz;
                double d = (x + 0.5 - from.getX()) * (x + 0.5 - from.getX()) + (z + 0.5 - from.getZ()) * (z + 0.5 - from.getZ());
                if (d >= bestDistance) continue;
                try {
                    int top = Native.highestBlockY(id.toString(), x, z);
                    if (top == Integer.MIN_VALUE) continue;
                    if (typeAt(new Pos(x, top, z)) == org.bukkit.Material.LIGHTNING_ROD) { best = new Pos(x, top, z); bestDistance = d; }
                } catch (UnsupportedOperationException | IllegalStateException noData) { /* column not readable */ }
            }
        }
        return best == null ? null : new org.bukkit.Location(proxy, best.x(), best.y(), best.z());
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
            case "isChunkInUse" -> { return chunkLoaded((int) args[0], (int) args[1]); }
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
                boolean generated = (boolean) HostBridge.call("World", id, "isChunkGenerated(int,int)", Boolean.class, (int) args[0], (int) args[1]);
                var state = generated ? io.papermc.paper.world.chunk.ChunkGenerationState.GENERATED : io.papermc.paper.world.chunk.ChunkGenerationState.NOT_GENERATED;
                return method.equals("getChunkGenerationState") ? state : java.util.concurrent.CompletableFuture.completedFuture(state);
            }
            case "unloadChunk", "unloadChunkRequest" -> {
                // The server unloads a chunk by itself once nobody holds it, so a request succeeds
                // exactly when the chunk is already unloaded and fails while a player or the spawn holds it.
                int cx = args[0] instanceof org.bukkit.Chunk c ? c.getX() : (int) args[0];
                int cz = args[0] instanceof org.bukkit.Chunk c ? c.getZ() : (int) args[1];
                return !chunkLoaded(cx, cz);
            }
            case "loadChunk" -> {
                // Chunk handles are views that need no loading step. `generate` (default true)
                // forces generation by reading one block of the chunk through the world view.
                int cx, cz; boolean generate = true;
                if (args[0] instanceof org.bukkit.Chunk c) { cx = c.getX(); cz = c.getZ(); if (args.length > 1) generate = (boolean) args[1]; }
                else { cx = (int) args[0]; cz = (int) args[1]; if (args.length > 2) generate = (boolean) args[2]; }
                if ((boolean) HostBridge.call("World", id, "isChunkGenerated(int,int)", Boolean.class, cx, cz)) return true;
                if (!generate) return false;
                typeAt(new Pos(cx << 4, 64, cz << 4));
                return true;
            }
            case "findLightningRod" -> { return findRod((org.bukkit.Location) args[0]); }
            case "findLightningTarget" -> {
                var at = (org.bukkit.Location) args[0];
                var rod = findRod(at);
                if (rod != null) return rod;
                int top = Native.highestBlockY(id.toString(), at.getBlockX(), at.getBlockZ());
                if (top == Integer.MIN_VALUE) throw new UnsupportedOperationException("Ferrite: no height data at " + at.getBlockX() + "," + at.getBlockZ());
                return new org.bukkit.Location(proxy, at.getBlockX(), top + 1, at.getBlockZ());
            }
            case "getLoadedChunks" -> {
                var text = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
                var chunks = new java.util.ArrayList<org.bukkit.Chunk>();
                if (text != null && !text.isEmpty()) for (var pair : text.split(String.valueOf(HostBridge.ITEM))) { var parts = pair.split(","); chunks.add(chunk(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]))); }
                return chunks.toArray(new org.bukkit.Chunk[0]);
            }
            case "getType" -> { if (args.length == 3) return typeAt(new Pos((int) args[0], (int) args[1], (int) args[2])); }
            case "getBlockData" -> { if (args.length == 3) return FerriteBlockData.create(typeAt(new Pos((int) args[0], (int) args[1], (int) args[2]))); }
            case "setBlockData" -> {
                if (args.length == 4) { setType(new Pos((int) args[0], (int) args[1], (int) args[2]), ((org.bukkit.block.data.BlockData) args[3]).getMaterial()); return null; }
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
            case "getEntity" -> { for (var player : playersHere()) if (player.getUniqueId().equals(args[0])) return player; return null; }
            case "getEntitiesByClass" -> {
                var types = args.length == 1 && args[0] instanceof Class<?>[] array ? array : new Class<?>[]{(Class<?>) args[0]};
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                for (var player : playersHere()) for (var type : types) if (type.isInstance(player)) { out.add(player); break; }
                return out;
            }
            case "getEntitiesByClasses" -> {
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                for (var player : playersHere()) for (var type : (Class<?>[]) args[0]) if (type.isInstance(player)) { out.add(player); break; }
                return out;
            }
            case "getNearbyEntities" -> {
                var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
                java.util.function.Predicate<org.bukkit.entity.Entity> filter = args[args.length - 1] instanceof java.util.function.Predicate<?> f ? (java.util.function.Predicate<org.bukkit.entity.Entity>) f : e -> true;
                for (var player : playersHere()) {
                    var at = player.getLocation();
                    boolean hit;
                    if (args[0] instanceof org.bukkit.util.BoundingBox box) hit = box.contains(at.getX(), at.getY(), at.getZ());
                    else { var c = (org.bukkit.Location) args[0]; hit = inBox(at, c.getX(), c.getY(), c.getZ(), (double) args[1], (double) args[2], (double) args[3]); }
                    if (hit && filter.test(player)) out.add(player);
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

    /** The vanilla dimension a world name stands for: the Nether and End are suffixed, everything else is the overworld. */
    private org.bukkit.World.Environment environment() {
        if (name.endsWith("_nether")) return org.bukkit.World.Environment.NETHER;
        if (name.endsWith("_the_end")) return org.bukkit.World.Environment.THE_END;
        return org.bukkit.World.Environment.NORMAL;
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
            case "getPlayerCount", "getEntityCount" -> playersHere().size();
            case "getViewDistance", "getSendViewDistance" -> server.getViewDistance();
            case "getSimulationDistance" -> server.getSimulationDistance();
            case "getPersistentDataContainer" -> data;
            case "getPluginChunkTickets" -> {
                var tickets = new java.util.LinkedHashMap<org.bukkit.plugin.Plugin, java.util.Collection<org.bukkit.Chunk>>();
                chunkTickets.forEach((key, plugins) -> { for (var plugin : plugins) tickets.computeIfAbsent(plugin, ignored -> new java.util.ArrayList<>()).add(chunk(keyX(key), keyZ(key))); });
                yield tickets;
            }
            case "addPluginChunkTicket" -> chunkTickets.computeIfAbsent(chunkKey((int) args[0], (int) args[1]), ignored -> java.util.concurrent.ConcurrentHashMap.newKeySet()).add(java.util.Objects.requireNonNull((org.bukkit.plugin.Plugin) args[2], "plugin"));
            case "removePluginChunkTicket" -> { var plugins = chunkTickets.get(chunkKey((int) args[0], (int) args[1])); yield plugins != null && plugins.remove((org.bukkit.plugin.Plugin) args[2]); }
            case "removePluginChunkTickets" -> { chunkTickets.values().forEach(plugins -> plugins.remove((org.bukkit.plugin.Plugin) args[0])); yield null; }
            case "isChunkForceLoaded" -> forceLoaded.contains(chunkKey((int) args[0], (int) args[1]));
            case "setChunkForceLoaded" -> { if ((boolean) args[2]) forceLoaded.add(chunkKey((int) args[0], (int) args[1])); else forceLoaded.remove(chunkKey((int) args[0], (int) args[1])); yield null; }
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
            case "canGenerateStructures" -> true;
            case "generateTree" -> {
                // (Location, TreeType) | (Location, Random, TreeType); a BlockChangeDelegate cannot be honoured.
                if (args.length < 2 || !(args[0] instanceof org.bukkit.Location at) || !(args[args.length - 1] instanceof org.bukkit.TreeType type)) yield NONE;
                var feature = treeFeature(type);
                if (feature == null) yield false;
                yield Boolean.parseBoolean((String) HostBridge.call("World", id, "placeFeature(Location,String)", String.class, at, feature));
            }
            case "getChunkCount" -> {
                var text = (String) HostBridge.call("World", id, "getLoadedChunkCoordinates()", String.class);
                yield text == null || text.isEmpty() ? 0 : text.split(String.valueOf(HostBridge.ITEM)).length;
            }
            case "getVoidDamageAmount" -> voidDamageAmount;
            case "setVoidDamageAmount" -> { voidDamageAmount = (float) args[0]; yield null; }
            case "getVoidDamageMinBuildHeightOffset" -> voidDamageOffset;
            case "setVoidDamageMinBuildHeightOffset" -> { voidDamageOffset = (double) args[0]; yield null; }
            case "getFeatureFlags", "getListeningPluginChannels" -> java.util.Set.of();
            case "getGenerator", "getBiomeProvider", "getEnderDragonBattle" -> null;
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
        @Override public java.util.Collection<org.bukkit.generator.structure.StructurePiece> getPieces() { return java.util.List.of(); }
        @Override public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() { return new FerriteDataContainer(); }
    }

    /** Biome and structure searches, answered by the vanilla world generation host. */
    private Object locate(String name, Object[] args) {
        if (name.equals("hasStructureAt")) {
            if (args.length != 2 || !(args[0] instanceof io.papermc.paper.math.Position at) || !(args[1] instanceof org.bukkit.generator.structure.Structure structure)) return NONE;
            return Boolean.parseBoolean((String) HostBridge.call("World", id, "hasStructureAt(String,int,int,int)", String.class,
                structure.getKey().toString(), at.blockX(), at.blockY(), at.blockZ()));
        }
        if (!(args[0] instanceof org.bukkit.Location origin)) return NONE;
        if (name.equals("locateNearestStructure")) {
            if (args.length != 4 || !(args[1] instanceof org.bukkit.generator.structure.Structure structure)) return NONE;
            var at = search(true, structure.getKey().toString(), (int) args[2], origin);
            return at == null ? null : new FoundStructure(structure, at);
        }
        // Biomes: (Location, int radius, Biome...) | (Location, int, int, int, Biome...) | (Location, Biome, int[, int]).
        int radius;
        java.util.List<org.bukkit.block.Biome> wanted = new java.util.ArrayList<>();
        boolean legacy = args[1] instanceof org.bukkit.block.Biome;
        if (legacy) { wanted.add((org.bukkit.block.Biome) args[1]); radius = (int) args[2]; }
        else {
            radius = (int) args[1];
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
