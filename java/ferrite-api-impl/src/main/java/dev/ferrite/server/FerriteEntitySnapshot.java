package dev.ferrite.server;

import java.util.UUID;

/**
 * A Bukkit {@link org.bukkit.entity.EntitySnapshot}: the NBT form plus the live values a
 * snapshot keeps so tests and {@code createEntity} can use them without asking the host again.
 *
 * <p>Every getter is local, so reading a snapshot after the server stopped never calls into the
 * JVM's host bridge and never panics. {@link #isValid()} records whether the entity still existed
 * when the snapshot was taken; {@code createEntity} still creates a new entity from the data.
 */
final class FerriteEntitySnapshot implements org.bukkit.entity.EntitySnapshot {
    /** A marker the host sends when the SNBT does not describe a known entity. */
    static final String INVALID = "\u001binvalid";

    private final org.bukkit.entity.EntityType type;
    private final String snbt;
    private final org.bukkit.Location location;
    private final UUID uniqueId;
    private final double health;
    private final org.bukkit.util.Vector velocity;
    private final String name;
    private volatile org.bukkit.World world;
    private volatile boolean valid = true;

    private FerriteEntitySnapshot(org.bukkit.entity.EntityType type, String snbt, org.bukkit.Location location,
                                  UUID uniqueId, double health, org.bukkit.util.Vector velocity, String name) {
        this.type = java.util.Objects.requireNonNull(type, "type");
        this.snbt = java.util.Objects.requireNonNull(snbt, "snbt");
        this.location = location;
        this.uniqueId = uniqueId;
        this.health = health;
        this.velocity = velocity;
        this.name = name;
    }

    /** Decodes the host's field-separated snapshot reply. */
    static FerriteEntitySnapshot fromWire(String reply, String snbt) {
        var parts = reply.split(java.util.regex.Pattern.quote(String.valueOf(HostBridge.ITEM)), -1);
        if (parts.length != 12) throw new IllegalArgumentException("Ferrite: malformed entity snapshot from the host");
        var key = org.bukkit.NamespacedKey.fromString(parts[0]);
        var type = key == null ? null : org.bukkit.Registry.ENTITY_TYPE.get(key);
        if (type == null) throw new IllegalArgumentException("Ferrite: unknown entity type " + parts[0]);
        var location = new org.bukkit.Location(null,
                Double.parseDouble(parts[1]), Double.parseDouble(parts[2]), Double.parseDouble(parts[3]),
                Float.parseFloat(parts[4]), Float.parseFloat(parts[5]));
        UUID uuid = parts[6].equals(HostBridge.NULL) ? null : UUID.fromString(parts[6]);
        double health = parts[7].equals(HostBridge.NULL) ? Double.NaN : Double.parseDouble(parts[7]);
        var velocity = new org.bukkit.util.Vector(
                Double.parseDouble(parts[8]), Double.parseDouble(parts[9]), Double.parseDouble(parts[10]));
        String name = parts[11].equals(HostBridge.NULL) ? null
                : new String(java.util.Base64.getDecoder().decode(parts[11]), java.nio.charset.StandardCharsets.UTF_8);
        return new FerriteEntitySnapshot(type, snbt, location, uuid, health, velocity, name);
    }

    /** Takes a snapshot of a live handle; values the host does not expose stay absent. */
    static FerriteEntitySnapshot of(org.bukkit.entity.Entity entity) {
        var type = entity.getType();
        var at = entity.getLocation();
        var uuid = entity.getUniqueId();
        var velocity = entity.getVelocity();
        double health = Double.NaN;
        try {
            if (entity instanceof org.bukkit.entity.LivingEntity living) health = living.getHealth();
        } catch (UnsupportedOperationException | IllegalStateException unknown) { /* not exposed */ }
        String name = null;
        try { name = entity.getCustomName(); } catch (UnsupportedOperationException | IllegalStateException unknown) { /* not exposed */ }
        boolean valid;
        try { valid = entity.isValid(); } catch (UnsupportedOperationException | IllegalStateException gone) { valid = false; }
        var fields = new java.util.LinkedHashMap<String, Object>();
        fields.put("id", type.getKey().toString());
        fields.put("Pos", java.util.List.of(at.getX(), at.getY(), at.getZ()));
        fields.put("Rotation", java.util.List.of(at.getYaw(), at.getPitch()));
        fields.put("UUID", words(uuid));
        fields.put("Motion", java.util.List.of(velocity.getX(), velocity.getY(), velocity.getZ()));
        if (!Double.isNaN(health)) fields.put("Health", (float) health);
        if (name != null) fields.put("CustomName", name);
        var snapshot = new FerriteEntitySnapshot(type, Snbt.of(fields), at.clone(), uuid, health, velocity.clone(), name);
        snapshot.world = at.getWorld();
        snapshot.valid = valid;
        return snapshot;
    }

    /** Vanilla's UUID form: four big-endian ints. */
    private static int[] words(UUID uuid) {
        long most = uuid.getMostSignificantBits(), least = uuid.getLeastSignificantBits();
        return new int[] {(int) (most >> 32), (int) most, (int) (least >> 32), (int) least};
    }

    @Override public org.bukkit.entity.EntityType getEntityType() { return type; }
    @Override public String getAsString() { return snbt; }

    @Override public org.bukkit.entity.Entity createEntity(org.bukkit.World world) {
        java.util.Objects.requireNonNull(world, "world");
        var at = location == null ? world.getSpawnLocation() : location.clone();
        at.setWorld(world);
        return createEntity(at);
    }

    @Override public org.bukkit.entity.Entity createEntity(org.bukkit.Location at) {
        java.util.Objects.requireNonNull(at, "location");
        var target = at.clone();
        if (target.getWorld() == null) throw new IllegalArgumentException("Location has no world");
        var entity = target.getWorld().spawnEntity(target, type);
        world = target.getWorld();
        valid = true;
        return entity;
    }

    org.bukkit.Location getLocation() { return location == null ? null : location.clone(); }
    UUID getUniqueId() { return uniqueId; }
    double getHealth() { return health; }
    org.bukkit.util.Vector getVelocity() { return velocity.clone(); }
    String getName() { return name; }
    org.bukkit.World getWorld() { return world; }
    boolean isValid() { return valid; }

    @Override public String toString() {
        return "EntitySnapshot{" + type + (uniqueId == null ? "" : " " + uniqueId) + "}";
    }
}
