package dev.ferrite.api.boss;

import org.bukkit.NamespacedKey;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarFlag;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.KeyedBossBar;

/** Keyed {@link KeyedBossBar} variant registered through {@link org.bukkit.Server#createBossBar}. */
public final class FerriteKeyedBossBar extends FerriteBossBar implements KeyedBossBar {

    private final NamespacedKey key;

    public FerriteKeyedBossBar(
            NamespacedKey key, String title, BarColor color, BarStyle style, BarFlag... flags) {
        super(title, color, style, flags);
        if (key == null) {
            throw new IllegalArgumentException("key cannot be null");
        }
        this.key = key;
    }

    @Override
    public NamespacedKey getKey() {
        return key;
    }
}
