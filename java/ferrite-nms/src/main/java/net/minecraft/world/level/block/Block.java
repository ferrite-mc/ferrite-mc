// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.level.block;

import java.util.Objects;
import net.minecraft.world.level.block.state.BlockState;
import org.bukkit.Material;

/**
 * Clean-room block wrapper: it holds only a Bukkit material key and never a Rust world pointer.
 * The real block data lives in the host and is reached through {@code ServerLevel}.
 */
public final class Block {
    private final Material material;

    public Block(Material material) {
        this.material = Objects.requireNonNull(material, "material");
        if (!material.isBlock()) {
            throw new IllegalArgumentException(material + " is not a block in Minecraft 26.3");
        }
    }

    public Material getBukkitMaterial() {
        return material;
    }

    public String getName() {
        return material.getKey().toString();
    }

    /**
     * The default 26.3 state for this block. Paper creates the property set; without the Bukkit
     * facade there is no honest way to invent the default properties, so the failure is explicit.
     */
    public BlockState defaultBlockState() {
        try {
            return new BlockState(this, material.createBlockData().getAsString(), -1);
        } catch (RuntimeException error) {
            throw new IllegalStateException(
                    "Cannot resolve the 26.3 default block state for " + material
                            + " without the Bukkit facade's BlockData factory",
                    error);
        }
    }

    @Override
    public boolean equals(Object other) {
        return this == other || other instanceof Block block && material == block.material;
    }

    @Override
    public int hashCode() {
        return material.hashCode();
    }

    @Override
    public String toString() {
        return "Block{" + getName() + "}";
    }
}
