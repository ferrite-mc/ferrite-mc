package dev.ferrite.server;

import com.destroystokyo.paper.profile.PlayerProfile;
import java.time.Duration;
import java.time.Instant;
import java.util.Date;
import java.util.HashSet;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;

/** Player bans: remembered here and applied through the console ban and pardon commands. */
@SuppressWarnings({"unchecked", "rawtypes"})
final class FerriteProfileBanList implements org.bukkit.ban.ProfileBanList {
    private final Map<String, Entry> bans = new ConcurrentHashMap<>();
    /** Expiry runs off the server threads: it only removes local state and asks the host to pardon. */
    private static final ScheduledExecutorService EXPIRY = Executors.newSingleThreadScheduledExecutor(task -> {
        var thread = new Thread(task, "ferrite-ban-expiry");
        thread.setDaemon(true);
        return thread;
    });

    private static String key(String name) { return name.toLowerCase(Locale.ROOT); }
    private static String nameOf(org.bukkit.profile.PlayerProfile profile) {
        var name = java.util.Objects.requireNonNull(profile, "profile").getName();
        if (name == null) throw new IllegalArgumentException("Profile needs a name to be banned");
        return name;
    }
    private Entry current(String name) {
        var entry = bans.get(key(name));
        if (entry != null && entry.expires != null && entry.expires.before(new Date())) { expire(entry); return null; }
        return entry;
    }

    @Override public org.bukkit.BanEntry<PlayerProfile> getBanEntry(PlayerProfile profile) { return current(nameOf(profile)); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E getBanEntry(org.bukkit.profile.PlayerProfile profile) { return (E) current(nameOf(profile)); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E getBanEntry(String target) { return (E) current(target); }

    @Override public org.bukkit.BanEntry<PlayerProfile> addBan(PlayerProfile profile, String reason, Date expires, String source) { return (org.bukkit.BanEntry<PlayerProfile>) add(profile, reason, expires, source); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E addBan(org.bukkit.profile.PlayerProfile profile, String reason, Date expires, String source) { return (E) add(profile, reason, expires, source); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E addBan(String target, String reason, Date expires, String source) {
        return (E) add(new FerriteProfile(null, target), reason, expires, source);
    }
    @Override public org.bukkit.BanEntry<PlayerProfile> addBan(PlayerProfile profile, String reason, Instant expires, String source) { return addBan(profile, reason, expires == null ? null : Date.from(expires), source); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E addBan(org.bukkit.profile.PlayerProfile profile, String reason, Instant expires, String source) { return (E) add(profile, reason, expires == null ? null : Date.from(expires), source); }
    @Override public org.bukkit.BanEntry<PlayerProfile> addBan(PlayerProfile profile, String reason, Duration duration, String source) { return addBan(profile, reason, duration == null ? (Date) null : Date.from(Instant.now().plus(duration)), source); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> E addBan(org.bukkit.profile.PlayerProfile profile, String reason, Duration duration, String source) { return (E) add(profile, reason, duration == null ? null : Date.from(Instant.now().plus(duration)), source); }

    private Entry add(org.bukkit.profile.PlayerProfile profile, String reason, Date expires, String source) {
        var name = nameOf(profile);
        var entry = new Entry(name, profile instanceof PlayerProfile full ? full : new FerriteProfile(profile.getUniqueId(), name), reason, expires, source);
        bans.put(key(name), entry);
        entry.save();
        return entry;
    }

    @Override public Set<org.bukkit.BanEntry> getBanEntries() { return (Set) getEntries(); }
    @Override public <E extends org.bukkit.BanEntry<? super PlayerProfile>> Set<E> getEntries() {
        var out = new HashSet<E>();
        for (var name : bans.keySet()) { var entry = current(name); if (entry != null) out.add((E) entry); }
        return out;
    }
    @Override public boolean isBanned(PlayerProfile profile) { return current(nameOf(profile)) != null; }
    @Override public boolean isBanned(org.bukkit.profile.PlayerProfile profile) { return current(nameOf(profile)) != null; }
    @Override public boolean isBanned(String target) { return current(target) != null; }
    @Override public void pardon(PlayerProfile profile) { pardon(nameOf(profile)); }
    @Override public void pardon(org.bukkit.profile.PlayerProfile profile) { pardon(nameOf(profile)); }
    @Override public void pardon(String target) {
        HostBridge.callVoid("Server", "server", "pardonProfile(String)", target);
        var entry = bans.remove(key(target));
        if (entry != null) entry.cancelExpiry();
    }

    /** Drops an expired entry and tells the host, without failing the expiry thread. */
    private void expire(Entry entry) {
        if (!bans.remove(key(entry.name), entry)) return;
        entry.cancelExpiry();
        try {
            HostBridge.callVoid("Server", "server", "pardonProfile(String)", entry.name);
        } catch (UnsupportedOperationException offline) {
            // No host is attached; the local ban is still gone.
        }
    }

    private final class Entry implements org.bukkit.BanEntry<PlayerProfile> {
        private final String name;
        private final PlayerProfile profile;
        private Date created = new Date();
        private String reason, source;
        private Date expires;
        private ScheduledFuture<?> expiryTask;
        Entry(String name, PlayerProfile profile, String reason, Date expires, String source) {
            this.name = name; this.profile = profile; this.reason = reason; this.source = source;
            this.expires = expires == null ? null : new Date(expires.getTime());
        }
        @Override public String getTarget() { return name; }
        @Override public PlayerProfile getBanTarget() { return profile; }
        @Override public Date getCreated() { return new Date(created.getTime()); }
        @Override public void setCreated(Date value) { created = new Date(value.getTime()); }
        @Override public String getSource() { return source; }
        @Override public void setSource(String value) { source = value; }
        @Override public Date getExpiration() { return expires == null ? null : new Date(expires.getTime()); }
        @Override public void setExpiration(Date value) { expires = value == null ? null : new Date(value.getTime()); schedule(); }
        @Override public String getReason() { return reason; }
        @Override public void setReason(String value) { reason = value; }
        @Override public void save() {
            HostBridge.callVoid("Server", "server", "banProfile(String,String)", name, reason == null ? "" : reason);
            schedule();
        }
        private synchronized void schedule() {
            if (expiryTask != null) { expiryTask.cancel(false); expiryTask = null; }
            if (expires == null) return;
            long delay = Math.max(0, expires.getTime() - System.currentTimeMillis());
            expiryTask = EXPIRY.schedule(() -> FerriteProfileBanList.this.expire(this), delay, TimeUnit.MILLISECONDS);
        }
        private synchronized void cancelExpiry() {
            if (expiryTask != null) { expiryTask.cancel(false); expiryTask = null; }
        }
        @Override public void remove() { pardon(name); }
    }
}
