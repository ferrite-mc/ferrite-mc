package dev.ferrite.server;

import net.kyori.adventure.text.Component;
import org.bukkit.MusicInstrument;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;

/** A read-only snapshot of data/<namespace>/instrument/<key>.json, loaded on first property read. */
final class FerriteMusicInstrument extends MusicInstrument {
    private final NamespacedKey key;
    private final FerriteRegistryValueData data;

    FerriteMusicInstrument(NamespacedKey key) {
        this.key = key;
        this.data = new FerriteRegistryValueData("instrument", key);
    }

    @Override public NamespacedKey getKey() { return key; }
    /** Vanilla use_duration is measured in seconds. */
    @Override public float getDuration() { return data.number("use_duration", 0, Float.MAX_VALUE); }
    @Override public float getRange() { return data.number("range", Float.MIN_VALUE, Float.MAX_VALUE); }
    @Override public Sound getSound() { return data.sound("sound_event"); }
    @Override public Component description() { return data.component("description", false); }
    @Override public String translationKey() { return data.translationKey("description"); }
    @Override public boolean equals(Object other) { return other instanceof FerriteMusicInstrument value && key.equals(value.key); }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public String toString() { return "MusicInstrument[" + key + "]"; }
}
