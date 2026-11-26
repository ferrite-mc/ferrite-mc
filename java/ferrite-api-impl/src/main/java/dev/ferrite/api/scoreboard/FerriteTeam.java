package dev.ferrite.api.scoreboard;

import java.util.ArrayList;
import java.util.Collection;
import java.util.EnumMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.audience.Audience;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.format.TextColor;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.scoreboard.NameTagVisibility;
import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.Team;

/** State-backed {@link Team} implementation for {@link FerriteScoreboard}. */
@SuppressWarnings("deprecation")
public final class FerriteTeam implements Team {

    private static final Map<ChatColor, NamedTextColor> BUKKIT_TO_ADVENTURE = new EnumMap<>(ChatColor.class);
    private static final Map<NamedTextColor, ChatColor> ADVENTURE_TO_BUKKIT = Map.ofEntries(
            Map.entry(NamedTextColor.BLACK, ChatColor.BLACK),
            Map.entry(NamedTextColor.DARK_BLUE, ChatColor.DARK_BLUE),
            Map.entry(NamedTextColor.DARK_GREEN, ChatColor.DARK_GREEN),
            Map.entry(NamedTextColor.DARK_AQUA, ChatColor.DARK_AQUA),
            Map.entry(NamedTextColor.DARK_RED, ChatColor.DARK_RED),
            Map.entry(NamedTextColor.DARK_PURPLE, ChatColor.DARK_PURPLE),
            Map.entry(NamedTextColor.GOLD, ChatColor.GOLD),
            Map.entry(NamedTextColor.GRAY, ChatColor.GRAY),
            Map.entry(NamedTextColor.DARK_GRAY, ChatColor.DARK_GRAY),
            Map.entry(NamedTextColor.BLUE, ChatColor.BLUE),
            Map.entry(NamedTextColor.GREEN, ChatColor.GREEN),
            Map.entry(NamedTextColor.AQUA, ChatColor.AQUA),
            Map.entry(NamedTextColor.RED, ChatColor.RED),
            Map.entry(NamedTextColor.LIGHT_PURPLE, ChatColor.LIGHT_PURPLE),
            Map.entry(NamedTextColor.YELLOW, ChatColor.YELLOW),
            Map.entry(NamedTextColor.WHITE, ChatColor.WHITE));

    static {
        for (Map.Entry<NamedTextColor, ChatColor> entry : ADVENTURE_TO_BUKKIT.entrySet()) {
            BUKKIT_TO_ADVENTURE.put(entry.getValue(), entry.getKey());
        }
    }

    private final FerriteScoreboard scoreboard;
    private final String name;
    private final Set<String> entries = new LinkedHashSet<>();
    private final Map<Option, OptionStatus> options = new EnumMap<>(Option.class);

    private Component displayName;
    private Component prefix = Component.empty();
    private Component suffix = Component.empty();
    private NamedTextColor color;
    private boolean allowFriendlyFire = true;
    private boolean canSeeFriendlyInvisibles = true;
    private boolean registered = true;

    FerriteTeam(FerriteScoreboard scoreboard, String name) {
        this.scoreboard = Objects.requireNonNull(scoreboard, "scoreboard");
        this.name = Objects.requireNonNull(name, "name");
        this.displayName = Component.text(name);
        for (Option option : Option.values()) {
            this.options.put(option, OptionStatus.ALWAYS);
        }
    }

    @Override
    public String getName() {
        checkState();
        return name;
    }

    @Override
    public Component displayName() {
        checkState();
        return displayName;
    }

    @Override
    public void displayName(Component displayName) {
        checkState();
        this.displayName = displayName == null ? Component.empty() : displayName;
    }

    @Override
    public Component prefix() {
        checkState();
        return prefix;
    }

    @Override
    public void prefix(Component prefix) {
        checkState();
        this.prefix = prefix == null ? Component.empty() : prefix;
    }

    @Override
    public Component suffix() {
        checkState();
        return suffix;
    }

    @Override
    public void suffix(Component suffix) {
        checkState();
        this.suffix = suffix == null ? Component.empty() : suffix;
    }

    @Override
    public boolean hasColor() {
        checkState();
        return color != null;
    }

