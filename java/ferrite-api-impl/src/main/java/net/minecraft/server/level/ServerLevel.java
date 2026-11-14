package net.minecraft.server.level;

import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunk;

/**
 * The NMS level for exactly one Bukkit world. A handle is created once per world and cached, so
 * two calls for the same world see the same object and a handle can never read another world's
 * state: every method calls the wrapped world, whose reads the owning region answers.
 */
public class ServerLevel extends net.minecraft.world.level.Level {
    private static final java.util.Map<java.util.UUID, ServerLevel> HANDLES = new java.util.concurrent.ConcurrentHashMap<>();

    /** The one handle for {@code world}. */
    public static ServerLevel forWorld(org.bukkit.World world) {
        java.util.Objects.requireNonNull(world, "world");
        return HANDLES.compute(world.getUID(), (id, existing) ->
                existing != null && existing.getWorld().equals(world) ? existing : new ServerLevel(world));
    }

    public ServerLevel(org.bukkit.World world) { super(world); }

    public org.bukkit.World getWorld() { return world; }
    public java.util.UUID getUUID() { return world.getUID(); }
    public String getName() { return world.getName(); }
    public org.bukkit.Difficulty getDifficulty() { return world.getDifficulty(); }
    public org.bukkit.WorldBorder getWorldBorder() { return world.getWorldBorder(); }
    public net.minecraft.server.MinecraftServer getServer() { return net.minecraft.server.MinecraftServer.get(); }

    public java.util.List<ServerPlayer> players() {
        java.util.List<ServerPlayer> list = new java.util.ArrayList<>();
        for (org.bukkit.entity.Player p : world.getPlayers()) if (p instanceof org.bukkit.craftbukkit.entity.CraftPlayer c) list.add(c.getHandle());
        return list;
    }

    public ServerPlayer getPlayerByUUID(java.util.UUID id) {
        var player = world.getPlayers().stream().filter(p -> p.getUniqueId().equals(id)).findFirst().orElse(null);
        return player instanceof org.bukkit.craftbukkit.entity.CraftPlayer c ? c.getHandle() : null;
    }

    public long getSeed() { return world.getSeed(); }
    public net.minecraft.core.BlockPos getSharedSpawnPos() { var l = world.getSpawnLocation(); return new net.minecraft.core.BlockPos(l.getBlockX(), l.getBlockY(), l.getBlockZ()); }
    public String dimensionKey() { return world.getKey().toString(); }

    /**
     * The live state at {@code pos}; the read goes through the world that owns it. A position the
     * Java diff has never seen is air, matching vanilla's unloaded-chunk reads, not another world.
     */
    public BlockState getBlockState(BlockPos pos) {
        try {
            return new BlockState(world.getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getBlockData());
        } catch (UnsupportedOperationException unknownPosition) {
            return new BlockState(org.bukkit.Material.AIR.createBlockData());
        }
    }
    public BlockState getBlockState(int x, int y, int z) { return getBlockState(new BlockPos(x, y, z)); }

    /** A view of the block entity at {@code pos}; it holds no copy of the tile data. */
    public BlockEntity getBlockEntity(BlockPos pos) { return new BlockEntity(world, pos); }

    public LevelChunk getChunk(int x, int z) { return new LevelChunk(world.getChunkAt(x, z)); }
    public LevelChunk getChunkAt(BlockPos pos) { return getChunk(pos.getX() >> 4, pos.getZ() >> 4); }

    public java.util.List<Entity> getAllEntities() {
        var out = new java.util.ArrayList<Entity>();
        for (var e : world.getEntities()) out.add(Entity.of(e));
        return out;
    }
    public Entity getEntity(java.util.UUID id) { var e = world.getEntity(id); return e == null ? null : Entity.of(e); }
}
