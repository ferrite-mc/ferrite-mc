package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Effect;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.EquipmentSlot;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Projectile launches, effect data and the equipment slot set on a player. */
class PlayerBreadthTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000e4";
    private static final String PLAYER_ID = "00000000-0000-0000-0000-0000000000ac";
    private static org.bukkit.entity.Player player;
    private final List<String[]> calls = new ArrayList<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "breadth_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer(PLAYER_ID, "Breadth", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, PLAYER_ID, "Breadth");
    }

    @AfterAll static void stop() {
        // The player must go first: a player left in an unregistered world breaks World#playEffect for later classes.
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        server.unregisterPlayer(player.getUniqueId());
        var world = org.bukkit.Bukkit.getWorld("breadth_world");
        if (world != null) server.unregisterWorld(world.getUID());
    }

    @BeforeEach void goHome() {
        player.teleport(new org.bukkit.Location(org.bukkit.Bukkit.getWorld("breadth_world"), 0, 65, 0, 0f, 0f));
        calls.clear();
        HostBridge.transport = (owner, subject, method, args) -> {
            var parts = new ArrayList<String>();
            parts.add(owner);
            parts.add(method);
            for (var part : String.valueOf(args).split(java.util.regex.Pattern.quote(String.valueOf(HostBridge.ARG)), -1)) parts.add(part);
            calls.add(parts.toArray(String[]::new));
            return "";
        };
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    private String[] only(String method) {
        var matching = calls.stream().filter(call -> call[1].equals(method)).toList();
        assertEquals(1, matching.size(), calls.stream().map(call -> String.join("|", call)).toList().toString());
        return matching.get(0);
    }

    // ---- canCriticalAttack / getSentChunkKeys -------------------------------------------------------------------

    private void answer(java.util.Map<String, String> replies) {
        HostBridge.transport = (owner, subject, method, args) -> replies.getOrDefault(method, "");
    }

    @Test void aCriticalAttackNeedsAChargedFallingSwingAgainstALivingTarget() {
        // The player is its own living target; fall distance and water are tracked in Java, the rest comes from the host.
        var target = player;
        player.getWorld().getBlockAt(0, 65, 0).setType(org.bukkit.Material.AIR);
        var falling = new java.util.HashMap<String, String>(java.util.Map.of(
            "getAttackCooldown()", "1.0", "isOnGround()", "false", "isClimbing()", "false",
            "hasPotionEffect(PotionEffectType)", "false", "isInsideVehicle()", "false", "isSprinting()", "false",
            "addPotionEffect(PotionEffect)", "true"));
        answer(falling);
        player.setFallDistance(2.5f);
        assertTrue(player.canCriticalAttack(target));
        for (var spoiler : java.util.Map.of("getAttackCooldown()", "0.5", "isOnGround()", "true",
                "isClimbing()", "true",
                "isInsideVehicle()", "true", "isSprinting()", "true").entrySet()) {
            var spoiled = new java.util.HashMap<>(falling);
            spoiled.put(spoiler.getKey(), spoiler.getValue());
            answer(spoiled);
            assertFalse(player.canCriticalAttack(target), spoiler.getKey());
        }
        answer(falling);
        player.addPotionEffect(new org.bukkit.potion.PotionEffect(org.bukkit.potion.PotionEffectType.BLINDNESS, 100, 0));
        assertFalse(player.canCriticalAttack(target), "blind");
        player.removePotionEffect(org.bukkit.potion.PotionEffectType.BLINDNESS);
        assertTrue(player.canCriticalAttack(target), "blindness removed");
        player.setFallDistance(0f);
        assertFalse(player.canCriticalAttack(target), "no fall distance");
    }

    @Test void sentChunkKeysArePackedChunkCoordinates() {
        answer(java.util.Map.of("getSentChunkCoordinates()", "3,-4" + HostBridge.ITEM + "0,1"));
        assertEquals(java.util.Set.of(org.bukkit.Chunk.getChunkKey(3, -4), org.bukkit.Chunk.getChunkKey(0, 1)), player.getSentChunkKeys());
    }

    // ---- launchProjectile ------------------------------------------------------------------------------------

    private void assertLaunch(Class<? extends org.bukkit.entity.Projectile> requested, EntityType expected, double power) {
        calls.clear();
        var handle = player.launchProjectile(requested);
        assertInstanceOf(requested, handle);
        assertEquals(expected, handle.getType());
        // launchThrown(Location, EntityType, dx, dy, dz, owner, entity): the player looks along +z at pitch 0
        var call = only("launchThrown(Location,EntityType,double,double,double,String,String)");
        assertEquals(expected.name(), call[3].toUpperCase(java.util.Locale.ROOT), String.join("|", call));
        assertEquals(0.0, Double.parseDouble(call[4]), 1e-9);
        assertEquals(0.0, Double.parseDouble(call[5]), 1e-9);
        assertEquals(power, Double.parseDouble(call[6]), 1e-9);
        assertEquals(PLAYER_ID, call[7]);
        assertEquals(handle.getUniqueId().toString(), call[8]);
    }

    @Test void throwablesLaunchAtTheirVanillaItemPower() {
        assertLaunch(org.bukkit.entity.Snowball.class, EntityType.SNOWBALL, 1.5);
        assertLaunch(org.bukkit.entity.Egg.class, EntityType.EGG, 1.5);
        assertLaunch(org.bukkit.entity.EnderPearl.class, EntityType.ENDER_PEARL, 1.5);
        assertLaunch(org.bukkit.entity.ThrownExpBottle.class, EntityType.EXPERIENCE_BOTTLE, 0.7);
        assertLaunch(org.bukkit.entity.ThrownPotion.class, EntityType.SPLASH_POTION, 0.5);
        assertLaunch(org.bukkit.entity.SplashPotion.class, EntityType.SPLASH_POTION, 0.5);
        assertLaunch(org.bukkit.entity.LingeringPotion.class, EntityType.LINGERING_POTION, 0.5);
        assertLaunch(org.bukkit.entity.WindCharge.class, EntityType.WIND_CHARGE, 1.5);
    }

    @Test void anExplicitVelocityReplacesTheDefaultPower() {
        var handle = player.launchProjectile(org.bukkit.entity.ThrownExpBottle.class, new org.bukkit.util.Vector(1, 2, 3));
        assertEquals(EntityType.EXPERIENCE_BOTTLE, handle.getType());
        var call = only("launchThrown(Location,EntityType,double,double,double,String,String)");
        assertEquals(1.0, Double.parseDouble(call[4]), 1e-9);
        assertEquals(2.0, Double.parseDouble(call[5]), 1e-9);
        assertEquals(3.0, Double.parseDouble(call[6]), 1e-9);
    }

    @Test void arrowsStillUseTheArrowCall() {
        var arrow = player.launchProjectile(org.bukkit.entity.Arrow.class);
        assertEquals(EntityType.ARROW, arrow.getType());
        var call = only("spawnArrow(Location,double,double,double,String)");
        assertEquals(3.0, Double.parseDouble(call[5]), 1e-9);
    }

    @Test void aTridentLaunchesAtItsVanillaPowerOwnedByThePlayer() {
        assertLaunch(org.bukkit.entity.Trident.class, EntityType.TRIDENT, 2.5);
    }

    @Test void aTridentWithAnExplicitVelocityKeepsEveryComponent() {
        var handle = player.launchProjectile(org.bukkit.entity.Trident.class, new org.bukkit.util.Vector(0.25, -1, 2));
        assertEquals(EntityType.TRIDENT, handle.getType());
        var call = only("launchThrown(Location,EntityType,double,double,double,String,String)");
        assertEquals("TRIDENT", call[3].toUpperCase(java.util.Locale.ROOT));
        assertEquals(0.25, Double.parseDouble(call[4]), 1e-9);
        assertEquals(-1.0, Double.parseDouble(call[5]), 1e-9);
        assertEquals(2.0, Double.parseDouble(call[6]), 1e-9);
        assertEquals(PLAYER_ID, call[7]);
    }

    @Test void potionContentsCannotBeDroppedSilentlyByACallback() {
        var error = assertThrows(UnsupportedOperationException.class, () -> player.launchProjectile(
                org.bukkit.entity.SplashPotion.class, null,
                potion -> potion.setItem(new org.bukkit.inventory.ItemStack(org.bukkit.Material.SPLASH_POTION))));
        assertTrue(error.getMessage().contains("setItem"), error.getMessage());
        assertTrue(calls.isEmpty(), "a rejected callback launches nothing");
    }

    // ---- playEffect ------------------------------------------------------------------------------------------

    private int dataSent(Effect effect, Object data) {
        calls.clear();
        player.playEffect(player.getLocation(), effect, data);
        var call = only("playLevelEvent(Location,int,int)");
        assertEquals(Integer.toString(effect.getId()), call[3], effect.name());
        return Integer.parseInt(call[4]);
    }

    @Test void booleanEffectsUseTheEncodingVanillaSends() {
        assertEquals(1, dataSent(Effect.COMPOSTER_FILL_ATTEMPT, true));
        assertEquals(0, dataSent(Effect.COMPOSTER_FILL_ATTEMPT, false));
        assertEquals(1, dataSent(Effect.VAULT_ACTIVATE, true));
        assertEquals(0, dataSent(Effect.VAULT_DEACTIVATE, false));
        assertEquals(1, dataSent(Effect.ENDER_DRAGON_BREATH, true));
        assertEquals(-1, dataSent(Effect.ENDER_DRAGON_BREATH, false));
    }

    @Test void colourAxisFaceAndIntegerEffects() {
        // PotionContents#getColor for an experience bottle is 0xFF385DC6, which vanilla sends as -13083194
        assertEquals(-13083194, dataSent(Effect.POTION_BREAK, org.bukkit.Color.fromRGB(0x385DC6)));
        assertEquals(0xFF123456, dataSent(Effect.INSTANT_POTION_BREAK, org.bukkit.Color.fromRGB(0x123456)));
        assertEquals(0, dataSent(Effect.ELECTRIC_SPARK, org.bukkit.Axis.X));
        assertEquals(2, dataSent(Effect.ELECTRIC_SPARK, org.bukkit.Axis.Z));
        assertEquals(0, dataSent(Effect.WHITE_SMOKE_SHOOT, org.bukkit.block.BlockFace.DOWN));
        assertEquals(1, dataSent(Effect.WHITE_SMOKE_SHOOT, org.bukkit.block.BlockFace.UP));
        assertEquals(2, dataSent(Effect.WHITE_SMOKE_SHOOT, org.bukkit.block.BlockFace.NORTH));
        assertEquals(5, dataSent(Effect.WHITE_SMOKE_SHOOT, org.bukkit.block.BlockFace.EAST));
        assertThrows(IllegalArgumentException.class, () -> player.playEffect(player.getLocation(), Effect.WHITE_SMOKE_SHOOT, org.bukkit.block.BlockFace.SELF));
        assertEquals(15, dataSent(Effect.BONE_MEAL_USE, 15));
        assertEquals(750, dataSent(Effect.SMASH_ATTACK, Integer.valueOf(750)));
        assertEquals(0, dataSent(Effect.BONE_MEAL_USE, null));
    }

    @Test void blockDataEffectsSendTheGlobalBlockStateId() {
        var stone = org.bukkit.Material.STONE.createBlockData();
        assertEquals(1, dataSent(Effect.STEP_SOUND, stone));
        assertEquals(1, dataSent(Effect.DESTROY_BLOCK, stone));
        assertEquals(1, dataSent(Effect.BRUSH_BLOCK_COMPLETE, stone));
        // chest[type=left,waterlogged=true]: the report lists it as state 5545
        assertEquals(5545, dataSent(Effect.DESTROY_BLOCK_WITH_SOUND, org.bukkit.Bukkit.createBlockData("minecraft:chest[type=left,waterlogged=true]")));
    }

    @Test void unverifiedEncodingsAndWrongTypesFailWithTheReason() {
        var song = assertThrows(UnsupportedOperationException.class,
                () -> player.playEffect(player.getLocation(), Effect.RECORD_PLAY, org.bukkit.Registry.JUKEBOX_SONG.get(org.bukkit.NamespacedKey.minecraft("13"))));
        assertTrue(song.getMessage().contains("registry id"), song.getMessage());
        var smoke = assertThrows(UnsupportedOperationException.class,
                () -> player.playEffect(player.getLocation(), Effect.SMOKE_SHOOT, org.bukkit.block.BlockFace.NORTH));
        assertTrue(smoke.getMessage().contains("SMOKE_SHOOT"), smoke.getMessage());
        assertThrows(UnsupportedOperationException.class, () -> player.playEffect(player.getLocation(), Effect.EXTINGUISH, true));
        assertThrows(IllegalArgumentException.class, () -> player.playEffect(player.getLocation(), Effect.COMPOSTER_FILL_ATTEMPT, org.bukkit.Color.RED));
        assertThrows(IllegalArgumentException.class, () -> player.playEffect(player.getLocation(), Effect.DOOR_TOGGLE, "open"));
        assertTrue(calls.isEmpty(), "refused effects send nothing");
    }

    @Test void everyEffectThatTakesDataEitherSendsOrExplainsWhyNot() {
        var samples = java.util.Map.<Class<?>, Object>of(
                Boolean.class, true,
                Integer.class, 1,
                org.bukkit.block.BlockFace.class, org.bukkit.block.BlockFace.NORTH,
                org.bukkit.block.data.BlockData.class, org.bukkit.Material.STONE.createBlockData(),
                org.bukkit.Color.class, org.bukkit.Color.WHITE,
                org.bukkit.Axis.class, org.bukkit.Axis.Y,
                org.bukkit.JukeboxSong.class, org.bukkit.Registry.JUKEBOX_SONG.get(org.bukkit.NamespacedKey.minecraft("13")));
        int sent = 0, refused = 0;
        for (var effect : Effect.values()) {
            if (effect.getData() == null) continue;
            var sample = samples.get(effect.getData());
            assertNotNull(sample, effect + " takes " + effect.getData());
            try {
                FerriteEffectData.levelEventData(effect, sample);
                sent++;
            } catch (UnsupportedOperationException reason) {
                assertTrue(reason.getMessage().contains(effect.name()), reason.getMessage());
                refused++;
            }
        }
        assertEquals(32, sent + refused, "all data-carrying effects are covered");
        // refused: RECORD_PLAY, SMOKE_SHOOT, DESTROY_PROGRESS(_WITH_SOUND), EXTINGUISH and the four trial spawner flags
        assertEquals(9, refused);
        assertEquals(23, sent);
    }

    // ---- equipment slots ---------------------------------------------------------------------------------------

    @Test void everySlotAPlayerHasMapsToItsInventoryIndex() {
        var inventory = player.getInventory();
        var expected = java.util.Map.of(
                EquipmentSlot.HAND, 0, EquipmentSlot.OFF_HAND, 40,
                EquipmentSlot.FEET, 36, EquipmentSlot.LEGS, 37, EquipmentSlot.CHEST, 38, EquipmentSlot.HEAD, 39);
        int amount = 1;
        for (var entry : expected.entrySet()) {
            var stack = new org.bukkit.inventory.ItemStack(org.bukkit.Material.STICK, amount++);
            inventory.setItem(entry.getKey(), stack);
            assertEquals(stack, inventory.getItem(entry.getKey()), entry.getKey().name());
            assertEquals(stack, inventory.getItem(entry.getValue()), entry.getKey() + " is index " + entry.getValue());
            assertEquals(stack, player.getEquipment().getItem(entry.getKey()), entry.getKey().name());
        }
        assertEquals(expected.size(), EquipmentSlot.values().length - 2);
    }

    @Test void bodyAndSaddleAreRefusedBecauseAPlayerCannotUseThem() {
        var inventory = player.getInventory();
        for (var slot : new EquipmentSlot[]{EquipmentSlot.BODY, EquipmentSlot.SADDLE}) {
            var read = assertThrows(UnsupportedOperationException.class, () -> inventory.getItem(slot));
            assertTrue(read.getMessage().contains(slot.name()) && read.getMessage().contains("canUseSlot"), read.getMessage());
            assertThrows(UnsupportedOperationException.class, () -> inventory.setItem(slot, new org.bukkit.inventory.ItemStack(org.bukkit.Material.STICK)));
        }
    }
}
