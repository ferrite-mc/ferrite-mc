package dev.ferrite.api.server;

import dev.ferrite.api.entity.FerritePlayerProfile;
import java.net.InetAddress;
import java.net.UnknownHostException;
import java.time.Duration;
import java.time.Instant;
import java.util.Date;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.BanEntry;
import org.bukkit.BanList;
import org.bukkit.ban.IpBanList;
import org.bukkit.ban.ProfileBanList;
import org.bukkit.profile.PlayerProfile;

/** Clean-room in-memory {@link IpBanList} and {@link ProfileBanList} implementations. */
final class FerriteBanLists {

    private FerriteBanLists() {}

    static final class FerriteBanEntry<T> implements BanEntry<T> {
        private final String target;
        private final T banTarget;
        private final Map<String, FerriteBanEntry<T>> backingMap;
        private Date created = new Date();
        private String source = "Ferrite";
        private Date expiration;
        private String reason = "Banned by an operator.";

        FerriteBanEntry(
                String target,
                T banTarget,
                String reason,
                Date expiration,
                String source,
                Map<String, FerriteBanEntry<T>> backingMap) {
            this.target = Objects.requireNonNull(target, "target");
            this.banTarget = banTarget;
            this.backingMap = backingMap;
            if (reason != null) {
                this.reason = reason;
            }
            this.expiration = expiration == null ? null : new Date(expiration.getTime());
            if (source != null) {
                this.source = source;
            }
        }

        @Override
        public String getTarget() {
            return target;
        }

        @Override
        public T getBanTarget() {
            return banTarget;
        }

        @Override
        public Date getCreated() {
            return new Date(created.getTime());
        }

        @Override
        public void setCreated(Date created) {
            this.created = Objects.requireNonNull(created, "created");
        }

        @Override
        public String getSource() {
            return source;
        }

        @Override
        public void setSource(String source) {
            this.source = Objects.requireNonNull(source, "source");
        }

        @Override
        public Date getExpiration() {
            return expiration == null ? null : new Date(expiration.getTime());
        }

        @Override
        public void setExpiration(Date expiration) {
            this.expiration = expiration == null ? null : new Date(expiration.getTime());
        }

        @Override
        public String getReason() {
            return reason;
        }

        @Override
        public void setReason(String reason) {
            this.reason = reason;
        }

        @Override
        public void save() {
            backingMap.put(target, this);
        }

        @Override
        public void remove() {
            backingMap.remove(target, this);
        }
    }

    static final class FerriteIpBanList implements IpBanList {
        private final Map<String, FerriteBanEntry<InetAddress>> entries = new ConcurrentHashMap<>();
        private final Set<String> ipBansSet;

        FerriteIpBanList(Set<String> ipBansSet) {
            this.ipBansSet = ipBansSet;
        }

        private void sync() {
            for (String ip : ipBansSet) {
                entries.computeIfAbsent(ip, k -> new FerriteBanEntry<>(
                        k, parseAddress(k), null, null, null, entries));
            }
        }

