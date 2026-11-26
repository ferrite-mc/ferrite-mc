package dev.ferrite.fixtures.typical;

import io.papermc.paper.datacomponent.DataComponentType;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.List;
import java.util.logging.Level;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.scheduler.BukkitTask;

/**
 * The Lane 2 typical fixture plugin.
 *
 * <p>It exercises the supported first vertical slice through the public Bukkit API only
 * and records every observable result in {@code <dataFolder>/typical.log}. The facade
 * integration test loads the built jar with a URLClassLoader, runs the lifecycle, fires
 * events, dispatches commands, ticks the scheduler and then asserts on that log.
 */
public final class TypicalPlugin extends JavaPlugin {

    private final Object logLock = new Object();
    private int syncRuns;
    private BukkitTask repeating;
    private BukkitTask delayed;
    private BukkitTask async;

    @Override
    public void onLoad() {
        record("lifecycle:onLoad");
    }

    @Override
    public void onEnable() {
        try {
            record("lifecycle:onEnable:thread=" + Thread.currentThread().getName());
            getLogger().info("TypicalFixture onEnable");
            saveDefaultConfig();
            record("config:greeting=" + getConfig().getString("greeting", "<missing>"));
            record("config:exists=" + Path.of(getDataFolder().getPath(), "config.yml").toFile().isFile());

            getServer().getPluginManager().registerEvents(new TypicalListener(this), this);

            var command = getCommand("ferrite-hello");
            if (command == null) {
                record("command:missing");
            } else {
                HelloCommand executor = new HelloCommand(this);
                command.setExecutor(executor);
                command.setTabCompleter(executor);
                record("command:registered=" + command.getName()
                        + ":permission=" + command.getPermission()
                        + ":aliases=" + command.getAliases());
            }
            var worldCommand = getCommand("ferrite-world");
            if (worldCommand != null) {
                worldCommand.setExecutor((sender, cmd, label, args) -> {
                    var world = getServer().getWorlds().get(0);
                    boolean chunksLoaded = world.getLoadedChunks().length > 0;
                    var entities = world.getEntities();
                    var living = world.getLivingEntities();
                    var pigs = world.getEntitiesByClass(org.bukkit.entity.Pig.class);
                    boolean hasPlayer = entities.stream()
                            .anyMatch(e -> e instanceof org.bukkit.entity.Player);
                    boolean hasPig = !pigs.isEmpty();
                    boolean livingHasPig = living.stream()
                            .anyMatch(e -> e instanceof org.bukkit.entity.Pig);
                    boolean nearbyHasPig = pigs.stream().anyMatch(pig -> world
                            .getNearbyEntities(pig.getLocation(), 1.0, 1.0, 1.0)
                            .contains(pig));
                    record("world-bulk:chunksLoaded=" + chunksLoaded
                            + ":hasPlayer=" + hasPlayer
                            + ":hasPig=" + hasPig
                            + ":livingHasPig=" + livingHasPig
                            + ":nearbyHasPig=" + nearbyHasPig);
                    var firstPlayer = world.getPlayers().isEmpty() ? null : world.getPlayers().get(0);
                    var firstPig = pigs.stream().findFirst().orElse(null);
                    if (firstPlayer != null && firstPig != null) {
                        var playerMain = firstPlayer.getEquipment().getItemInMainHand().getType();
                        var pigHelmet = new org.bukkit.inventory.ItemStack(
                                org.bukkit.Material.GOLDEN_HELMET, 1);
                        pigHelmet.editMeta(meta -> meta.setDisplayName("Pig Helm"));
                        firstPig.getEquipment().setHelmet(pigHelmet);
                        firstPig.getEquipment().setHelmetDropChance(0.5f);
                        firstPig.getEquipment().setItem(
                                org.bukkit.inventory.EquipmentSlot.SADDLE,
                                new org.bukkit.inventory.ItemStack(org.bukkit.Material.SADDLE, 1));
                        firstPig.getEquipment().setItemInMainHand(
                                new org.bukkit.inventory.ItemStack(org.bukkit.Material.IRON_SWORD, 1));
                        var pigId = firstPig.getUniqueId();
                        getServer().getScheduler().runTaskLater(this, () -> {
                            var fresh = world.getEntity(pigId);
                            if (fresh instanceof org.bukkit.entity.LivingEntity livingFresh) {
                                var eq = livingFresh.getEquipment();
                                var helmetItem = eq.getHelmet();
                                var helmet = helmetItem != null
                                        ? helmetItem.getType().name()
                                        : "NONE";
                                var helmetName = helmetItem != null && helmetItem.hasItemMeta()
                                        ? helmetItem.getItemMeta().getDisplayName()
                                        : "NONE";
                                var saddleItem = eq.getItem(org.bukkit.inventory.EquipmentSlot.SADDLE);
                                var saddle = saddleItem != null ? saddleItem.getType().name() : "NONE";
                                var hand = eq.getItemInMainHand().getType().name();
                                record("entity-equipment:playerMainHand=" + playerMain.name()
                                        + ":pigHelmet=" + helmet
                                        + ":pigHelmetName=" + helmetName
                                        + ":pigHelmetDrop=" + eq.getHelmetDropChance()
                                        + ":pigSaddle=" + saddle
                                        + ":pigMainHand=" + hand);
                            }
                        }, 10L);
                    }
                    return true;
                });
            }

            repeating = getServer().getScheduler().runTaskTimer(
                    this, () -> record("scheduler:sync-repeat:" + (++syncRuns)), 1L, 2L);
            delayed = getServer().getScheduler().runTaskLater(
                    this, () -> record("scheduler:sync-delayed"), 3L);
            async = getServer().getScheduler().runTaskAsynchronously(
                    this, () -> record("scheduler:async:thread=" + Thread.currentThread().getName()));
            record("scheduler:ids=" + repeating.getTaskId() + "," + delayed.getTaskId() + "," + async.getTaskId());

            itemProbe();
            worldProbe();
            foliaProbe();
            pluginMessagingProbe();
            unsupportedProbe();

            record("lifecycle:onEnable:done");
        } catch (RuntimeException exception) {
            record("lifecycle:onEnable:error=" + exception);
            getLogger().log(Level.SEVERE, "TypicalFixture onEnable failed", exception);
            throw exception;
        }
    }

