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
    private final java.util.Map<Column, Integer> heights = new java.util.concurrent.ConcurrentHashMap<>();
    private org.bukkit.World proxy;
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
        var resolved = org.bukkit.Material.matchMaterial(key);
        if (resolved == null) throw new IllegalStateException("Unknown block material " + key);
        return resolved;
    }
    private void setType(Pos pos, org.bukkit.Material material) {
        if (!Native.worldSetBlock(id.toString(), pos.x(), pos.y(), pos.z(), material.getKey().toString())) throw new IllegalStateException("No world command handler");
        blocks.put(pos, material);
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
            case "getLightFromSky" -> { int level = Native.skyLight(id.toString(), pos.x(), pos.y(), pos.z()); if (level < 0) throw new UnsupportedOperationException("Ferrite: no light data at " + pos); yield (byte) level; }
            case "setType" -> { setType(pos, java.util.Objects.requireNonNull((org.bukkit.Material) args[0])); yield null; }
            case "getBlockData" -> FerriteBlockData.create(typeAt(pos));
            case "setBlockData" -> { setType(pos, ((org.bukkit.block.data.BlockData) args[0]).getMaterial()); yield null; }
            case "isEmpty" -> typeAt(pos).isAir();
            case "isLiquid" -> { var type = typeAt(pos); yield type == org.bukkit.Material.WATER || type == org.bukkit.Material.LAVA; }
            case "getRelative" -> relative(pos, (org.bukkit.block.BlockFace) args[0]);
            case "hashCode" -> java.util.Objects.hash(id, pos);
            case "equals" -> self == args[0];
            case "toString" -> "Block[" + id + ":" + pos + "]";
            default -> throw new UnsupportedOperationException("Ferrite: not implemented: Block#" + method.getName());
        });
    }
    private org.bukkit.Chunk chunk(int x, int z) {
        return (org.bukkit.Chunk) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.Chunk.class}, (self, method, args) -> switch (method.getName()) {
            case "getX" -> x; case "getZ" -> z; case "getWorld" -> proxy;
            case "getBlock" -> block(new Pos((x << 4) + (int) args[0], (int) args[1], (z << 4) + (int) args[2]));
            case "hashCode" -> java.util.Objects.hash(id, x, z);
            case "equals" -> self == args[0];
            case "toString" -> "Chunk[" + id + ":" + x + "," + z + "]";
            default -> throw new UnsupportedOperationException("Ferrite: not implemented: Chunk#" + method.getName());
        });
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getUID" -> id; case "getName" -> name; case "getTime", "getFullTime" -> time;
            case "getSpawnLocation" -> location(spawn);
            case "getHighestBlockYAt" -> { if (args.length > 2 || (args.length == 2 && args[1] instanceof org.bukkit.HeightMap)) throw new UnsupportedOperationException("Ferrite: heightmap-specific queries are not implemented"); int x, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); z = l.getBlockZ(); } else { x = (int) args[0]; z = (int) args[1]; } var height = heights.get(new Column(x, z)); if (height == null) { int value = Native.highestBlockY(id.toString(), x, z); if (value == Integer.MIN_VALUE) throw new UnsupportedOperationException("Ferrite: no height data at " + x + "," + z); height = value; heights.put(new Column(x, z), height); } yield height; }
            case "getBiome" -> { int x, y, z; if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX(); y = l.getBlockY(); z = l.getBlockZ(); } else { x = (int) args[0]; y = (int) args[1]; z = (int) args[2]; } var key = Native.biomeAt(id.toString(), x, y, z); if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at " + x + "," + y + "," + z); var biome = org.bukkit.Registry.BIOME.get(org.bukkit.NamespacedKey.fromString(key)); if (biome == null) throw new IllegalStateException("Unknown biome " + key); yield biome; }
            case "getBlockAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield block(new Pos(l.getBlockX(), l.getBlockY(), l.getBlockZ())); yield block(new Pos((int) args[0], (int) args[1], (int) args[2])); }
            case "getChunkAt" -> { if (args.length == 1 && args[0] instanceof org.bukkit.Location l) yield chunk(l.getBlockX() >> 4, l.getBlockZ() >> 4); yield chunk((int) args[0], (int) args[1]); }
            case "getPlayers", "getEntities", "getLivingEntities" -> server.getOnlinePlayers().stream().filter(p -> { try { return p.getWorld() == proxy; } catch (UnsupportedOperationException | IllegalStateException e) { return false; } }).toList();
            case "hashCode" -> id.hashCode();
            case "equals" -> self == args[0];
            case "toString" -> "World[" + id + "]";
            default -> throw new UnsupportedOperationException("Ferrite: not implemented: World#" + method.getName());
        };
    }
}
