package dev.ferrite.server;

import java.util.Set;
import org.bukkit.FeatureFlag;
import org.bukkit.GameRule;
import org.bukkit.NamespacedKey;

/** An immutable canonical rule definition, independent of any world's current rule value. */
final class FerriteGameRule<T> extends GameRule<T> {
    private final NamespacedKey key;
    private final Class<T> type;
    private final T defaultValue;
    private final String translation;
    private final Set<FeatureFlag> features;

    private FerriteGameRule(FerriteGameRuleData.Definition definition, Class<T> type) {
        key = definition.key();
        this.type = type;
        defaultValue = type.cast(definition.defaultValue());
        translation = definition.translationKey();
        features = Set.copyOf(definition.features());
    }

    static GameRule<?> create(FerriteGameRuleData.Definition definition) {
        if (definition.type() == Boolean.class) return new FerriteGameRule<>(definition, Boolean.class);
        if (definition.type() == Integer.class) return new FerriteGameRule<>(definition, Integer.class);
        throw new IllegalArgumentException("Unsupported game-rule type " + definition.type());
    }

    @Override public NamespacedKey getKey() { return key; }
    @Override public String getName() { return key.toString(); }
    @Override public Class<T> getType() { return type; }
    @Override public T getDefaultValue() { return defaultValue; }
    @Override public String translationKey() { return translation; }
    @Override public Set<FeatureFlag> requiredFeatures() { return features; }
    @Override public boolean equals(Object other) { return other instanceof FerriteGameRule<?> rule && key.equals(rule.key); }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public String toString() { return "GameRule[" + key + "]"; }
}
