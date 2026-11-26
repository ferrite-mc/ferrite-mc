package dev.ferrite.api.scoreboard;

import dev.ferrite.api.internal.Unsupported;
import io.papermc.paper.persistence.PersistentDataContainerView;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;
import java.util.Date;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import net.kyori.adventure.text.object.PlayerHeadObjectContents;
import org.bukkit.BanEntry;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.OfflinePlayer;
import org.bukkit.Statistic;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Player;

/** Minimal {@link OfflinePlayer} adapter used for offline player lookups and scoreboard entries. */
@SuppressWarnings("deprecation")
public final class FerriteOfflinePlayer implements OfflinePlayer {

    private static final java.util.Set<UUID> OFFLINE_OPS =
            java.util.concurrent.ConcurrentHashMap.newKeySet();
    private static final java.util.Set<UUID> WHITELISTED =
            java.util.concurrent.ConcurrentHashMap.newKeySet();
    private static final java.util.Map<UUID, java.util.Map<String, Integer>> STATISTICS =
            new java.util.concurrent.ConcurrentHashMap<>();
    private static final java.util.Map<UUID, dev.ferrite.api.item.FerritePersistentDataContainer>
            PDCS = new java.util.concurrent.ConcurrentHashMap<>();

    public static java.util.Set<OfflinePlayer> offlineOperators() {
        java.util.Set<OfflinePlayer> result = new java.util.LinkedHashSet<>();
        for (UUID id : OFFLINE_OPS) {
            result.add(new FerriteOfflinePlayer(id, id.toString()));
        }
        return result;
    }

    public static java.util.Set<OfflinePlayer> whitelistedPlayers() {
        java.util.Set<OfflinePlayer> result = new java.util.LinkedHashSet<>();
        for (UUID id : WHITELISTED) {
            result.add(new FerriteOfflinePlayer(id, id.toString()));
        }
        return result;
    }

    private final String name;
    private final UUID uniqueId;