        private static InetAddress parseAddress(String target) {
            try {
                return InetAddress.getByName(target);
            } catch (UnknownHostException ignored) {
                return null;
            }
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super InetAddress>> E getBanEntry(String target) {
            sync();
            return target == null ? null : (E) entries.get(target.trim());
        }

        @Override
        public BanEntry<InetAddress> getBanEntry(InetAddress target) {
            return target == null ? null : getBanEntry(target.getHostAddress());
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super InetAddress>> E addBan(
                String target, String reason, Date expires, String source) {
            Objects.requireNonNull(target, "target");
            String key = target.trim();
            FerriteBanEntry<InetAddress> entry =
                    new FerriteBanEntry<>(key, parseAddress(key), reason, expires, source, entries);
            entries.put(key, entry);
            ipBansSet.add(key);
            return (E) entry;
        }

        @Override
        public BanEntry<InetAddress> addBan(
                InetAddress target, String reason, Date expires, String source) {
            Objects.requireNonNull(target, "target");
            String key = target.getHostAddress();
            FerriteBanEntry<InetAddress> entry =
                    new FerriteBanEntry<>(key, target, reason, expires, source, entries);
            entries.put(key, entry);
            ipBansSet.add(key);
            return entry;
        }

        @Override
        public BanEntry<InetAddress> addBan(
                InetAddress target, String reason, Instant expires, String source) {
            return addBan(target, reason, expires == null ? null : Date.from(expires), source);
        }

        @Override
        public BanEntry<InetAddress> addBan(
                InetAddress target, String reason, Duration duration, String source) {
            Date exp = duration == null ? null : Date.from(Instant.now().plus(duration));
            return addBan(target, reason, exp, source);
        }

        @Override
        @SuppressWarnings({"rawtypes", "deprecation"})
        public Set<BanEntry> getBanEntries() {
            sync();
            return new LinkedHashSet<>(entries.values());
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super InetAddress>> Set<E> getEntries() {
            sync();
            Set<E> out = new LinkedHashSet<>();
            for (FerriteBanEntry<InetAddress> entry : entries.values()) {
                out.add((E) entry);
            }
            return out;
        }

        @Override
        public boolean isBanned(InetAddress target) {
            return target != null && isBanned(target.getHostAddress());
        }

        @Override
        public boolean isBanned(String target) {
            sync();
            return target != null && (entries.containsKey(target.trim()) || ipBansSet.contains(target.trim()));
        }

        @Override
        public void pardon(InetAddress target) {
            if (target != null) {
                pardon(target.getHostAddress());
            }
        }

        @Override
        public void pardon(String target) {
            if (target != null) {
                String key = target.trim();
                entries.remove(key);
                ipBansSet.remove(key);
            }
        }
    }

    static final class FerriteProfileBanList implements ProfileBanList {
        private final Map<String, FerriteBanEntry<com.destroystokyo.paper.profile.PlayerProfile>> entries =
                new ConcurrentHashMap<>();

        private static String keyOf(PlayerProfile target) {
            if (target.getUniqueId() != null) {
                return target.getUniqueId().toString();
            }
            return target.getName() == null ? "unknown" : target.getName();
        }

        private static com.destroystokyo.paper.profile.PlayerProfile toPaper(PlayerProfile profile) {
            if (profile instanceof com.destroystokyo.paper.profile.PlayerProfile paper) {
                return paper;
            }
            return new FerritePlayerProfile(profile.getUniqueId(), profile.getName());
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E getBanEntry(
                String target) {
            return target == null ? null : (E) entries.get(target);
        }

        @Override
        public BanEntry<com.destroystokyo.paper.profile.PlayerProfile> getBanEntry(
                com.destroystokyo.paper.profile.PlayerProfile target) {
            return target == null ? null : entries.get(keyOf(target));
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E getBanEntry(
                PlayerProfile target) {
            return target == null ? null : (E) entries.get(keyOf(target));
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E addBan(
                String target, String reason, Date expires, String source) {
            Objects.requireNonNull(target, "target");
            var entry = new FerriteBanEntry<>(
                    target, new FerritePlayerProfile(null, target), reason, expires, source, entries);
            entries.put(target, entry);
            return (E) entry;
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E addBan(
                PlayerProfile target, String reason, Date expires, String source) {
            Objects.requireNonNull(target, "target");
            String key = keyOf(target);
            var entry = new FerriteBanEntry<>(key, toPaper(target), reason, expires, source, entries);
            entries.put(key, entry);
            return (E) entry;
        }

        @Override
        public BanEntry<com.destroystokyo.paper.profile.PlayerProfile> addBan(
                com.destroystokyo.paper.profile.PlayerProfile target,
                String reason,
                Date expires,
                String source) {
            Objects.requireNonNull(target, "target");
            String key = keyOf(target);
            var entry = new FerriteBanEntry<>(key, target, reason, expires, source, entries);
            entries.put(key, entry);
            return entry;
        }

        @Override
        public BanEntry<com.destroystokyo.paper.profile.PlayerProfile> addBan(
                com.destroystokyo.paper.profile.PlayerProfile target,
                String reason,
                Instant expires,
                String source) {
            return addBan(target, reason, expires == null ? null : Date.from(expires), source);
        }

        @Override
        public BanEntry<com.destroystokyo.paper.profile.PlayerProfile> addBan(
                com.destroystokyo.paper.profile.PlayerProfile target,
                String reason,
                Duration duration,
                String source) {
            Date exp = duration == null ? null : Date.from(Instant.now().plus(duration));
            return addBan(target, reason, exp, source);
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E addBan(
                PlayerProfile target, String reason, Instant expires, String source) {
            return (E) addBan(target, reason, expires == null ? null : Date.from(expires), source);
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E addBan(
                PlayerProfile target, String reason, Duration duration, String source) {
            Date exp = duration == null ? null : Date.from(Instant.now().plus(duration));
            return (E) addBan(target, reason, exp, source);
        }

        @Override
        @SuppressWarnings({"rawtypes", "deprecation"})
        public Set<BanEntry> getBanEntries() {
            return new LinkedHashSet<>(entries.values());
        }

        @Override
        @SuppressWarnings("unchecked")
        public <E extends BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> Set<E> getEntries() {
            Set<E> out = new LinkedHashSet<>();
            for (var entry : entries.values()) {
                out.add((E) entry);
            }
            return out;
        }

        @Override
        public boolean isBanned(com.destroystokyo.paper.profile.PlayerProfile target) {
            return target != null && entries.containsKey(keyOf(target));
        }

        @Override
        public boolean isBanned(PlayerProfile target) {
            return target != null && entries.containsKey(keyOf(target));
        }

        @Override
        public boolean isBanned(String target) {
            return target != null && entries.containsKey(target);
        }

        @Override
        public void pardon(com.destroystokyo.paper.profile.PlayerProfile target) {
            if (target != null) {
                entries.remove(keyOf(target));
            }
        }

        @Override
        public void pardon(PlayerProfile target) {
            if (target != null) {
                entries.remove(keyOf(target));
            }
        }

        @Override
        public void pardon(String target) {
            if (target != null) {
                entries.remove(target);
            }
        }
    }
}