    @Override
    public void onDisable() {
        record("lifecycle:onDisable:syncRuns=" + syncRuns);
    }

    /** Append one observable line to the fixture log and mirror it through the plugin log. */
    public void record(String line) {
        synchronized (logLock) {
            Path folder = getDataFolder().toPath();
            Path log = folder.resolve("typical.log");
            try {
                Files.createDirectories(folder);
                Files.writeString(
                        log,
                        line + System.lineSeparator(),
                        StandardCharsets.UTF_8,
                        StandardOpenOption.CREATE,
                        StandardOpenOption.APPEND);
            } catch (IOException exception) {
                throw new UncheckedIOException("could not write fixture log", exception);
            }
        }
        getLogger().info("fixture: " + line);
    }

    private void itemProbe() {
        ItemStack plain = new ItemStack(Material.IRON_INGOT, 1);
        ItemMeta detached = plain.getItemMeta();
        detached.setDisplayName("Detached");
        record("item:plain-unpolluted=" + !plain.hasItemMeta());

        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD, 2);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Ferrite Blade");
        meta.setLore(List.of("first line", "second line"));
        ((Damageable) meta).setDamage(12);
        meta.setCustomModelData(42);
        meta.setUnbreakable(true);
        meta.addItemFlags(ItemFlag.HIDE_UNBREAKABLE);
        meta.setItemModel(new NamespacedKey(this, "ferrite_blade_model"));
        meta.setTooltipStyle(new NamespacedKey(this, "ferrite_tooltip"));
        meta.setEnchantable(15);
        meta.setMaxStackSize(8);
        var food = meta.getFood();
        food.setNutrition(6);
        food.setSaturation(2.5f);
        food.setCanAlwaysEat(true);
        meta.setFood(food);
        meta.setHideTooltip(true);
        var tool = meta.getTool();
        tool.setDefaultMiningSpeed(6.5f);
        tool.setDamagePerBlock(2);
        meta.setTool(tool);
        var equippable = meta.getEquippable();
        equippable.setSlot(org.bukkit.inventory.EquipmentSlot.HEAD);
        meta.setEquippable(equippable);
        var cooldown = meta.getUseCooldown();
        cooldown.setCooldownSeconds(2.5f);
        cooldown.setCooldownGroup(new NamespacedKey(this, "blade_cd"));
        meta.setUseCooldown(cooldown);
        var jukebox = meta.getJukeboxPlayable();
        jukebox.setSong(org.bukkit.JukeboxSong.CAT);
        meta.setJukeboxPlayable(jukebox);
        NamespacedKey key = new NamespacedKey(this, "fixture_key");
        meta.getPersistentDataContainer().set(key, PersistentDataType.STRING, "pdc-value");
        NamespacedKey nestedKey = new NamespacedKey(this, "nested_key");
        var nestedPdc = meta.getPersistentDataContainer().getAdapterContext().newPersistentDataContainer();
        nestedPdc.set(key, PersistentDataType.INTEGER, 123);
        meta.getPersistentDataContainer().set(nestedKey, PersistentDataType.TAG_CONTAINER, nestedPdc);
        stack.setItemMeta(meta);

        // Mutating detached meta after setItemMeta must not affect stack.
        meta.setDisplayName("DetachedChange");

