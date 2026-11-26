package dev.ferrite.api.boss;

import java.util.EnumSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarFlag;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.BossBar;
import org.bukkit.entity.Player;

/** State-backed {@link BossBar} implementation for the Ferrite Bukkit facade. */
public sealed class FerriteBossBar implements BossBar permits FerriteKeyedBossBar {

    private final Set<BarFlag> flags = EnumSet.noneOf(BarFlag.class);
    private final Set<Player> players = new LinkedHashSet<>();

    private String title;
    private BarColor color;
    private BarStyle style;
    private double progress = 1.0;
    private boolean visible = true;

    public FerriteBossBar(String title, BarColor color, BarStyle style, BarFlag... flags) {
        this.title = title == null ? "" : title;
        this.color = requireNonNullArg(color, "color");
        this.style = requireNonNullArg(style, "style");
        if (flags != null) {
            for (BarFlag flag : flags) {
                if (flag != null) {
                    this.flags.add(flag);
                }
            }
        }
    }

    @Override
    public synchronized String getTitle() {
        return title;
    }

    @Override
    public synchronized void setTitle(String title) {
        this.title = title == null ? "" : title;
    }

    @Override
    public synchronized BarColor getColor() {
        return color;
    }

    @Override
    public synchronized void setColor(BarColor color) {
        this.color = requireNonNullArg(color, "color");
    }

    @Override
    public synchronized BarStyle getStyle() {
        return style;
    }

    @Override
    public synchronized void setStyle(BarStyle style) {
        this.style = requireNonNullArg(style, "style");
    }

    @Override
    public synchronized void removeFlag(BarFlag flag) {
        if (flag != null) {
            flags.remove(flag);
        }
    }

    @Override
    public synchronized void addFlag(BarFlag flag) {
        requireNonNullArg(flag, "flag");
        flags.add(flag);
    }

    @Override
    public synchronized boolean hasFlag(BarFlag flag) {
        return flag != null && flags.contains(flag);
    }

    @Override
    public synchronized void setProgress(double progress) {
        if (Double.isNaN(progress) || progress < 0.0 || progress > 1.0) {
            throw new IllegalArgumentException(
                    "Progress must be between 0.0 and 1.0, was " + progress);
        }
        this.progress = progress;
    }

    @Override
    public synchronized double getProgress() {
        return progress;
    }

    @Override
    public synchronized void addPlayer(Player player) {
        requireNonNullArg(player, "player");
        players.add(player);
    }

    @Override
    public synchronized void removePlayer(Player player) {
        if (player != null) {
            players.remove(player);
        }
    }

    @Override
    public synchronized void removeAll() {
        players.clear();
    }

    @Override
    public synchronized List<Player> getPlayers() {
        return List.copyOf(players);
    }

    @Override
    public synchronized void setVisible(boolean visible) {
        this.visible = visible;
    }

    @Override
    public synchronized boolean isVisible() {
        return visible;
    }

    @Override
    @SuppressWarnings("deprecation")
    public void show() {
        setVisible(true);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void hide() {
        setVisible(false);
    }

    private static <T> T requireNonNullArg(T value, String name) {
        if (value == null) {
            throw new IllegalArgumentException(name + " cannot be null");
        }
        return value;
    }
}
