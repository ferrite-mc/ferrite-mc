package dev.ferrite.typical;

import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * A deliberately ordinary plugin: YAML config, a command with tab completion, a join listener and
 * a repeating scheduler task. The plugin-lifecycle test loads it end to end, so it doubles as the
 * acceptance fixture for the Bukkit surface real plugins touch at enable time.
 */
public final class TypicalPlugin extends JavaPlugin implements Listener {
    private int ticks;
    private org.bukkit.scheduler.BukkitTask timer;

    private java.io.File folder() {
        var folder = new java.io.File(getDataFolder().getParentFile(), "FerriteTypical");
        try { java.nio.file.Files.createDirectories(folder.toPath()); } catch (java.io.IOException e) { throw new java.io.UncheckedIOException(e); }
        return folder;
    }

    private void write(String name, String content) {
        try { java.nio.file.Files.writeString(new java.io.File(folder(), name).toPath(), content); }
        catch (java.io.IOException e) { throw new java.io.UncheckedIOException(e); }
    }

    private void append(String line) {
        try { java.nio.file.Files.writeString(new java.io.File(folder(), "events.txt").toPath(), line + "\n",
                java.nio.charset.StandardCharsets.UTF_8, java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND); }
        catch (java.io.IOException e) { throw new java.io.UncheckedIOException(e); }
    }

    @Override public void onEnable() {
        saveDefaultConfig();
        String greeting = getConfig().getString("greeting", "hello");
        write("config.txt", greeting + "|" + getConfig().getInt("interval", 10) + "|" + getConfig().getString("reward", "DIAMOND"));
        getLogger().info("FerriteTypical enabled: " + greeting);
        getServer().getPluginManager().registerEvents(this, this);

        // Paper's region scheduler: the host must route this to the region owning (0, 0).
        var world = getServer().getWorlds().get(0);
        getServer().getRegionScheduler().run(this, world, 0, 0, task -> append("region task " + task.isRepeatingTask()));

        // Every Bukkit scheduling entry point the bridge claims: immediate, delayed and async.
        getServer().getScheduler().runTask(this, () -> append("scheduler task"));
        getServer().getScheduler().runTaskLater(this, () -> append("later task"), 2);
        getServer().getScheduler().runTaskAsynchronously(this, () -> append("async task"));
        getServer().getScheduler().runTaskLaterAsynchronously(this, () -> append("later async task"), 2);
        getServer().getScheduler().runTaskTimerAsynchronously(this, task -> { append("timer async task"); task.cancel(); }, 1, 1);

        // Cancelling a repeating async task from the sync scheduler thread must stop it.
        var asyncTimer = getServer().getScheduler().runTaskTimerAsynchronously(this, () -> append("async tick"), 1, 1);
        getServer().getScheduler().runTaskLater(this, () -> { asyncTimer.cancel(); append("async cancelled"); }, 6);

        var command = getCommand("typical");
        command.setExecutor((sender, cmd, label, args) -> execute(sender, args));
        command.setTabCompleter((sender, cmd, alias, args) -> java.util.List.of("ping", "where", "info", "give"));
        append("tab " + command.tabComplete(getServer().getConsoleSender(), "typical", new String[]{"p"}));

        int interval = Math.max(1, getConfig().getInt("interval", 10));
        timer = getServer().getScheduler().runTaskTimer(this, () -> {
            ticks++;
            append("task " + ticks);
            if (ticks == 3) write("task.txt", "3");
            if (ticks == 5) { write("cancelled.txt", "5"); timer.cancel(); }
        }, 1, interval);
        write("enabled.txt", "ok");
    }

