package dev.ferrite.server;

import org.bukkit.NamespacedKey;
import org.bukkit.damage.*;

/** Data-backed registry definition; this does not apply damage to entities. */
final class FerriteDamageType implements DamageType {
    private final NamespacedKey key;
    private final FerriteDamageData.Definition data;
    FerriteDamageType(NamespacedKey key) {
        this.key = key;
        data = FerriteDamageData.canonical().definition(key);
    }
    @Override public NamespacedKey getKey() { return key; }
    @Override public String getTranslationKey() { return "death.attack." + data.message(); }
    @Override public DamageScaling getDamageScaling() { return data.scaling(); }
    @Override public DamageEffect getDamageEffect() { return data.effect(); }
    @Override public DeathMessageType getDeathMessageType() { return data.death(); }
    @Override public float getExhaustion() { return data.exhaustion(); }
    @Override public boolean equals(Object other) { return other instanceof FerriteDamageType value && key.equals(value.key); }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public String toString() { return "DamageType[" + key + "]"; }
}
