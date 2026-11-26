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

/** Minimal {@link OfflinePlayer} adapter used for offline scoreboard entries. */
@SuppressWarnings("deprecation")
final class FerriteOfflinePlayer implements OfflinePlayer {

    private final String name;
    private final UUID uniqueId;

    FerriteOfflinePlayer(String name) {
        this.name = Objects.requireNonNull(name, "name");
        this.uniqueId = UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8));
    }

    static OfflinePlayer resolve(String entry) {
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
        return online != null && online.isOp();
    }

    @Override
    public void setOp(boolean value) {
        Player online = getPlayer();
        if (online != null) {
            online.setOp(value);
            return;
        }
        throw Unsupported.exception("OfflinePlayer#setOp");
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
        return false;
    }

    @Override
    public void setWhitelisted(boolean value) {
        throw Unsupported.exception("OfflinePlayer#setWhitelisted");
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

    @Override
    public void incrementStatistic(Statistic statistic) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public void incrementStatistic(Statistic statistic, int amount) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic, int amount) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public void setStatistic(Statistic statistic, int newValue) {
        throw Unsupported.exception("OfflinePlayer#setStatistic");
    }

    @Override
    public int getStatistic(Statistic statistic) {
        throw Unsupported.exception("OfflinePlayer#getStatistic");
    }

    @Override
    public void incrementStatistic(Statistic statistic, Material material) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic, Material material) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public int getStatistic(Statistic statistic, Material material) {
        throw Unsupported.exception("OfflinePlayer#getStatistic");
    }

    @Override
    public void incrementStatistic(Statistic statistic, Material material, int amount) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic, Material material, int amount) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public void setStatistic(Statistic statistic, Material material, int newValue) {
        throw Unsupported.exception("OfflinePlayer#setStatistic");
    }

    @Override
    public void incrementStatistic(Statistic statistic, EntityType entityType) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic, EntityType entityType) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public int getStatistic(Statistic statistic, EntityType entityType) {
        throw Unsupported.exception("OfflinePlayer#getStatistic");
    }

    @Override
    public void incrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        throw Unsupported.exception("OfflinePlayer#incrementStatistic");
    }

    @Override
    public void decrementStatistic(Statistic statistic, EntityType entityType, int amount) {
        throw Unsupported.exception("OfflinePlayer#decrementStatistic");
    }

    @Override
    public void setStatistic(Statistic statistic, EntityType entityType, int newValue) {
        throw Unsupported.exception("OfflinePlayer#setStatistic");
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
        throw Unsupported.exception("OfflinePlayer#getPersistentDataContainer");
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
