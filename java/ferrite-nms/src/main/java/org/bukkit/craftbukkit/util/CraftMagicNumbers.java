// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.util;

import dev.ferrite.nms.FerriteNmsVersion;
import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.HostVersion;
import dev.ferrite.nms.host.NmsHostBinding;
import java.util.Objects;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.data.BlockData;

/**
 * Clean-room 26.3 registry conversions. Unknown values fail loudly; they never become air or an
 * empty item. Global block-state IDs and the data version come from the attached Ferrite host,
 * not from a guessed table.
 */
public final class CraftMagicNumbers {
    private CraftMagicNumbers() {}

    public static Material getMaterial(String key) {
        Objects.requireNonNull(key, "key");
        Material material = Material.matchMaterial(key);
        if (material == null) {
            throw new IllegalArgumentException("Unknown material " + key + " in Minecraft 26.3");
        }
        return material;
    }

    public static NamespacedKey getKey(Material material) {
        return Objects.requireNonNull(material, "material").getKey();
    }

    public static Block getBlock(Material material) {
        return new Block(Objects.requireNonNull(material, "material"));
    }

    public static Item getItem(Material material) {
        return new Item(Objects.requireNonNull(material, "material"));
    }

    public static BlockState getBlockState(BlockData data) {
        Objects.requireNonNull(data, "data");
        return new BlockState(getBlock(data.getMaterial()), data.getAsString(), -1);
    }

    public static Material getMaterial(BlockData data) {
        return Objects.requireNonNull(data, "data").getMaterial();
    }

    public static Material getMaterial(Block block) {
        return Objects.requireNonNull(block, "block").getBukkitMaterial();
    }

    public static Material getMaterial(Item item) {
        return Objects.requireNonNull(item, "item").getBukkitMaterial();
    }

    public static Material getMaterial(BlockState state) {
        return Objects.requireNonNull(state, "state").getBukkitMaterial();
    }

    /** The 26.3 global block-state ID resolved by the host's registry. */
    public static int getBlockStateId(BlockData data) {
        Objects.requireNonNull(data, "data");
        return NmsHostBinding.require().registryBlockStateId(data.getAsString());
    }

    public static int getBlockStateId(BlockState state) {
        return Objects.requireNonNull(state, "state").resolveGlobalId();
    }

    /** The attached host's 26.3 data version, after the same compatibility gate as binding. */
    public static int getDataVersion() {
        HostVersion version = NmsHostBinding.require().version();
        FerriteNmsVersion.requireSupported(version);
        return version.dataVersion();
    }

    public static boolean isLegacy(Material material) {
        return Objects.requireNonNull(material, "material").isLegacy();
    }

    /** Legacy conversion needs Lane 2's {@code UnsafeValues}; it is not part of this slice. */
    public static Material fromLegacy(Material material) {
        Objects.requireNonNull(material, "material");
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.util.CraftMagicNumbers",
                "fromLegacy",
                "legacy material conversion needs Lane 2's UnsafeValues facade, which the first NMS slice does not bind");
    }
}
