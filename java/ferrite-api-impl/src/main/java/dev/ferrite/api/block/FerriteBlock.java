package dev.ferrite.api.block;

import dev.ferrite.api.world.FerriteWorld;
import java.util.Objects;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;

/** A live block handle for the supported material read/write slice. */
public final class FerriteBlock extends UnsupportedBlockBase {

    private final FerriteWorld world;
    private final int x;
    private final int y;
    private final int z;

    public FerriteBlock(FerriteWorld world, int x, int y, int z) {
        this.world = world;
        this.x = x;
        this.y = y;
        this.z = z;
    }

    @Override
    public Material getType() {
        return world.blockStore().getType(x, y, z);
    }

    @Override
    public void setType(Material type) {
        world.blockStore().setType(x, y, z, type);
    }

    @Override
    public void setType(Material type, boolean applyPhysics) {
        setType(type);
    }

    @Override
    public org.bukkit.Chunk getChunk() {
        return world.getChunkAt(x >> 4, z >> 4);
    }

    @Override
    public Location getLocation(Location loc) {
        if (loc == null) {
            return null;
        }
        loc.setWorld(world);
        loc.setX(x);
        loc.setY(y);
        loc.setZ(z);
        loc.setYaw(0.0f);
        loc.setPitch(0.0f);
        return loc;
    }

    @Override
    public BlockFace getFace(Block block) {
        if (block == null || block.getWorld() != world) {
            return null;
        }
        int dx = block.getX() - x;
        int dy = block.getY() - y;
        int dz = block.getZ() - z;
        for (BlockFace face : BlockFace.values()) {
            if (face.getModX() == dx && face.getModY() == dy && face.getModZ() == dz) {
                return face;
            }
        }
        return null;
    }

    @Override
    public byte getLightLevel() {
        return (byte) Math.max(getLightFromSky(), getLightFromBlocks());
    }

    @Override
    public byte getLightFromSky() {
        return isEmpty() ? (byte) 15 : (byte) 0;
    }

    @Override
    public byte getLightFromBlocks() {
        Material m = getType();
        if (m == Material.TORCH || m == Material.WALL_TORCH || m == Material.GLOWSTONE
                || m == Material.SEA_LANTERN || m == Material.LANTERN || m == Material.LAVA
                || m == Material.SHROOMLIGHT || m == Material.JACK_O_LANTERN) {
            return 15;
        }
        return 0;
    }

    @Override
    public boolean isBlockPowered() {
        return getType() == Material.REDSTONE_BLOCK || getType() == Material.REDSTONE_TORCH;
    }

