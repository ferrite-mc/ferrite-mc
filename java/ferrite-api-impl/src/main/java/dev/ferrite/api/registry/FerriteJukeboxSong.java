package dev.ferrite.api.registry;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import net.kyori.adventure.text.Component;
import org.bukkit.JukeboxSong;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.Sound;

/** Registry-backed {@link JukeboxSong} value from generated 26.3 data. */
public final class FerriteJukeboxSong implements JukeboxSong {

    private final NamespacedKey key;
    private final JsonObject json;

    public FerriteJukeboxSong(NamespacedKey key, JsonObject json) {
        this.key = key;
        this.json = json;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getTranslationKey() {
        return "jukebox_song." + this.key.getNamespace() + "." + this.key.getKey();
    }

    @Override
    public Sound getSound() {
        JsonElement sound = this.json.get("sound_event");
        if (sound != null && sound.isJsonPrimitive()) {
            NamespacedKey soundKey = NamespacedKey.fromString(sound.getAsString());
            if (soundKey != null) {
                Sound resolved = Registry.SOUND_EVENT.get(soundKey);
                if (resolved != null) {
                    return resolved;
                }
            }
        }
        return Registry.SOUND_EVENT.stream().findFirst().orElse(null);
    }

    @Override
    public Component getDescription() {
        JsonElement desc = this.json.get("description");
        if (desc != null && desc.isJsonObject()) {
            JsonElement translate = desc.getAsJsonObject().get("translate");
            if (translate != null && translate.isJsonPrimitive()) {
                return Component.translatable(translate.getAsString());
            }
        }
        return Component.translatable(getTranslationKey());
    }

    @Override
    public float getLengthInSeconds() {
        JsonElement len = this.json.get("length_in_seconds");
        return len != null && len.isJsonPrimitive() ? len.getAsFloat() : 0.0F;
    }

    @Override
    public int getComparatorOutput() {
        JsonElement output = this.json.get("comparator_output");
        return output != null && output.isJsonPrimitive() ? output.getAsInt() : 0;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof JukeboxSong other && this.key.equals(other.getKey());
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
