// Clean-room Ferrite NMS fixture. No Mojang code or assets.
package dev.ferrite.fixtures.nms;

import dev.ferrite.nms.FerriteNmsVersion;
import dev.ferrite.nms.bridge.NmsBridge;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.List;
import java.util.concurrent.TimeUnit;
import java.util.logging.Level;
import net.kyori.adventure.text.Component;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.phys.Vec3;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.data.BlockData;
import org.bukkit.craftbukkit.entity.CraftEntity;
import org.bukkit.craftbukkit.entity.CraftPlayer;
import org.bukkit.craftbukkit.inventory.CraftItemStack;
import org.bukkit.craftbukkit.util.CraftMagicNumbers;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.util.Vector;

/**
 * Loads under Lane 1's host, next to Lane 2's facade, and exercises the selected NMS surface.
 * Any failure throws from {@code onEnable}, so the host reports the fixture as failed rather
 * than silently continuing.
 */
public final class NmsFixturePlugin extends JavaPlugin {
    private static volatile String lastReport = "not run";
    private static volatile boolean disabled;

    @Override
    public void onEnable() {
        disabled = false;
        try {
            runFixture();
            getServer().getPluginManager().registerEvents(new NmsEntityAcceptanceListener(), this);
            lastReport = "ok";
            recordAcceptance("nms-enable:ok");
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
        disabled = true;
        recordAcceptance("nms-disable:ok");
        getLogger().info("Ferrite NMS fixture disabled; last result: " + lastReport);
    }

    private void recordAcceptance(String line) {
        try {
            Path dir = getDataFolder().toPath();
            Files.createDirectories(dir);
            Files.writeString(
                    dir.resolve("acceptance.log"),
                    line + System.lineSeparator(),
                    StandardCharsets.UTF_8,
                    StandardOpenOption.CREATE,
                    StandardOpenOption.APPEND);
        } catch (IOException ignored) {
            // Non-fatal outside acceptance test runs.
        }
    }

    private final class NmsEntityAcceptanceListener implements Listener {
        @EventHandler
        public void onJoin(PlayerJoinEvent event) {
            Player player = event.getPlayer();
            ServerPlayer sp = NmsBridge.player(player);
            Entity entity = NmsBridge.entity(player);
            try {
                entity.setEquipmentAsync("HAND", "minecraft:diamond_sword").get(5, TimeUnit.SECONDS);
                entity.setEquipmentAsync("HEAD", "minecraft:iron_helmet").get(5, TimeUnit.SECONDS);
                entity.setCustomNameAsync("AcceptanceGuard", true).get(5, TimeUnit.SECONDS);
                entity.setPoseAsync("STANDING").get(5, TimeUnit.SECONDS);
                entity.setVelocityAsync(0.1, 0.0, 0.0).get(5, TimeUnit.SECONDS);
                sp.teleportAsync(
                                sp.level(),
                                sp.getX(),
                                sp.getY(),
                                sp.getZ(),
                                sp.getYRot(),
                                sp.getXRot())
                        .get(5, TimeUnit.SECONDS);
                Entity refreshed = Entity.of(player.getUniqueId());
                String hand = refreshed.equipmentSlot("HAND");
                String head = refreshed.equipmentSlot("HEAD");
                String name = refreshed.customName();
                String pose = refreshed.entityState() == null ? "NONE" : refreshed.entityState().pose();
                recordAcceptance(
                        "nms-join:"
                                + player.getName()
                                + ":equip="
                                + hand
                                + ","
                                + head
                                + ":name="
                                + name
                                + ":pose="
                                + pose);
            } catch (Exception error) {
                recordAcceptance("nms-join-error:" + error);
            }
        }

        @EventHandler
        public void onQuit(PlayerQuitEvent event) {
            recordAcceptance("nms-quit:" + event.getPlayer().getName());
        }
    }

    /** The result of the last enable, for Lane 1's integration harness. */
    public static String lastReport() {
        return lastReport;
    }

    /** Whether {@link #onDisable()} has run after the last enable. */
    public static boolean wasDisabled() {
        return disabled;
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
        ServerPlayer playerHandle = exercisePlayers(level);
        Entity entityHandle = exerciseEntities(level);
        exerciseExplicitRefusals(
                level,
                playerHandle,
                entityHandle != null ? entityHandle : playerHandle,
                findFirstCraftPlayer(),
                findFirstCraftEntity());
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

    private ServerPlayer exercisePlayers(ServerLevel level) {
        var players = getServer().getOnlinePlayers();
        if (players.isEmpty()) {
            getLogger().warning("No online player; the NMS player fixture is not exercised");
            return null;
        }
        Player player = players.iterator().next();
        ServerPlayer handle = player instanceof CraftPlayer craftPlayer
                ? craftPlayer.getHandle()
                : NmsBridge.player(player);
        check(NmsBridge.entity(player) == NmsBridge.player(player),
                "the player and entity bridge entries returned different handles for one player");
        check(handle.getUUID().equals(player.getUniqueId()), "ServerPlayer UUID mismatch");
        check(handle.getName().equals(player.getName()), "ServerPlayer name mismatch");
        check(handle.level().getUUID().equals(player.getWorld().getUID()),
                "ServerPlayer world mismatch");
        var location = player.getLocation();
        check(Double.compare(handle.getX(), location.getX()) == 0, "ServerPlayer X mismatch");
        check(Double.compare(handle.getY(), location.getY()) == 0, "ServerPlayer Y mismatch");
        check(Double.compare(handle.getZ(), location.getZ()) == 0, "ServerPlayer Z mismatch");
        check(Float.compare(handle.getYRot(), location.getYaw()) == 0, "ServerPlayer yaw mismatch");
        check(Float.compare(handle.getXRot(), location.getPitch()) == 0, "ServerPlayer pitch mismatch");
        check(handle.gameMode().equalsIgnoreCase(player.getGameMode().name()),
                "ServerPlayer gameMode mismatch");
        check(handle.bukkitGameMode() == player.getGameMode(), "ServerPlayer bukkitGameMode mismatch");
        check(handle.getFoodLevel() == player.getFoodLevel(), "ServerPlayer foodLevel mismatch");
        check(Float.compare(handle.getHealth(), (float) player.getHealth()) == 0,
                "ServerPlayer health mismatch");
        if (player instanceof CraftPlayer craftPlayer) {
            check(craftPlayer.getGameMode() == player.getGameMode(), "CraftPlayer gameMode mismatch");
            check(Double.compare(craftPlayer.getHealth(), player.getHealth()) == 0,
                    "CraftPlayer health mismatch");
            check(craftPlayer.getFoodLevel() == player.getFoodLevel(), "CraftPlayer foodLevel mismatch");
        }
        // Safe writes are asynchronous; never block the server thread waiting for the region.
        handle.setHealthAsync((float) player.getHealth()).whenComplete((ignored, error) -> {
            if (error != null) {
                getLogger().warning("NMS fixture health write failed: " + error);
            }
        });
        handle.setGameModeAsync(handle.gameMode()).whenComplete((ignored, error) -> {
            if (error != null) {
                getLogger().warning("NMS fixture gameMode write failed: " + error);
            }
        });
        handle.teleportAsync(level, handle.getX(), handle.getY(), handle.getZ(), handle.getYRot(), handle.getXRot())
                .whenComplete((ignored, error) -> {
                    if (error != null) {
                        getLogger().warning("NMS fixture player teleport failed: " + error);
                    }
                });
        return handle;
    }

    private Entity exerciseEntities(ServerLevel level) {
        org.bukkit.World world = getServer().getWorlds().get(0);
        for (org.bukkit.entity.Entity entity : world.getEntities()) {
            if (entity instanceof Player) {
                continue;
            }
            Entity handle = entity instanceof CraftEntity craftEntity
                    ? craftEntity.getHandle()
                    : NmsBridge.entity(entity);
            check(handle.getUUID().equals(entity.getUniqueId()), "Entity UUID mismatch");
            check(handle.level().getUUID().equals(world.getUID()), "Entity world mismatch");
            var location = entity.getLocation();
            check(Double.compare(handle.getX(), location.getX()) == 0, "Entity X mismatch");
            check(Double.compare(handle.getY(), location.getY()) == 0, "Entity Y mismatch");
            check(Double.compare(handle.getZ(), location.getZ()) == 0, "Entity Z mismatch");
            check(handle.position().equals(new Vec3(handle.getX(), handle.getY(), handle.getZ())),
                    "Entity position() mismatch");
            BlockPos blockPos = handle.blockPosition();
            check(blockPos.x == (int) Math.floor(handle.getX())
                            && blockPos.y == (int) Math.floor(handle.getY())
                            && blockPos.z == (int) Math.floor(handle.getZ()),
                    "Entity blockPosition() mismatch");
            Vec3 velocity = handle.getDeltaMovement();
            check(velocity != null, "Entity velocity was null");
            check(handle.getPassengers().size() == handle.passengerIds().size(),
                    "Entity passengers count mismatch");
            handle.getVehicle();
            if (entity instanceof CraftEntity craftEntity) {
                var craftLoc = craftEntity.getLocation();
                check(Double.compare(craftLoc.getX(), handle.getX()) == 0, "CraftEntity location X mismatch");
                Vector craftVel = craftEntity.getVelocity();
                check(Double.compare(craftVel.getX(), velocity.x()) == 0, "CraftEntity velocity X mismatch");
                check(craftEntity.isInsideVehicle() == handle.isPassenger(),
                        "CraftEntity isInsideVehicle mismatch");
                craftEntity.teleportAsync(craftLoc).whenComplete((ignored, error) -> {
                    if (error != null) {
                        getLogger().warning("NMS fixture CraftEntity teleport failed: " + error);
                    }
                });
            }
            handle.teleportAsync(level, handle.getX(), handle.getY(), handle.getZ(), handle.getYRot(), handle.getXRot())
                    .whenComplete((ignored, error) -> {
                        if (error != null) {
                            getLogger().warning("NMS fixture entity teleport failed: " + error);
                        }
                    });
            return handle;
        }
        return null;
    }

    private CraftPlayer findFirstCraftPlayer() {
        for (Player player : getServer().getOnlinePlayers()) {
            if (player instanceof CraftPlayer craftPlayer) {
                return craftPlayer;
            }
        }
        return null;
    }

    private CraftEntity findFirstCraftEntity() {
        org.bukkit.World world = getServer().getWorlds().get(0);
        for (org.bukkit.entity.Entity entity : world.getEntities()) {
            if (entity instanceof CraftEntity craftEntity) {
                return craftEntity;
            }
        }
        return null;
    }

    private void exerciseExplicitRefusals(
            ServerLevel level,
            ServerPlayer playerHandle,
            Entity entityHandle,
            CraftPlayer craftPlayer,
            CraftEntity craftEntity) {
        var spawn = level.getSpawnPos();
        expectUnsupported(() -> level.setBlock(spawn, level.getBlockState(spawn)), "setBlock");
        expectUnsupported(() -> CraftMagicNumbers.fromLegacy(Material.STONE), "fromLegacy");
        expectUnsupported(() -> CraftItemStack.asCraftMirror(
                CraftItemStack.asNMSCopy(new org.bukkit.inventory.ItemStack(Material.STONE))),
                "asCraftMirror");
        if (entityHandle != null) {
            expectUnsupported(() -> entityHandle.setPos(0.0, 64.0, 0.0), "setPos");
            expectUnsupported(() -> entityHandle.teleportTo(level, 0.0, 64.0, 0.0), "teleportTo");
            expectUnsupported(() -> entityHandle.setDeltaMovement(Vec3.ZERO), "setDeltaMovement");
            expectUnsupported(entityHandle::getBukkitEntity, "getBukkitEntity");
        }
        if (playerHandle != null) {
            expectUnsupported(() -> playerHandle.setGameMode(GameMode.SURVIVAL), "setGameMode");
            expectUnsupported(() -> playerHandle.setGameMode("SURVIVAL"), "setGameMode");
            expectUnsupported(() -> playerHandle.setHealth(20.0F), "setHealth");
            expectUnsupported(playerHandle::getConnection, "getConnection");
            expectUnsupported(() -> playerHandle.openMenu(null), "openMenu");
        }
        if (craftEntity != null) {
            expectUnsupported(() -> craftEntity.teleport(craftEntity.getLocation()), "teleport");
            expectUnsupported(() -> craftEntity.setVelocity(new Vector(0, 0, 0)), "setVelocity");
        }
        if (craftPlayer != null) {
            expectUnsupported(() -> craftPlayer.setGameMode(GameMode.SURVIVAL), "setGameMode");
            expectUnsupported(() -> craftPlayer.setHealth(20.0), "setHealth");
            expectUnsupported(() -> craftPlayer.setFoodLevel(20), "setFoodLevel");
        }
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
