package dev.ferrite.server;

import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Pattern;

/** Decodes World#getEntities replies and keeps one live wrapper per entity UUID. */
final class WorldEntities {
    static final char FIELD = '\u001e';
    private final java.util.Map<UUID, org.bukkit.entity.Entity> cache = new ConcurrentHashMap<>();

    org.bukkit.entity.Entity get(UUID uuid, org.bukkit.entity.EntityType type, org.bukkit.Location at) {
        return cache.computeIfAbsent(uuid, ignored -> SpawnedEntity.of(type, at, at.getWorld(), uuid));
    }

    static java.util.List<org.bukkit.entity.Entity> decode(String reply, org.bukkit.World world, WorldEntities cache) {
        if (reply == null || reply.isEmpty()) {
            cache.cache.clear();
            return java.util.List.of();
        }
        var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
        var live = new java.util.HashSet<UUID>();
        for (String record : reply.split(Pattern.quote(String.valueOf(HostBridge.ITEM)), -1)) {
            if (record.isEmpty()) continue;
            String[] fields = record.split(Pattern.quote(String.valueOf(FIELD)), -1);
            if (fields.length != 7) throw new IllegalStateException("Ferrite: malformed entity query reply");
            UUID uuid = UUID.fromString(fields[0]);
            live.add(uuid);
            var type = type(fields[1]);
            double x = Double.parseDouble(fields[2]);
            double y = Double.parseDouble(fields[3]);
            double z = Double.parseDouble(fields[4]);
            float yaw = Float.parseFloat(fields[5]);
            float pitch = Float.parseFloat(fields[6]);
            if (!Double.isFinite(x) || !Double.isFinite(y) || !Double.isFinite(z)
                    || !Float.isFinite(yaw) || !Float.isFinite(pitch)) {
                throw new IllegalStateException("Ferrite: non-finite entity position");
            }
            out.add(cache.get(uuid, type, new org.bukkit.Location(world, x, y, z, yaw, pitch)));
        }
        cache.cache.keySet().retainAll(live);
        return java.util.List.copyOf(out);
    }

    private static org.bukkit.entity.EntityType type(String kind) {
        String path = kind;
        int colon = kind.indexOf(':');
        if (colon >= 0) path = kind.substring(colon + 1);
        var type = org.bukkit.entity.EntityType.fromName(kind);
        if (type == null) type = org.bukkit.entity.EntityType.fromName(path);
        if (type == null) {
            try { type = org.bukkit.entity.EntityType.valueOf(path.toUpperCase(java.util.Locale.ROOT)); }
            catch (IllegalArgumentException unknown) { type = null; }
        }
        if (type == null || type == org.bukkit.entity.EntityType.UNKNOWN) {
            throw new UnsupportedOperationException("Ferrite: unsupported entity kind " + kind);
        }
        return type;
    }
}
