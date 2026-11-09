package dev.ferrite.test;
import org.bukkit.event.*;
import org.bukkit.plugin.java.JavaPlugin;
public final class TestPlugin extends JavaPlugin implements Listener {
    private void writeMarker(String name) {
        try {
            var folder = new java.io.File(getDataFolder().getParentFile(), "TestPlugin");
            java.nio.file.Files.createDirectories(folder.toPath());
            java.nio.file.Files.writeString(new java.io.File(folder, name).toPath(), "ok");
        } catch (java.io.IOException e) { throw new java.io.UncheckedIOException(e); }
    }
    private void appendEvent(String line) {
        try {
            var folder = new java.io.File(getDataFolder().getParentFile(), "TestPlugin");
            java.nio.file.Files.createDirectories(folder.toPath());
            java.nio.file.Files.writeString(new java.io.File(folder, "events.txt").toPath(), line + "\n", java.nio.charset.StandardCharsets.UTF_8, java.nio.file.StandardOpenOption.CREATE, java.nio.file.StandardOpenOption.APPEND);
        } catch (java.io.IOException e) { throw new java.io.UncheckedIOException(e); }
    }
    @EventHandler public void onJoin(org.bukkit.event.player.PlayerJoinEvent event) { appendEvent("join " + event.getPlayer().getName()); }
    @EventHandler public void onQuit(org.bukkit.event.player.PlayerQuitEvent event) { appendEvent("quit " + event.getPlayer().getName()); }
    @EventHandler public void onMove(org.bukkit.event.player.PlayerMoveEvent event) {
        appendEvent("move " + event.getPlayer().getName() + " " + event.getTo().getBlockX() + " " + event.getTo().getBlockY() + " " + event.getTo().getBlockZ());
        if (event.getTo().getY() > 90.0) { appendEvent("move-cancel " + event.getPlayer().getName()); event.setCancelled(true); }
    }
    @EventHandler public void onInteract(org.bukkit.event.player.PlayerInteractEvent event) {
        var clicked = event.getClickedBlock();
        appendEvent("interact " + (event.getAction() == org.bukkit.event.block.Action.LEFT_CLICK_BLOCK ? "left" : "right") + " " + clicked.getX() + " " + clicked.getY() + " " + clicked.getZ());
    }
    @EventHandler public void onBreak(org.bukkit.event.block.BlockBreakEvent event) {
        var block = event.getBlock();
        appendEvent("break " + block.getX() + " " + block.getY() + " " + block.getZ());
        appendEvent("break-type " + block.getType().getKey() + " " + block.getWorld().getName());
        if (block.getX() == 1) { appendEvent("break-cancel " + block.getX() + " " + block.getY() + " " + block.getZ()); event.setCancelled(true); }
    }
    @EventHandler public void onCommand(org.bukkit.event.player.PlayerCommandPreprocessEvent event) {
        appendEvent("command " + event.getMessage());
        if (event.getMessage().equals("/testping nope")) event.setCancelled(true);
    }
    @EventHandler public void onPlace(org.bukkit.event.block.BlockPlaceEvent event) {
        var block = event.getBlockPlaced();
        appendEvent("place " + block.getX() + " " + block.getY() + " " + block.getZ());
        appendEvent("place-cancel " + block.getX() + " " + block.getY() + " " + block.getZ());
        event.setCancelled(true);
    }
    @EventHandler public void onChat(io.papermc.paper.event.player.AsyncChatEvent event) {
        String message = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(event.message());
        appendEvent("chat " + message);
        if (message.startsWith("cancel")) event.setCancelled(true);
    }
    @EventHandler public void onChunkLoad(org.bukkit.event.world.ChunkLoadEvent event) { appendEvent("chunk-load " + event.getChunk().getX() + " " + event.getChunk().getZ()); }
    @EventHandler public void onChunkUnload(org.bukkit.event.world.ChunkUnloadEvent event) { appendEvent("chunk-unload " + event.getChunk().getX() + " " + event.getChunk().getZ()); }
    @EventHandler public void onWorldLoad(org.bukkit.event.world.WorldLoadEvent event) { appendEvent("world-load " + event.getWorld().getName()); }
    @EventHandler public void onWeather(org.bukkit.event.weather.WeatherChangeEvent event) { appendEvent("weather " + event.toWeatherState()); }
    @EventHandler public void onDamage(org.bukkit.event.entity.EntityDamageEvent event) {
        appendEvent("damage " + event.getCause() + " " + event.getDamage());
        if (event.getEntity() instanceof org.bukkit.entity.Player player) {
            if (player.getName().equals("EventError")) throw new IllegalStateException("event failure");
            if (player.getName().equals("EventProbe")) {
                if (event.getCause() == org.bukkit.event.entity.EntityDamageEvent.DamageCause.FALL && event.getDamage() == 5.0) event.setCancelled(true);
                else if (event.getDamage() == 7.0) event.setDamage(2.5);
            }
        }
    }
    @EventHandler public void onDeath(org.bukkit.event.entity.PlayerDeathEvent event) {
        appendEvent("death " + event.getPlayer().getName());
        String name = event.getPlayer().getName();
        if (name.equals("EventError")) throw new IllegalStateException("event failure");
        if (name.equals("UnsupportedDeath")) event.setKeepLevel(true);
        if (name.equals("EventProbe")) {
            event.getDrops().clear();
            event.getDrops().add(new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND, 2));
            event.setDroppedExp(3);
        }
    }
    @EventHandler public void onEntityDeath(org.bukkit.event.entity.EntityDeathEvent event) { appendEvent("entity-death " + event.getEntityType()); }
    @EventHandler public void onRespawn(org.bukkit.event.player.PlayerRespawnEvent event) { appendEvent("respawn " + event.getRespawnLocation().getBlockY()); }
    @EventHandler public void onLevel(org.bukkit.event.player.PlayerLevelChangeEvent event) { appendEvent("level " + event.getOldLevel() + " " + event.getNewLevel()); }
    @EventHandler public void onExp(org.bukkit.event.player.PlayerExpChangeEvent event) { appendEvent("exp " + event.getAmount()); }
    @EventHandler public void onInventoryClick(org.bukkit.event.inventory.InventoryClickEvent event) { appendEvent("click " + event.getSlot()); }
    @EventHandler public void onInventoryDrag(org.bukkit.event.inventory.InventoryDragEvent event) {
        var slots = event.getRawSlots().stream().sorted().map(String::valueOf).collect(java.util.stream.Collectors.joining(","));
        appendEvent("drag " + slots);
    }
    @EventHandler public void onInventoryClose(org.bukkit.event.inventory.InventoryCloseEvent event) { appendEvent("close " + event.getPlayer().getName()); }
    @EventHandler public void onLegacyChat(org.bukkit.event.player.AsyncPlayerChatEvent event) {
        appendEvent("legacy-chat " + event.getMessage());
        if (event.getMessage().startsWith("cancel")) event.setCancelled(true);
    }
    @Override public void onEnable() {
        getLogger().info("FerriteTest enabled");
        getLogger().info("server name " + getServer().getName());
        getLogger().info("initial ip ban " + getServer().getIPBans().contains("192.0.2.7"));
        getLogger().info("FerriteTest settings " + getServer().getMotd() + " " + getServer().getMaxPlayers() + " " + getServer().getPort() + " " + getServer().getViewDistance() + " " + getServer().hasWhitelist());
        if (!getName().equals("FerriteLegacyTest")) getLogger().info(dev.ferrite.dependency.Helper.greeting());
        getCommand("testping").setExecutor((sender, command, label, args) -> { sender.sendMessage("pong"); return true; });
        var inventoryCommand = getCommand("test-inventory");
        if (inventoryCommand != null) inventoryCommand.setExecutor((sender, command, label, args) -> {
            if (!(sender instanceof org.bukkit.entity.Player player)) return false;
            var inventory = getServer().createInventory(null, 9, "Lifecycle");
            inventory.setItem(0, new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 1));
            player.openInventory(inventory);
            appendEvent("inventory-open " + player.getName());
            return true;
        });
        var dragSetup = getCommand("drag-setup");
        if (dragSetup != null) dragSetup.setExecutor((sender, command, label, args) -> {
            if (!(sender instanceof org.bukkit.entity.Player player)) return false;
            player.getInventory().setItem(0, new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 40));
            appendEvent("drag-setup");
            return true;
        });
        var verify = getCommand("verify-features");
        if (verify != null) verify.setExecutor((sender, command, label, args) -> {
            if (!(sender instanceof org.bukkit.entity.Player player)) return false;
            var at = player.getLocation();
            at.getWorld().strikeLightningEffect(at.clone().add(5, 0, 0));
            var board = getServer().getScoreboardManager().getNewScoreboard();
            var objective = board.registerNewObjective("verify", org.bukkit.scoreboard.Criteria.DUMMY, net.kyori.adventure.text.Component.text("Verify"));
            objective.setDisplaySlot(org.bukkit.scoreboard.DisplaySlot.SIDEBAR);
            objective.getScore("Bot").setScore(7);
            var team = board.registerNewTeam("vteam");
            team.color(net.kyori.adventure.text.format.NamedTextColor.GREEN);
            team.addEntry("Bot");
            player.setScoreboard(board);
            player.showBossBar(net.kyori.adventure.bossbar.BossBar.bossBar(net.kyori.adventure.text.Component.text("Boss"), 0.5f, net.kyori.adventure.bossbar.BossBar.Color.RED, net.kyori.adventure.bossbar.BossBar.Overlay.PROGRESS));
            player.sendEquipmentChange(player, org.bukkit.inventory.EquipmentSlot.HEAD, new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_HELMET));
            appendEvent("verify chunk-sent " + player.isChunkSent(org.bukkit.Chunk.getChunkKey(0, 0)) + " sent-chunks>0 " + (player.getSentChunks().size() > 0));
            return true;
        });
        var verifyItem = getCommand("verify-item");
        if (verifyItem != null) verifyItem.setExecutor((sender, command, label, args) -> {
            if (!(sender instanceof org.bukkit.entity.Player player)) return false;
            var staff = new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_SWORD, 1);
            var meta = staff.getItemMeta();
            var pdc = meta.getPersistentDataContainer();
            pdc.set(new org.bukkit.NamespacedKey("ferrite", "owner"), org.bukkit.persistence.PersistentDataType.STRING, "bot\nline");
            pdc.set(new org.bukkit.NamespacedKey("ferrite", "level"), org.bukkit.persistence.PersistentDataType.INTEGER, 12);
            pdc.set(new org.bukkit.NamespacedKey("ferrite", "flag"), org.bukkit.persistence.PersistentDataType.BOOLEAN, true);
            pdc.set(new org.bukkit.NamespacedKey("ferrite", "arc"), org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY, new int[]{3, -4});
            meta.addItemFlags(org.bukkit.inventory.ItemFlag.HIDE_ENCHANTS, org.bukkit.inventory.ItemFlag.HIDE_DYE, org.bukkit.inventory.ItemFlag.HIDE_ADDITIONAL_TOOLTIP);
            staff.setItemMeta(meta);
            player.getInventory().setItem(8, staff);
            appendEvent("verify-item set");
            return true;
        });
        getCommand("ferrite-test").setTabCompleter((sender, command, alias, args) -> java.util.List.of("hello"));
        getCommand("ferrite-test").setExecutor((sender, command, label, args) -> { if (sender instanceof org.bukkit.entity.Player player) { getLogger().info("offline snapshot " + getServer().getOfflinePlayer("KnownOffline").getUniqueId().equals(java.util.UUID.fromString("00000000-0000-0000-0000-00000000002c"))); getLogger().info("player snapshot " + player.getHealth() + " " + player.getFoodLevel() + " " + player.getGameMode()); var w = getServer().getWorld("probe_world"); if (w != null) { if (player.getInventory().getItem(0).getType() != org.bukkit.Material.STONE) throw new IllegalStateException("inventory snapshot"); player.getInventory().setItem(0, new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIRT, 2)); getServer().setWhitelist(false); getServer().banIP("192.0.2.2"); player.displayName(net.kyori.adventure.text.Component.text("Display")); player.sendTitle("Title", "Subtitle", 1, 2, 3); player.playSound(player.getLocation(), "minecraft:entity.player.levelup", org.bukkit.SoundCategory.PLAYERS, 1.0f, 0.5f); player.kickPlayer("bye"); player.setOp(false); player.clearActivePotionEffects(); player.addPotionEffect(new org.bukkit.potion.PotionEffect(org.bukkit.potion.PotionEffectType.SPEED, 200, 1)); player.setHealth(10.0); player.setFoodLevel(9); player.setGameMode(org.bukkit.GameMode.ADVENTURE); if (player.getWorld() != w || !player.teleport(new org.bukkit.Location(w, 5, 70, 6))) throw new IllegalStateException("player routing"); if (w.getHighestBlockYAt(1, 2) != 64 || w.getChunkAt(0, 0).getBlock(1, 64, 2).getType() != org.bukkit.Material.STONE) throw new IllegalStateException("chunk snapshot"); var block = w.getBlockAt(1, 64, 2); getLogger().info("world snapshot " + w.getName() + " " + block.getType()); block.setType(org.bukkit.Material.DIRT); } } var api = getServer(); getLogger().info("surface-query " + api.getCurrentTick() + " " + api.matchPlayer("Ferrite").size() + " " + (api.getOfflinePlayerIfCached("KnownOffline") != null) + " " + api.isStopping() + " " + (api.shutdownMessage() == null) + " " + api.broadcast(net.kyori.adventure.text.Component.text("surface"), "ferrite.surface")); sender.sendMessage("hello " + String.join(" ", args)); dev.ferrite.server.Native.dispatchCommand((org.bukkit.command.CommandSender) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.command.ConsoleCommandSender.class}, (p, m, a) -> null), "spoof"); dev.ferrite.server.Native.dispatchCommand(sender, "bridge-command"); return true; });
        org.bukkit.World world = (org.bukkit.World) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.World.class}, (proxy, method, args) -> {
            if (method.getName().equals("getUID")) return java.util.UUID.fromString("00000000-0000-0000-0000-000000000001");
            throw new UnsupportedOperationException(method.getName());
        });
        getServer().getRegionScheduler().run(this, world, -4, 8, task -> { if (getServer().isOwnedByCurrentRegion(world, -4, 8) && getServer().isOwnedByCurrentRegion(world, io.papermc.paper.math.Position.block(-64, 0, 128)) && getServer().isOwnedByCurrentRegion(new org.bukkit.Location(world, -64, 0, 128)) && !getServer().isOwnedByCurrentRegion(world, -4, 8, 1)) getLogger().info("FerriteTest owned"); if (getServer().isOwnedByCurrentRegion(world, -4, 8, -3, 8)) getLogger().info("FerriteTest range owned"); getLogger().info("FerriteTest region"); });
        ((dev.ferrite.server.FerriteServer) getServer()).entityScheduler(java.util.UUID.fromString("00000000-0000-0000-0000-000000000002"), () -> true).run(this, task -> getLogger().info("FerriteTest entity"), null);
        getServer().getPluginManager().registerEvents(this, this);
        getServer().getPluginManager().callEvent(new TestEvent());
        getServer().getGlobalRegionScheduler().runAtFixedRate(this, task -> getLogger().info("FerriteTest global"), 1, 1);
        getServer().getScheduler().runTaskTimer(this, () -> getLogger().info("FerriteTest tick"), 1, 1);
        var api = getServer();
        var manager = api.getPluginManager();
        getLogger().info("surface " + api.getDefaultGameMode() + " " + api.getAllowFlight() + " " + api.getAllowNether() + " " + api.getAllowEnd() + " " + api.getGenerateStructures() + " " + api.getMaxWorldSize() + " " + api.getSimulationDistance() + " " + api.getSpawnRadius() + " " + api.getIdleTimeout() + " " + api.getPauseWhenEmptyTime() + " " + api.getConnectionThrottle() + " " + api.isWhitelistEnforced() + " " + manager.useTimings() + " " + api.isStopping() + " " + api.broadcast(net.kyori.adventure.text.Component.text("surface"), "ferrite.surface"));
        getLogger().info("surface-motd " + net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(api.motd()));
        getLogger().info("surface-folder " + api.getPluginsFolder().isAbsolute() + " " + (api.getEntity(java.util.UUID.fromString("00000000-0000-0000-0000-00000000002c")) == null));
        var factory = api.getItemFactory();
        getLogger().info("surface-api ip=[" + api.getIp() + "] hardcore=" + api.isHardcore() + " logips=" + api.isLoggingIPs() + " secure=" + api.isEnforcingSecureProfiles() + " transfers=" + api.isAcceptingTransfers() + " hide=" + api.getHideOnlinePlayers() + " force-default=" + api.forcesDefaultGameMode() + " container=" + api.getWorldContainer().isAbsolute() + " level=" + api.getLevelDirectory().isAbsolute() + " update=" + api.getUpdateFolder() + "/" + api.getUpdateFolderFile().getName() + " warnings=" + api.getWarningState() + " paused=" + api.isPaused() + " ticking=" + api.isTickingWorlds() + " global=" + api.isGlobalTickThread() + " chained=" + api.getMaxChainedNeighborUpdates() + " enabled=" + api.getInitialEnabledPacks() + " disabled=" + api.getInitialDisabledPacks() + " pack=" + (api.getResourcePack() == null) + " serverpack=" + (api.getServerResourcePack() == null) + " pack-required=" + api.isResourcePackRequired() + " bosses=" + api.getBossBars().hasNext() + " boss=" + (api.getBossBar(new org.bukkit.NamespacedKey("ferrite", "none")) == null) + " remove-boss=" + api.removeBossBar(new org.bukkit.NamespacedKey("ferrite", "none")) + " throttle=" + api.getConnectionThrottle() + " registry=" + (api.getRegistry(org.bukkit.enchantments.Enchantment.class).get(org.bukkit.NamespacedKey.minecraft("sharpness")) != null));
        var stoneMeta = factory.getItemMeta(org.bukkit.Material.STONE);
        getLogger().info("surface-itemfactory meta=" + (stoneMeta != null) + " air=" + (factory.getItemMeta(org.bukkit.Material.AIR) == null) + " applicable=" + factory.isApplicable(stoneMeta, org.bukkit.Material.STONE) + " not-applicable=" + factory.isApplicable(stoneMeta, org.bukkit.Material.AIR) + " copied=" + (factory.asMetaFor(stoneMeta, org.bukkit.Material.DIRT) != null) + " leather=" + factory.getDefaultLeatherColor().asRGB() + " egg=" + (factory.getSpawnEgg(org.bukkit.entity.EntityType.ZOMBIE) == org.bukkit.Material.ZOMBIE_SPAWN_EGG) + " name=" + net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(factory.displayName(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE))));
        var dispatched = api.dispatchCommand(api.getConsoleSender(), "testping");
        getLogger().info("surface-common dispatched=" + dispatched + " online=" + api.getOnlinePlayers().size() + " worlds=" + api.getWorlds().size() + " plugins=" + api.getPluginManager().getPlugins().length + " self-enabled=" + api.getPluginManager().isPluginEnabled(this) + " player=" + (api.getPlayer("Nobody") == null));
        api.savePlayers();
        api.reloadPermissions();
        writeMarker("enabled.txt");
    }
    @EventHandler public void onTest(TestEvent event) { getLogger().info("FerriteTest event"); }
    @Override public void onDisable() { getLogger().info("FerriteTest disabled"); writeMarker("disabled.txt"); }
}
