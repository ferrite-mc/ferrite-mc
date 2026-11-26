package dev.ferrite.api.entity;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import net.kyori.adventure.key.Key;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeInstance;
import org.bukkit.attribute.AttributeModifier;

/** Mutable {@link AttributeInstance} supporting vanilla modifier math for Paper/Spigot plugins. */
public final class FerriteAttributeInstance implements AttributeInstance {

    private final Attribute attribute;
    private final double defaultValue;
    private double baseValue;
    private final Map<Key, AttributeModifier> modifiers = new LinkedHashMap<>();

    public FerriteAttributeInstance(Attribute attribute, double defaultValue) {
        this.attribute = Objects.requireNonNull(attribute, "attribute");
        this.defaultValue = defaultValue;
        this.baseValue = defaultValue;
    }

    @Override
    public Attribute getAttribute() {
        return attribute;
    }

    @Override
    public double getBaseValue() {
        return baseValue;
    }

    @Override
    public void setBaseValue(double value) {
        this.baseValue = value;
    }

    @Override
    public Collection<AttributeModifier> getModifiers() {
        return Collections.unmodifiableList(new ArrayList<>(modifiers.values()));
    }

    @Override
    public void addModifier(AttributeModifier modifier) {
        Objects.requireNonNull(modifier, "modifier");
        Key key = modifier.getKey();
        if (modifiers.containsKey(key)) {
            throw new IllegalArgumentException("Modifier already applied: " + key);
        }
        modifiers.put(key, modifier);
    }

    @Override
    public void addTransientModifier(AttributeModifier modifier) {
        addModifier(modifier);
    }

    @Override
    public void removeModifier(AttributeModifier modifier) {
        Objects.requireNonNull(modifier, "modifier");
        modifiers.remove(modifier.getKey());
    }

    @Override
    @SuppressWarnings("removal")
    public void removeModifier(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        modifiers.values().removeIf(m -> uuid.equals(m.getUniqueId()));
    }

    @Override
    public void removeModifier(Key key) {
        Objects.requireNonNull(key, "key");
        modifiers.remove(key);
    }

    @Override
    @SuppressWarnings("removal")
    public AttributeModifier getModifier(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        for (AttributeModifier m : modifiers.values()) {
            if (uuid.equals(m.getUniqueId())) {
                return m;
            }
        }
        return null;
    }

    @Override
    public AttributeModifier getModifier(Key key) {
        Objects.requireNonNull(key, "key");
        return modifiers.get(key);
    }

    @Override
    public double getValue() {
        double add = baseValue;
        for (AttributeModifier m : modifiers.values()) {
            if (m.getOperation() == AttributeModifier.Operation.ADD_NUMBER) {
                add += m.getAmount();
            }
        }
        double total = add;
        for (AttributeModifier m : modifiers.values()) {
            if (m.getOperation() == AttributeModifier.Operation.ADD_SCALAR) {
                total += add * m.getAmount();
            }
        }
        for (AttributeModifier m : modifiers.values()) {
            if (m.getOperation() == AttributeModifier.Operation.MULTIPLY_SCALAR_1) {
                total *= 1.0 + m.getAmount();
            }
        }
        return total;
    }

    @Override
    public double getDefaultValue() {
        return defaultValue;
    }
}
