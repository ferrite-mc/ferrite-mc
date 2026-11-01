package dev.ferrite.server;

import java.util.List;
import java.util.Set;

/**
 * Server-side boss bar registry entry.
 *
 * <p>Progress, colour, style, flags and viewers are real Bukkit state. Nothing is sent to
 * clients yet: the Rust host has no boss-bar packet path, which is documented in the bridge
 * README instead of silently dropping the bar.
 */
final class FerriteBossBar implements org.bukkit.boss.KeyedBossBar {
    private final org.bukkit.NamespacedKey key;
    private volatile String title;
    private volatile org.bukkit.boss.BarColor color;
    private volatile org.bukkit.boss.BarStyle style;
    private final Set<org.bukkit.boss.BarFlag> flags = java.util.EnumSet.noneOf(org.bukkit.boss.BarFlag.class);
    private volatile double progress = 1.0;
    private volatile boolean visible = true;
    private final List<org.bukkit.entity.Player> players = new java.util.concurrent.CopyOnWriteArrayList<>();

    FerriteBossBar(org.bukkit.NamespacedKey key, String title, org.bukkit.boss.BarColor color, org.bukkit.boss.BarStyle style, org.bukkit.boss.BarFlag... flags) {
        this.key = java.util.Objects.requireNonNull(key, "key");
        this.title = java.util.Objects.requireNonNull(title, "title");
        this.color = java.util.Objects.requireNonNull(color, "color");
        this.style = java.util.Objects.requireNonNull(style, "style");
        if (flags != null) for (var flag : flags) if (flag != null) this.flags.add(flag);
    }

    @Override public org.bukkit.NamespacedKey getKey() { return key; }
    @Override public String getTitle() { return title; }
    @Override public void setTitle(String value) { title = java.util.Objects.requireNonNull(value, "title"); }
    @Override public org.bukkit.boss.BarColor getColor() { return color; }
    @Override public void setColor(org.bukkit.boss.BarColor value) { color = java.util.Objects.requireNonNull(value, "color"); }
    @Override public org.bukkit.boss.BarStyle getStyle() { return style; }
    @Override public void setStyle(org.bukkit.boss.BarStyle value) { style = java.util.Objects.requireNonNull(value, "style"); }
    @Override public synchronized void removeFlag(org.bukkit.boss.BarFlag flag) { flags.remove(flag); }
    @Override public synchronized void addFlag(org.bukkit.boss.BarFlag flag) { flags.add(java.util.Objects.requireNonNull(flag, "flag")); }
    @Override public synchronized boolean hasFlag(org.bukkit.boss.BarFlag flag) { return flags.contains(flag); }
    @Override public void setProgress(double value) {
        if (!(value >= 0.0 && value <= 1.0)) throw new IllegalArgumentException("progress must be between 0 and 1");
        progress = value;
    }
    @Override public double getProgress() { return progress; }
    @Override public void addPlayer(org.bukkit.entity.Player player) { if (player != null && !players.contains(player)) players.add(player); }
    @Override public void removePlayer(org.bukkit.entity.Player player) { players.remove(player); }
    @Override public void removeAll() { players.clear(); }
    @Override public List<org.bukkit.entity.Player> getPlayers() { return List.copyOf(players); }
    @Override public void setVisible(boolean value) { visible = value; }
    @Override public boolean isVisible() { return visible; }
    @Override public void show() { visible = true; }
    @Override public void hide() { visible = false; }
    @Override public String toString() { return "BossBar[" + key + "]"; }
}
