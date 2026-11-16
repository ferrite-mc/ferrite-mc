// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.level.block.state;

import dev.ferrite.nms.host.NmsHostBinding;
import java.util.Objects;
import net.minecraft.world.level.block.Block;
import org.bukkit.Material;

/**
 * Immutable block-state value. The global state ID is resolved through the host registry when
 * this state was constructed from a Bukkit {@code BlockData} rather than read from a world.
 */
public final class BlockState {
    private final Block block;
    private final String asString;
    private final int globalId;

    public BlockState(Block block, String asString, int globalId) {
        this.block = Objects.requireNonNull(block, "block");
        this.asString = Objects.requireNonNull(asString, "asString");
        if (globalId < -1) {
            throw new IllegalArgumentException("globalId must be -1 (unresolved) or non-negative");
        }
        String blockName = block.getName();
        if (!asString.equals(blockName) && !asString.startsWith(blockName + "[")) {
            throw new IllegalArgumentException(
                    "Block state " + asString + " does not belong to " + blockName);
        }
        this.globalId = globalId;
    }

    public Block getBlock() {
        return block;
    }

    public Material getBukkitMaterial() {
        return block.getBukkitMaterial();
    }

    public String getAsString() {
        return asString;
    }

    public int getGlobalId() {
        return globalId;
    }

    public boolean hasGlobalId() {
        return globalId >= 0;
    }

    public boolean isAir() {
        return block.getBukkitMaterial() == Material.AIR;
    }

    /** The 26.3 global state ID, asking the host registry when it was not in a world snapshot. */
    public int resolveGlobalId() {
        if (globalId >= 0) {
            return globalId;
        }
        return NmsHostBinding.require().registryBlockStateId(asString);
    }

    @Override
    public boolean equals(Object other) {
        return this == other || other instanceof BlockState state && asString.equals(state.asString);
    }

    @Override
    public int hashCode() {
        return asString.hashCode();
    }

    @Override
    public String toString() {
        return "BlockState{" + asString + (globalId >= 0 ? ", id=" + globalId : "") + "}";
    }
}
