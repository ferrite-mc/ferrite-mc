package dev.ferrite.server;

/** Entry points for the host bridge. Fallback mode is intended for Java tests. */
public final class Native {
    private Native() {}
    private record FallbackRegion(long id, long due, String world, int x, int z) {}
    private record FallbackEntity(long id, long due) {}
    private static final java.util.List<FallbackEntity> fallbackEntities = new java.util.ArrayList<>();
    private record FallbackGlobal(long id, long due) {}
    private static final java.util.List<FallbackGlobal> fallbackGlobals = new java.util.ArrayList<>();
    private static volatile long fallbackTick;
    private static final java.util.List<FallbackRegion> fallbackRegions = new java.util.ArrayList<>();
    private static final ThreadLocal<FallbackRegion> currentFallbackRegion = new ThreadLocal<>();
    static void tickFallback() {
        if (enabled()) return;
        java.util.List<FallbackRegion> ready = new java.util.ArrayList<>();
        synchronized (fallbackRegions) {
            fallbackTick++;
            fallbackRegions.removeIf(entry -> { if (entry.due() <= fallbackTick) { ready.add(entry); return true; } return false; });
        }
        java.util.List<FallbackGlobal> globals = new java.util.ArrayList<>();
        synchronized (fallbackGlobals) { fallbackGlobals.removeIf(entry -> { if (entry.due() <= fallbackTick) { globals.add(entry); return true; } return false; }); }
        for (FallbackGlobal entry : globals) Bootstrap.runGlobalTask(entry.id());
        java.util.List<FallbackEntity> entities = new java.util.ArrayList<>();
        synchronized (fallbackEntities) { fallbackEntities.removeIf(entry -> { if (entry.due() <= fallbackTick) { entities.add(entry); return true; } return false; }); }
        for (FallbackEntity entry : entities) Bootstrap.runEntityTask(entry.id());
        for (FallbackRegion entry : ready) {
            currentFallbackRegion.set(entry);
            try { Bootstrap.runRegionTask(entry.id()); } finally { currentFallbackRegion.remove(); }
        }
    }
    static boolean enabled() { return !"false".equals(System.getProperty("ferrite.native")); }
    static boolean serverBan(String ip, String reason, long expires, String source, boolean ban) { return !enabled() || Bindings.serverBan(ip, reason, expires, source, ban); }
    static boolean serverWhitelist(boolean enabled) { return !enabled() || Bindings.serverWhitelist(enabled); }
    static boolean playerSound(String player, String world, String sound, int category, double x, double y, double z, float volume, float pitch, boolean seeded, long seed) { return !enabled() || Bindings.playerSound(player, world, sound, category, x, y, z, volume, pitch, seeded, seed); }
    static boolean playerVisual(String player, int kind, String first, String second, int fade, int stay, int out) { return !enabled() || Bindings.playerVisual(player, kind, first, second, fade, stay, out); }
    static boolean playerOperator(String player, boolean operator) { return !enabled() || Bindings.playerOperator(player, operator); }
    static boolean playerInventory(String player, int slot, String material, int amount) { return !enabled() || Bindings.playerInventory(player, slot, material, amount); }
    static boolean playerTeleport(String player, String world, double x, double y, double z, float yaw, float pitch) { return !enabled() || Bindings.playerTeleport(player, world, x, y, z, yaw, pitch); }
    static String blockType(String world, int x, int y, int z) { return enabled() ? Bindings.blockType(world, x, y, z) : null; }
    static int skyLight(String world, int x, int y, int z) { return enabled() ? Bindings.skyLight(world, x, y, z) : -1; }
    static int highestBlockY(String world, int x, int z) { return enabled() ? Bindings.highestBlockY(world, x, z) : Integer.MIN_VALUE; }
    static String biomeAt(String world, int x, int y, int z) { return enabled() ? Bindings.biomeAt(world, x, y, z) : null; }
    static boolean playerMove(String player, String name, double fromX, double fromY, double fromZ, float fromYaw, float fromPitch, double toX, double toY, double toZ, float toYaw, float toPitch) { return enabled() ? Bindings.playerMove(player, name, fromX, fromY, fromZ, fromYaw, fromPitch, toX, toY, toZ, toYaw, toPitch) : Bootstrap.firePlayerMove(player, name, fromX, fromY, fromZ, fromYaw, fromPitch, toX, toY, toZ, toYaw, toPitch); }
    static boolean playerInteract(String player, String name, int action, int x, int y, int z, int face, String type) { return enabled() ? Bindings.playerInteract(player, name, action, x, y, z, face, type) : Bootstrap.firePlayerInteract(player, name, action, x, y, z, face, type); }
    static boolean blockBreak(String player, String name, int x, int y, int z, String type) { return enabled() ? Bindings.blockBreak(player, name, x, y, z, type) : Bootstrap.fireBlockBreak(player, name, x, y, z, type); }
    static boolean blockPlace(String player, String name, int x, int y, int z, int face, String placed, String replaced) { return enabled() ? Bindings.blockPlace(player, name, x, y, z, face, placed, replaced) : Bootstrap.fireBlockPlace(player, name, x, y, z, face, placed, replaced); }
    static boolean playerCommand(String player, String name, String message) { return enabled() ? Bindings.playerCommand(player, name, message) : Bootstrap.firePlayerCommand(player, name, message); }
    static boolean worldSetBlock(String world, int x, int y, int z, String material) { return !enabled() || Bindings.worldSetBlock(world, x, y, z, material); }
    static void playerState(String player, int kind, double value) { if (enabled()) Bindings.playerState(player, kind, value); }
    static void playerMessage(String player, String message, boolean component) { if (enabled()) Bindings.playerMessage(player, message, component); }
    private static volatile boolean linked;
    /** Marks the native bridge as available; the host registers natives before Bootstrap.start. */
    static void link() { linked = enabled(); }
    static void eventRegistered(String type) { if (linked) Bindings.eventRegistered(type); }
    static void eventsCleared() { if (linked) Bindings.eventsCleared(); }
    public static void log(String message) { if (enabled()) Bindings.log(message); else System.out.println(message); }
    public static void scheduleGlobal(long taskId, long delay) { if (enabled()) Bindings.scheduleGlobal(taskId, delay); else synchronized (fallbackGlobals) { fallbackGlobals.add(new FallbackGlobal(taskId, fallbackTick + Math.max(1, delay))); } }
    public static void scheduleRegion(long taskId, String world, int chunkX, int chunkZ, long delay, String plugin) { if (enabled()) Bindings.scheduleRegion(taskId, world, chunkX, chunkZ, delay, plugin); else synchronized (fallbackRegions) { fallbackRegions.add(new FallbackRegion(taskId, fallbackTick + Math.max(1, delay), world, chunkX, chunkZ)); } }
    public static void scheduleEntity(long taskId, String entity, long delay, String plugin) { if (enabled()) Bindings.scheduleEntity(taskId, entity, delay, plugin); else synchronized (fallbackEntities) { fallbackEntities.add(new FallbackEntity(taskId, fallbackTick + Math.max(1, delay))); } }
    public static boolean isRegionThread(String world, int chunkX, int chunkZ) { if (enabled()) return Bindings.isRegionThread(world, chunkX, chunkZ); FallbackRegion region = currentFallbackRegion.get(); return region != null && region.world().equals(world) && region.x() == chunkX && region.z() == chunkZ; }
    public static long currentTick() { return enabled() ? Bindings.currentTick() : fallbackTick; }
    public static boolean dispatchCommand(org.bukkit.command.CommandSender sender, String line) {
        org.bukkit.Server server = org.bukkit.Bukkit.getServer();
        String identity;
        if (sender == server.getConsoleSender()) identity = "console";
        else if (sender instanceof org.bukkit.entity.Player player && server.getPlayer(player.getUniqueId()) == player) identity = player.getUniqueId().toString();
        else return false;
        return enabled() ? Bindings.dispatchCommand(identity, line) : server.dispatchCommand(sender, line);
    }
    static final class Bindings {
        static native void log(String message);
        static native boolean serverBan(String ip, String reason, long expires, String source, boolean ban);
        static native boolean serverWhitelist(boolean enabled);
        static native boolean playerSound(String player, String world, String sound, int category, double x, double y, double z, float volume, float pitch, boolean seeded, long seed);
        static native boolean playerVisual(String player, int kind, String first, String second, int fade, int stay, int out);
        static native boolean playerOperator(String player, boolean operator);
        static native boolean playerInventory(String player, int slot, String material, int amount);
        static native boolean playerTeleport(String player, String world, double x, double y, double z, float yaw, float pitch);
        static native String blockType(String world, int x, int y, int z);
        static native int skyLight(String world, int x, int y, int z);
        static native int highestBlockY(String world, int x, int z);
        static native String biomeAt(String world, int x, int y, int z);
        static native boolean playerMove(String player, String name, double fromX, double fromY, double fromZ, float fromYaw, float fromPitch, double toX, double toY, double toZ, float toYaw, float toPitch);
        static native boolean playerInteract(String player, String name, int action, int x, int y, int z, int face, String type);
        static native boolean blockBreak(String player, String name, int x, int y, int z, String type);
        static native boolean blockPlace(String player, String name, int x, int y, int z, int face, String placed, String replaced);
        static native boolean playerCommand(String player, String name, String message);
        static native boolean worldSetBlock(String world, int x, int y, int z, String material);
        static native void playerState(String player, int kind, double value);
        static native void playerMessage(String player, String message, boolean component);
        static native void eventRegistered(String type);
        static native void eventsCleared();
        static native void scheduleGlobal(long taskId, long delay);
        static native void scheduleRegion(long taskId, String world, int chunkX, int chunkZ, long delay, String plugin);
        static native void scheduleEntity(long taskId, String entity, long delay, String plugin);
        static native boolean isRegionThread(String world, int chunkX, int chunkZ);
        static native long currentTick();
        static native boolean dispatchCommand(String sender, String line);
    }
}
