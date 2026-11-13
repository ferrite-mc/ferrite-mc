package dev.ferrite.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.TranslatableComponent;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;

/** Private, lazily published snapshot of one extracted registry file. No JSON escapes this class. */
final class FerriteRegistryValueData {
    private final String kind;
    private final NamespacedKey key;
    private JsonObject snapshot;

    FerriteRegistryValueData(String kind, NamespacedKey key) {
        this.kind = kind;
        this.key = key;
    }

    private synchronized JsonObject json() {
        if (snapshot != null) return snapshot;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/" + kind + "/" + key.getKey() + ".json");
        if (file == null || !file.isFile())
            throw new UnsupportedOperationException("Ferrite: no " + kind + " data for " + key);
        try {
            snapshot = JsonParser.parseString(Files.readString(file.toPath())).getAsJsonObject();
            return snapshot;
        } catch (IOException error) {
            throw new UncheckedIOException("Cannot read " + kind + " " + key, error);
        } catch (RuntimeException error) {
            throw invalid("JSON object", error);
        }
    }

    private IllegalStateException invalid(String field, Throwable cause) {
        return new IllegalStateException("Ferrite: invalid " + kind + " " + key + " field " + field, cause);
    }

    private JsonElement required(String field) {
        var value = json().get(field);
        if (value == null || value.isJsonNull()) throw invalid(field, null);
        return value;
    }

    float number(String field, float minimum, float maximum) {
        var value = required(field);
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) throw invalid(field, null);
        // Reject a negative JSON number before float conversion can round it to -0.0.
        if (minimum >= 0 && value.getAsBigDecimal().signum() < 0) throw invalid(field, null);
        var number = value.getAsFloat();
        if (!Float.isFinite(number) || number < minimum || number > maximum) throw invalid(field, null);
        return number;
    }

    int integer(String field, int minimum, int maximum) {
        var value = required(field);
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber()) throw invalid(field, null);
        try {
            int number = value.getAsBigDecimal().intValueExact();
            if (number < minimum || number > maximum) throw invalid(field, null);
            return number;
        } catch (ArithmeticException | NumberFormatException error) {
            throw invalid(field, error);
        }
    }

    NamespacedKey key(String field) {
        var value = required(field);
        if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()) throw invalid(field, null);
        var result = NamespacedKey.fromString(value.getAsString());
        if (result == null) throw invalid(field, null);
        return result;
    }

    Sound sound(String field) {
        var sound = org.bukkit.Registry.SOUND_EVENT.get(key(field));
        if (sound == null) throw invalid(field, null);
        return sound;
    }

    Component component(String field, boolean optional) {
        if (optional && !json().has(field)) return null;
        var value = required(field);
        try {
            return GsonComponentSerializer.gson().deserialize(value.toString());
        } catch (RuntimeException error) {
            throw invalid(field, error);
        }
    }

    String translationKey(String field) {
        if (component(field, false) instanceof TranslatableComponent translated) return translated.key();
        throw new UnsupportedOperationException("Ferrite: " + kind + " " + key + " " + field + " has no translation key");
    }
}
