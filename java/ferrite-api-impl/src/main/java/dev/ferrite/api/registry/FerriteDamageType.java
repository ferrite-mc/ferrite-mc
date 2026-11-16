package dev.ferrite.api.registry;

import com.google.gson.JsonObject;
import java.util.Locale;
import org.bukkit.NamespacedKey;
import org.bukkit.damage.DamageEffect;
import org.bukkit.damage.DamageScaling;
import org.bukkit.damage.DamageType;
import org.bukkit.damage.DeathMessageType;

/** A damage type backed by a {@code data/minecraft/damage_type/<name>.json} entry. */
public final class FerriteDamageType implements DamageType {

    private final NamespacedKey key;
    private final JsonObject json;

    public FerriteDamageType(NamespacedKey key, JsonObject json) {
        this.key = key;
        this.json = json == null ? new JsonObject() : json;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public String getTranslationKey() {
        return "death.attack." + VanillaData.string(this.json, "message_id", this.key.getKey());
    }

    @Override
    public DamageScaling getDamageScaling() {
        String scaling = VanillaData.string(this.json, "scaling", "when_caused_by_living_non_player");
        try {
            return DamageScaling.valueOf(scaling.toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException unknown) {
            return DamageScaling.WHEN_CAUSED_BY_LIVING_NON_PLAYER;
        }
    }

    @Override
    public DamageEffect getDamageEffect() {
        // The facade has no combat/sound engine, so every vanilla type reports the
        // default hurt effect; the datapack only carries message and exhaustion data.
        return DamageEffect.HURT;
    }

    @Override
    public DeathMessageType getDeathMessageType() {
        String type = VanillaData.string(this.json, "death_message_type", "default");
        try {
            return DeathMessageType.valueOf(type.toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException unknown) {
            return DeathMessageType.DEFAULT;
        }
    }

    @Override
    public float getExhaustion() {
        return this.json.has("exhaustion") ? this.json.get("exhaustion").getAsFloat() : 0.0F;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteDamageType type && this.key.equals(type.key);
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return "FerriteDamageType[" + this.key + "]";
    }
}
