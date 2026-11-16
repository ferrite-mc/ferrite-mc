// Clean-room Ferrite NMS fixture. No Mojang code or assets.
package dev.ferrite.fixtures.nms;

import dev.ferrite.nms.FerriteNmsVersion;
import dev.ferrite.nms.bridge.NmsBridge;
import java.util.List;
import java.util.logging.Level;
import net.kyori.adventure.text.Component;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.item.ItemStack;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.data.BlockData;
import org.bukkit.craftbukkit.inventory.CraftItemStack;
import org.bukkit.craftbukkit.util.CraftMagicNumbers;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Loads under Lane 1's host, next to Lane 2's facade, and exercises the selected NMS surface.
 * Any failure throws from {@code onEnable}, so the host reports the fixture as failed rather
 * than silently continuing.
 */
public final class NmsFixturePlugin extends JavaPlugin {
    private static volatile String lastReport = "not run";

    @Override
    public void onEnable() {
        try {
            runFixture();
            lastReport = "ok";
            getLogger().info("Ferrite NMS fixture: " + lastReport);
        } catch (Throwable error) {
            lastReport = "failed: " + error;
            getLogger().log(Level.SEVERE, "Ferrite NMS fixture failed: " + error, error);
            throw error instanceof RuntimeException runtime
                    ? runtime
                    : new IllegalStateException("Ferrite NMS fixture failed", error);
        }
    }

    @Override
    public void onDisable() {
        getLogger().info("Ferrite NMS fixture disabled; last result: " + lastReport);
    }

    /** The result of the last enable, for Lane 1's integration harness. */
    public static String lastReport() {
        return lastReport;
    }

    private void runFixture() {
        check(NmsBridge.isInstalled(),
                "NmsBridge has no NmsHandleProvider; Lane 1 must install Lane 2's provider "
                        + "after the host is attached and before POSTWORLD plugins enable");
        check(FerriteNmsVersion.DATA_VERSION == 5023, "data version constant is not 5023");
        check(FerriteNmsVersion.PROTOCOL_VERSION == 777, "protocol constant is not 777");

        exerciseItems();
        ServerLevel level = exerciseWorld();
        exerciseMagicNumbers(level);
        exercisePlayers();
        exerciseExplicitRefusals(level);
    }

    private void exerciseItems() {
        NamespacedKey key = new NamespacedKey(this, "nms");
        org.bukkit.inventory.ItemStack bukkit =
                new org.bukkit.inventory.ItemStack(Material.DIAMOND_SWORD, 2);
        var meta = bukkit.getItemMeta();
        check(meta != null, "Diamond sword has no item meta");
        meta.displayName(Component.text("Ferrite NMS"));
        meta.lore(List.of(Component.text("line one"), Component.text("line two")));
        meta.getPersistentDataContainer().set(key, PersistentDataType.STRING, "roundtrip");
        check(bukkit.setItemMeta(meta), "Could not set fixture item meta");

        ItemStack nms = CraftItemStack.asNMSCopy(bukkit);
        check(!nms.isEmpty(), "CraftItemStack.asNMSCopy returned an empty stack");
        check(nms.getCount() == 2, "NMS count was " + nms.getCount());
        check(nms.getBukkitMaterial() == Material.DIAMOND_SWORD, "NMS material changed");

        org.bukkit.inventory.ItemStack back = CraftItemStack.asBukkitCopy(nms);
        check(back != null, "CraftItemStack.asBukkitCopy returned null");
        check(back.getType() == Material.DIAMOND_SWORD, "round trip changed the material");
        check(back.getAmount() == 2, "round trip changed the amount");
        var backMeta = back.getItemMeta();
        check(backMeta != null, "round trip lost item meta");
        check(backMeta.hasDisplayName(), "round trip lost the display name");
        check(Component.text("Ferrite NMS").equals(backMeta.displayName()),
                "round trip changed the display name");
        check(backMeta.lore() != null && backMeta.lore().size() == 2, "round trip lost lore");
        check("roundtrip".equals(backMeta.getPersistentDataContainer()
                        .get(key, PersistentDataType.STRING)),
                "round trip lost persistent data");
    }

