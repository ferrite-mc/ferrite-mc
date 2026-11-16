package dev.ferrite.api.item;

import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.Sound;
import org.bukkit.Tag;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.meta.components.EquippableComponent;

/**
 * Facade implementation of {@link EquippableComponent} storing equipment slot, sounds,
 * model/overlay keys, allowed entities and boolean flags with defensive copying and
 * serialization.
 */
public final class FerriteEquippableComponent implements EquippableComponent {

    private EquipmentSlot slot = EquipmentSlot.HEAD;
    private Sound equipSound;
    private NamespacedKey model;
    private NamespacedKey cameraOverlay;
    private Set<EntityType> allowedEntities;
    private boolean dispensable = true;
    private boolean swappable = true;
    private boolean damageOnHurt = true;
    private boolean equipOnInteract;
    private boolean canBeSheared;
    private Sound shearingSound;

    public FerriteEquippableComponent() {
    }

    public FerriteEquippableComponent(EquippableComponent source) {
        Objects.requireNonNull(source, "source");
        setSlot(source.getSlot());
        this.equipSound = source.getEquipSound();
        this.model = source.getModel();
        this.cameraOverlay = source.getCameraOverlay();
        Collection<EntityType> allowed = source.getAllowedEntities();
        if (allowed != null) {
            this.allowedEntities = new LinkedHashSet<>(allowed);
        }
        this.dispensable = source.isDispensable();
        this.swappable = source.isSwappable();
        this.damageOnHurt = source.isDamageOnHurt();
        this.equipOnInteract = source.isEquipOnInteract();
        this.canBeSheared = source.canBeSheared();
        this.shearingSound = source.getShearingSound();
    }

    @Override
    public EquipmentSlot getSlot() {
        return slot;
    }

    @Override
    public void setSlot(EquipmentSlot slot) {
        this.slot = Objects.requireNonNull(slot, "slot");
    }

    @Override
    public Sound getEquipSound() {
        return equipSound;
    }

    @Override
    public void setEquipSound(Sound sound) {
        this.equipSound = sound;
    }

    @Override
    public NamespacedKey getModel() {
        return model;
    }

    @Override
    public void setModel(NamespacedKey key) {
        this.model = key;
    }

    @Override
    public NamespacedKey getCameraOverlay() {
        return cameraOverlay;
    }

    @Override
    public void setCameraOverlay(NamespacedKey key) {
        this.cameraOverlay = key;
    }

    @Override
    public Collection<EntityType> getAllowedEntities() {
        return allowedEntities == null
                ? null
                : Collections.unmodifiableSet(new LinkedHashSet<>(allowedEntities));
    }

    @Override
    public void setAllowedEntities(EntityType entities) {
        if (entities == null) {
            this.allowedEntities = null;
            return;
        }
        this.allowedEntities = new LinkedHashSet<>(Set.of(entities));
    }

    @Override
    public void setAllowedEntities(Collection<EntityType> entities) {
        if (entities == null) {
            this.allowedEntities = null;
            return;
        }
        Set<EntityType> copy = new LinkedHashSet<>(entities.size());
        for (EntityType type : entities) {
            copy.add(Objects.requireNonNull(type, "allowedEntity"));
        }
        this.allowedEntities = copy;
    }

    @Override
    public void setAllowedEntities(Tag<EntityType> tag) {
        if (tag == null) {
            this.allowedEntities = null;
            return;
        }
        setAllowedEntities(tag.getValues());
    }

    @Override
    public boolean isDispensable() {
        return dispensable;
    }

    @Override
    public void setDispensable(boolean dispensable) {
        this.dispensable = dispensable;
    }

    @Override
    public boolean isSwappable() {
        return swappable;
    }

    @Override
    public void setSwappable(boolean swappable) {
        this.swappable = swappable;
    }

    @Override
    public boolean isDamageOnHurt() {
        return damageOnHurt;
    }

    @Override
    public void setDamageOnHurt(boolean damage) {
        this.damageOnHurt = damage;
    }

    @Override
    public boolean isEquipOnInteract() {
        return equipOnInteract;
    }

    @Override
    public void setEquipOnInteract(boolean equip) {
        this.equipOnInteract = equip;
    }

    @Override
    public boolean canBeSheared() {
        return canBeSheared;
    }

