package dev.ferrite.fixtures.typical;

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
import org.bukkit.inventory.ItemStack;
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

            repeating = getServer().getScheduler().runTaskTimer(
                    this, () -> record("scheduler:sync-repeat:" + (++syncRuns)), 1L, 2L);
            delayed = getServer().getScheduler().runTaskLater(
                    this, () -> record("scheduler:sync-delayed"), 3L);
            async = getServer().getScheduler().runTaskAsynchronously(
                    this, () -> record("scheduler:async:thread=" + Thread.currentThread().getName()));
            record("scheduler:ids=" + repeating.getTaskId() + "," + delayed.getTaskId() + "," + async.getTaskId());

            itemProbe();
            worldProbe();
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
        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD, 2);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Ferrite Blade");
        meta.setLore(List.of("first line", "second line"));
        NamespacedKey key = new NamespacedKey(this, "fixture_key");
        meta.getPersistentDataContainer().set(key, PersistentDataType.STRING, "pdc-value");
        stack.setItemMeta(meta);

        record("item:type=" + stack.getType().name() + ":amount=" + stack.getAmount());
        record("item:name=" + stack.getItemMeta().getDisplayName());
        record("item:lore=" + stack.getItemMeta().getLore());
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

    private void unsupportedProbe() {
        try {
            getServer().getWorlds().get(0).getWorldBorder();
            record("unsupported:world-border:NO-EXCEPTION");
        } catch (UnsupportedOperationException exception) {
            record("unsupported:world-border=" + exception.getMessage());
        }
        try {
            getServer().getRegionScheduler();
            record("unsupported:region-scheduler:NO-EXCEPTION");
        } catch (UnsupportedOperationException exception) {
            record("unsupported:region-scheduler=" + exception.getMessage());
        }
    }
}
