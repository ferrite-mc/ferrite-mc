package dev.ferrite.server;

import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;
import java.util.Set;
import net.kyori.adventure.bossbar.BossBar;
import net.kyori.adventure.text.Component;

/**
 * Server-side boss bar registry entry.
 *
 * <p>A bar owns one Adventure {@link BossBar}. Showing it to a viewer goes through
 * {@link org.bukkit.entity.Player#showBossBar}, the bridge path that already turns Adventure
 * boss-bar changes into per-player client packets. Progress, colour, style, flags and viewer
 * membership therefore reach the live client; the host only handles ordinary per-player packet
 * commands and no region-owned world or entity state is read here.
 *
 * <p>Viewers are an explicit list, as in Bukkit: adding or removing a player shows or hides the
 * bar for that player only. Hiding the bar keeps its attributes so showing it again recreates it.
 */
final class FerriteBossBar implements org.bukkit.boss.KeyedBossBar {
    private final org.bukkit.NamespacedKey key;
    private final BossBar bar;
    private volatile String title;
    private volatile boolean visible = true;
    private final List<org.bukkit.entity.Player> players = new ArrayList<>();
    private final Set<org.bukkit.boss.BarFlag> flags = EnumSet.noneOf(org.bukkit.boss.BarFlag.class);

    FerriteBossBar(org.bukkit.NamespacedKey key, String title, org.bukkit.boss.BarColor color, org.bukkit.boss.BarStyle style, org.bukkit.boss.BarFlag... flags) {
        this.key = java.util.Objects.requireNonNull(key, "key");
        this.title = java.util.Objects.requireNonNull(title, "title");
        if (flags != null) for (var flag : flags) if (flag != null) this.flags.add(flag);
        this.bar = BossBar.bossBar(Component.text(this.title), 1.0f, color(color), overlay(style), adventureFlags(this.flags));
    }

    private static BossBar.Color color(org.bukkit.boss.BarColor color) {
        return switch (java.util.Objects.requireNonNull(color, "color")) {
            case PINK -> BossBar.Color.PINK;
            case BLUE -> BossBar.Color.BLUE;
            case RED -> BossBar.Color.RED;
            case GREEN -> BossBar.Color.GREEN;
            case YELLOW -> BossBar.Color.YELLOW;
            case PURPLE -> BossBar.Color.PURPLE;
            case WHITE -> BossBar.Color.WHITE;
        };
    }

    private static BossBar.Overlay overlay(org.bukkit.boss.BarStyle style) {
        return switch (java.util.Objects.requireNonNull(style, "style")) {
            case SOLID -> BossBar.Overlay.PROGRESS;
            case SEGMENTED_6 -> BossBar.Overlay.NOTCHED_6;
            case SEGMENTED_10 -> BossBar.Overlay.NOTCHED_10;
            case SEGMENTED_12 -> BossBar.Overlay.NOTCHED_12;
            case SEGMENTED_20 -> BossBar.Overlay.NOTCHED_20;
        };
    }

    private static BossBar.Flag flag(org.bukkit.boss.BarFlag flag) {
        return switch (java.util.Objects.requireNonNull(flag, "flag")) {
            case DARKEN_SKY -> BossBar.Flag.DARKEN_SCREEN;
            case PLAY_BOSS_MUSIC -> BossBar.Flag.PLAY_BOSS_MUSIC;
            case CREATE_FOG -> BossBar.Flag.CREATE_WORLD_FOG;
        };
    }

    private static Set<BossBar.Flag> adventureFlags(Set<org.bukkit.boss.BarFlag> flags) {
        var out = EnumSet.noneOf(BossBar.Flag.class);
        for (var flag : flags) out.add(flag(flag));
        return out;
    }

    @Override public org.bukkit.NamespacedKey getKey() { return key; }
    @Override public String getTitle() { return title; }
    @Override public void setTitle(String value) { title = java.util.Objects.requireNonNull(value, "title"); bar.name(Component.text(title)); }
    @Override public org.bukkit.boss.BarColor getColor() { return org.bukkit.boss.BarColor.valueOf(bar.color().name()); }
    @Override public void setColor(org.bukkit.boss.BarColor value) { bar.color(color(value)); }
    @Override public org.bukkit.boss.BarStyle getStyle() { return switch (bar.overlay()) { case PROGRESS -> org.bukkit.boss.BarStyle.SOLID; case NOTCHED_6 -> org.bukkit.boss.BarStyle.SEGMENTED_6; case NOTCHED_10 -> org.bukkit.boss.BarStyle.SEGMENTED_10; case NOTCHED_12 -> org.bukkit.boss.BarStyle.SEGMENTED_12; case NOTCHED_20 -> org.bukkit.boss.BarStyle.SEGMENTED_20; }; }
    @Override public void setStyle(org.bukkit.boss.BarStyle value) { bar.overlay(overlay(value)); }
    @Override public synchronized void removeFlag(org.bukkit.boss.BarFlag flag) { if (flags.remove(java.util.Objects.requireNonNull(flag, "flag"))) bar.removeFlag(flag(flag)); }
    @Override public synchronized void addFlag(org.bukkit.boss.BarFlag flag) { if (flags.add(java.util.Objects.requireNonNull(flag, "flag"))) bar.addFlag(flag(flag)); }
    @Override public synchronized boolean hasFlag(org.bukkit.boss.BarFlag flag) { return flags.contains(java.util.Objects.requireNonNull(flag, "flag")); }
    @Override public void setProgress(double value) {
        if (!(value >= 0.0 && value <= 1.0)) throw new IllegalArgumentException("progress must be between 0 and 1");
        bar.progress((float) value);
    }
    @Override public double getProgress() { return bar.progress(); }

    /** Adds a viewer; the bar is shown at once when it is visible. */
    @Override public void addPlayer(org.bukkit.entity.Player player) {
        java.util.Objects.requireNonNull(player, "player");
        synchronized (this) {
            if (players.contains(player)) return;
            players.add(player);
            if (!visible) return;
        }
        player.showBossBar(bar);
        // A concurrent hide may have completed before the show reached the player.
        if (!visible) player.hideBossBar(bar);
    }

    @Override public void removePlayer(org.bukkit.entity.Player player) {
        java.util.Objects.requireNonNull(player, "player");
        boolean removed;
        synchronized (this) { removed = players.remove(player); }
        if (removed) player.hideBossBar(bar);
    }

    @Override public void removeAll() {
        List<org.bukkit.entity.Player> existing;
        synchronized (this) { existing = List.copyOf(players); players.clear(); }
        for (var player : existing) player.hideBossBar(bar);
    }

    @Override public List<org.bukkit.entity.Player> getPlayers() {
        synchronized (this) { return List.copyOf(players); }
    }

    @Override public void setVisible(boolean value) {
        List<org.bukkit.entity.Player> existing;
        synchronized (this) {
            if (visible == value) return;
            visible = value;
            existing = List.copyOf(players);
        }
        for (var player : existing) {
            if (value) player.showBossBar(bar); else player.hideBossBar(bar);
        }
    }

    @Override public boolean isVisible() { return visible; }
    @Override public void show() { setVisible(true); }
    @Override public void hide() { setVisible(false); }
    @Override public String toString() { return "BossBar[" + key + "]"; }
}