    private boolean execute(org.bukkit.command.CommandSender sender, String[] args) {
        String sub = args.length == 0 ? "info" : args[0].toLowerCase(java.util.Locale.ROOT);
        append("command " + sub);
        switch (sub) {
            case "ping" -> sender.sendMessage("pong " + sender.getName());
            case "where" -> {
                if (!(sender instanceof org.bukkit.entity.Player player)) { sender.sendMessage("where: player only"); return true; }
                try {
                    var location = player.getLocation();
                    sender.sendMessage("where " + location.getWorld().getName() + " " + location.getBlockX() + " " + location.getBlockY() + " " + location.getBlockZ());
                } catch (IllegalStateException e) {
                    sender.sendMessage("where unavailable");
                }
            }
            case "info" -> {
                try {
                    var world = getServer().getWorlds().get(0);
                    var spawn = world.getSpawnLocation();
                    sender.sendMessage("info " + world.getName() + " spawn " + spawn.getBlockX() + " " + spawn.getBlockY() + " " + spawn.getBlockZ() + " players " + world.getPlayers().size());
                } catch (IllegalStateException | IndexOutOfBoundsException e) {
                    sender.sendMessage("info unavailable");
                }
            }
            case "give" -> {
                if (!(sender instanceof org.bukkit.entity.Player player)) { sender.sendMessage("give: player only"); return true; }
                var material = org.bukkit.Material.matchMaterial(String.valueOf(getConfig().getString("reward", "DIAMOND")));
                if (material == null) { sender.sendMessage("give: unknown material"); return true; }
                int amount = Math.max(1, getConfig().getInt("amount", 1));
                try {
                    player.getInventory().setItem(0, new org.bukkit.inventory.ItemStack(material, amount));
                    var now = player.getInventory().getItem(0);
                    append("give " + now.getType() + " " + now.getAmount());
                    sender.sendMessage("gave " + material.getKey() + " x" + amount);
                } catch (IllegalStateException e) {
                    sender.sendMessage("give unavailable");
                }
            }
            case "state" -> {
                if (!(sender instanceof org.bukkit.entity.Player player)) { sender.sendMessage("state: player only"); return true; }
                player.setHealth(7.5);
                player.setFoodLevel(11);
                player.setLevel(12);
                player.setExp(0.25f);
                player.setTotalExperience(123);
                append("state " + player.getHealth() + " " + player.getFoodLevel() + " " + player.getLevel() + " " + player.getExp() + " " + player.getTotalExperience());
            }
            case "tp" -> {
                if (!(sender instanceof org.bukkit.entity.Player player)) { sender.sendMessage("tp: player only"); return true; }
                boolean moved = player.teleport(new org.bukkit.Location(player.getWorld(), 2.5, 70.0, 2.5));
                var location = player.getLocation();
                append("tp " + moved + " " + location.getX() + " " + location.getY() + " " + location.getZ());
            }
            case "world" -> {
                var world = getServer().getWorlds().get(0);
                var ground = world.getBlockAt(0, 64, 0);
                var air = world.getBlockAt(0, 65, 0);
                append("block " + ground.getType());
                append("air " + air.getType() + " sky " + air.getLightFromSky() + " dark " + world.getBlockAt(0, 63, 0).getLightFromSky());
                append("highest " + world.getHighestBlockYAt(0, 0) + " biome " + world.getBiome(0, 64, 0).getKey());
            }
            case "surface" -> {
                var srv = getServer();
                append("ip " + srv.getIp() + " " + srv.getPort() + " " + srv.getViewDistance() + " " + srv.getSimulationDistance() + " " + srv.getMaxPlayers());
                append("container " + srv.getWorldContainer().getName() + " update " + srv.getUpdateFolder());
                int recipes = 0;
                for (var iterator = srv.recipeIterator(); iterator.hasNext(); ) { iterator.next(); recipes++; }
                var stick = srv.getRecipe(org.bukkit.NamespacedKey.minecraft("stick"));
                append("recipes " + recipes + " stick " + stick.getResult().getType() + " " + stick.getResult().getAmount() + " " + srv.hasRecipe(org.bukkit.NamespacedKey.minecraft("stick")));
                var forStick = srv.getRecipesFor(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STICK));
                boolean stickKey = forStick.stream().anyMatch(recipe -> recipe instanceof org.bukkit.Keyed keyed && keyed.getKey().getKey().equals("stick"));
                append("for-stick " + forStick.size() + " " + stickKey);
                boolean coals = false, diamond = false;
                for (var tag : srv.getTags(org.bukkit.Tag.REGISTRY_ITEMS, org.bukkit.Material.class)) if (tag.getKey().getKey().equals("coals")) { coals = tag.isTagged(org.bukkit.Material.COAL) && tag.isTagged(org.bukkit.Material.CHARCOAL); diamond = tag.isTagged(org.bukkit.Material.DIAMOND); }
                boolean logs = false;
                for (var tag : srv.getTags(org.bukkit.Tag.REGISTRY_BLOCKS, org.bukkit.Material.class)) if (tag.getKey().getKey().equals("logs")) logs = tag.isTagged(org.bukkit.Material.OAK_LOG);
                append("tags coals " + coals + " diamond " + diamond + " logs " + logs);
                var key = new org.bukkit.NamespacedKey("ferrite", "probe");
                var bar = srv.createBossBar(key, "Boss", org.bukkit.boss.BarColor.RED, org.bukkit.boss.BarStyle.SOLID);
                bar.setProgress(0.25);
                append("boss " + (srv.getBossBar(key) == bar) + " " + bar.getProgress() + " " + bar.getTitle() + " " + srv.getBossBars().hasNext() + " " + srv.removeBossBar(key));
            }
            default -> sender.sendMessage("usage: /typical <ping|where|info|give|state|tp|world|surface>");
        }
        return true;
    }

    @EventHandler public void onJoin(PlayerJoinEvent event) {
        var player = event.getPlayer();
        append("join " + player.getName() + " " + player.getGameMode() + " " + player.hasPermission("ferrite.typical.use"));
        player.sendMessage(getConfig().getString("greeting", "hello") + " " + player.getName());
    }

    @Override public void onDisable() {
        if (timer != null) timer.cancel();
        getLogger().info("FerriteTypical disabled");
        write("disabled.txt", "ok");
    }
}