    @Override
    public TextColor color() {
        checkState();
        if (color == null) {
            throw new IllegalStateException("Team color is unset");
        }
        return color;
    }

    @Override
    public void color(NamedTextColor color) {
        checkState();
        this.color = color;
    }

    @Override
    public String getDisplayName() {
        checkState();
        return LegacyComponentSerializer.legacySection().serialize(displayName);
    }

    @Override
    public void setDisplayName(String displayName) {
        checkState();
        if (displayName == null) {
            throw new IllegalArgumentException("Display name cannot be null");
        }
        this.displayName = LegacyComponentSerializer.legacySection().deserialize(displayName);
    }

    @Override
    public String getPrefix() {
        checkState();
        return LegacyComponentSerializer.legacySection().serialize(prefix);
    }

    @Override
    public void setPrefix(String prefix) {
        checkState();
        if (prefix == null) {
            throw new IllegalArgumentException("Prefix cannot be null");
        }
        this.prefix = LegacyComponentSerializer.legacySection().deserialize(prefix);
    }

    @Override
    public String getSuffix() {
        checkState();
        return LegacyComponentSerializer.legacySection().serialize(suffix);
    }

    @Override
    public void setSuffix(String suffix) {
        checkState();
        if (suffix == null) {
            throw new IllegalArgumentException("Suffix cannot be null");
        }
        this.suffix = LegacyComponentSerializer.legacySection().deserialize(suffix);
    }

    @Override
    public ChatColor getColor() {
        checkState();
        return color == null ? ChatColor.RESET : ADVENTURE_TO_BUKKIT.getOrDefault(color, ChatColor.RESET);
    }

    @Override
    public void setColor(ChatColor color) {
        checkState();
        if (color == null || color == ChatColor.RESET) {
            this.color = null;
            return;
        }
        NamedTextColor mapped = BUKKIT_TO_ADVENTURE.get(color);
        if (mapped == null) {
            throw new IllegalArgumentException("Team color must be a color ChatColor, got " + color);
        }
        this.color = mapped;
    }

    @Override
    public boolean allowFriendlyFire() {
        checkState();
        return allowFriendlyFire;
    }

    @Override
    public void setAllowFriendlyFire(boolean enabled) {
        checkState();
        this.allowFriendlyFire = enabled;
    }

    @Override
    public boolean canSeeFriendlyInvisibles() {
        checkState();
        return canSeeFriendlyInvisibles;
    }

    @Override
    public void setCanSeeFriendlyInvisibles(boolean enabled) {
        checkState();
        this.canSeeFriendlyInvisibles = enabled;
    }

    @Override
    public NameTagVisibility getNameTagVisibility() {
        checkState();
        return switch (getOption(Option.NAME_TAG_VISIBILITY)) {
            case ALWAYS -> NameTagVisibility.ALWAYS;
            case NEVER -> NameTagVisibility.NEVER;
            case FOR_OTHER_TEAMS -> NameTagVisibility.HIDE_FOR_OTHER_TEAMS;
            case FOR_OWN_TEAM -> NameTagVisibility.HIDE_FOR_OWN_TEAM;
        };
    }

    @Override
    public void setNameTagVisibility(NameTagVisibility visibility) {
        checkState();
        if (visibility == null) {
            throw new IllegalArgumentException("Visibility cannot be null");
        }
        OptionStatus status = switch (visibility) {
            case ALWAYS -> OptionStatus.ALWAYS;
            case NEVER -> OptionStatus.NEVER;
            case HIDE_FOR_OTHER_TEAMS -> OptionStatus.FOR_OTHER_TEAMS;
            case HIDE_FOR_OWN_TEAM -> OptionStatus.FOR_OWN_TEAM;
        };
        setOption(Option.NAME_TAG_VISIBILITY, status);
    }

    @Override
    public synchronized Set<OfflinePlayer> getPlayers() {
        checkState();
        Set<OfflinePlayer> result = new LinkedHashSet<>();
        for (String entry : entries) {
            result.add(FerriteOfflinePlayer.resolve(entry));
        }
        return Set.copyOf(result);
    }

    @Override
    public synchronized Set<String> getEntries() {
        checkState();
        return Set.copyOf(entries);
    }

    @Override
    public synchronized int getSize() {
        checkState();
        return entries.size();
    }

