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
        assertTrue(world.isVoidDamageEnabled());
        var voidCalls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { voidCalls.add(method + "|" + args); return ""; };
        try {
            world.setVoidDamageAmount(8f);
            world.setVoidDamageEnabled(false);
            assertThrows(IllegalArgumentException.class, () -> world.setVoidDamageAmount(-1f));
        } finally { HostBridge.transport = null; }
        assertEquals(8f, world.getVoidDamageAmount());
        assertFalse(world.isVoidDamageEnabled());
        assertEquals(2, voidCalls.size());
        assertTrue(voidCalls.get(1).startsWith("setVoidDamage(boolean,float,double)|false" + HostBridge.ARG + "8.0"));
        world.setChunkForceLoaded(2, 3, true);
        assertTrue(world.isChunkForceLoaded(2, 3));
        assertEquals(1, world.getForceLoadedChunks().size());
        world.setChunkForceLoaded(2, 3, false);
        assertFalse(world.isChunkForceLoaded(2, 3));
        var plugin = (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class}, (proxy, method, args) -> switch (method.getName()) {
            case "hashCode" -> 7;
            case "equals" -> proxy == args[0];
            case "isEnabled" -> true;
            case "getName" -> "ticket-plugin";
            default -> null;
        });
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

    @Test void droppedItemStateRoundTripsAndStaysSafeWhenTheEntityIsGone() {
        var calls = new java.util.ArrayList<String>();
        var stack = new java.util.concurrent.atomic.AtomicReference<>(new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND, 1));
        var delay = new java.util.concurrent.atomic.AtomicInteger(10);
        var owner = new java.util.concurrent.atomic.AtomicReference<java.util.UUID>();
        HostBridge.transport = (hostOwner, subject, method, args) -> {
            calls.add(hostOwner + "#" + method);
            if (hostOwner.equals("World")) return "";
            if (hostOwner.equals("Entity") && method.equals("isValid()")) return "true";
            return switch (method) {
                case "getItemStack()" -> stack.get().getType().name() + HostBridge.ITEM + stack.get().getAmount();
                case "setItemStack(ItemStack)" -> {
                    var parts = args.split(String.valueOf(HostBridge.ITEM));
                    stack.set(new org.bukkit.inventory.ItemStack(org.bukkit.Material.matchMaterial(parts[0]), Integer.parseInt(parts[1])));
                    yield "";
                }
                case "getPickupDelay()" -> Integer.toString(delay.get());
                case "setPickupDelay(int)" -> { delay.set(Integer.parseInt(args)); yield ""; }
                case "getOwner()" -> owner.get() == null ? HostBridge.NULL : owner.get().toString();
                case "setOwner(UUID)" -> { owner.set(args.equals(HostBridge.NULL) ? null : java.util.UUID.fromString(args)); yield ""; }
                case "setUnlimitedLifetime(boolean)", "setCanMobPickup(boolean)", "setCanPlayerPickup(boolean)", "setWillAge(boolean)" -> "";
                case "isUnlimitedLifetime()", "canMobPickup()", "canPlayerPickup()", "willAge()" -> "true";
                case "getHealth()" -> "5";
                case "setHealth(int)" -> "";
                default -> null;
            };
        };
        try {
            var world = player.getWorld();
            var item = world.dropItemNaturally(new org.bukkit.Location(world, 1, 70, 2), new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND, 1));
            assertEquals(1, item.getItemStack().getAmount());
            item.setItemStack(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 2));
            assertEquals(org.bukkit.Material.STONE, item.getItemStack().getType());
            assertEquals(2, item.getItemStack().getAmount());
            assertEquals(10, item.getPickupDelay());
            item.setPickupDelay(4);
            assertEquals(4, item.getPickupDelay());
            var ownerId = java.util.UUID.randomUUID();
            item.setOwner(ownerId);
            assertEquals(ownerId, item.getOwner());
            item.setOwner(null);
            assertNull(item.getOwner());
            item.setUnlimitedLifetime(true);
            item.setCanMobPickup(false);
            item.setCanPlayerPickup(false);
            item.setWillAge(false);
            item.setHealth(9);

            HostBridge.transport = (hostOwner, subject, method, args) -> method.equals("isValid()") ? "false" : null;
            assertFalse(item.isValid());
            assertTrue(item.isDead());
            assertEquals(2, item.getItemStack().getAmount(), "a vanished item keeps its last known stack");
        } finally { HostBridge.transport = null; }
        assertTrue(calls.contains("Item#setItemStack(ItemStack)"));
        assertTrue(calls.contains("Item#setPickupDelay(int)"));
        assertTrue(calls.contains("Item#setOwner(UUID)"));
        assertTrue(calls.contains("Item#setUnlimitedLifetime(boolean)"));
        assertTrue(calls.contains("Item#setCanMobPickup(boolean)"));
        assertTrue(calls.contains("Item#setCanPlayerPickup(boolean)"));
        assertTrue(calls.contains("Item#setWillAge(boolean)"));
        assertTrue(calls.contains("Item#setHealth(int)"));
    }

    @Test void entitySnapshotsFollowTheLiveEntityAndStayReadableAfterShutdown() {
        var world = player.getWorld();
        var position = "4.0" + HostBridge.ITEM + "65.0" + HostBridge.ITEM + "6.0" + HostBridge.ITEM + "90.0" + HostBridge.ITEM + "10.0";
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "spawnEntity(Location,EntityType,String)" -> "";
            case "getEntityPosition()" -> position;
            case "getVelocity()" -> "0.0" + HostBridge.ITEM + "0.5" + HostBridge.ITEM + "0.0";
            case "getHealth()" -> "8.0";
            case "isValid()" -> "true";
            default -> null;
        };
        try {
            var cow = world.spawnEntity(new org.bukkit.Location(world, 4, 65, 6), org.bukkit.entity.EntityType.COW);
            var snapshot = (FerriteEntitySnapshot) cow.createSnapshot();
            assertEquals(org.bukkit.entity.EntityType.COW, snapshot.getEntityType());
            assertEquals(4.0, snapshot.getLocation().getX());
            assertEquals(6.0, snapshot.getLocation().getZ());
            assertEquals(8.0, snapshot.getHealth());
            assertEquals(0.5, snapshot.getVelocity().getY());
            assertTrue(snapshot.isValid());
            assertTrue(snapshot.getAsString().contains("minecraft:cow"));

            HostBridge.transport = (owner, subject, method, args) -> method.equals("isValid()") ? "false" : null;
            var gone = (FerriteEntitySnapshot) cow.createSnapshot();
            assertFalse(gone.isValid());
            assertEquals(org.bukkit.entity.EntityType.COW, gone.getEntityType());

            HostBridge.transport = null;
            Native.close();
            assertEquals(org.bukkit.entity.EntityType.COW, snapshot.getEntityType());
            assertEquals(4.0, snapshot.getLocation().getX());
            assertThrows(IllegalStateException.class, () -> ((FerriteServer) org.bukkit.Bukkit.getServer()).getEntityFactory().createEntitySnapshot("{}"));
        } finally {
            HostBridge.transport = null;
            Native.link();
        }
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
        assertEquals(java.util.List.of("getItemStack()", "dropItemNaturally(Location,ItemStack,String)", "getItemStack()", "dropItem(Location,ItemStack,String)"), calls);
    }

    @Test void craftWorldDropMethodsUseTheLiveWorldPath() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var at = new org.bukkit.Location(player.getWorld(), 3, 70, 4);
            var craft = new org.bukkit.craftbukkit.CraftWorld(player.getWorld());
            var stone = new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 1);
            var diamond = new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND, 2);
            assertNotNull(craft.dropItem(at, stone));
            assertNotNull(craft.dropItem(at, stone, item -> {}));
            assertNotNull(craft.dropItemNaturally(at, diamond));
            assertNotNull(craft.dropItemNaturally(at, diamond, item -> {}));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of(
                "dropItem(Location,ItemStack,String)",
                "dropItem(Location,ItemStack,String)",
                "dropItemNaturally(Location,ItemStack,String)",
                "dropItemNaturally(Location,ItemStack,String)"), calls);
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
        assertEquals(java.util.List.of("getItemStack()", "dropStack(ItemStack)", "getItemStack()"), calls);
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

    @Test void playersShootProjectilesThroughTheHost() {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        try {
            var arrow = player.launchProjectile(org.bukkit.entity.Arrow.class, new org.bukkit.util.Vector(0, 0, 2), null);
            assertEquals(org.bukkit.entity.EntityType.ARROW, arrow.getType());
            var small = player.launchProjectile(org.bukkit.entity.SmallFireball.class, new org.bukkit.util.Vector(0, 0, 1), null);
            assertEquals(org.bukkit.entity.EntityType.SMALL_FIREBALL, small.getType());
            assertThrows(UnsupportedOperationException.class, () -> player.launchProjectile(org.bukkit.entity.DragonFireball.class, null, null));
        } finally { HostBridge.transport = null; }
        assertEquals(java.util.List.of(
            "spawnArrow(Location,double,double,double,String)",
            "launchThrown(Location,EntityType,double,double,double,String,String)"), calls);
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
            try { player.getInventory().setItem(4, named); } catch (IllegalStateException noNative) { /* no host in unit tests */ }
            assertTrue(calls.stream().anyMatch(c -> c.startsWith("setItemMeta(int,String)[4") && c.contains("x")), calls.toString());
            var flagged = worn.clone();
            var flaggedMeta = flagged.getItemMeta();
            flaggedMeta.addItemFlags(org.bukkit.inventory.ItemFlag.HIDE_ENCHANTS);
            flagged.setItemMeta(flaggedMeta);
            try { player.getInventory().setItem(5, flagged); } catch (IllegalStateException noNative) { /* no host in unit tests */ }
            assertTrue(calls.stream().anyMatch(c -> c.startsWith("setItemMeta(int,String)[5") && c.contains("HIDE_ENCHANTS")), calls.toString());
        } finally { HostBridge.transport = null; }
    }

    @Test void itemPdcKeepsTypesAndReachesTheHostSlot() throws Exception {
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "[" + args + "]"); return ""; };
        try {
            var item = new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE);
            var meta = item.getItemMeta();
            var pdc = meta.getPersistentDataContainer();
            var byteKey = new org.bukkit.NamespacedKey("ferrite", "byte");
            var shortKey = new org.bukkit.NamespacedKey("ferrite", "short");
            var intKey = new org.bukkit.NamespacedKey("ferrite", "int");
            var longKey = new org.bukkit.NamespacedKey("ferrite", "long");
            var floatKey = new org.bukkit.NamespacedKey("ferrite", "float");
            var doubleKey = new org.bukkit.NamespacedKey("ferrite", "double");
            var boolKey = new org.bukkit.NamespacedKey("ferrite", "bool");
            var stringKey = new org.bukkit.NamespacedKey("ferrite", "string");
            var bytesKey = new org.bukkit.NamespacedKey("ferrite", "bytes");
            var intsKey = new org.bukkit.NamespacedKey("ferrite", "ints");
            var longsKey = new org.bukkit.NamespacedKey("ferrite", "longs");
            pdc.set(byteKey, org.bukkit.persistence.PersistentDataType.BYTE, (byte) 4);
            pdc.set(shortKey, org.bukkit.persistence.PersistentDataType.SHORT, (short) 5);
            pdc.set(intKey, org.bukkit.persistence.PersistentDataType.INTEGER, 6);
            pdc.set(longKey, org.bukkit.persistence.PersistentDataType.LONG, 7L);
            pdc.set(floatKey, org.bukkit.persistence.PersistentDataType.FLOAT, 1.5f);
            pdc.set(doubleKey, org.bukkit.persistence.PersistentDataType.DOUBLE, 2.5);
            pdc.set(boolKey, org.bukkit.persistence.PersistentDataType.BOOLEAN, true);
            pdc.set(stringKey, org.bukkit.persistence.PersistentDataType.STRING, "hello\nworld");
            pdc.set(bytesKey, org.bukkit.persistence.PersistentDataType.BYTE_ARRAY, new byte[]{1, -2});
            pdc.set(intsKey, org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY, new int[]{3, -4});
            pdc.set(longsKey, org.bukkit.persistence.PersistentDataType.LONG_ARRAY, new long[]{5L, -6L});
            meta.addItemFlags(org.bukkit.inventory.ItemFlag.HIDE_ENCHANTS, org.bukkit.inventory.ItemFlag.HIDE_DYE, org.bukkit.inventory.ItemFlag.HIDE_DESTROYS);
            assertEquals((byte) 4, pdc.get(byteKey, org.bukkit.persistence.PersistentDataType.BYTE));
            assertEquals((short) 5, pdc.get(shortKey, org.bukkit.persistence.PersistentDataType.SHORT));
            assertEquals(6, pdc.get(intKey, org.bukkit.persistence.PersistentDataType.INTEGER));
            assertEquals(7L, pdc.get(longKey, org.bukkit.persistence.PersistentDataType.LONG));
            assertEquals(1.5f, pdc.get(floatKey, org.bukkit.persistence.PersistentDataType.FLOAT));
            assertEquals(2.5, pdc.get(doubleKey, org.bukkit.persistence.PersistentDataType.DOUBLE));
            assertTrue(pdc.get(boolKey, org.bukkit.persistence.PersistentDataType.BOOLEAN));
            assertEquals("hello\nworld", pdc.get(stringKey, org.bukkit.persistence.PersistentDataType.STRING));
            assertArrayEquals(new byte[]{1, -2}, pdc.get(bytesKey, org.bukkit.persistence.PersistentDataType.BYTE_ARRAY));
            assertArrayEquals(new int[]{3, -4}, pdc.get(intsKey, org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY));
            assertArrayEquals(new long[]{5L, -6L}, pdc.get(longsKey, org.bukkit.persistence.PersistentDataType.LONG_ARRAY));
            assertNull(pdc.get(byteKey, org.bukkit.persistence.PersistentDataType.INTEGER), "wrong type access is null");
            assertFalse(pdc.has(byteKey, org.bukkit.persistence.PersistentDataType.BOOLEAN));
            // Nested containers and lists used to be refused; they are supported now (see ItemPdcNestingTest).
            pdc.set(new org.bukkit.NamespacedKey("ferrite", "nested"), org.bukkit.persistence.PersistentDataType.TAG_CONTAINER, new FerriteDataContainer());
            assertTrue(pdc.has(new org.bukkit.NamespacedKey("ferrite", "nested"), org.bukkit.persistence.PersistentDataType.TAG_CONTAINER));
            pdc.remove(new org.bukkit.NamespacedKey("ferrite", "nested"));
            pdc.remove(longKey);
            assertFalse(pdc.has(longKey));
            meta.removeItemFlags(org.bukkit.inventory.ItemFlag.HIDE_DESTROYS);
            assertTrue(item.setItemMeta(meta));
            var clone = item.clone();
            assertTrue(clone.isSimilar(item));
            var bytes = ((FerriteDataContainer) clone.getItemMeta().getPersistentDataContainer()).serializeToBytes();
            var reloaded = new FerriteDataContainer();
            reloaded.readFromBytes(bytes, true);
            assertEquals("hello\nworld", reloaded.get(stringKey, org.bukkit.persistence.PersistentDataType.STRING));
            assertArrayEquals(new int[]{3, -4}, reloaded.get(intsKey, org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY));
            assertTrue(reloaded.get(boolKey, org.bukkit.persistence.PersistentDataType.BOOLEAN));
            try { player.getInventory().setItem(6, clone); } catch (IllegalStateException noNative) { /* no host in unit tests */ }
            var payload = calls.stream().filter(c -> c.startsWith("setItemMeta(int,String)[6")).findFirst().orElseThrow();
            assertTrue(payload.contains("HIDE_ENCHANTS") && payload.contains("ferrite:string"), payload);
            assertFalse(payload.contains("HIDE_DESTROYS"), "a removed item flag reached the host: " + payload);
            assertFalse(payload.contains("\"ferrite:long\":"), "a removed PDC key reached the host: " + payload);
            assertTrue(payload.contains("HIDE_DYE"), payload);
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
        HostBridge.transport = (owner, subject, method, args) -> { if (!method.startsWith("allocateMap(")) calls.add(new Object[]{subject, method, args}); return ""; };
        try {
            var view = new FerriteMapView(player.getWorld());
            view.addRenderer(new org.bukkit.map.MapRenderer() {
                @Override public void render(org.bukkit.map.MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player viewer) {
                    canvas.setPixel(2, 1, (byte) 44);
                    canvas.setPixelColor(0, 0, java.awt.Color.WHITE);
                }
            });
            var rendered = view.render(player);
            var pixels = rendered.pixels();
            assertEquals(44, pixels[1 * 128 + 2]);
            assertNotEquals(0, pixels[0]);
            assertEquals("", rendered.decorations(), "no cursors means an empty decoration list");
            view.setScale(org.bukkit.map.MapView.Scale.FAR);
            view.setLocked(true);
            player.sendMap(view);
            assertEquals(player.getUniqueId().toString(), calls.get(0)[0]);
            assertEquals("sendMapData(int,int,boolean,byte[],String)", calls.get(0)[1]);
            var args = ((String) calls.get(0)[2]).split(String.valueOf(HostBridge.ARG), -1);
            assertEquals(String.valueOf(view.getScale().getValue()), args[1]);
            assertEquals("true", args[2]);
            assertEquals("", args[4]);
        } finally { HostBridge.transport = null; }
    }

    @Test void mapCursorsCarryTypeBoundsDirectionAndPlainCaptions() {
        var calls = new java.util.ArrayList<Object[]>();
        HostBridge.transport = (owner, subject, method, args) -> { if (!method.startsWith("allocateMap(")) calls.add(new Object[]{subject, method, args}); return ""; };
        try {
            assertEquals(0, org.bukkit.map.MapCursor.Type.PLAYER.getValue());
            assertEquals(26, org.bukkit.map.MapCursor.Type.RED_X.getValue());
            assertEquals("minecraft:red_x", org.bukkit.map.MapCursor.Type.RED_X.getKey().toString());
            var view = new FerriteMapView(player.getWorld());
            view.addRenderer(new org.bukkit.map.MapRenderer() {
                @Override public void render(org.bukkit.map.MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player viewer) {
                    canvas.getCursors().addCursor(new org.bukkit.map.MapCursor((byte) -128, (byte) 127, (byte) 3,
                        org.bukkit.map.MapCursor.Type.PLAYER, true, net.kyori.adventure.text.Component.text("Alex")));
                    canvas.getCursors().addCursor(new org.bukkit.map.MapCursor((byte) 127, (byte) -128, (byte) 15,
                        org.bukkit.map.MapCursor.Type.RED_X, true));
                    canvas.getCursors().addCursor(new org.bukkit.map.MapCursor((byte) 1, (byte) 2, (byte) 0,
                        org.bukkit.map.MapCursor.Type.RED_MARKER, false));
                }
            });
            var rendered = view.render(player);
            var caption = java.util.Base64.getEncoder().encodeToString("Alex".getBytes(java.nio.charset.StandardCharsets.UTF_8));
            var expected = "minecraft:player-12812731" + caption
                + "" + "minecraft:red_x127-128150";
            assertEquals(expected, rendered.decorations(), "hidden cursors are omitted");
            view.setScale(org.bukkit.map.MapView.Scale.FAR);
            view.setLocked(true);
            player.sendMap(view);
            var args = ((String) calls.get(0)[2]).split(String.valueOf(HostBridge.ARG), -1);
            assertEquals(player.getUniqueId().toString(), calls.get(0)[0]);
            assertEquals("sendMapData(int,int,boolean,byte[],String)", calls.get(0)[1]);
            assertEquals(String.valueOf(view.getScale().getValue()), args[1]);
            assertEquals("true", args[2]);
            assertEquals(expected, args[4]);
        } finally { HostBridge.transport = null; }
    }

    @Test void mapUpdatesUseTheRequestedViewer() {
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000bb", "Other", false);
        org.bukkit.entity.Player other;
        try {
            var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
            ensure.setAccessible(true);
            other = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000bb", "Other");
        } catch (ReflectiveOperationException e) { throw new AssertionError(e); }
        var calls = new java.util.ArrayList<Object[]>();
        HostBridge.transport = (owner, subject, method, args) -> { if (!method.startsWith("allocateMap(")) calls.add(new Object[]{subject, method, args}); return ""; };
        try {
            var view = new FerriteMapView(player.getWorld());
            view.addRenderer(new org.bukkit.map.MapRenderer() {
                @Override public void render(org.bukkit.map.MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player viewer) {
                    canvas.getCursors().addCursor(new org.bukkit.map.MapCursor((byte) 0, (byte) 0, (byte) 0,
                        org.bukkit.map.MapCursor.Type.PLAYER, true,
                        net.kyori.adventure.text.Component.text(viewer.getName())));
                }
            });
            player.sendMap(view);
            other.sendMap(view);
            assertEquals(2, calls.size());
            assertEquals(player.getUniqueId().toString(), calls.get(0)[0]);
            assertEquals(other.getUniqueId().toString(), calls.get(1)[0]);
            var encode = java.util.Base64.getEncoder();
            var first = ((String) calls.get(0)[2]).split(String.valueOf(HostBridge.ARG), -1)[4];
            var second = ((String) calls.get(1)[2]).split(String.valueOf(HostBridge.ARG), -1)[4];
            assertEquals("minecraft:player0001"
                + encode.encodeToString(player.getName().getBytes(java.nio.charset.StandardCharsets.UTF_8)), first);
            assertEquals("minecraft:player0001"
                + encode.encodeToString(other.getName().getBytes(java.nio.charset.StandardCharsets.UTF_8)), second);
        } finally { HostBridge.transport = null; }
    }

    @Test void chunkUnloadsAndGenerationStateFollowTheHost() {
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "isChunkLoaded(int,int)" -> args.startsWith("1") ? "true" : "false";
            case "isChunkGenerated(int,int)" -> args.startsWith("1") ? "true" : "false";
            case "unloadChunk(int,int)" -> args.startsWith("1") ? "false" : "true";
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
        var calls = new java.util.ArrayList<Object[]>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(new Object[]{owner, subject, method, args}); return ""; };
        try {
            var seen = new java.util.concurrent.atomic.AtomicReference<org.bukkit.entity.SmallFireball>();
            var small = player.launchProjectile(org.bukkit.entity.SmallFireball.class, null, seen::set);
            assertEquals(org.bukkit.entity.EntityType.SMALL_FIREBALL, small.getType());
            assertSame(small, seen.get());
            var pearl = player.launchProjectile(org.bukkit.entity.EnderPearl.class);
            assertEquals(org.bukkit.entity.EntityType.ENDER_PEARL, pearl.getType());
            player.launchProjectile(org.bukkit.entity.Snowball.class);
            player.launchProjectile(org.bukkit.entity.Arrow.class);
            assertEquals(org.bukkit.entity.EntityType.FIREBALL, player.launchProjectile(org.bukkit.entity.Fireball.class).getType());
            var launch = calls.stream()
                .filter(call -> "launchThrown(Location,EntityType,double,double,double,String,String)".equals(call[2]))
                .findFirst().orElseThrow();
            assertEquals("Player", launch[0]);
            assertEquals(player.getUniqueId().toString(), launch[1]);
            var args = ((String) launch[3]).split(String.valueOf(HostBridge.ARG), -1);
            assertEquals("SMALL_FIREBALL", args[1]);
            assertEquals(player.getUniqueId().toString(), args[5]);
            assertEquals(small.getUniqueId().toString(), args[6]);
        } finally { HostBridge.transport = null; }
    }

    @Test void damageTheHostReportsEndsUpInTheCombatTracker() {
        var tracker = player.getCombatTracker();
        assertFalse(tracker.isInCombat());
        Bootstrap.fireEntityDamage(player.getUniqueId().toString(), player.getName(), "fall", 3.0);
        assertTrue(tracker.isInCombat());
        assertTrue(tracker.isTakingDamage());
        assertEquals(1, tracker.getCombatEntries().size());
        assertEquals(3.0f, tracker.getCombatEntries().get(0).getDamage());
        tracker.resetCombatState();
        assertFalse(tracker.isInCombat());
    }

    @Test void noLightningRodInReachMeansNoLocation() {
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "findLightningRod(int,int,int)" -> "";
            case "findLightningTarget(int,int,int)" -> "1 64 1";
            default -> "";
        };
        try {
            assertNull(player.getWorld().findLightningRod(new org.bukkit.Location(player.getWorld(), 1, 64, 1)));
            assertEquals(64, player.getWorld().findLightningTarget(new org.bukkit.Location(player.getWorld(), 1, 64, 1)).getBlockY());
        } finally { HostBridge.transport = null; }
    }

    @Test void onlySignsCanBeSentAsBlockUpdates() {
        var chest = (org.bukkit.block.TileState) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.block.TileState.class},
            (self, method, args) -> method.getName().equals("getType") ? org.bukkit.Material.CHEST : null);
        assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(new org.bukkit.Location(player.getWorld(), 0, 64, 0), chest));
    }

    @Test void connectionForwardsToThePlayer() {
        var connection = player.getConnection();
        assertSame(player, connection.getPlayer());
        assertEquals(player.isOnline(), connection.isConnected());
        assertThrows(UnsupportedOperationException.class, connection::reenterConfiguration);
    }

    @Test void postEffectsMirrorTheListAndDeduplicate() {
        var effects = player.postEffects();
        assertTrue(effects.values().isEmpty());
        assertFalse(effects.remove(net.kyori.adventure.key.Key.key("minecraft:blur")));
        assertFalse(effects.clear());
        effects.update();
    }

    @Test void worldPathComesFromTheHost() {
        var world = player.getWorld();
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getWorldPath()") ? "/srv/world" : "";
        try { assertEquals(java.nio.file.Path.of("/srv/world"), world.getWorldPath()); } finally { HostBridge.transport = null; }
        HostBridge.transport = (owner, subject, method, args) -> "";
        try { assertThrows(UnsupportedOperationException.class, world::getWorldPath); } finally { HostBridge.transport = null; }
    }

    private static String idNbt(String id) {
        var out = new java.io.ByteArrayOutputStream();
        try (var data = new java.io.DataOutputStream(out)) {
            data.writeByte(10); data.writeShort(0);
            data.writeByte(8); data.writeUTF("id"); data.writeUTF(id);
            data.writeByte(0);
        } catch (java.io.IOException e) { throw new IllegalStateException(e); }
        var hex = new StringBuilder();
        for (byte b : out.toByteArray()) hex.append(String.format("%02x", b));
        return hex.toString();
    }

    @Test void tileEntityCountsComeFromTheLoadedChunks() {
        var world = player.getWorld();
        char item = HostBridge.ITEM;
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "getLoadedChunkCoordinates()" -> "0,0" + item + "1,0";
            case "ferriteBlockEntities(int,int,int,int,int,int)" -> args.startsWith("0")
                ? "1,64,1," + idNbt("minecraft:furnace") + item + "2,64,2," + idNbt("minecraft:sign")
                : "17,64,1," + idNbt("minecraft:chest");
            default -> "";
        };
        try {
            assertEquals(3, world.getTileEntityCount());
            assertEquals(2, world.getTickableTileEntityCount());
        } finally { HostBridge.transport = null; }
    }

    @Test void poiSearchScansLoadedChunksNearestFirst() {
        var world = player.getWorld();
        world.getBlockAt(3, 64, 3).setType(org.bukkit.Material.BELL);
        world.getBlockAt(9, 64, 9).setType(org.bukkit.Material.BELL);
        var meeting = new FerritePoiType(org.bukkit.NamespacedKey.minecraft("meeting"));
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getLoadedChunkCoordinates()") ? "0,0" : "";
        try {
            var origin = new org.bukkit.Location(world, 4, 64, 4);
            var nearest = world.locateNearestPoi(origin, meeting, 16);
            assertEquals(3, nearest.getBlockX());
            var all = world.locateAllPoiInRange(origin, type -> type.getKey().getKey().equals("meeting"), 16);
            assertEquals(2, all.size());
            assertEquals(9, all.get(1).location().getBlockX());
            assertTrue(world.locateAllPoiInRange(origin, type -> type.getKey().getKey().equals("meeting"), 2).stream().allMatch(hit -> hit.location().getBlockX() == 3));
            assertNull(world.locateNearestPoi(new org.bukkit.Location(world, 100, 64, 100), meeting, 4));
            assertThrows(UnsupportedOperationException.class, () -> world.locateNearestPoi(origin, meeting, 16, io.papermc.paper.entity.poi.PoiType.Occupancy.HAS_SPACE));
        } finally { HostBridge.transport = null; }
    }

    @Test void styledMapCaptionsTravelAsComponentNbt() throws Exception {
        var styled = net.kyori.adventure.text.Component.text("Ranger", net.kyori.adventure.text.format.NamedTextColor.RED, net.kyori.adventure.text.format.TextDecoration.BOLD)
            .append(net.kyori.adventure.text.Component.text(" II"));
        var bytes = FerriteMapView.componentNbt(styled);
        var in = new java.io.DataInputStream(new java.io.ByteArrayInputStream(bytes));
        assertEquals(10, in.readByte());
        @SuppressWarnings("unchecked") var root = (java.util.Map<String, Object>) StructureNbt.payloadForTest(in, 10);
        assertEquals("Ranger", root.get("text"));
        assertEquals("red", root.get("color"));
        assertEquals((byte) 1, root.get("bold"));
        assertEquals("[{text= II}]", String.valueOf(root.get("extra")));
        var view = new FerriteMapView(player.getWorld());
        view.addRenderer(new org.bukkit.map.MapRenderer() {
            @Override public void render(org.bukkit.map.MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player viewer) {
                canvas.getCursors().addCursor(new org.bukkit.map.MapCursor((byte) 1, (byte) 2, (byte) 3, org.bukkit.map.MapCursor.Type.PLAYER, true, styled));
            }
        });
        var fields = view.render(player).decorations().split(String.valueOf((char) 30), -1);
        assertEquals("2", fields[4]);
        assertArrayEquals(bytes, java.util.Base64.getDecoder().decode(fields[5]));
    }
}
