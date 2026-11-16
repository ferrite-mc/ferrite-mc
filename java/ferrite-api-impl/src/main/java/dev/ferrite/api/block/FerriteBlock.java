package dev.ferrite.api.block;

import dev.ferrite.api.world.FerriteWorld;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;

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
