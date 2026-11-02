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
    @Override public int hashCode() { return id.hashCode(); }
    @Override public boolean equals(Object other) { return other instanceof org.bukkit.entity.Player player && id.equals(player.getUniqueId()); }
    @Override public String toString() { return "Player[" + id + "]"; }
}
