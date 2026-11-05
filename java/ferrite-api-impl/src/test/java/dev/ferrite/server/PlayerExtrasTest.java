package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Statistic;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class PlayerExtrasTest {
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e1", "extras_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000aa", "Extras", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000aa", "Extras");
    }

    /** Other test classes share the player and leave it in worlds they unloaded, so bring it home. */
    @org.junit.jupiter.api.BeforeEach void goHome() {
        player.teleport(new org.bukkit.Location(org.bukkit.Bukkit.getWorld("extras_world"), 0, 65, 0));
    }

    @org.junit.jupiter.api.AfterAll static void stop() {
        var world = org.bukkit.Bukkit.getWorld("extras_world");
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    @Test void attributesApplyModifiersInVanillaOrder() {
        var speed = player.getAttribute(Attribute.MOVEMENT_SPEED);
        assertEquals(0.1, speed.getValue(), 1e-9);
        speed.setBaseValue(1.0);
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "add"), 1.0, AttributeModifier.Operation.ADD_NUMBER));
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "half"), 0.5, AttributeModifier.Operation.ADD_SCALAR));
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "double"), 1.0, AttributeModifier.Operation.MULTIPLY_SCALAR_1));
        assertEquals((1.0 + 1.0) * 1.5 * 2.0, speed.getValue(), 1e-9);
        assertSame(speed, player.getAttribute(Attribute.MOVEMENT_SPEED));
    }

    @Test void statisticsTrackQualifiersAndRejectNegatives() {
        player.incrementStatistic(Statistic.JUMP, 3);
        player.decrementStatistic(Statistic.JUMP);
        assertEquals(2, player.getStatistic(Statistic.JUMP));
        player.setStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.STONE, 5);
        assertEquals(5, player.getStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.STONE));
        assertEquals(0, player.getStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.DIRT));
        assertThrows(IllegalArgumentException.class, () -> player.getStatistic(Statistic.MINE_BLOCK));
        assertThrows(IllegalArgumentException.class, () -> player.setStatistic(Statistic.JUMP, -1));
    }

    @Test void conversationsRunAndAbandon() {
        var prompt = new org.bukkit.conversations.StringPrompt() {
            String got;
            @Override public String getPromptText(org.bukkit.conversations.ConversationContext c) { return "Name?"; }
            @Override public org.bukkit.conversations.Prompt acceptInput(org.bukkit.conversations.ConversationContext c, String input) { got = input; return END_OF_CONVERSATION; }
        };
        var conversation = new org.bukkit.conversations.ConversationFactory(null).withFirstPrompt(prompt).withLocalEcho(false).buildConversation(player);
        assertTrue(player.beginConversation(conversation));
        assertTrue(player.isConversing());
        assertNull(Bootstrap.fireChat("00000000-0000-0000-0000-0000000000aa", "Extras", "Alex"));
        assertEquals("Alex", prompt.got);
        assertFalse(player.isConversing());
    }

    @Test void bodyBookkeepingReadsBackWhatWasSet() {
        player.setArrowsInBody(4, false);
        assertEquals(4, player.getArrowsInBody());
        assertThrows(IllegalArgumentException.class, () -> player.setArrowsInBody(-1, false));
        player.setPlayerTime(6000, false);
        assertEquals(6000, player.getPlayerTime());
        assertFalse(player.isPlayerTimeRelative());
        player.resetPlayerTime();
        assertTrue(player.isPlayerTimeRelative());
        assertFalse(player.isInsideVehicle());
        assertTrue(player.getPassengers().isEmpty());
    }

    @Test void visibilityListingAndAlliesAreRemembered() {
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000cc", "Friend", false);
        org.bukkit.entity.Player other;
        try {
            var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
            ensure.setAccessible(true);
            other = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000cc", "Friend");
        } catch (ReflectiveOperationException e) { throw new AssertionError(e); }
        assertTrue(player.canSee(other));
        player.hidePlayer(other);
        assertFalse(player.canSee(other));
        player.showPlayer(other);
        assertTrue(player.canSee(other));
        assertTrue(player.isListed(other));
        assertTrue(player.unlistPlayer(other));
        assertFalse(player.isListed(other));
        assertTrue(player.listPlayer(other));
        assertTrue(player.isAlliedTo(player));
        player.addAdditionalAlliedRule(net.kyori.adventure.key.Key.key("test", "rule"), e -> true);
        assertTrue(player.isAlliedTo(other));
        player.removeAdditionalAlliedRule(net.kyori.adventure.key.Key.key("test", "rule"));
        assertTrue(player.getAdditionalAlliedRules().isEmpty());
        assertSame(player.getEnderChest(), player.getEnderChest());
        assertEquals(27, player.getEnderChest().getSize());
        player.resetIdleDuration();
        assertTrue(player.getIdleDuration().toSeconds() < 5);
    }

    @Test void nmsTitlePacketsReachThePlayerApi() {
        var connection = ((org.bukkit.craftbukkit.entity.CraftPlayer) player).getHandle().connection;
        var text = net.minecraft.network.chat.Component.literal("Hi");
        connection.send(new net.minecraft.network.protocol.game.ClientboundSetSubtitleTextPacket(text));
        connection.send(new net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket(5, 40, 5));
        connection.send(new net.minecraft.network.protocol.game.ClientboundSetTitleTextPacket(text));
        connection.send(new net.minecraft.network.protocol.game.ClientboundSetActionBarTextPacket(text));
        connection.send(new net.minecraft.network.protocol.game.ClientboundClearTitlesPacket(true));
    }

    @Test void windowPropertiesGoToTheHostAndOutOfRangeValuesAreRefused() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            assertTrue(player.setWindowProperty(org.bukkit.inventory.InventoryView.Property.BREW_TIME, 400));
            assertFalse(player.setWindowProperty(org.bukkit.inventory.InventoryView.Property.BREW_TIME, 70000));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("setWindowProperty(int,int)|0" + HostBridge.ARG + "400"), calls);
    }

    @Test void worldEffectsReachOnlyPlayersInRange() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(subject + "|" + method); return ""; };
        try {
            var world = player.getWorld();
            var here = player.getLocation();
            world.playEffect(here, org.bukkit.Effect.CLICK2, 0, 10);
            assertTrue(calls.contains(player.getUniqueId() + "|playLevelEvent(Location,int,int)"));
            calls.clear();
            world.playEffect(here.clone().add(500, 0, 0), org.bukkit.Effect.CLICK2, 0, 10);
            assertTrue(calls.isEmpty());
        } finally { HostBridge.transport = null; }
    }

    @Test void legacyHandAccessorsUseTheMainHand() {
        HostBridge.transport = (owner, subject, method, args) -> "";
        try {
            player.setItemInHand(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STICK, 3));
            assertEquals(org.bukkit.Material.STICK, player.getItemInHand().getType());
            assertEquals(3, player.getInventory().getItemInMainHand().getAmount());
        } finally { HostBridge.transport = null; }
    }

    @Test void cookiesAreSentBase64EncodedAndLimitedTo5120Bytes() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            player.storeCookie(new org.bukkit.NamespacedKey("test", "session"), new byte[]{1, 2, 3});
            assertThrows(IllegalArgumentException.class, () -> player.storeCookie(new org.bukkit.NamespacedKey("test", "big"), new byte[5121]));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("storeCookie(NamespacedKey,String)|test:session" + HostBridge.ARG + "AQID"), calls);
    }

    @Test void merchantRecipesAreEncodedForTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            var merchant = new FerriteMerchant("Shop");
            var recipe = new org.bukkit.inventory.MerchantRecipe(new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND), 0, 8, false, 2, 0.05f);
            recipe.addIngredient(new org.bukkit.inventory.ItemStack(org.bukkit.Material.EMERALD, 3));
            merchant.setRecipes(java.util.List.of(recipe));
            try { player.openMerchant(merchant, true); } catch (RuntimeException noView) { /* no open view offline */ }
            assertNull(player.openMerchant(new FerriteMerchant("Empty"), true));
        } finally { HostBridge.transport = null; }
        assertEquals(1, calls.size());
        var expected = "Shop" + HostBridge.ARG + "DIAMOND" + HostBridge.ITEM + "1" + (char) 0x1c + "EMERALD" + HostBridge.ITEM + "3" + (char) 0x1c
            + HostBridge.NULL + (char) 0x1c + "0" + (char) 0x1c + "8" + (char) 0x1c + "2" + (char) 0x1c + "0.05";
        assertEquals("openMerchant(String,String)|" + expected, calls.get(0));
    }

    @Test void rememberedSettingsReadBackTheirOwnValuesAndValidateTheScale() {
        assertNull(player.getCustomName());
        player.setCustomName("Boss");
        player.setCustomNameVisible(true);
        player.setCollidable(false);
        player.setHealthScale(6.0);
        assertEquals("Boss", player.getCustomName());
        assertTrue(player.isCustomNameVisible());
        assertFalse(player.isCollidable());
        assertEquals(6.0, player.getHealthScale());
        assertTrue(player.hasAI());
        assertThrows(IllegalArgumentException.class, () -> player.setHealthScale(0));
        assertThrows(IllegalArgumentException.class, () -> player.setHealthScale(Double.NaN));
    }

    @Test void currentInputDecodesTheKeyFlags() {
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getInputFlags()") ? "81" : null;
        try {
            var input = player.getCurrentInput();
            assertTrue(input.isForward());
            assertTrue(input.isJump());
            assertTrue(input.isSprint());
            assertFalse(input.isBackward());
            assertFalse(input.isSneak());
        } finally { HostBridge.transport = null; }
    }

    @Test void resourcePackStatusIsNullUntilTheClientAnswers() {
        assertNull(player.getResourcePackStatus());
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getResourcePackStatus()") ? "DECLINED" : null;
        try {
            assertEquals(org.bukkit.event.player.PlayerResourcePackStatusEvent.Status.DECLINED, player.getResourcePackStatus());
        } finally { HostBridge.transport = null; }
    }

    @Test void leftoverPlayerValuesAreReadBack() {
        var p = player;
        p.teleport(new org.bukkit.Location(player.getWorld(), 0, 65, 0, 90f, 0f));
        assertEquals(90f, p.getBodyYaw());
        p.setBodyYaw(10f);
        assertEquals(10f, p.getBodyYaw());
        p.setEnchantmentSeed(1234);
        assertEquals(1234, p.getEnchantmentSeed());
        assertTrue(p.getEnderPearls().isEmpty());
        assertEquals(net.kyori.adventure.util.TriState.NOT_SET, p.getFrictionState());
        p.setFrictionState(net.kyori.adventure.util.TriState.TRUE);
        assertEquals(net.kyori.adventure.util.TriState.TRUE, p.getFrictionState());
        assertNull(p.getLastDeathLocation());
        p.setLastDeathLocation(new org.bukkit.Location(player.getWorld(), 1, 2, 3));
        assertEquals(2.0, p.getLastDeathLocation().getY());
        assertEquals(80, p.getStarvationRate());
        p.setStarvationRate(40);
        assertEquals(40, p.getStarvationRate());
        p.increaseWardenWarningLevel();
        assertEquals(1, p.getWardenWarningLevel());
        assertTrue(p.isVisibleByDefault());
        p.setVisibleByDefault(false);
        assertFalse(p.isVisibleByDefault());
        assertEquals("minecraft:default", p.getWaypointStyle().asString());
        p.setNoPhysics(true);
        assertTrue(p.hasNoPhysics());
    }

    @Test void smallPlayerAndWorldLeftovers() {
        assertTrue(player.canUseEquipmentSlot(org.bukkit.inventory.EquipmentSlot.HEAD));
        assertFalse(player.canUseEquipmentSlot(org.bukkit.inventory.EquipmentSlot.BODY));
        assertNull(player.releaseLeftShoulderEntity());
        assertEquals(player.getUniqueId().toString(), player.serialize().get("UUID"));
        var world = player.getWorld();
        assertTrue(world.canGenerateStructures());
        assertNull(world.getRaid(1));
        assertEquals(4.0f, world.getVoidDamageAmount());
        world.setVoidDamageAmount(8f);
        assertEquals(8f, world.getVoidDamageAmount());
        world.setChunkForceLoaded(2, 3, true);
        assertTrue(world.isChunkForceLoaded(2, 3));
        assertEquals(1, world.getForceLoadedChunks().size());
        world.setChunkForceLoaded(2, 3, false);
        assertFalse(world.isChunkForceLoaded(2, 3));
        var plugin = (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class}, (proxy, method, args) -> method.getName().equals("hashCode") ? 7 : method.getName().equals("equals") ? proxy == args[0] : null);
        assertTrue(world.addPluginChunkTicket(1, 1, plugin));
        assertFalse(world.addPluginChunkTicket(1, 1, plugin));
        assertEquals(1, world.getPluginChunkTickets().get(plugin).size());
        world.removePluginChunkTickets(plugin);
        assertTrue(world.getPluginChunkTickets().isEmpty());
    }

    @Test void clientOptionsComeFromTheClientSettings() {
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "getLocale()" -> "ko_kr";
            case "getMainHand()" -> "LEFT";
            case "getClientViewDistance()" -> "12";
            case "getClientChatMode()" -> "2";
            case "getClientSkinParts()" -> "65";
            case "getClientChatColors()" -> "false";
            default -> null;
        };
        try {
            assertEquals("ko_kr", player.getLocale());
            assertEquals("ko", player.locale().getLanguage());
            assertEquals(org.bukkit.inventory.MainHand.LEFT, player.getMainHand());
            assertEquals(12, player.getClientOption(com.destroystokyo.paper.ClientOption.VIEW_DISTANCE));
            assertEquals(com.destroystokyo.paper.ClientOption.ChatVisibility.HIDDEN, player.getClientOption(com.destroystokyo.paper.ClientOption.CHAT_VISIBILITY));
            assertEquals(false, player.getClientOption(com.destroystokyo.paper.ClientOption.CHAT_COLORS_ENABLED));
            var parts = player.getClientOption(com.destroystokyo.paper.ClientOption.SKIN_PARTS);
            assertTrue(parts.hasCapeEnabled());
            assertTrue(parts.hasHatsEnabled());
            assertFalse(parts.hasJacketEnabled());
        } finally { HostBridge.transport = null; }
        assertEquals("en_us", player.getLocale());
    }

    @Test void bossBarsAreSentAndKeptInStep() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var bar = net.kyori.adventure.bossbar.BossBar.bossBar(net.kyori.adventure.text.Component.text("Boss"), 0.5f, net.kyori.adventure.bossbar.BossBar.Color.RED, net.kyori.adventure.bossbar.BossBar.Overlay.PROGRESS);
            player.showBossBar(bar);
            player.showBossBar(bar);
            bar.progress(0.25f);
            bar.name(net.kyori.adventure.text.Component.text("Other"));
            bar.color(net.kyori.adventure.bossbar.BossBar.Color.BLUE);
            assertTrue(player.activeBossBars().iterator().hasNext());
            player.hideBossBar(bar);
            bar.progress(0.75f);
            assertFalse(player.activeBossBars().iterator().hasNext());
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("bossBarAdd(String,String,float,int,int,int)", "bossBarProgress(String,float)", "bossBarName(String,String)", "bossBarStyle(String,int,int)", "bossBarRemove(String)"), calls);
    }

    @Test void droppedItemsGoToTheHostAndComeBackAsAHandle() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var at = new org.bukkit.Location(player.getWorld(), 1, 70, 2);
            var seen = new java.util.ArrayList<org.bukkit.Material>();
            var item = player.getWorld().dropItemNaturally(at, new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND, 3), i -> seen.add(i.getItemStack().getType()));
            assertEquals(3, item.getItemStack().getAmount());
            assertEquals(java.util.List.of(org.bukkit.Material.DIAMOND), seen);
            player.getWorld().dropItem(at, new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 1));
            assertThrows(IllegalArgumentException.class, () -> player.getWorld().dropItem(at, new org.bukkit.inventory.ItemStack(org.bukkit.Material.AIR, 1)));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("dropItemNaturally(Location,ItemStack,String)", "dropItem(Location,ItemStack,String)"), calls);
    }

    @Test void playersThrowItemsThroughTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var seen = new java.util.ArrayList<Integer>();
            var item = player.dropItem(new org.bukkit.inventory.ItemStack(org.bukkit.Material.APPLE, 2), true, i -> seen.add(i.getItemStack().getAmount()));
            assertEquals(2, item.getItemStack().getAmount());
            assertEquals(java.util.List.of(2), seen);
            assertThrows(IllegalArgumentException.class, () -> player.dropItem(new org.bukkit.inventory.ItemStack(org.bukkit.Material.AIR, 1), true, null));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("dropStack(ItemStack)"), calls);
    }

    @Test void spawnedEntitiesGoToTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var at = new org.bukkit.Location(player.getWorld(), 0, 70, 0);
            var cow = player.getWorld().spawnEntity(at, org.bukkit.entity.EntityType.COW);
            assertEquals(org.bukkit.entity.EntityType.COW, cow.getType());
            var sand = player.getWorld().spawnFallingBlock(at, org.bukkit.Material.SAND.createBlockData());
            assertEquals(org.bukkit.entity.EntityType.FALLING_BLOCK, sand.getType());
            assertThrows(IllegalArgumentException.class, () -> player.getWorld().spawnFallingBlock(at, org.bukkit.Material.DIAMOND, (byte) 0));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("spawnEntity(Location,EntityType,String)", "spawnFallingBlock(Location,Material,String)"), calls);
    }

    @Test void equipmentMapIsSentSlotBySlot() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var items = new java.util.LinkedHashMap<org.bukkit.inventory.EquipmentSlot, org.bukkit.inventory.ItemStack>();
            items.put(org.bukkit.inventory.EquipmentSlot.HEAD, new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_HELMET));
            items.put(org.bukkit.inventory.EquipmentSlot.FEET, new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_BOOTS));
            player.sendEquipmentChange(player, items);
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("sendEquipmentChange(LivingEntity,EquipmentSlot,ItemStack)", "sendEquipmentChange(LivingEntity,EquipmentSlot,ItemStack)"), calls);
    }

    @Test void sentChunksComeFromTheHost() {
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getSentChunkCoordinates()") ? "1,2" + HostBridge.ITEM + "-3,4" : "";
        try {
            var chunks = player.getSentChunks();
            assertEquals(2, chunks.size());
            assertTrue(chunks.stream().anyMatch(c -> c.getX() == -3 && c.getZ() == 4));
        } finally { HostBridge.transport = null; }
    }

    @Test void lightningStrikesGoToTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var at = new org.bukkit.Location(player.getWorld(), 3, 70, 3);
            assertEquals(org.bukkit.entity.EntityType.LIGHTNING_BOLT, player.getWorld().strikeLightning(at).getType());
            assertEquals(org.bukkit.entity.EntityType.LIGHTNING_BOLT, player.getWorld().strikeLightningEffect(at).getType());
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("strikeLightning(Location)", "strikeLightningEffect(Location)"), calls);
    }

    @Test void playersShootArrowsThroughTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var arrow = player.launchProjectile(org.bukkit.entity.Arrow.class, new org.bukkit.util.Vector(0, 0, 2), null);
            assertEquals(org.bukkit.entity.EntityType.ARROW, arrow.getType());
            assertThrows(UnsupportedOperationException.class, () -> player.launchProjectile(org.bukkit.entity.Fireball.class, null, null));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("spawnArrow(Location,double,double,double,String)"), calls);
    }

    @Test void cookiesAreRetrievedFromTheClient() throws Exception {
        var polls = new java.util.concurrent.atomic.AtomicInteger();
        HostBridge.transport = (owner, subject, method, args) ->
            method.equals("getCookieReply(NamespacedKey)") ? (polls.incrementAndGet() < 3 ? "pending" : "AQID") : "";
        try {
            var bytes = player.retrieveCookie(new org.bukkit.NamespacedKey("test", "token")).get(5, java.util.concurrent.TimeUnit.SECONDS);
            assertArrayEquals(new byte[]{1, 2, 3}, bytes);
            HostBridge.transport = (owner, subject, method, args) -> method.equals("getCookieReply(NamespacedKey)") ? HostBridge.NULL : "";
            assertNull(player.retrieveCookie(new org.bukkit.NamespacedKey("test", "none")).get(5, java.util.concurrent.TimeUnit.SECONDS));
        } finally { HostBridge.transport = null; }
    }

    @Test void spawnedEntityHandlesReachTheLiveEntity() {
        var seen = new java.util.ArrayList<String>();
        var sep = String.valueOf(HostBridge.ITEM);
        HostBridge.transport = (owner, subject, method, args) -> {
            seen.add(owner + ":" + method);
            return switch (method) {
                case "isValid()" -> "true";
                case "getEntityPosition()" -> "5.5" + sep + "70" + sep + "-1" + sep + "90" + sep + "0";
                case "getVelocity()" -> "0" + sep + "0.5" + sep + "0";
                case "getHealth()" -> "7.0";
                default -> "";
            };
        };
        try {
            var world = org.bukkit.Bukkit.getWorld("extras_world");
            var cow = (org.bukkit.entity.LivingEntity) world.spawnEntity(new org.bukkit.Location(world, 0, 70, 0), org.bukkit.entity.EntityType.COW);
            assertTrue(cow.isValid());
            assertEquals(5.5, cow.getLocation().getX());
            assertEquals(0.5, cow.getVelocity().getY());
            assertEquals(7.0, cow.getHealth());
            cow.setVelocity(new org.bukkit.util.Vector(0, 1, 0));
            cow.damage(3.0);
            cow.remove();
            player.attack(cow);
        } finally { HostBridge.transport = null; }
        assertTrue(seen.contains("Entity:setVelocity(Vector)") && seen.contains("Entity:damage(double)") && seen.contains("Entity:remove()") && seen.contains("Player:attack(String)"), seen.toString());
    }

    @Test void scoreboardsAreShownAndKeptInStep() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args.replace(String.valueOf(HostBridge.ARG), ",")); return ""; };
        try {
            var board = org.bukkit.Bukkit.getScoreboardManager().getNewScoreboard();
            var objective = board.registerNewObjective("kills", org.bukkit.scoreboard.Criteria.DUMMY, net.kyori.adventure.text.Component.text("Kills"));
            objective.setDisplaySlot(org.bukkit.scoreboard.DisplaySlot.SIDEBAR);
            objective.getScore("Alex").setScore(3);
            player.setScoreboard(board);
            assertSame(board, player.getScoreboard());
            objective.getScore("Alex").setScore(4);
            objective.getScore("Alex").resetScore();
            objective.unregister();
            player.setScoreboard(org.bukkit.Bukkit.getScoreboardManager().getNewScoreboard());
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of(
            "sbObjective(String,int,String,int)|kills,0,Kills,0",
            "sbDisplay(int,String)|1,kills",
            "sbScore(String,String,int)|Alex,kills,3",
            "sbScore(String,String,int)|Alex,kills,4",
            "sbReset(String,String)|Alex,kills",
            "sbObjective(String,int,String,int)|kills,1,,0"), calls);
    }

    @Test void teamsAreSentAndChangesFollow() {
        var calls = new java.util.ArrayList<String>();
        var part = String.valueOf(HostBridge.ITEM);
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args.replace(String.valueOf(HostBridge.ARG), ";").replace(part, ",")); return ""; };
        try {
            var board = org.bukkit.Bukkit.getScoreboardManager().getNewScoreboard();
            var team = board.registerNewTeam("red");
            team.addEntry("Alex");
            team.color(net.kyori.adventure.text.format.NamedTextColor.RED);
            player.setScoreboard(board);
            team.addEntry("Bob");
            team.setAllowFriendlyFire(false);
            team.unregister();
            player.setScoreboard(org.bukkit.Bukkit.getScoreboardManager().getNewScoreboard());
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of(
            "sbTeam(String,int,String,String)|red;0;red,3,0,0,12,,;Alex",
            "sbTeam(String,int,String,String)|red;3;;Bob",
            "sbTeam(String,int,String,String)|red;2;red,2,0,0,12,,;",
            "sbTeam(String,int,String,String)|red;1;;"), calls);
    }

    @Test void joiningPlayersSeeTheMainScoreboard() {
        var calls = new java.util.ArrayList<String>();
        var main = org.bukkit.Bukkit.getScoreboardManager().getMainScoreboard();
        var objective = main.registerNewObjective("joinboard", org.bukkit.scoreboard.Criteria.DUMMY, net.kyori.adventure.text.Component.text("Join"));
        objective.setDisplaySlot(org.bukkit.scoreboard.DisplaySlot.SIDEBAR);
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            Bootstrap.firePlayerJoin(player.getUniqueId().toString(), player.getName());
        } finally { HostBridge.transport = null; objective.unregister(); }
        assertTrue(calls.contains("sbObjective(String,int,String,int)"), calls.toString());
        assertSame(main, player.getScoreboard());
    }

    @Test void worldRayTracesFindTheFirstBlockWithExactHitData() {
        var world = org.bukkit.Bukkit.getWorld("extras_world");
        for (int x = 0; x < 3; x++) world.getBlockAt(x, 80, 0).setType(org.bukkit.Material.AIR);
        world.getBlockAt(3, 80, 0).setType(org.bukkit.Material.STONE);
        var start = new org.bukkit.Location(world, 0.5, 80.5, 0.5);
        var hit = world.rayTraceBlocks(start, new org.bukkit.util.Vector(1, 0, 0), 10);
        assertNotNull(hit);
        assertEquals(3, hit.getHitBlock().getX());
        assertEquals(org.bukkit.block.BlockFace.WEST, hit.getHitBlockFace());
        assertEquals(3.0, hit.getHitPosition().getX(), 1e-9);
        assertEquals(80.5, hit.getHitPosition().getY(), 1e-9);
        assertNull(world.rayTraceBlocks(start, new org.bukkit.util.Vector(1, 0, 0), 1.0));
        assertThrows(IllegalArgumentException.class, () -> world.rayTraceBlocks(start, new org.bukkit.util.Vector(0, 0, 0), 5));
        HostBridge.transport = (owner, subject, method, args) -> method.equals("isSolid()") ? "true" : "";
        try {
            var all = world.rayTrace(start, new org.bukkit.util.Vector(2, 0, 0), 10, org.bukkit.FluidCollisionMode.NEVER, true, 0.0, null);
            assertEquals(3, all.getHitBlock().getX());
        } finally { HostBridge.transport = null; }
    }

    @Test void collisionChecksUseTheSolidBlocksAroundTheBody() {
        var world = org.bukkit.Bukkit.getWorld("extras_world");
        for (int x = 49; x <= 50; x++) for (int y = 200; y <= 201; y++) for (int z = 49; z <= 50; z++) world.getBlockAt(x, y, z).setType(org.bukkit.Material.AIR);
        HostBridge.transport = (owner, subject, method, args) -> method.equals("isSolid()") ? "true" : "";
        try {
            var spot = new org.bukkit.Location(world, 50, 200, 50);
            assertFalse(player.collidesAt(spot));
            world.getBlockAt(50, 200, 50).setType(org.bukkit.Material.STONE);
            assertTrue(player.collidesAt(spot));
            assertTrue(player.wouldCollideUsing(new org.bukkit.util.BoundingBox(50.1, 200.1, 50.1, 50.4, 200.4, 50.4)));
        } finally { HostBridge.transport = null; }
    }

    @Test void settingABiomeGoesToTheHost() {
        var seen = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { seen.add(method + "|" + args.replace(String.valueOf(HostBridge.ARG), ",")); return ""; };
        try {
            org.bukkit.Bukkit.getWorld("extras_world").setBiome(8, 70, -3, org.bukkit.block.Biome.DESERT);
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of("setBiome(int,int,int,Biome)|8,70,-3,minecraft:desert"), seen);
    }

    @Test void gameEventsHaveNoListenersAndDoNothing() {
        var world = org.bukkit.Bukkit.getWorld("extras_world");
        world.sendGameEvent(player, org.bukkit.GameEvent.EXPLODE, new org.bukkit.util.Vector(1, 64, 1));
    }

    @Test void toolsWearOutAndNothingMendableLeavesXpAlone() {
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getMaxDurability()") ? "1561" : "";
        try {
            var pick = new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_PICKAXE);
            var worn = player.damageItemStack(pick, 10);
            assertEquals(10, ((org.bukkit.inventory.meta.Damageable) worn.getItemMeta()).getDamage());
            assertEquals(0, ((org.bukkit.inventory.meta.Damageable) pick.getItemMeta()).getDamage(), "the original is untouched");
            assertTrue(player.damageItemStack(worn, 1551).getType().isAir(), "a tool that wears out breaks");
            assertSame(pick, player.damageItemStack(pick, 0));
            assertEquals(30, player.applyMending(30), "with nothing mendable worn, no xp is used up");
        } finally { HostBridge.transport = null; }
    }

    @Test void wornToolsReachTheHostAsMaterialAmountAndDamage() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(method + "[" + args + "]");
            return method.equals("getMaxDurability()") ? "1561" : "";
        };
        try {
            var worn = player.damageItemStack(new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_PICKAXE), 7);
            try { player.getInventory().setItem(3, worn); } catch (IllegalStateException noNative) { /* no host in unit tests */ }
            assertTrue(calls.stream().anyMatch(c -> c.startsWith("setItemDamage(int,int)[3") && c.endsWith("7]")), calls.toString());
            var named = worn.clone();
            var meta = named.getItemMeta();
            meta.displayName(net.kyori.adventure.text.Component.text("x"));
            named.setItemMeta(meta);
            assertThrows(UnsupportedOperationException.class, () -> player.getInventory().setItem(4, named));
        } finally { HostBridge.transport = null; }
    }

    @Test void ridingIsAskedOfTheHostAndMobsComeBackAsHandles() {
        var pig = java.util.UUID.fromString("00000000-0000-0000-0000-00000000p1c0".replace("p1c0", "0a1c"));
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(owner + "#" + method);
            return switch (method) {
                case "getVehicle()" -> pig.toString();
                case "isInsideVehicle()", "leaveVehicle()" -> "true";
                case "getEntityType()" -> "minecraft:pig";
                case "getPassengers()" -> pig.toString();
                default -> "";
            };
        };
        try {
            assertTrue(player.isInsideVehicle());
            var vehicle = player.getVehicle();
            assertEquals(org.bukkit.entity.EntityType.PIG, vehicle.getType());
            assertEquals(pig, vehicle.getUniqueId());
            assertEquals(1, player.getPassengers().size());
            assertTrue(player.leaveVehicle());
        } finally { HostBridge.transport = null; }
    }

    @Test void booksAreSentAsTitleAuthorAndPlainPages() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            var book = new org.bukkit.inventory.ItemStack(org.bukkit.Material.WRITTEN_BOOK);
            var meta = (org.bukkit.inventory.meta.BookMeta) book.getItemMeta();
            meta.title(net.kyori.adventure.text.Component.text("T"));
            meta.author(net.kyori.adventure.text.Component.text("A"));
            meta.addPages(net.kyori.adventure.text.Component.text("p1"), net.kyori.adventure.text.Component.text("p2"));
            book.setItemMeta(meta);
            player.openBook(book);
            assertEquals(1, calls.size());
            assertTrue(calls.get(0).startsWith("openBookContent(String)|TAp1"), calls.get(0));
            assertThrows(IllegalArgumentException.class, () -> player.openBook(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE)));
        } finally { HostBridge.transport = null; }
    }

    @Test void mapsArePaintedByTheirRenderersAndSentAsColours() {
        var calls = new java.util.ArrayList<Object[]>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(new Object[]{method, args}); return ""; };
        try {
            var view = new FerriteMapView(player.getWorld());
            view.addRenderer(new org.bukkit.map.MapRenderer() {
                @Override public void render(org.bukkit.map.MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player viewer) {
                    canvas.setPixel(2, 1, (byte) 44);
                    canvas.setPixelColor(0, 0, java.awt.Color.WHITE);
                }
            });
            var pixels = view.render(player);
            assertEquals(44, pixels[1 * 128 + 2]);
            assertNotEquals(0, pixels[0]);
            player.sendMap(view);
            assertEquals("sendMapData(int,int,boolean,byte[])", calls.get(0)[0]);
        } finally { HostBridge.transport = null; }
    }

    @Test void chunkUnloadsAndGenerationStateFollowTheHost() {
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "isChunkLoaded(int,int)" -> args.startsWith("1") ? "true" : "false";
            case "isChunkGenerated(int,int)" -> args.startsWith("1") ? "true" : "false";
            default -> "";
        };
        try {
            var world = player.getWorld();
            assertFalse(world.unloadChunk(1, 0), "a held chunk stays");
            assertTrue(world.unloadChunk(2, 0), "an unloaded chunk counts as unloaded");
            assertEquals(io.papermc.paper.world.chunk.ChunkGenerationState.GENERATED, world.getChunkGenerationState(1, 0));
            assertEquals(io.papermc.paper.world.chunk.ChunkGenerationState.NOT_GENERATED, world.getChunkGenerationState(2, 0));
        } finally { HostBridge.transport = null; }
    }

    @Test void thrownProjectilesAreLaunchedWithTheirOwner() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "#" + method); return ""; };
        try {
            var pearl = player.launchProjectile(org.bukkit.entity.EnderPearl.class);
            assertEquals(org.bukkit.entity.EntityType.ENDER_PEARL, pearl.getType());
            player.launchProjectile(org.bukkit.entity.Snowball.class);
            player.launchProjectile(org.bukkit.entity.Arrow.class);
            assertTrue(calls.contains("Player#launchThrown(Location,EntityType,double,double,double,String,String)"), calls.toString());
            assertTrue(calls.contains("Player#spawnArrow(Location,double,double,double,String)"), calls.toString());
            assertThrows(UnsupportedOperationException.class, () -> player.launchProjectile(org.bukkit.entity.Fireball.class));
        } finally { HostBridge.transport = null; }
    }
}
