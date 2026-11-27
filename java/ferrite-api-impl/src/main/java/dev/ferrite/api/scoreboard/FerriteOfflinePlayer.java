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
        return new dev.ferrite.api.entity.FerritePlayerProfile(getUniqueId(), name);
    }

    @Override
    public boolean isBanned() {
        return false;
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Date expires, String source) {
        // Ban entries live in the Rust server's ban list, which the facade cannot mutate.
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Instant expires, String source) {
        // Ban entries live in the Rust server's ban list, which the facade cannot mutate.
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(
            String reason, Duration duration, String source) {
        // Ban entries live in the Rust server's ban list, which the facade cannot mutate.
        throw Unsupported.exception("OfflinePlayer#ban");
    }

    @Override
    public boolean isWhitelisted() {
        return isWhitelisted(getUniqueId());
    }

    @Override
    public void setWhitelisted(boolean value) {
        setWhitelisted(getUniqueId(), value);
    }

    public static boolean isWhitelisted(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        return WHITELISTED.contains(uuid);
    }

    public static void setWhitelisted(UUID uuid, boolean value) {
        Objects.requireNonNull(uuid, "uuid");
        if (value) {
            WHITELISTED.add(uuid);
        } else {
            WHITELISTED.remove(uuid);
        }
    }

    @Override
    public long getFirstPlayed() {
        Player online = getPlayer();
        return online == null ? 0L : online.getFirstPlayed();
    }

    @Override
    public long getLastPlayed() {
        Player online = getPlayer();
        return online == null ? 0L : online.getLastPlayed();
    }

    @Override
    public boolean hasPlayedBefore() {
        return isOnline();
    }

    @Override
    public long getLastLogin() {
        Player online = getPlayer();
        return online == null ? 0L : online.getLastLogin();
    }

    @Override
    public long getLastSeen() {
        Player online = getPlayer();
        return online == null ? 0L : online.getLastSeen();
    }

    @Override
    public Location getRespawnLocation(boolean loadLocationAndValidate) {
        Player online = getPlayer();
        return online == null ? null : online.getRespawnLocation(loadLocationAndValidate);
    }

    private static Map<String, Integer> statsMap(UUID uuid) {
        return STATISTICS.computeIfAbsent(
                uuid, ignored -> new java.util.concurrent.ConcurrentHashMap<>());
    }

    private static String statKey(Statistic statistic, String suffix) {
        Objects.requireNonNull(statistic, "statistic");
        return suffix == null ? statistic.name() : statistic.name() + ":" + suffix;
    }

    public static void incrementStat(UUID uuid, Statistic statistic, int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap(uuid).merge(statKey(statistic, null), amount, Integer::sum);
    }

    public static void decrementStat(UUID uuid, Statistic statistic, int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap(uuid).compute(statKey(statistic, null), (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    public static void setStat(UUID uuid, Statistic statistic, int newValue) {
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap(uuid).put(statKey(statistic, null), newValue);
    }

    public static int getStat(UUID uuid, Statistic statistic) {
        return statsMap(uuid).getOrDefault(statKey(statistic, null), 0);
    }

    public static void incrementStat(UUID uuid, Statistic statistic, Material material, int amount) {
        Objects.requireNonNull(material, "material");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap(uuid).merge(statKey(statistic, "m:" + material.name()), amount, Integer::sum);
    }

    public static void decrementStat(UUID uuid, Statistic statistic, Material material, int amount) {
        Objects.requireNonNull(material, "material");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        String key = statKey(statistic, "m:" + material.name());
        statsMap(uuid).compute(key, (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    public static void setStat(UUID uuid, Statistic statistic, Material material, int newValue) {
        Objects.requireNonNull(material, "material");
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap(uuid).put(statKey(statistic, "m:" + material.name()), newValue);
    }

    public static int getStat(UUID uuid, Statistic statistic, Material material) {
        Objects.requireNonNull(material, "material");
        return statsMap(uuid).getOrDefault(statKey(statistic, "m:" + material.name()), 0);
    }

    public static void incrementStat(UUID uuid, Statistic statistic, EntityType entityType, int amount) {
        Objects.requireNonNull(entityType, "entityType");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        statsMap(uuid).merge(statKey(statistic, "e:" + entityType.name()), amount, Integer::sum);
    }

    public static void decrementStat(UUID uuid, Statistic statistic, EntityType entityType, int amount) {
        Objects.requireNonNull(entityType, "entityType");
        if (amount < 0) {
            throw new IllegalArgumentException("amount must be >= 0");
        }
        String key = statKey(statistic, "e:" + entityType.name());
        statsMap(uuid).compute(key, (k, v) -> Math.max(0, (v == null ? 0 : v) - amount));
    }

    public static void setStat(UUID uuid, Statistic statistic, EntityType entityType, int newValue) {
        Objects.requireNonNull(entityType, "entityType");
        if (newValue < 0) {
            throw new IllegalArgumentException("newValue must be >= 0");
        }
        statsMap(uuid).put(statKey(statistic, "e:" + entityType.name()), newValue);
    }

    public static int getStat(UUID uuid, Statistic statistic, EntityType entityType) {
        Objects.requireNonNull(entityType, "entityType");
        return statsMap(uuid).getOrDefault(statKey(statistic, "e:" + entityType.name()), 0);
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
        incrementStat(getUniqueId(), statistic, amount);
    }

    @Override
    public void decrementStatistic(Statistic statistic, int amount) {
        decrementStat(getUniqueId(), statistic, amount);
    }

    @Override
    public void setStatistic(Statistic statistic, int newValue) {
        setStat(getUniqueId(), statistic, newValue);
    }

    @Override
    public int getStatistic(Statistic statistic) {
        return getStat(getUniqueId(), statistic);
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
        return getStat(getUniqueId(), statistic, material);
    }

    @Override
    public void incrementStatistic(Statistic statistic, Material material, int amount) {
        incrementStat(getUniqueId(), statistic, material, amount);
    }

    @Override
    public void decrementStatistic(Statistic statistic, Material material, int amount) {
        decrementStat(getUniqueId(), statistic, material, amount);
    }

    @Override
    public void setStatistic(Statistic statistic, Material material, int newValue) {
        setStat(getUniqueId(), statistic, material, newValue);
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
        return getStat(getUniqueId(), statistic, entityType);
    }

    @Override
    public void incrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        incrementStat(getUniqueId(), statistic, entityType, amount);
    }

    @Override
    public void decrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        decrementStat(getUniqueId(), statistic, entityType, amount);
    }

    @Override
    public void setStatistic(Statistic statistic, EntityType entityType, int newValue) {
        setStat(getUniqueId(), statistic, entityType, newValue);
    }

    @Override
    public Location getLastDeathLocation() {
        // Death locations are not persisted by the facade; a live player has none either.
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
        // No skin profile/texture data is stored for offline players.
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
