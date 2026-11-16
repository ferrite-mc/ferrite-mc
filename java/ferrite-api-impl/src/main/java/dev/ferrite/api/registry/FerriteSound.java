package dev.ferrite.api.registry;

import java.util.Locale;
import net.kyori.adventure.key.Key;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;

/** Registry-backed {@link Sound} value for Paper 26.3. */
public final class FerriteSound implements Sound {

    private final NamespacedKey key;
    private final int ordinal;

    public FerriteSound(NamespacedKey key, int ordinal) {
        this.key = key;
        this.ordinal = ordinal;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public Key key() {
        return this.key;
    }

    @Override
    public String name() {
        return this.key.getKey().toUpperCase(Locale.ROOT).replace('.', '_');
    }

    @Override
    public int ordinal() {
        return this.ordinal;
    }

    @Override
    public int compareTo(Sound other) {
        return this.key.toString().compareTo(other.getKey().toString());
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof Sound other && this.key.equals(other.getKey());
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return this.key.toString();
    }
}
