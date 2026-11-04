package dev.ferrite.server;

/** Stand-ins for Bukkit event objects fired from the host. */
final class FerriteEventObjects {
    private FerriteEventObjects() {}

    /** Vanilla's direction order: down, up, north, south, west, east. */
    private static final org.bukkit.block.BlockFace[] FACES = {
        org.bukkit.block.BlockFace.DOWN,
        org.bukkit.block.BlockFace.UP,
        org.bukkit.block.BlockFace.NORTH,
        org.bukkit.block.BlockFace.SOUTH,
        org.bukkit.block.BlockFace.WEST,
        org.bukkit.block.BlockFace.EAST,
    };

    static org.bukkit.block.BlockFace face(int index) { return FACES[Math.floorMod(index, FACES.length)]; }

    /** A block that knows its position, world and type; the host sends no other block data. */
    static org.bukkit.block.Block block(int x, int y, int z, org.bukkit.Material type) {
        return block(Bootstrap.defaultWorld(), x, y, z, type);
    }

    static org.bukkit.block.Block block(org.bukkit.World world, int x, int y, int z, org.bukkit.Material type) {
        // Publish the type the host sent so later getType/setType calls on the same block work.
        FerriteWorld.setSnapshot(world, x, y, z, type);
        return world.getBlockAt(x, y, z);
    }

    /** A chunk proxy in the default world; heightmaps and entities are not published. */
    static org.bukkit.Chunk chunk(org.bukkit.World world, int x, int z) {
        return FerriteWorld.chunkAt(world, x, z);
    }

    /** The replaced block state of a {@code BlockPlaceEvent}; identity and type only. */
    static org.bukkit.block.BlockState state(org.bukkit.block.Block block, org.bukkit.Material type) {
        return (org.bukkit.block.BlockState) java.lang.reflect.Proxy.newProxyInstance(FerriteEventObjects.class.getClassLoader(), new Class<?>[]{org.bukkit.block.BlockState.class}, (self, method, args) -> switch (method.getName()) {
            case "getBlock" -> block;
            case "getX" -> block.getX();
            case "getY" -> block.getY();
            case "getZ" -> block.getZ();
            case "getWorld" -> block.getWorld();
            case "getLocation" -> block.getLocation();
            case "getType" -> type;
            case "getBlockData" -> FerriteBlockData.create(type);
            case "hashCode" -> block.hashCode();
            case "equals" -> self == args[0];
            case "toString" -> "BlockState[" + block + "]";
            default -> HostBridge.dispatch("BlockState", "", method, args);
        });
    }

    /** A living entity the host does not model beyond identity, type and location. */
    static org.bukkit.entity.LivingEntity living(java.util.UUID id, org.bukkit.entity.EntityType type, org.bukkit.Location location, double health) {
        return (org.bukkit.entity.LivingEntity) java.lang.reflect.Proxy.newProxyInstance(FerriteEventObjects.class.getClassLoader(), new Class<?>[]{org.bukkit.entity.LivingEntity.class}, (self, method, args) -> switch (method.getName()) {
            case "getUniqueId" -> id;
            case "getType" -> type;
            case "getWorld" -> location.getWorld();
            case "getLocation" -> location;
            case "getHealth" -> health;
            case "getEntityId" -> id.hashCode();
            case "isValid", "isDead" -> false;
            case "hashCode" -> id.hashCode();
            case "equals" -> args[0] instanceof org.bukkit.entity.Entity other && id.equals(other.getUniqueId());
            case "toString" -> "LivingEntity[" + type + ":" + id + "]";
            default -> HostBridge.dispatch("LivingEntity", "", method, args);
        });
    }
}
