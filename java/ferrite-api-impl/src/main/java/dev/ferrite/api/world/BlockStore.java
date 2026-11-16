package dev.ferrite.api.world;

import java.util.Objects;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;

/**
 * Where the facade reads and writes block state.
 *
 * <p>This is an in-process seam, not a wire op. Until Lane 1 freezes the host contract,
 * the facade ships {@link InMemoryBlockStore}; once the contract exists a host adapter can
 * implement this interface over the owner-region block ops without changing any Bukkit
 * calling code.
 */
public interface BlockStore {

    Material getType(int x, int y, int z);

    void setType(int x, int y, int z, Material material);

    /** Convenience used by tests and the in-memory implementation. */
    default Material getType(Block block) {
        return getType(block.getX(), block.getY(), block.getZ());
    }

    /** Convenience used by tests and the in-memory implementation. */
    default void setType(Block block, Material material) {
        Objects.requireNonNull(block, "block");
        setType(block.getX(), block.getY(), block.getZ(), material);
    }

    static BlockStore inMemory(Material defaultBlock, World world) {
        return new InMemoryBlockStore(defaultBlock);
    }
}
