package dev.ferrite.server;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.MusicInstrument;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;

/** A goat-horn instrument read from the generated {@code data/<namespace>/instrument/<key>.json}. */
final class FerriteMusicInstrument extends MusicInstrument {
    private final NamespacedKey key;
    private volatile JsonObject json;

    FerriteMusicInstrument(NamespacedKey key) { this.key = key; }

    private JsonObject json() {
        var cached = json;
        if (cached != null) return cached;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/instrument/" + key.getKey() + ".json");
        if (file == null || !file.isFile()) throw new UnsupportedOperationException("Ferrite: no instrument data for " + key);
        try {
            return json = JsonParser.parseString(Files.readString(file.toPath(), StandardCharsets.UTF_8)).getAsJsonObject();
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }

    private float number(String field) {
        var value = json().get(field);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber())
            throw new UnsupportedOperationException("Ferrite: instrument " + key + " has no " + field);
        return value.getAsFloat();
    }

    @Override public NamespacedKey getKey() { return key; }
    /** Vanilla {@code use_duration}, in seconds. */
    @Override public float getDuration() { return number("use_duration"); }
    @Override public float getRange() { return number("range"); }
    @Override public Sound getSound() {
        var value = json().get("sound_event");
        var sound = value == null || !value.isJsonPrimitive() ? null : org.bukkit.Registry.SOUND_EVENT.get(NamespacedKey.fromString(value.getAsString()));
        if (sound == null) throw new UnsupportedOperationException("Ferrite: instrument " + key + " has no known sound_event");
        return sound;
    }
    @Override public Component description() {
        var value = json().get("description");
        if (value == null) throw new UnsupportedOperationException("Ferrite: instrument " + key + " has no description");
        return GsonComponentSerializer.gson().deserialize(value.toString());
    }
    @Override public String translationKey() { return "instrument." + key.getNamespace() + "." + key.getKey(); }
    @Override public String toString() { return "MusicInstrument[" + key + "]"; }
}
