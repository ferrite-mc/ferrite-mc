package dev.ferrite.api.boss;

import com.google.gson.Gson;
import java.util.EnumSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarFlag;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.BossBar;
import org.bukkit.entity.Player;

/** State-backed {@link BossBar} implementation for the Ferrite Bukkit facade. */
public sealed class FerriteBossBar implements BossBar permits FerriteKeyedBossBar {

    private static final Gson HOST_GSON = new Gson();

    private final UUID barId = UUID.randomUUID();
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
        if (visible) {
            broadcastToHost("title");
        }
    }

    @Override
    public synchronized BarColor getColor() {
        return color;
    }

    @Override
    public synchronized void setColor(BarColor color) {
        this.color = requireNonNullArg(color, "color");
        if (visible) {
            broadcastToHost("style");
        }
    }

    @Override
    public synchronized BarStyle getStyle() {
        return style;
    }

    @Override
    public synchronized void setStyle(BarStyle style) {
        this.style = requireNonNullArg(style, "style");
        if (visible) {
            broadcastToHost("style");
        }
    }

    @Override
    public synchronized void removeFlag(BarFlag flag) {
        if (flag != null && flags.remove(flag) && visible) {
            broadcastToHost("flags");
        }
    }

    @Override
    public synchronized void addFlag(BarFlag flag) {
        requireNonNullArg(flag, "flag");
        if (flags.add(flag) && visible) {
            broadcastToHost("flags");
        }
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
        if (visible) {
            broadcastToHost("progress");
        }
    }

    @Override
    public synchronized double getProgress() {
        return progress;
    }

    @Override
    public synchronized void addPlayer(Player player) {
        requireNonNullArg(player, "player");
        if (players.add(player) && visible) {
            forwardToHost(player, "add");
        }
    }

    @Override
    public synchronized void removePlayer(Player player) {
        if (player != null && players.remove(player) && visible) {
            forwardToHost(player, "remove");
        }
    }

    @Override
    public synchronized void removeAll() {
        if (visible) {
            for (Player player : players) {
                forwardToHost(player, "remove");
            }
        }
        players.clear();
    }

    @Override
    public synchronized List<Player> getPlayers() {
        return List.copyOf(players);
    }

    @Override
    public synchronized void setVisible(boolean visible) {
        if (this.visible == visible) {
            return;
        }
        this.visible = visible;
        broadcastToHost(visible ? "add" : "remove");
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

    private void broadcastToHost(String operation) {
        for (Player player : players) {
            forwardToHost(player, operation);
        }
    }

    private void forwardToHost(Player player, String operation) {
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("player_uuid", player.getUniqueId().toString());
            req.put("bar_uuid", barId.toString());
            req.put("operation", operation);
            req.put("title", title);
            req.put("progress", (float) progress);
            req.put("color", colorToProtocol(color));
            req.put("overlay", styleToProtocol(style));
            req.put("flags", flagsToMask());
            dev.ferrite.host.Bootstrap.playerBossBarUpdate(HOST_GSON.toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private static int colorToProtocol(BarColor color) {
        return switch (color) {
            case PINK -> 0;
            case BLUE -> 1;
            case RED -> 2;
            case GREEN -> 3;
            case YELLOW -> 4;
            case PURPLE -> 5;
            case WHITE -> 6;
        };
    }

    private static int styleToProtocol(BarStyle style) {
        return switch (style) {
            case SOLID -> 0;
            case SEGMENTED_6 -> 1;
            case SEGMENTED_10 -> 2;
            case SEGMENTED_12 -> 3;
            case SEGMENTED_20 -> 4;
        };
    }

    private int flagsToMask() {
        int mask = 0;
        if (flags.contains(BarFlag.DARKEN_SKY)) {
            mask |= 0x01;
        }
        if (flags.contains(BarFlag.PLAY_BOSS_MUSIC)) {
            mask |= 0x02;
        }
        if (flags.contains(BarFlag.CREATE_FOG)) {
            mask |= 0x04;
        }
        return mask;
    }

    private static <T> T requireNonNullArg(T value, String name) {
        if (value == null) {
            throw new IllegalArgumentException(name + " cannot be null");
        }
        return value;
    }
}
