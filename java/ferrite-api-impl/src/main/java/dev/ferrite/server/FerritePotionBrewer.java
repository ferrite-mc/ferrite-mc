package dev.ferrite.server;

import io.papermc.paper.potion.PotionMix;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.Map;
import org.bukkit.NamespacedKey;
import org.bukkit.potion.PotionBrewer;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionType;

/**
 * Potion brewing (Server#getPotionBrewer).
 *
 * <p>Plugin mixes are recipe-managed like CraftBukkit does, so they show up in
 * {@link org.bukkit.Server#getRecipe} and can be removed again. Vanilla potion effects come from
 * {@link FerritePotionTypeData}, which backs {@code UnsafeValues#getInternalPotionData}.
 */
final class FerritePotionBrewer implements PotionBrewer {
    /** Mixes added through this brewer, so {@link #resetPotionMixes()} can remove exactly those. */
    private final Map<NamespacedKey, PotionMix> custom = new LinkedHashMap<>();

    @Override public synchronized void addPotionMix(PotionMix mix) {
        java.util.Objects.requireNonNull(mix, "mix");
        PotionBrewer.super.addPotionMix(mix);
        custom.put(mix.getKey(), mix);
    }

    @Override public synchronized void removePotionMix(NamespacedKey key) {
        java.util.Objects.requireNonNull(key, "key");
        PotionBrewer.super.removePotionMix(key);
        custom.remove(key);
    }

    @Override public synchronized void resetPotionMixes() {
        for (var key : java.util.List.copyOf(custom.keySet())) PotionBrewer.super.removePotionMix(key);
        custom.clear();
    }

    @Override public Collection<PotionEffect> getEffects(PotionType type, boolean upgrade, boolean extended) {
        java.util.Objects.requireNonNull(type, "type");
        if (upgrade && extended) throw new IllegalArgumentException("A potion cannot be both upgraded and extended");
        if (upgrade) {
            if (!type.isUpgradeable()) throw new IllegalArgumentException(type + " cannot be upgraded");
            type = variant("STRONG_" + type.name(), type);
        } else if (extended) {
            if (!type.isExtendable()) throw new IllegalArgumentException(type + " cannot be extended");
            type = variant("LONG_" + type.name(), type);
        }
        return java.util.List.copyOf(type.getPotionEffects());
    }

    /** Vanilla stores the strong and long variants as separate enum values. */
    private static PotionType variant(String name, PotionType base) {
        try {
            return PotionType.valueOf(name);
        } catch (IllegalArgumentException missing) {
            throw new IllegalArgumentException(base + " has no " + name + " variant", missing);
        }
    }
}