        record("item:type=" + stack.getType().name() + ":amount=" + stack.getAmount());
        record("item:name=" + stack.getItemMeta().getDisplayName());
        record("item:lore=" + stack.getItemMeta().getLore());
        record("item:damage=" + ((Damageable) stack.getItemMeta()).getDamage());
        record("item:cmd=" + stack.getItemMeta().getCustomModelData());
        record("item:unbreakable=" + stack.getItemMeta().isUnbreakable());
        record("item:flags=" + stack.getItemFlags());
        record("item:model=" + stack.getItemMeta().getItemModel());
        record("item:tooltip=" + stack.getItemMeta().getTooltipStyle());
        record("item:enchantable=" + stack.getItemMeta().getEnchantable());
        record("item:food=" + stack.getItemMeta().getFood().getNutrition());
        record("item:hide-tooltip=" + stack.getItemMeta().isHideTooltip());
        record("item:tool-speed=" + stack.getItemMeta().getTool().getDefaultMiningSpeed());
        record("item:equippable-slot=" + stack.getItemMeta().getEquippable().getSlot());
        record("item:use-cooldown=" + stack.getItemMeta().getUseCooldown().getCooldownSeconds());
        record("item:jukebox-song=" + stack.getItemMeta().getJukeboxPlayable().getSong().getKey());
        record("item:nested-pdc=" + stack.getPersistentDataContainer()
                .get(nestedKey, PersistentDataType.TAG_CONTAINER)
                .get(key, PersistentDataType.INTEGER));
        record("item:pdc=" + stack.getItemMeta().getPersistentDataContainer()
                .get(key, PersistentDataType.STRING));
        record("item:pdc-view=" + stack.getPersistentDataContainer().get(key, PersistentDataType.STRING));
        ItemStack copy = stack.clone();
        record("item:clone-equal=" + copy.equals(stack) + ":similar=" + copy.isSimilar(stack));
        record("item:max-stack=" + stack.getMaxStackSize());
    }

    private void worldProbe() {
        World world = getServer().getWorlds().get(0);
        record("world:name=" + world.getName()
                + ":env=" + world.getEnvironment()
                + ":time=" + world.getTime()
                + ":uid=" + world.getUID());
        Block block = world.getBlockAt(0, 64, 0);
        record("world:block-before=" + block.getType().name());
        block.setType(Material.GOLD_BLOCK);
        record("world:block-after=" + world.getBlockAt(0, 64, 0).getType().name());
        record("world:spawn=" + world.getSpawnLocation().getBlockX() + ","
                + world.getSpawnLocation().getBlockY() + ","
                + world.getSpawnLocation().getBlockZ());
    }

    private void pluginMessagingProbe() {
        var messenger = getServer().getMessenger();
        messenger.registerOutgoingPluginChannel(this, "ferrite:echo");
        messenger.registerIncomingPluginChannel(this, "ferrite:echo", (channel, player, message) -> {
            String text = new String(message, StandardCharsets.UTF_8);
            record("plugin-msg:in=" + channel + ":player=" + player.getName() + ":text=" + text);
            if ("server-broadcast".equals(text)) {
                getServer().sendPluginMessage(
                        this, "ferrite:echo", "server:pong".getBytes(StandardCharsets.UTF_8));
            } else {
                player.sendPluginMessage(
                        this, "ferrite:echo", ("echo:" + text).getBytes(StandardCharsets.UTF_8));
            }
        });
        boolean unregisteredRejected = false;
        try {
            getServer().sendPluginMessage(this, "ferrite:unregistered", new byte[] {1});
        } catch (IllegalArgumentException
                | org.bukkit.plugin.messaging.ChannelNotRegisteredException expected) {
            unregisteredRejected = true;
        }
        record("plugin-msg:unregistered-rejected=" + unregisteredRejected);
    }

    private void foliaProbe() {
        World world = getServer().getWorlds().get(0);
        getServer().getRegionScheduler().run(this, world, 0, 0, task ->
                record("folia:region=ok:owned=" + getServer().isOwnedByCurrentRegion(world, 0, 0)));
        getServer().getGlobalRegionScheduler().run(this, task ->
                record("folia:global=ok:global=" + getServer().isGlobalTickThread()));
        getServer().getAsyncScheduler().runNow(this, task ->
                record("folia:async=ok:thread=" + Thread.currentThread().getName()));
    }

    private void unsupportedProbe() {
        try {
            getServer().getWorlds().get(0).getWorldBorder();
            record("unsupported:world-border:NO-EXCEPTION");
        } catch (UnsupportedOperationException exception) {
            record("unsupported:world-border=" + exception.getMessage());
        }
        try {
            ItemStack probe = new ItemStack(Material.DIAMOND_SWORD, 1);
            NamespacedKey lockKey = NamespacedKey.minecraft("creative_slot_lock");
            probe.setData(new DataComponentType.NonValued() {
                @Override
                public NamespacedKey getKey() {
                    return lockKey;
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            });
            record("unsupported:item-component:NO-EXCEPTION");
        } catch (UnsupportedOperationException exception) {
            record("unsupported:item-component=" + exception.getMessage());
        }
        try {
            ItemStack probe = new ItemStack(Material.PAPER, 1);
            NamespacedKey badPdc = new NamespacedKey(this, "bad_pdc");
            probe.editPersistentDataContainer(
                    pdc -> pdc.set(
                            badPdc,
                            PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.strings()),
                            List.of(List.of("a"))));
            record("unsupported:pdc-type:NO-EXCEPTION");
        } catch (UnsupportedOperationException exception) {
            record("unsupported:pdc-type=" + exception.getMessage());
        }
    }
}
