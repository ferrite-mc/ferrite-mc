package dev.ferrite.server;

import com.google.gson.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import org.bukkit.NamespacedKey;
import org.bukkit.damage.*;

/** Immutable definitions decoded by the official codec at build time, including its optional defaults. */
final class FerriteDamageData {
    record Effect(String name, org.bukkit.Sound sound) implements DamageEffect {
        @Override public org.bukkit.Sound getSound() { return sound; }
    }
    record Definition(String message, float exhaustion, DamageScaling scaling, DamageEffect effect, DeathMessageType death) {}
    private final Map<NamespacedKey, Definition> types;
    private final Map<String, Effect> effects;
    private FerriteDamageData(Map<NamespacedKey, Definition> types, Map<String, Effect> effects) {
        this.types = Map.copyOf(types);
        this.effects = Map.copyOf(effects);
    }
    private static final class Holder {
        static final FerriteDamageData DATA = load(FerriteDamageData.class.getResourceAsStream("/ferrite/damage-types.json"), FerriteRegistryAccess.damageTypeKeys());
    }
    static FerriteDamageData canonical() { return Holder.DATA; }
    Definition definition(NamespacedKey key) {
        var result = types.get(key);
        if (result == null) throw new IllegalStateException("Missing decoded damage type " + key);
        return result;
    }
    DamageEffect effect(String name) {
        var result = effects.get(Objects.requireNonNull(name, "effect name"));
        if (result == null) throw new IllegalArgumentException("Unknown damage effect " + name);
        return result;
    }
    static FerriteDamageData load(InputStream input, Set<String> expectedKeys) {
        if (input == null) throw new IllegalStateException("Missing decoded official damage-type data");
        try (var reader = new InputStreamReader(input, StandardCharsets.UTF_8)) {
            var root = JsonParser.parseReader(reader).getAsJsonObject();
            if (!"26.3".equals(string(root.get("version")))) throw new IllegalArgumentException("Wrong damage data version");
            var effects = new HashMap<String, Effect>();
            for (var entry : root.getAsJsonObject("effects").entrySet()) {
                var key = NamespacedKey.fromString(string(entry.getValue()));
                var sound = key == null ? null : org.bukkit.Registry.SOUND_EVENT.get(key);
                if (sound == null) throw new IllegalArgumentException("Unknown damage sound " + entry);
                effects.put(entry.getKey(), new Effect(entry.getKey(), sound));
            }
            if (!effects.keySet().equals(Set.of("hurt", "thorns", "drowning", "burning", "poking", "freezing")))
                throw new IllegalArgumentException("Damage effects differ from installed API");
            var types = new HashMap<NamespacedKey, Definition>();
            var jsonTypes = root.getAsJsonObject("types");
            if (jsonTypes.size() == 0 || !jsonTypes.keySet().equals(expectedKeys)) throw new IllegalArgumentException("Damage keys differ from generated manifest");
            for (var entry : jsonTypes.entrySet()) {
                var key = NamespacedKey.fromString(entry.getKey());
                if (key == null || !key.toString().equals(entry.getKey())) throw new IllegalArgumentException("Invalid damage type key");
                var json = entry.getValue().getAsJsonObject();
                var number = json.getAsJsonPrimitive("exhaustion");
                if (!number.isNumber() || !Float.isFinite(number.getAsFloat())) throw new IllegalArgumentException("Invalid exhaustion for " + key);
                var effect = effects.get(string(json.get("effects")));
                if (effect == null) throw new IllegalArgumentException("Unknown damage effect for " + key);
                types.put(key, new Definition(string(json.get("message_id")), number.getAsFloat(),
                        DamageScaling.valueOf(string(json.get("scaling")).toUpperCase(Locale.ROOT)), effect,
                        DeathMessageType.valueOf(string(json.get("death_message_type")).toUpperCase(Locale.ROOT))));
            }
            return new FerriteDamageData(types, effects);
        } catch (Exception error) {
            throw new IllegalStateException("Invalid decoded official damage-type data: " + error.getMessage(), error);
        }
    }
    private static String string(JsonElement value) {
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString() || value.getAsString().isEmpty())
            throw new IllegalArgumentException("Expected nonempty damage data string");
        return value.getAsString();
    }
}
