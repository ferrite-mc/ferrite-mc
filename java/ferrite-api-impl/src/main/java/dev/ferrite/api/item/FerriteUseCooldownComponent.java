package dev.ferrite.api.item;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.meta.components.UseCooldownComponent;

/**
 * Facade implementation of {@link UseCooldownComponent} storing cooldown seconds and
 * optional cooldown group with defensive copying and serialization.
 */
public final class FerriteUseCooldownComponent implements UseCooldownComponent {

    private float cooldownSeconds;
    private NamespacedKey cooldownGroup;

    public FerriteUseCooldownComponent() {
    }

    public FerriteUseCooldownComponent(float cooldownSeconds, NamespacedKey cooldownGroup) {
        setCooldownSeconds(cooldownSeconds);
        this.cooldownGroup = cooldownGroup;
    }

    public FerriteUseCooldownComponent(UseCooldownComponent source) {
        Objects.requireNonNull(source, "source");
        float seconds = source.getCooldownSeconds();
        if (Float.isNaN(seconds) || Float.isInfinite(seconds) || seconds < 0.0F) {
            throw new IllegalArgumentException(
                    "cooldownSeconds must be finite and >= 0, was " + seconds);
        }
        this.cooldownSeconds = seconds;
        this.cooldownGroup = source.getCooldownGroup();
    }

    @Override
    public float getCooldownSeconds() {
        return cooldownSeconds;
    }

    @Override
    public void setCooldownSeconds(float cooldown) {
        if (Float.isNaN(cooldown) || Float.isInfinite(cooldown) || cooldown <= 0.0F) {
            throw new IllegalArgumentException(
                    "cooldownSeconds must be finite and > 0, was " + cooldown);
        }
        this.cooldownSeconds = cooldown;
    }

    @Override
    public NamespacedKey getCooldownGroup() {
        return cooldownGroup;
    }

    @Override
    public void setCooldownGroup(NamespacedKey cooldownGroup) {
        this.cooldownGroup = cooldownGroup;
    }

    public FerriteUseCooldownComponent copy() {
        return new FerriteUseCooldownComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("seconds", cooldownSeconds);
        if (cooldownGroup != null) {
            result.put("cooldown-group", cooldownGroup.toString());
        }
        return result;
    }

    public static FerriteUseCooldownComponent deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        FerriteUseCooldownComponent component = new FerriteUseCooldownComponent();
        Object seconds = args.get("seconds");
        if (seconds instanceof Number number) {
            component.setCooldownSeconds(number.floatValue());
        }
        Object group = args.get("cooldown-group");
        if (group != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(group));
            if (key == null) {
                throw new IllegalArgumentException("Invalid cooldown-group key: " + group);
            }
            component.setCooldownGroup(key);
        }
        return component;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteUseCooldownComponent other)) {
            return false;
        }
        return Float.compare(cooldownSeconds, other.cooldownSeconds) == 0
                && Objects.equals(cooldownGroup, other.cooldownGroup);
    }

    @Override
    public int hashCode() {
        return Objects.hash(cooldownSeconds, cooldownGroup);
    }

    @Override
    public String toString() {
        return "FerriteUseCooldownComponent{seconds="
                + cooldownSeconds
                + ", cooldownGroup="
                + cooldownGroup
                + "}";
    }
}
