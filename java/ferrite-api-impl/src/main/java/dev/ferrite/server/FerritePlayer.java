package dev.ferrite.server;

/** A player snapshot; effects are sent to the host. */
final class FerritePlayer extends org.bukkit.craftbukkit.entity.CraftHumanEntity {
    private final FerriteServer server;
    private final java.util.UUID id;
    private final String name;
    private boolean operator;
    private final FerriteDataContainer data = new FerriteDataContainer();
    private final FerritePlayerInventory inventory = new FerritePlayerInventory(this);
    private net.kyori.adventure.text.Component display;
    private org.bukkit.Location location;
    private double health = 20.0;
    private int food = 20;
    private int experienceLevel;
    private float experienceProgress;
    private int totalExperience;
    private org.bukkit.GameMode mode = org.bukkit.GameMode.SURVIVAL;
    private FerritePlayer(FerriteServer server, java.util.UUID id, String name, boolean operator, double health, int food, int mode, org.bukkit.Location location) {
        this.server = server; this.id = id; this.name = name; this.operator = operator; this.health = health; this.food = food; this.mode = java.util.Objects.requireNonNull(org.bukkit.GameMode.getByValue(mode)); this.location = location;
        this.perm = new org.bukkit.permissions.PermissibleBase(this);
    }
    static org.bukkit.entity.Player create(FerriteServer server, java.util.UUID id, String name, boolean operator, double health, int food, int mode, org.bukkit.Location location) { return new FerritePlayer(server, id, name, operator, health, food, mode, location); }
    void inventorySnapshot(int slot, String material, int amount) { inventory.setSnapshot(slot, material, amount); }
    /** Refreshes the host-owned state without replacing the player object plugins already hold. */
    void updateSnapshot(double health, int food, int mode, org.bukkit.Location location, boolean operator) {
        this.health = health; this.food = food; this.mode = java.util.Objects.requireNonNull(org.bukkit.GameMode.getByValue(mode));
        this.location = location; this.operator = operator; this.perm.recalculatePermissions();
    }
    void updateLocation(org.bukkit.Location value) { this.location = value; }
    @Override public org.bukkit.inventory.PlayerInventory getInventory() { return inventory.proxy; }
    @Override public java.util.UUID getUniqueId() { return id; }
    @Override public String getName() { return name; }
    @Override public String getDisplayName() { return net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(displayName()); }
    @Override public void setDisplayName(String value) { displayName(value == null ? null : net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().deserialize(value)); }
    @Override public net.kyori.adventure.text.Component displayName() { return display == null ? net.kyori.adventure.text.Component.text(name) : display; }
    @Override public void displayName(net.kyori.adventure.text.Component value) { String json = value == null ? null : net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().serialize(value); if (!Native.playerVisual(id.toString(), 0, json, null, 0, 0, 0)) throw new IllegalStateException("No player command handler"); display = value; }
    @Override public void kickPlayer(String reason) { kick(reason == null ? null : net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().deserialize(reason), org.bukkit.event.player.PlayerKickEvent.Cause.PLUGIN); }
    @Override public void kick(net.kyori.adventure.text.Component reason, org.bukkit.event.player.PlayerKickEvent.Cause cause) { if (cause != org.bukkit.event.player.PlayerKickEvent.Cause.PLUGIN) throw new UnsupportedOperationException("Ferrite: unsupported kick cause " + cause); String json = reason == null ? null : net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().serialize(reason); if (!Native.playerVisual(id.toString(), 1, json, null, 0, 0, 0)) throw new IllegalStateException("No player command handler"); }
    private void play(org.bukkit.Location location, String sound, org.bukkit.SoundCategory category, float volume, float pitch, boolean seeded, long seed) { var world = java.util.Objects.requireNonNull(location.getWorld()); if (!Native.playerSound(id.toString(), world.getUID().toString(), sound, category.ordinal(), location.getX(), location.getY(), location.getZ(), volume, pitch, seeded, seed)) throw new IllegalStateException("No player command handler or invalid sound"); }
    @Override public void playSound(org.bukkit.Location location, String sound, org.bukkit.SoundCategory category, float volume, float pitch) { play(location, sound, category, volume, pitch, false, 0); }
    @Override public void playSound(org.bukkit.Location location, String sound, org.bukkit.SoundCategory category, float volume, float pitch, long seed) { play(location, sound, category, volume, pitch, true, seed); }
    @Override public void playSound(org.bukkit.Location location, org.bukkit.Sound sound, org.bukkit.SoundCategory category, float volume, float pitch) { playSound(location, sound.getKey().toString(), category, volume, pitch); }
    @Override public void playSound(org.bukkit.Location location, org.bukkit.Sound sound, org.bukkit.SoundCategory category, float volume, float pitch, long seed) { playSound(location, sound.getKey().toString(), category, volume, pitch, seed); }
    @Override public void playSound(org.bukkit.Location location, String sound, float volume, float pitch) { playSound(location, sound, org.bukkit.SoundCategory.MASTER, volume, pitch); }
    @Override public void playSound(org.bukkit.Location location, org.bukkit.Sound sound, float volume, float pitch) { playSound(location, sound, org.bukkit.SoundCategory.MASTER, volume, pitch); }
    @Override public void sendTitle(String title, String subtitle) { sendTitle(title, subtitle, -1, -1, -1); }
    @Override public void sendTitle(String title, String subtitle, int fadeIn, int stay, int fadeOut) { if (!Native.playerVisual(id.toString(), 2, title, subtitle, fadeIn, stay, fadeOut)) throw new IllegalStateException("No player command handler"); }
    @Override public org.bukkit.Server getServer() { return server; }
    @Override public org.bukkit.entity.Player getPlayer() { return this; }
    @Override public boolean isOnline() { return server.getPlayer(id) == this; }
    @Override public boolean isOp() { return operator; }
    @Override public void setOp(boolean value) { if (!Native.playerOperator(id.toString(), value)) throw new IllegalStateException("No player command handler"); operator = value; perm.recalculatePermissions(); }
    /** Players registered before their first move fall back to the main world spawn, as CraftBukkit does. */
    private org.bukkit.Location resolvedLocation() {
        if (location != null) return location;
        var worlds = server.getWorlds();
        if (worlds.isEmpty()) throw new IllegalStateException("Player location unavailable: the server has no world");
        return worlds.getFirst().getSpawnLocation();
    }
    @Override public org.bukkit.World getWorld() { return resolvedLocation().getWorld(); }
    @Override public org.bukkit.Location getLocation() { return resolvedLocation().clone(); }
    @Override public boolean teleport(org.bukkit.Location destination) { var world = java.util.Objects.requireNonNull(destination.getWorld()); if (!Native.playerTeleport(id.toString(), world.getUID().toString(), destination.getX(), destination.getY(), destination.getZ(), destination.getYaw(), destination.getPitch())) return false; location = destination.clone(); return true; }
    @Override public boolean teleport(org.bukkit.Location destination, org.bukkit.event.player.PlayerTeleportEvent.TeleportCause cause) { return teleport(destination); }
    @Override public java.util.concurrent.CompletableFuture<Boolean> teleportAsync(org.bukkit.Location destination) { return java.util.concurrent.CompletableFuture.completedFuture(teleport(destination)); }
    @Override public java.util.concurrent.CompletableFuture<Boolean> teleportAsync(org.bukkit.Location destination, org.bukkit.event.player.PlayerTeleportEvent.TeleportCause cause) { return teleportAsync(destination); }
    @Override public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() { return data; }
    @Override public double getHealth() { return health; }
    @Override public void setHealth(double value) { if (!Double.isFinite(value) || value < 0) throw new IllegalArgumentException("health"); health = value; Native.playerState(id.toString(), 0, value); }
    @Override public int getFoodLevel() { return food; }
    @Override public void setFoodLevel(int value) { if (value < 0 || value > 20) throw new IllegalArgumentException("food"); food = value; Native.playerState(id.toString(), 1, value); }
    @Override public int getLevel() { return experienceLevel; }
    @Override public void setLevel(int value) { if (value < 0) throw new IllegalArgumentException("level"); experienceLevel = value; Native.playerState(id.toString(), 3, value); }
    @Override public float getExp() { return experienceProgress; }
    @Override public void setExp(float value) { if (!(value >= 0.0f && value <= 1.0f)) throw new IllegalArgumentException("exp"); experienceProgress = value; Native.playerState(id.toString(), 4, value); }
    @Override public int getTotalExperience() { return totalExperience; }
    @Override public void setTotalExperience(int value) { if (value < 0) throw new IllegalArgumentException("total experience"); totalExperience = value; Native.playerState(id.toString(), 5, value); }
    @Override public org.bukkit.GameMode getGameMode() { return mode; }
    @Override public void setGameMode(org.bukkit.GameMode value) { mode = java.util.Objects.requireNonNull(value); Native.playerState(id.toString(), 2, value.getValue()); }
    @Override public void sendMessage(String message) { Native.playerMessage(id.toString(), message, false); }
    @Override public void sendMessage(String[] messages) { for (String message : messages) sendMessage(message); }
    @Override public void sendMessage(java.util.UUID sender, String message) { sendMessage(message); }
    @Override public void sendMessage(java.util.UUID sender, String[] messages) { sendMessage(messages); }
    @Override public void sendMessage(net.kyori.adventure.text.Component component) { Native.playerMessage(id.toString(), net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().serialize(component), true); }
    // --- Entity state the bridge owns: metadata, scoreboard tags, derived location values. ---
    private final java.util.Map<String, java.util.Map<org.bukkit.plugin.Plugin, org.bukkit.metadata.MetadataValue>> metadata = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<String> scoreboardTags = java.util.concurrent.ConcurrentHashMap.newKeySet();
    @Override public void setMetadata(String key, org.bukkit.metadata.MetadataValue value) { var owner = java.util.Objects.requireNonNull(value.getOwningPlugin(), "owning plugin"); metadata.computeIfAbsent(key, k -> new java.util.concurrent.ConcurrentHashMap<>()).put(owner, value); }
    @Override public java.util.List<org.bukkit.metadata.MetadataValue> getMetadata(String key) { var values = metadata.get(key); return values == null ? java.util.List.of() : java.util.List.copyOf(values.values()); }
    @Override public boolean hasMetadata(String key) { var values = metadata.get(key); return values != null && !values.isEmpty(); }
    @Override public void removeMetadata(String key, org.bukkit.plugin.Plugin owner) { var values = metadata.get(key); if (values != null) { values.remove(owner); if (values.isEmpty()) metadata.remove(key, values); } }
    /** Vanilla caps an entity at 1024 tags. */
    @Override public boolean addScoreboardTag(String tag) { return scoreboardTags.size() < 1024 && scoreboardTags.add(tag); }
    @Override public boolean removeScoreboardTag(String tag) { return scoreboardTags.remove(tag); }
    @Override public java.util.Set<String> getScoreboardTags() { return scoreboardTags; }
    @Override public String getScoreboardEntryName() { return name; }
    @Override public org.bukkit.entity.EntityType getType() { return org.bukkit.entity.EntityType.PLAYER; }
    @Override public double getX() { return resolvedLocation().getX(); }
    @Override public double getY() { return resolvedLocation().getY(); }
    @Override public double getZ() { return resolvedLocation().getZ(); }
    @Override public float getYaw() { return resolvedLocation().getYaw(); }
    @Override public float getPitch() { return resolvedLocation().getPitch(); }
    @Override public org.bukkit.Location getLocation(org.bukkit.Location target) { if (target == null) return null; var at = resolvedLocation(); target.setWorld(at.getWorld()); target.setX(at.getX()); target.setY(at.getY()); target.setZ(at.getZ()); target.setYaw(at.getYaw()); target.setPitch(at.getPitch()); return target; }
    /** Vanilla standing eye height; the host does not report sneaking or swimming poses yet. */
    @Override public double getEyeHeight() { return 1.62; }
    @Override public double getEyeHeight(boolean ignorePose) { return 1.62; }
    @Override public org.bukkit.Location getEyeLocation() { return getLocation().add(0, getEyeHeight(), 0); }
    @Override public boolean isDead() { return health <= 0; }
    @Override public boolean isValid() { return isOnline() && !isDead(); }
    @Override public boolean isConnected() { return isOnline(); }
    @Override public double getMaxHealth() { return 20.0; }
    @Override public void heal(double amount) { heal(amount, org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason.CUSTOM); }
    @Override public void heal(double amount, org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason reason) { if (!(amount > 0)) throw new IllegalArgumentException("amount must be positive"); setHealth(Math.min(getMaxHealth(), health + amount)); }
    /** Vanilla {@code Player#getXpNeededForNextLevel}. */
    @Override public int getExpToLevel() { return experienceToLevel(experienceLevel); }
    @Override public int getExperiencePointsNeededForNextLevel() { return getExpToLevel(); }
    static int experienceToLevel(int level) { return level >= 30 ? 112 + (level - 30) * 9 : level >= 15 ? 37 + (level - 15) * 5 : 7 + level * 2; }
    /** Points banked in the current level plus every completed level, as Paper computes it. */
    @Override public int calculateTotalExperiencePoints() { int points = 0; for (int level = 0; level < experienceLevel; level++) points += experienceToLevel(level); return points + Math.round(experienceProgress * getExpToLevel()); }
    /** Vanilla {@code Player#giveExperiencePoints}: progress carries over into new levels. */
    @Override public void giveExp(int amount) { giveExp(amount, false); }
    @Override public void giveExp(int amount, boolean applyMending) {
        int level = experienceLevel; float progress = experienceProgress + (float) amount / experienceToLevel(level);
        int total = Math.max(0, Math.min(totalExperience + amount, Integer.MAX_VALUE));
        while (progress < 0) { if (level == 0) { progress = 0; break; } level--; progress = 1 + progress * experienceToLevel(level + 1) / experienceToLevel(level); }
        while (progress >= 1) { progress = (progress - 1) * experienceToLevel(level); level++; progress /= experienceToLevel(level); }
        setTotalExperience(total); if (level != experienceLevel) setLevel(level); setExp(Math.max(0f, Math.min(progress, 1f)));
    }
    @Override public void giveExpLevels(int amount) { setLevel(Math.max(0, experienceLevel + amount)); }
    @Override public boolean performCommand(String command) { return Native.dispatchCommand(this, command); }
    @Override public org.bukkit.Location getCompassTarget() { return getWorld().getSpawnLocation(); }
    @Override public boolean canSee(org.bukkit.entity.Player player) { return true; }
    @Override public boolean canSee(org.bukkit.entity.Entity entity) { return true; }
    /** Ferrite does not give players a personal border, so they all follow the world's. */
    @Override public org.bukkit.WorldBorder getWorldBorder() { return null; }
    @Override public int hashCode() { return id.hashCode(); }
    @Override public boolean equals(Object other) { return other instanceof org.bukkit.entity.Player player && id.equals(player.getUniqueId()); }
    @Override public String toString() { return "Player[" + id + "]"; }
}
