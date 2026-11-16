package dev.ferrite.api.registry;

import java.util.Locale;
import org.bukkit.NamespacedKey;
import org.bukkit.block.Biome;

/** A biome backed by a {@code data/minecraft/worldgen/biome/<name>.json} entry. */
public final class FerriteBiome implements Biome {

    private final NamespacedKey key;
    private final int ordinal;

    public FerriteBiome(NamespacedKey key, int ordinal) {
        this.key = key;
        this.ordinal = ordinal;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public String translationKey() {
        return "biome." + this.key;
    }

    @Override
    public String name() {
        return this.key.getKey().toUpperCase(Locale.ROOT);
    }

    @Override
    public int ordinal() {
        return this.ordinal;
    }

    @Override
    public int compareTo(Biome other) {
        if (other instanceof FerriteBiome biome) {
            return Integer.compare(this.ordinal, biome.ordinal);
        }
        return toString().compareTo(String.valueOf(other));
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteBiome biome && this.key.equals(biome.key);
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return "FerriteBiome[" + this.key + "]";
    }
}