    @Override
    public boolean isBlockIndirectlyPowered() {
        if (isBlockPowered()) {
            return true;
        }
        for (BlockFace f : new BlockFace[] {
            BlockFace.NORTH, BlockFace.SOUTH, BlockFace.EAST, BlockFace.WEST, BlockFace.UP, BlockFace.DOWN
        }) {
            if (getRelative(f).isBlockPowered()) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean isBlockFacePowered(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return getRelative(face).isBlockPowered();
    }

    @Override
    public boolean isBlockFaceIndirectlyPowered(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return getRelative(face).isBlockIndirectlyPowered();
    }

    @Override
    public int getBlockPower(BlockFace face) {
        Objects.requireNonNull(face, "face");
        return isBlockFacePowered(face) ? 15 : 0;
    }

    @Override
    public boolean isPassable() {
        Material m = getType();
        return m.isAir()
                || m == Material.WATER
                || m == Material.LAVA
                || m == Material.TORCH
                || m == Material.WALL_TORCH
                || m == Material.SOUL_TORCH
                || m == Material.SOUL_WALL_TORCH
                || m == Material.REDSTONE_TORCH
                || m == Material.REDSTONE_WALL_TORCH
                || m == Material.FIRE
                || m == Material.SOUL_FIRE
                || m == Material.SHORT_GRASS
                || m == Material.TALL_GRASS
                || m == Material.FERN
                || m == Material.DANDELION
                || m == Material.POPPY
                || m == Material.VINE
                || m == Material.RAIL
                || m == Material.POWERED_RAIL
                || m == Material.DETECTOR_RAIL
                || m == Material.ACTIVATOR_RAIL
                || m == Material.COBWEB;
    }

    @Override
    public boolean isSolid() {
        Material m = getType();
        return m.isBlock() && !isPassable();
    }

    @Override
    public boolean isCollidable() {
        return !isPassable();
    }

    @Override
    public boolean isBuildable() {
        return isSolid();
    }

    @Override
    public boolean isBurnable() {
        return getType().isBurnable();
    }

    @Override
    public boolean isReplaceable() {
        Material m = getType();
        return m.isAir() || m == Material.WATER || m == Material.LAVA;
    }

    @Override
    public org.bukkit.block.Biome getBiome() {
        return world.getBiome(x, y, z);
    }

    @Override
    public org.bukkit.block.Biome getComputedBiome() {
        return getBiome();
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops() {
        return getDrops(null, null);
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops(org.bukkit.inventory.ItemStack tool) {
        return getDrops(tool, null);
    }

    @Override
    public java.util.Collection<org.bukkit.inventory.ItemStack> getDrops(
            org.bukkit.inventory.ItemStack tool, org.bukkit.entity.Entity entity) {
        Material m = getType();
        if (m.isAir() || !m.isItem()) {
            return java.util.List.of();
        }
        return java.util.List.of(new org.bukkit.inventory.ItemStack(m, 1));
    }

    @Override
    public byte getData() {
        return 0;
    }

    @Override
    public int getX() {
        return x;
    }

    @Override
    public int getY() {
        return y;
    }

    @Override
    public int getZ() {
        return z;
    }

    @Override
    public World getWorld() {
        return world;
    }

    @Override
    public Location getLocation() {
        return new Location(world, x, y, z);
    }

    @Override
    public BlockData getBlockData() {
        return org.bukkit.Bukkit.createBlockData(getType());
    }

    @Override
    public void setBlockData(BlockData data) {
        setBlockData(data, true);
    }

    @Override
    public void setBlockData(BlockData data, boolean applyPhysics) {
        Objects.requireNonNull(data, "data");
        setType(data.getMaterial());
    }

    @Override
    public Block getRelative(int modX, int modY, int modZ) {
        return new FerriteBlock(world, x + modX, y + modY, z + modZ);
    }

    @Override
    public Block getRelative(BlockFace face) {
        return getRelative(face, 1);
    }

    @Override
    public Block getRelative(BlockFace face, int distance) {
        Objects.requireNonNull(face, "face");
        return getRelative(face.getModX() * distance, face.getModY() * distance, face.getModZ() * distance);
    }

    @Override
    public boolean isEmpty() {
        return getType().isAir();
    }

    @Override
    public boolean isLiquid() {
        Material type = getType();
        return type == Material.WATER || type == Material.LAVA;
    }

    @Override
    public BlockState getState() {
        return getState(true);
    }

    @Override
    public BlockState getState(boolean useSnapshot) {
        return new FerriteBlockState(this);
    }

    @Override
    public boolean breakNaturally() {
        return breakNaturally(null);
    }

    @Override
    public boolean breakNaturally(org.bukkit.inventory.ItemStack tool) {
        Material mat = getType();
        if (mat.isAir()) {
            return false;
        }
        setType(Material.AIR);
        if (mat.isItem()) {
            world.dropItemNaturally(getLocation().add(0.5, 0.5, 0.5), new org.bukkit.inventory.ItemStack(mat, 1));
        }
        return true;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteBlock other
                && other.world == world
                && other.x == x
                && other.y == y
                && other.z == z;
    }

    @Override
    public int hashCode() {
        return (world.getName().hashCode() * 31 + x) * 31 * 31 + y * 31 + z;
    }

    @Override
    public String toString() {
        return "FerriteBlock{" + world.getName() + ":" + x + "," + y + "," + z + "}";
    }
}
