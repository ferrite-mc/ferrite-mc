package dev.ferrite.api.scoreboard;

import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.RenderType;

/** {@link Criteria} implementation backing {@link org.bukkit.Bukkit#getScoreboardCriteria(String)}. */
public final class FerriteCriteria implements Criteria {

    private static final Set<String> READ_ONLY_DEFAULTS = Set.of(
            "deathCount",
            "playerKillCount",
            "totalKillCount",
            "health",
            "food",
            "air",
            "armor",
            "xp",
            "level");

    private static final Map<String, Criteria> CACHE = new ConcurrentHashMap<>();

    private final String name;
    private final boolean readOnly;
    private final RenderType defaultRenderType;

    private FerriteCriteria(String name, boolean readOnly, RenderType defaultRenderType) {
        this.name = name;
        this.readOnly = readOnly;
        this.defaultRenderType = defaultRenderType;
    }

    public static Criteria getOrCreate(String name) {
        if (name == null || name.isEmpty()) {
            throw new IllegalArgumentException("Criteria name cannot be null or empty");
        }
        return CACHE.computeIfAbsent(name, FerriteCriteria::createForName);
    }

    private static Criteria createForName(String name) {
        boolean readOnly = READ_ONLY_DEFAULTS.contains(name)
                || name.startsWith("teamkill.")
                || name.startsWith("killedByTeam.")
                || name.startsWith("minecraft.");
        RenderType renderType = "health".equals(name) ? RenderType.HEARTS : RenderType.INTEGER;
        return new FerriteCriteria(name, readOnly, renderType);
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public boolean isReadOnly() {
        return readOnly;
    }

    @Override
    public RenderType getDefaultRenderType() {
        return defaultRenderType;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof Criteria other && name.equals(other.getName());
    }

    @Override
    public int hashCode() {
        return name.hashCode();
    }

    @Override
    public String toString() {
        return "FerriteCriteria{" + name + "}";
    }
}