    @Override
    public void setCanBeSheared(boolean sheared) {
        this.canBeSheared = sheared;
    }

    @Override
    public Sound getShearingSound() {
        return shearingSound;
    }

    @Override
    public void setShearingSound(Sound sound) {
        this.shearingSound = sound;
    }

    public FerriteEquippableComponent copy() {
        return new FerriteEquippableComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("slot", slot.name());
        if (equipSound != null) {
            result.put("equip-sound", equipSound.getKey().toString());
        }
        if (model != null) {
            result.put("model", model.toString());
        }
        if (cameraOverlay != null) {
            result.put("camera-overlay", cameraOverlay.toString());
        }
        if (allowedEntities != null) {
            result.put("allowed-entities", allowedEntities.stream().map(EntityType::name).toList());
        }
        result.put("dispensable", dispensable);
        result.put("swappable", swappable);
        result.put("damage-on-hurt", damageOnHurt);
        result.put("equip-on-interact", equipOnInteract);
        result.put("can-be-sheared", canBeSheared);
        if (shearingSound != null) {
            result.put("shearing-sound", shearingSound.getKey().toString());
        }
        return result;
    }

    public static FerriteEquippableComponent deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        FerriteEquippableComponent component = new FerriteEquippableComponent();
        if (args.get("slot") != null) {
            component.setSlot(EquipmentSlot.valueOf(String.valueOf(args.get("slot"))));
        }
        if (args.get("equip-sound") != null) {
            component.setEquipSound(resolveSound(String.valueOf(args.get("equip-sound"))));
        }
        if (args.get("model") != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(args.get("model")));
            if (key == null) {
                throw new IllegalArgumentException("Invalid equippable model key: " + args.get("model"));
            }
            component.setModel(key);
        }
        if (args.get("camera-overlay") != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(args.get("camera-overlay")));
            if (key == null) {
                throw new IllegalArgumentException(
                        "Invalid camera-overlay key: " + args.get("camera-overlay"));
            }
            component.setCameraOverlay(key);
        }
        if (args.get("allowed-entities") instanceof Collection<?> list) {
            Set<EntityType> entities = new LinkedHashSet<>();
            for (Object entry : list) {
                entities.add(EntityType.valueOf(String.valueOf(entry)));
            }
            component.setAllowedEntities(entities);
        }
        if (args.get("dispensable") instanceof Boolean b) {
            component.setDispensable(b);
        }
        if (args.get("swappable") instanceof Boolean b) {
            component.setSwappable(b);
        }
        if (args.get("damage-on-hurt") instanceof Boolean b) {
            component.setDamageOnHurt(b);
        }
        if (args.get("equip-on-interact") instanceof Boolean b) {
            component.setEquipOnInteract(b);
        }
        if (args.get("can-be-sheared") instanceof Boolean b) {
            component.setCanBeSheared(b);
        }
        if (args.get("shearing-sound") != null) {
            component.setShearingSound(resolveSound(String.valueOf(args.get("shearing-sound"))));
        }
        return component;
    }

    private static Sound resolveSound(String keyText) {
        NamespacedKey key = NamespacedKey.fromString(keyText);
        if (key == null) {
            throw new IllegalArgumentException("Invalid sound key: " + keyText);
        }
        Sound sound = Registry.SOUND_EVENT.get(key);
        if (sound == null) {
            throw new UnsupportedOperationException(
                    "EquippableComponent#deserialize: unknown sound '" + key + "'");
        }
        return sound;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteEquippableComponent other)) {
            return false;
        }
        return dispensable == other.dispensable
                && swappable == other.swappable
                && damageOnHurt == other.damageOnHurt
                && equipOnInteract == other.equipOnInteract
                && canBeSheared == other.canBeSheared
                && slot == other.slot
                && Objects.equals(equipSound, other.equipSound)
                && Objects.equals(model, other.model)
                && Objects.equals(cameraOverlay, other.cameraOverlay)
                && Objects.equals(allowedEntities, other.allowedEntities)
                && Objects.equals(shearingSound, other.shearingSound);
    }

    @Override
    public int hashCode() {
        return Objects.hash(
                slot,
                equipSound,
                model,
                cameraOverlay,
                allowedEntities,
                dispensable,
                swappable,
                damageOnHurt,
                equipOnInteract,
                canBeSheared,
                shearingSound);
    }
}
