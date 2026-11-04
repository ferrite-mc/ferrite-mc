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
}
