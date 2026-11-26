package dev.ferrite.api.world;

import java.util.Map;
import java.util.Objects;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Material;

/**
 * Standalone block storage for the facade tests and for the first vertical slice.
 *
 * <p>Key packing follows the vanilla chunk-section layout: 12 bits of Y, 26 bits of X and
 * 26 bits of Z. Only material identity is stored; block data/components are not part of
 * this slice.
 */
public final class InMemoryBlockStore implements BlockStore {

    private final Map<Long, Material> blocks = new ConcurrentHashMap<>();
    private final Material defaultBlock;

    public InMemoryBlockStore(Material defaultBlock) {
        this.defaultBlock = Objects.requireNonNull(defaultBlock, "defaultBlock");
    }

    @Override
    public Material getType(int x, int y, int z) {
        return this.blocks.getOrDefault(key(x, y, z), this.defaultBlock);
    }

    @Override
    public void setType(int x, int y, int z, Material material) {
        Objects.requireNonNull(material, "material");
        // Material#isBlock needs the Paper block registry, which the host/world layer owns;
        // this store intentionally accepts any material for the facade slice.
        if (material == this.defaultBlock) {
            this.blocks.remove(key(x, y, z));
        } else {
            this.blocks.put(key(x, y, z), material);
        }
    }

    public int storedCount() {
        return this.blocks.size();
    }

    private static long key(int x, int y, int z) {
        return ((long) (x & 0x3FFFFFF) << 38) | ((long) (z & 0x3FFFFFF) << 12) | (y & 0xFFF);
    }
}