    private ServerLevel exerciseWorld() {
        check(!getServer().getWorlds().isEmpty(), "the fixture needs a loaded world");
        org.bukkit.World world = getServer().getWorlds().get(0);
        ServerLevel level = NmsBridge.level(world);
        check(level.getUUID().equals(world.getUID()), "ServerLevel UID does not match the world");
        check(level.getName().equals(world.getName()), "ServerLevel name does not match the world");
        check(level.getMinY() <= level.getMaxY(), "world height range is inverted");

        var spawn = level.getSpawnPos();
        var state = level.getBlockState(spawn);
        check(state != null, "spawn block state was null");
        check(state.getBukkitMaterial() == world.getBlockAt(spawn.getX(), spawn.getY(), spawn.getZ())
                        .getType(),
                "ServerLevel block state and Bukkit world disagree");
        // Loaded state is informational: the host may not have the spawn chunk loaded yet.
        level.getChunkSource().isChunkLoaded(spawn.getX() >> 4, spawn.getZ() >> 4);
        getLogger().info("NMS world " + level.getUUID() + " " + level.dimensionKey()
                + " spawn=" + spawn + " state=" + state.getAsString());
        return level;
    }

    private void exerciseMagicNumbers(ServerLevel level) {
        check(CraftMagicNumbers.getMaterial("minecraft:stone") == Material.STONE,
                "CraftMagicNumbers.getMaterial returned the wrong material");
        check(CraftMagicNumbers.getKey(Material.STONE).equals(NamespacedKey.minecraft("stone")),
                "CraftMagicNumbers.getKey returned the wrong key");
        check(CraftMagicNumbers.getBlock(Material.STONE).getBukkitMaterial() == Material.STONE,
                "CraftMagicNumbers.getBlock lost the material");
        check(CraftMagicNumbers.getItem(Material.DIAMOND).getBukkitMaterial() == Material.DIAMOND,
                "CraftMagicNumbers.getItem lost the material");
        check(CraftMagicNumbers.getDataVersion() == 5023,
                "CraftMagicNumbers.getDataVersion was " + CraftMagicNumbers.getDataVersion());

        var spawn = level.getSpawnPos();
        BlockData data = level.getBlockState(spawn).getBukkitMaterial()
                .createBlockData();
        check(CraftMagicNumbers.getBlockState(data).getBukkitMaterial() == data.getMaterial(),
                "CraftMagicNumbers.getBlockState changed the material");
        check(CraftMagicNumbers.getBlockStateId(data) >= 0,
                "the host did not resolve a global block-state id");
    }

    private void exercisePlayers() {
        var players = getServer().getOnlinePlayers();
        if (players.isEmpty()) {
            getLogger().warning("No online player; the NMS player fixture is not exercised");
            return;
        }
        var player = players.iterator().next();
        ServerPlayer handle = NmsBridge.player(player);
        check(NmsBridge.entity(player) == handle,
                "the player and entity bridge entries returned different handles for one player");
        check(handle.getUUID().equals(player.getUniqueId()), "ServerPlayer UUID mismatch");
        check(handle.getName().equals(player.getName()), "ServerPlayer name mismatch");
        check(handle.level().getUUID().equals(player.getWorld().getUID()),
                "ServerPlayer world mismatch");
        check(Float.compare(handle.getHealth(), (float) player.getHealth()) == 0,
                "ServerPlayer health mismatch");
        // A safe write is asynchronous; never block the server thread waiting for the region.
        handle.setHealthAsync((float) player.getHealth()).whenComplete((ignored, error) -> {
            if (error != null) {
                getLogger().warning("NMS fixture health write failed: " + error);
            }
        });
    }

    private void exerciseExplicitRefusals(ServerLevel level) {
        var spawn = level.getSpawnPos();
        expectUnsupported(() -> level.setBlock(spawn, level.getBlockState(spawn)), "setBlock");
        expectUnsupported(() -> CraftMagicNumbers.fromLegacy(Material.STONE), "fromLegacy");
        expectUnsupported(() -> CraftItemStack.asCraftMirror(
                CraftItemStack.asNMSCopy(new org.bukkit.inventory.ItemStack(Material.STONE))),
                "asCraftMirror");
    }

    private static void expectUnsupported(ThrowingRunnable action, String method) {
        try {
            action.run();
        } catch (UnsupportedOperationException error) {
            String message = String.valueOf(error.getMessage());
            check(message.contains(method), "refusal for " + method + " did not name the method: " + message);
            return;
        } catch (Exception error) {
            throw new IllegalStateException("Expected an explicit refusal for " + method, error);
        }
        throw new IllegalStateException("Expected an explicit refusal for " + method);
    }

    private static void check(boolean condition, String message) {
        if (!condition) {
            throw new IllegalStateException(message);
        }
    }

    @FunctionalInterface
    private interface ThrowingRunnable {
        void run() throws Exception;
    }
}