    @Override
    public Scoreboard getScoreboard() {
        return registered ? scoreboard : null;
    }

    @Override
    public void addPlayer(OfflinePlayer player) {
        checkState();
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        addEntry(FerriteScoreboard.entryName(player));
    }

    @Override
    public void addEntry(String entry) {
        checkState();
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        scoreboard.assignEntryToTeam(entry, this);
    }

    @Override
    public void addEntities(Collection<Entity> entities) {
        checkState();
        if (entities == null) {
            throw new IllegalArgumentException("Entities cannot be null");
        }
        for (Entity entity : entities) {
            addEntity(entity);
        }
    }

    @Override
    public void addEntries(Collection<String> entries) {
        checkState();
        if (entries == null) {
            throw new IllegalArgumentException("Entries cannot be null");
        }
        for (String entry : entries) {
            addEntry(entry);
        }
    }

    @Override
    public boolean removePlayer(OfflinePlayer player) {
        checkState();
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        return removeEntry(FerriteScoreboard.entryName(player));
    }

    @Override
    public boolean removeEntry(String entry) {
        checkState();
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        return scoreboard.removeEntryFromTeam(entry, this);
    }

    @Override
    public boolean removeEntities(Collection<Entity> entities) {
        checkState();
        if (entities == null) {
            throw new IllegalArgumentException("Entities cannot be null");
        }
        boolean changed = false;
        for (Entity entity : entities) {
            changed |= removeEntity(entity);
        }
        return changed;
    }

    @Override
    public boolean removeEntries(Collection<String> entries) {
        checkState();
        if (entries == null) {
            throw new IllegalArgumentException("Entries cannot be null");
        }
        boolean changed = false;
        for (String entry : entries) {
            changed |= removeEntry(entry);
        }
        return changed;
    }

    @Override
    public void unregister() {
        checkState();
        scoreboard.removeTeam(this);
        registered = false;
        synchronized (this) {
            entries.clear();
        }
    }

    String rawName() {
        return name;
    }

    @Override
    public boolean hasPlayer(OfflinePlayer player) {
        checkState();
        if (player == null) {
            throw new IllegalArgumentException("OfflinePlayer cannot be null");
        }
        return hasEntry(FerriteScoreboard.entryName(player));
    }

    @Override
    public synchronized boolean hasEntry(String entry) {
        checkState();
        if (entry == null) {
            throw new IllegalArgumentException("Entry cannot be null");
        }
        return entries.contains(entry);
    }

    @Override
    public synchronized OptionStatus getOption(Option option) {
        checkState();
        if (option == null) {
            throw new IllegalArgumentException("Option cannot be null");
        }
        return options.get(option);
    }

    @Override
    public synchronized void setOption(Option option, OptionStatus status) {
        checkState();
        if (option == null || status == null) {
            throw new IllegalArgumentException("Option and OptionStatus cannot be null");
        }
        options.put(option, status);
    }

    @Override
    public void addEntity(Entity entity) {
        checkState();
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        addEntry(FerriteScoreboard.entityEntryName(entity));
    }

    @Override
    public boolean removeEntity(Entity entity) {
        checkState();
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        return removeEntry(FerriteScoreboard.entityEntryName(entity));
    }

    @Override
    public boolean hasEntity(Entity entity) {
        checkState();
        if (entity == null) {
            throw new IllegalArgumentException("Entity cannot be null");
        }
        return hasEntry(FerriteScoreboard.entityEntryName(entity));
    }

    @Override
    public synchronized Iterable<? extends Audience> audiences() {
        if (!registered || Bukkit.getServer() == null) {
            return List.of();
        }
        List<Audience> result = new ArrayList<>();
        for (String entry : entries) {
            Player player = Bukkit.getPlayerExact(entry);
            if (player != null) {
                result.add(player);
            }
        }
        return List.copyOf(result);
    }

    synchronized void addEntryInternal(String entry) {
        entries.add(entry);
    }

    synchronized boolean removeEntryInternal(String entry) {
        return entries.remove(entry);
    }

    synchronized Set<String> snapshotEntries() {
        return Set.copyOf(entries);
    }

    private void checkState() {
        if (!registered) {
            throw new IllegalStateException("Unregistered scoreboard component");
        }
    }
}
