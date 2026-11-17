package dev.ferrite.api.block;

import dev.ferrite.api.internal.Unsupported;
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
        throw Unsupported.exception("Block#getState");
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