    public FerriteOfflinePlayer(String name) {
        this.name = Objects.requireNonNull(name, "name");
        this.uniqueId = UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8));
    }

    public FerriteOfflinePlayer(UUID uniqueId, String name) {
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
        this.name = name;
    }

    public static OfflinePlayer resolve(String entry) {
        Objects.requireNonNull(entry, "entry");
        if (Bukkit.getServer() != null) {
            Player online = Bukkit.getPlayerExact(entry);
            if (online != null) {
                return online;
            }
        }
        return new FerriteOfflinePlayer(entry);
    }

    @Override
    public boolean isOnline() {
        return getPlayer() != null;
    }

    @Override
    public boolean isConnected() {
        return isOnline();
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public UUID getUniqueId() {
        Player online = getPlayer();
        return online != null ? online.getUniqueId() : uniqueId;
    }

    @Override
    public Player getPlayer() {
        return Bukkit.getServer() == null ? null : Bukkit.getPlayerExact(name);
    }

    @Override
    public boolean isOp() {
        Player online = getPlayer();
        return online != null ? online.isOp() : OFFLINE_OPS.contains(uniqueId);
    }

    @Override
    public void setOp(boolean value) {
        Player online = getPlayer();
        if (online != null) {
            online.setOp(value);
        }
        if (value) {
            OFFLINE_OPS.add(uniqueId);
        } else {
            OFFLINE_OPS.remove(uniqueId);
        }
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile getPlayerProfile() {
        throw Unsupported.exception("OfflinePlayer#getPlayerProfile");
    }

    @Override
    public boolean isBanned() {
        return false;
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Date expires, String source) {
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Instant expires, String source) {
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Duration duration, String source) {
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public boolean isWhitelisted() {
        return WHITELISTED.contains(uniqueId);
    }

    @Override
    public void setWhitelisted(boolean value) {
        if (value) {
            WHITELISTED.add(uniqueId);
        } else {
            WHITELISTED.remove(uniqueId);
        }
    }

    @Override
    public long getFirstPlayed() {
        return 0L;
    }

    @Override
    public long getLastPlayed() {
        return 0L;
    }

    @Override
    public boolean hasPlayedBefore() {
        return isOnline();
    }

    @Override
    public long getLastLogin() {
        return 0L;
    }

    @Override
    public long getLastSeen() {
        return 0L;
    }

    @Override
    public Location getRespawnLocation(boolean loadLocationAndValidate) {
        return null;
    }

    private Map<String, Integer> statsMap() {
        return STATISTICS.computeIfAbsent(
                getUniqueId(), ignored -> new java.util.concurrent.ConcurrentHashMap<>());
    }

    private static String statKey(Statistic statistic, String suffix) {
        Objects.requireNonNull(statistic, "statistic");
        return suffix == null ? statistic.name() : statistic.name() + ":" + suffix;
    }

    @Override
    public void incrementStatistic(Statistic statistic) {
        incrementStatistic(statistic, 1);
    }

    @Override
    public void decrementStatistic(Statistic statistic) {
        decrementStatistic(statistic, 1);
    }

    @Override
    public void incrementStatistic(Statistic statistic, int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap().merge(statKey(statistic, null), amount, Integer::sum);
    }

    @Override
    public void decrementStatistic(Statistic statistic, int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        String key = statKey(statistic, null);
        statsMap().compute(key, (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    @Override
    public void setStatistic(Statistic statistic, int newValue) {
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap().put(statKey(statistic, null), newValue);
    }

    @Override
    public int getStatistic(Statistic statistic) {
        return statsMap().getOrDefault(statKey(statistic, null), 0);
    }

    @Override
    public void incrementStatistic(Statistic statistic, Material material) {
        incrementStatistic(statistic, material, 1);
    }

    @Override
    public void decrementStatistic(Statistic statistic, Material material) {
        decrementStatistic(statistic, material, 1);
    }

    @Override
    public int getStatistic(Statistic statistic, Material material) {
        Objects.requireNonNull(material, "material");
        return statsMap().getOrDefault(statKey(statistic, "m:" + material.name()), 0);
    }

    @Override
    public void incrementStatistic(Statistic statistic, Material material, int amount) {
        Objects.requireNonNull(material, "material");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap().merge(statKey(statistic, "m:" + material.name()), amount, Integer::sum);
    }

    @Override
    public void decrementStatistic(Statistic statistic, Material material, int amount) {
        Objects.requireNonNull(material, "material");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        String key = statKey(statistic, "m:" + material.name());
        statsMap().compute(key, (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    @Override
    public void setStatistic(Statistic statistic, Material material, int newValue) {
        Objects.requireNonNull(material, "material");
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap().put(statKey(statistic, "m:" + material.name()), newValue);
    }

    @Override
    public void incrementStatistic(Statistic statistic, EntityType entityType) {
        incrementStatistic(statistic, entityType, 1);
    }

    @Override
    public void decrementStatistic(Statistic statistic, EntityType entityType) {
        decrementStatistic(statistic, entityType, 1);
    }

    @Override
    public int getStatistic(Statistic statistic, EntityType entityType) {
        Objects.requireNonNull(entityType, "entityType");
        return statsMap().getOrDefault(statKey(statistic, "e:" + entityType.name()), 0);
    }

    @Override
    public void incrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        Objects.requireNonNull(entityType, "entityType");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap().merge(statKey(statistic, "e:" + entityType.name()), amount, Integer::sum);
    }

    @Override
    public void decrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        Objects.requireNonNull(entityType, "entityType");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        String key = statKey(statistic, "e:" + entityType.name());
        statsMap().compute(key, (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    @Override
    public void setStatistic(Statistic statistic, EntityType entityType, int newValue) {
        Objects.requireNonNull(entityType, "entityType");
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap().put(statKey(statistic, "e:" + entityType.name()), newValue);
    }

    @Override
    public Location getLastDeathLocation() {
        return null;
    }

    @Override
    public Location getLocation() {
        Player online = getPlayer();
        return online != null ? online.getLocation() : null;
    }

    @Override
    public PersistentDataContainerView getPersistentDataContainer() {
        return PDCS.computeIfAbsent(
                getUniqueId(), ignored -> new dev.ferrite.api.item.FerritePersistentDataContainer());
    }

    @Override
    public Map<String, Object> serialize() {
        return Map.of("UUID", getUniqueId().toString());
    }

    @Override
    public void applySkinToPlayerHeadContents(PlayerHeadObjectContents.Builder builder) {
        throw Unsupported.exception("OfflinePlayer#applySkinToPlayerHeadContents");
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof OfflinePlayer other && getUniqueId().equals(other.getUniqueId());
    }

    @Override
    public int hashCode() {
        return getUniqueId().hashCode();
    }

    @Override
    public String toString() {
        return "FerriteOfflinePlayer{" + name + "}";
    }
}
