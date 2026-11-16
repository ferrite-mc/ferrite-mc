package dev.ferrite.api.registry;

import java.util.Locale;
import net.kyori.adventure.text.Component;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;

/** Registry-backed {@link Attribute} value for Paper 26.3. */
public final class FerriteAttribute implements Attribute {

    private final NamespacedKey key;
    private final int ordinal;

    public FerriteAttribute(NamespacedKey key, int ordinal) {
        this.key = key;
        this.ordinal = ordinal;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public Sentiment getSentiment() {
        String id = this.key.getKey();
        if (id.contains("burning_time") || id.contains("fall_damage")) {
            return Sentiment.NEGATIVE;
        }
        return Sentiment.POSITIVE;
    }

    @Override
    public double getDefaultValue() {
        return 0.0;
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
    public String translationKey() {
        return "attribute.name." + this.key.getKey();
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getTranslationKey() {
        return translationKey();
    }

    @Override
    public int compareTo(Attribute other) {
        return this.key.toString().compareTo(other.getKey().toString());
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof Attribute other && this.key.equals(other.getKey());
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
