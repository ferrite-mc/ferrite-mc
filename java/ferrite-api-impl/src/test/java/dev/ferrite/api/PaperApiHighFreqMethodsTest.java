package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.destroystokyo.paper.ClientOption;
import com.destroystokyo.paper.SkinParts;
import com.destroystokyo.paper.block.TargetBlockInfo;
import com.destroystokyo.paper.profile.ProfileProperty;
import dev.ferrite.api.entity.FerriteEntitySnapshot;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import io.papermc.paper.entity.LookAnchor;
import dev.ferrite.api.world.WorldSpec;
import io.papermc.paper.InternalAPIBridge;
import io.papermc.paper.ban.BanListType;
import io.papermc.paper.math.Angle;
import io.papermc.paper.math.Position;
import io.papermc.paper.raytracing.RayTraceTarget;
import io.papermc.paper.world.MoonPhase;
import io.papermc.paper.world.chunk.ChunkGenerationState;
import java.net.InetAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.UUID;
import net.kyori.adventure.bossbar.BossBar;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.event.HoverEvent;
import net.kyori.adventure.util.TriState;
import net.md_5.bungee.api.chat.TextComponent;
import org.bukkit.BanList;
import org.bukkit.Bukkit;
import org.bukkit.Chunk;
import org.bukkit.Color;
import org.bukkit.Difficulty;
import org.bukkit.EntityEffect;
import org.bukkit.FluidCollisionMode;
import org.bukkit.GameMode;
import org.bukkit.HeightMap;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;
import org.bukkit.SoundCategory;
import org.bukkit.Statistic;
import org.bukkit.Tag;
import org.bukkit.WeatherType;
import org.bukkit.World;
import org.bukkit.WorldBorder;
import org.bukkit.WorldType;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.block.PistonMoveReaction;
import org.bukkit.damage.DamageSource;
import org.bukkit.damage.DamageType;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityCategory;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Player;
import org.bukkit.entity.Pose;
import org.bukkit.entity.SpawnCategory;
import org.bukkit.event.entity.EntityRegainHealthEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemFactory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.MainHand;
import org.bukkit.inventory.ShapedRecipe;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.metadata.FixedMetadataValue;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.Team;
import org.bukkit.util.BoundingBox;
import org.bukkit.util.RayTraceResult;
import org.bukkit.util.Vector;
import org.junit.jupiter.api.Test;

@SuppressWarnings({"deprecation", "removal"})
class PaperApiHighFreqMethodsTest {

    @Test
    void playerGeometryEnvironmentFlagsCombatAndXpWork() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("PlayerHighFreq");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer alice = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Alice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5)));
            FerritePlayer bob = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Bob", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 3.5, 65.0, 0.5)));

            assertTrue(alice.getEntityId() > 0);
            assertNotEquals(alice.getEntityId(), bob.getEntityId());
            assertEquals(EntityType.PLAYER, alice.getType());
            assertEquals(SpawnCategory.MISC, alice.getSpawnCategory());
            assertEquals(EntityCategory.NONE, alice.getCategory());
            assertTrue(alice.isInWorld());
            assertTrue(alice.isTicking());
            assertEquals(0.5, alice.getX(), 1e-6);
            assertEquals(65.0, alice.getY(), 1e-6);
            assertEquals(0.5, alice.getZ(), 1e-6);

            // Width, height, eyeHeight, bounding box & pose
            assertEquals(0.6, alice.getWidth(), 1e-6);
            assertEquals(1.8, alice.getHeight(), 1e-6);
            assertEquals(1.62, alice.getEyeHeight(), 1e-6);
            alice.setSneaking(true);
            assertEquals(1.5, alice.getHeight(), 1e-6);
            assertEquals(1.27, alice.getEyeHeight(), 1e-6);
            assertEquals(1.62, alice.getEyeHeight(true), 1e-6);
            assertEquals(Pose.SNEAKING, alice.getPose());
            alice.setSneaking(false);

            alice.setPose(Pose.SWIMMING, true);
            assertTrue(alice.hasFixedPose());
            assertEquals(Pose.SWIMMING, alice.getPose());
            alice.unsetFixedPose();
            assertFalse(alice.hasFixedPose());

            alice.setRotation(90.0f, 15.0f);
            assertEquals(90.0f, alice.getYaw(), 1e-6f);
            assertEquals(90.0f, alice.getBodyYaw(), 1e-6f);
            assertEquals(15.0f, alice.getPitch(), 1e-6f);
            assertEquals(BlockFace.WEST, alice.getFacing());
            alice.setRotation(Angle.relative(90.0f), Angle.absolute(-10.0f));
            assertEquals(180.0f, alice.getYaw(), 1e-6f);
            assertEquals(BlockFace.NORTH, alice.getFacing());
            alice.setBodyYaw(120.0f);
            assertEquals(120.0f, alice.getBodyYaw(), 1e-6f);

            alice.lookAt(bob, LookAnchor.EYES, LookAnchor.EYES);
            assertEquals(-90.0f, alice.getYaw(), 1.0f);

            assertEquals(List.of(bob), alice.getNearbyEntities(5.0, 5.0, 5.0));

            // Block environment flags
            world.getBlockAt(0, 64, 0).setType(Material.STONE);
            assertTrue(alice.isOnGround());
            assertTrue(alice.collidesAt(new Location(world, 0.5, 64.2, 0.5)));
            assertTrue(alice.wouldCollideUsing(new BoundingBox(0.0, 64.0, 0.0, 1.0, 65.0, 1.0)));

            world.getBlockAt(0, 65, 0).setType(Material.WATER);
            assertTrue(alice.isInWater());
            world.getBlockAt(0, 66, 0).setType(Material.WATER);
            assertTrue(alice.isUnderWater());
            world.getBlockAt(0, 65, 0).setType(Material.LADDER);
            assertTrue(alice.isClimbing());
            world.getBlockAt(0, 65, 0).setType(Material.POWDER_SNOW);
            assertTrue(alice.isInPowderedSnow());
            world.getBlockAt(0, 65, 0).setType(Material.AIR);
            world.getBlockAt(0, 66, 0).setType(Material.AIR);

            // Entity state flags
            alice.setGliding(true);
            assertTrue(alice.isGliding());
            alice.setGliding(false);
            alice.setSwimming(true);
            assertTrue(alice.isSwimming());
            alice.setSwimming(false);
            alice.setRiptiding(true);
            assertTrue(alice.isRiptiding());
            alice.setJumping(true);
            assertTrue(alice.isJumping());
            alice.setInvisible(true);
            alice.setGlowing(true);
            alice.setSilent(true);
            alice.setInvulnerable(true);
            assertTrue(alice.isInvisible());
            assertTrue(alice.isGlowing());
            assertTrue(alice.isSilent());
            assertTrue(alice.isMarkedInvulnerable());
            assertTrue(alice.isInvulnerableTo(DamageSource.builder(DamageType.GENERIC).build()));
            alice.setInvulnerable(false);
            alice.setGravity(false);
            assertFalse(alice.hasGravity());
            assertEquals(0.0, alice.getGravity(), 1e-6);
            assertEquals(0.08, alice.getDefaultGravity(), 1e-6);
            assertEquals(0.02f, alice.getAirDrag(), 1e-6f);
            alice.setCollidable(false);
            assertFalse(alice.isCollidable());
            alice.getCollidableExemptions().add(bob.getUniqueId());
            assertTrue(alice.getCollidableExemptions().contains(bob.getUniqueId()));
            alice.setNoPhysics(true);
            assertTrue(alice.hasNoPhysics());
            alice.setPersistent(true);
            assertTrue(alice.isPersistent());
            alice.setCanPickupItems(false);
            assertFalse(alice.getCanPickupItems());
            alice.setAI(false);
            assertFalse(alice.hasAI());
            alice.setAffectsSpawning(false);
            assertFalse(alice.getAffectsSpawning());
            alice.setFallDistance(3.5f);
            assertEquals(3.5f, alice.getFallDistance(), 1e-6f);
            alice.resetFlyingTicks();
            assertEquals(0.0f, alice.getFallDistance(), 1e-6f);
            alice.setTicksLived(100);
            assertEquals(100, alice.getTicksLived());
            assertThrows(IllegalArgumentException.class, () -> alice.setTicksLived(0));
            alice.setFrictionState(TriState.TRUE);
            assertEquals(TriState.TRUE, alice.getFrictionState());
            alice.setFlyingFallDamage(TriState.TRUE);
            assertEquals(TriState.TRUE, alice.hasFlyingFallDamage());
            assertEquals(PistonMoveReaction.MOVE, alice.getPistonMoveReaction());

            // Combat, fire/freeze, health, regen, XP
            assertEquals(20, alice.getMaxFireTicks());
            alice.setVisualFire(true);
            assertTrue(alice.isVisualFire());
            assertEquals(TriState.TRUE, alice.getVisualFire());
            alice.setFreezeTicks(150);
            assertTrue(alice.isFrozen());
            assertEquals(140, alice.getMaxFreezeTicks());
            alice.lockFreezeTicks(true);
            assertTrue(alice.isFreezeTickingLocked());
            alice.setMaximumAir(400);
            assertEquals(400, alice.getMaximumAir());
            alice.setPortalCooldown(50);
            assertEquals(50, alice.getPortalCooldown());
            alice.setMaximumNoDamageTicks(30);
            alice.setNoDamageTicks(10);
            alice.setNoActionTicks(5);
            assertEquals(30, alice.getMaximumNoDamageTicks());
            assertEquals(10, alice.getNoDamageTicks());
            assertEquals(5, alice.getNoActionTicks());

            alice.setMaxHealth(30.0);
            assertEquals(30.0, alice.getMaxHealth(), 1e-6);
            alice.setHealth(10.0);
            alice.heal(5.0, EntityRegainHealthEvent.RegainReason.REGEN);
            assertEquals(15.0, alice.getHealth(), 1e-6);
            alice.setAbsorptionAmount(4.0);
            assertEquals(4.0, alice.getAbsorptionAmount(), 1e-6);
            alice.setKiller(bob);
            assertSame(bob, alice.getKiller());
            alice.setHurtDirection(45.0f);
            assertEquals(45.0f, alice.getHurtDirection(), 1e-6f);
            alice.resetMaxHealth();
            assertEquals(20.0, alice.getMaxHealth(), 1e-6);
            alice.setHealthScale(40.0);
            assertTrue(alice.isHealthScaled());
            assertEquals(40.0, alice.getHealthScale(), 1e-6);

            alice.setSaturatedRegenRate(12);
            alice.setUnsaturatedRegenRate(90);
            alice.setStarvationRate(60);
            assertEquals(12, alice.getSaturatedRegenRate());
            assertEquals(90, alice.getUnsaturatedRegenRate());
            assertEquals(60, alice.getStarvationRate());

            alice.setExperienceLevelAndProgress(55);
            assertEquals(5, alice.getLevel());
            assertEquals(55, alice.calculateTotalExperiencePoints());
            assertEquals(17, alice.getExpToLevel());
            assertEquals(17, alice.getExperiencePointsNeededForNextLevel());

            assertEquals(5.0f, alice.getCooldownPeriod(), 0.01f);
            alice.resetCooldown();
            assertTrue(alice.getCooledAttackStrength(0.0f) <= 1.0f);
            assertFalse(alice.canBreatheUnderwater());
            alice.addPotionEffect(new PotionEffect(PotionEffectType.WATER_BREATHING, 100, 0));
            assertTrue(alice.canBreatheUnderwater());

            alice.setArrowsInBody(3, false);
            assertEquals(3, alice.getArrowsInBody());
            alice.setBeeStingersInBody(2);
            assertEquals(2, alice.getBeeStingersInBody());
            alice.increaseWardenWarningLevel();
            assertEquals(1, alice.getWardenWarningLevel());

            assertTrue(alice.canUseEquipmentSlot(EquipmentSlot.HAND));
            assertFalse(alice.canUseEquipmentSlot(EquipmentSlot.BODY));
            ItemStack sword = new ItemStack(Material.IRON_SWORD, 1);
            server.onMain(() -> alice.getInventory().setItemInMainHand(sword));
            assertEquals(Material.IRON_SWORD, alice.getWeapon().getType());
            server.onMain(() -> alice.damageItemStack(EquipmentSlot.HAND, 5));
            assertEquals(5, ((Damageable) alice.getInventory().getItemInMainHand().getItemMeta()).getDamage());

            // Unified online/offline statistics
            alice.incrementStatistic(Statistic.JUMP, 4);
            assertEquals(4, alice.getStatistic(Statistic.JUMP));
            assertEquals(4, Bukkit.getOfflinePlayer(alice.getUniqueId()).getStatistic(Statistic.JUMP));
        }
    }

    @Test
    void playerClientOptionsBorderTimeActiveItemRecipesAndRayTraceWork() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("PlayerUiPlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer alice = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Alice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5)));
            FerritePlayer bob = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Bob", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65.0, 4.5)));

            // UI / Client options / Header & Footer
            alice.sendPlayerListHeaderAndFooter(Component.text("Header"), Component.text("Footer"));
            assertEquals(Component.text("Header"), alice.playerListHeader());
            assertEquals(Component.text("Footer"), alice.playerListFooter());
            alice.setPlayerListOrder(7);
            assertEquals(7, alice.getPlayerListOrder());
            alice.customName(Component.text("CustomAlice"));
            assertEquals("CustomAlice", alice.getCustomName());
            alice.setCustomNameVisible(true);
            assertTrue(alice.isCustomNameVisible());

            Scoreboard board = Bukkit.getScoreboardManager().getNewScoreboard();
            Team team = board.registerNewTeam("gold");
            team.prefix(Component.text("[G] "));
            team.addPlayer(alice);
            alice.setScoreboard(board);
            assertTrue(alice.teamDisplayName().toString().contains("[G] "));

            BossBar advBar = BossBar.bossBar(Component.text("Quest"), 0.5f, BossBar.Color.GREEN, BossBar.Overlay.PROGRESS);
            alice.showBossBar(advBar);
            assertTrue(alice.activeBossBars().iterator().hasNext());
            alice.hideBossBar(advBar);
            assertFalse(alice.activeBossBars().iterator().hasNext());

            assertEquals(777, alice.getProtocolVersion());
            assertEquals(MainHand.RIGHT, alice.getClientOption(ClientOption.MAIN_HAND));
            assertEquals(10, alice.getClientOption(ClientOption.VIEW_DISTANCE));
            assertNotNull(alice.getClientOption(ClientOption.SKIN_PARTS));
            alice.setViewDistance(12);
            alice.setSimulationDistance(8);
            alice.setSendViewDistance(6);
            assertEquals(12, alice.getClientViewDistance());
            assertEquals(8, alice.getSimulationDistance());
            assertEquals(6, alice.getSendViewDistance());

            // Custom player worldBorder, time, weather
            WorldBorder personal = Bukkit.createWorldBorder();
            personal.setSize(100.0);
            alice.setWorldBorder(personal);
            assertSame(personal, alice.getWorldBorder());

            alice.setGameMode(GameMode.CREATIVE);
            assertEquals(GameMode.SURVIVAL, alice.getPreviousGameMode());
            alice.setPlayerTime(18000L, false);
            assertFalse(alice.isPlayerTimeRelative());
            assertEquals(18000L, alice.getPlayerTime());
            alice.resetPlayerTime();
            alice.setPlayerWeather(WeatherType.DOWNFALL);
            assertEquals(WeatherType.DOWNFALL, alice.getPlayerWeather());
            alice.resetPlayerWeather();
            assertNull(alice.getPlayerWeather());

            // Active item, cursor, recipes, cookies, allied rules
            server.onMain(() -> {
                alice.getInventory().setItemInOffHand(new ItemStack(Material.SHIELD, 1));
                alice.startUsingItem(EquipmentSlot.OFF_HAND);
                assertTrue(alice.hasActiveItem());
                assertTrue(alice.isHandRaised());
                assertTrue(alice.isBlocking());
                assertEquals(EquipmentSlot.OFF_HAND, alice.getActiveItemHand());
                alice.clearActiveItem();
                assertFalse(alice.hasActiveItem());

                alice.setItemOnCursor(new ItemStack(Material.EMERALD, 3));
                assertEquals(Material.EMERALD, alice.getItemOnCursor().getType());

                var giveRes = alice.give(List.of(new ItemStack(Material.DIAMOND, 4)), false);
                assertTrue(giveRes.leftovers().isEmpty());
                assertTrue(alice.getInventory().contains(Material.DIAMOND, 4));
                assertTrue(alice.dropItem(false));
            });

            NamespacedKey recipeKey = new NamespacedKey(plugin, "custom_bread");
            assertEquals(1, alice.discoverRecipes(List.of(recipeKey)));
            assertTrue(alice.hasDiscoveredRecipe(recipeKey));
            assertEquals(Set.of(recipeKey), alice.getDiscoveredRecipes());
            assertEquals(1, alice.undiscoverRecipes(List.of(recipeKey)));

            NamespacedKey cookieKey = new NamespacedKey(plugin, "session");
            alice.storeCookie(cookieKey, "ok".getBytes(StandardCharsets.UTF_8));
            assertEquals("ok", new String(alice.retrieveCookie(cookieKey).get(), StandardCharsets.UTF_8));

            Key allyKey = Key.key("ferrite", "friend");
            alice.addAdditionalAlliedRule(allyKey, e -> e.equals(bob));
            assertTrue(alice.isAlliedTo(bob));
            alice.removeAdditionalAlliedRule(allyKey);
            assertFalse(alice.isAlliedTo(bob));

            // Ray tracing: player looking +Z towards stone block at z=3 and bob at z=4.5
            alice.setRotation(0.0f, 0.0f);
            world.getBlockAt(0, 66, 3).setType(Material.STONE);
            assertEquals(Material.STONE, alice.getTargetBlockExact(10, FluidCollisionMode.NEVER).getType());
            assertEquals(
                    Material.STONE,
                    alice.getTargetBlock(10, TargetBlockInfo.FluidMode.NEVER).getType());
            assertNotNull(alice.getTargetBlockInfo(10, TargetBlockInfo.FluidMode.NEVER));
            assertFalse(alice.hasLineOfSight(bob));
            assertSame(bob, alice.getTargetEntity(10, true));
            assertNotNull(alice.getTargetEntityInfo(10, true));
            alice.playEffect(EntityEffect.HURT);
        }
    }

    @Test
    void worldCountsDimensionFlagsTicketsRayTraceAndBlockHelpersWork() throws Exception {
        try (TestServer server = new TestServer(b -> b
                .world(WorldSpec.of("world", World.Environment.NORMAL))
                .world(WorldSpec.of("world_nether", World.Environment.NETHER)))) {
            TestPlugin plugin = server.attachPlugin("WorldBlockPlugin");
            server.enable(plugin);
            FerriteWorld overworld = server.runtime().world("world");
            FerriteWorld nether = server.runtime().world("world_nether");

            assertTrue(overworld.isNatural());
            assertTrue(overworld.hasSkyLight());
            assertFalse(overworld.isUltraWarm());
            assertEquals(63, overworld.getSeaLevel());
            assertEquals(1.0, overworld.getCoordinateScale(), 1e-6);
            assertTrue(nether.hasCeiling());
            assertTrue(nether.isUltraWarm());
            assertEquals(32, nether.getSeaLevel());
            assertEquals(8.0, nether.getCoordinateScale(), 1e-6);
            assertEquals(WorldType.NORMAL, overworld.getWorldType());
            assertTrue(overworld.canGenerateStructures());

            assertTrue(overworld.setSpawnLocation(new Location(overworld, 12.0, 70.0, -8.0, 90.0f, 0.0f)));
            assertEquals(12.0, overworld.getSpawnLocation().getX(), 1e-6);
            assertTrue(overworld.setSpawnLocation(5, 64, 5, 180.0f));
            assertEquals(5.0, overworld.getSpawnLocation().getX(), 1e-6);

            overworld.setTime(6000L);
            assertTrue(overworld.isDayTime());
            assertEquals(MoonPhase.FULL_MOON, overworld.getMoonPhase());
            overworld.setClearWeatherDuration(200);
            assertTrue(overworld.isClearWeather());
            assertEquals(200, overworld.getClearWeatherDuration());

            overworld.setViewDistance(12);
            overworld.setSimulationDistance(9);
            overworld.setSendViewDistance(7);
            assertEquals(12, overworld.getViewDistance());
            assertEquals(9, overworld.getSimulationDistance());
            assertEquals(7, overworld.getSendViewDistance());
            overworld.setSpawnLimit(SpawnCategory.MONSTER, 50);
            assertEquals(50, overworld.getSpawnLimit(SpawnCategory.MONSTER));
            overworld.setTicksPerSpawns(SpawnCategory.ANIMAL, 200);
            assertEquals(200L, overworld.getTicksPerSpawns(SpawnCategory.ANIMAL));
            overworld.setVoidDamageEnabled(true);
            overworld.setVoidDamageAmount(6.0f);
            assertTrue(overworld.isVoidDamageEnabled());
            assertEquals(6.0f, overworld.getVoidDamageAmount(), 1e-6f);

            assertTrue(overworld.isGameRule("doDaylightCycle"));
            overworld.setBlockData(4, 70, 4, Bukkit.createBlockData(Material.DIAMOND_ORE));
            assertEquals(Material.DIAMOND_ORE, overworld.getType(4, 70, 4));
            assertEquals(70, overworld.getHighestBlockYAt(4, 4, HeightMap.MOTION_BLOCKING));

            // Block helpers & cross-handle metadata
            Block block = overworld.getBlockAt(4, 70, 4);
            assertTrue(block.isPreferredTool(new ItemStack(Material.DIAMOND_PICKAXE)));
            assertTrue(block.isValidTool(new ItemStack(Material.IRON_PICKAXE)));
            assertTrue(block.getBreakSpeed(server.onMain(() -> server.runtime().createPlayer(
                    "Miner", UUID.randomUUID(), false, GameMode.SURVIVAL, overworld, 0, 64, 0))) > 0.0f);
            assertEquals(PistonMoveReaction.MOVE, block.getPistonMoveReaction());
            assertEquals("block.minecraft.diamond_ore", block.translationKey());
            assertTrue(block.isSuffocating());
            assertTrue(block.canPlace(Bukkit.createBlockData(Material.STONE)));
            assertNotNull(block.getBlockSoundGroup().getBreakSound());
            assertNotNull(block.getSoundGroup().getStepSound());
            assertEquals(1, block.getCollisionShape().getBoundingBoxes().size());

            block.setMetadata("protected", new FixedMetadataValue(plugin, true));
            assertTrue(overworld.getBlockAt(4, 70, 4).hasMetadata("protected"));
            assertTrue(overworld.getBlockAt(4, 70, 4).getMetadata("protected").get(0).asBoolean());
            overworld.getBlockAt(4, 70, 4).removeMetadata("protected", plugin);
            assertFalse(block.hasMetadata("protected"));

            // World metadata, PDC, chunk tickets, raytrace
            overworld.setMetadata("region_ver", new FixedMetadataValue(plugin, 2));
            assertTrue(overworld.hasMetadata("region_ver"));
            overworld.getPersistentDataContainer().set(
                    new NamespacedKey(plugin, "season"), PersistentDataType.STRING, "spring");
            assertEquals("spring", overworld.getPersistentDataContainer().get(
                    new NamespacedKey(plugin, "season"), PersistentDataType.STRING));

            Chunk chunk = overworld.getChunkAt(0, 0);
            chunk.setForceLoaded(true);
            assertTrue(overworld.getForceLoadedChunks().contains(chunk));
            assertTrue(overworld.addPluginChunkTicket(0, 0, plugin));
            assertTrue(overworld.getPluginChunkTickets(0, 0).contains(plugin));
            assertFalse(overworld.getPluginChunkTickets().isEmpty());
            assertEquals(ChunkGenerationState.GENERATED, overworld.getChunkGenerationState(0, 0));
            assertEquals(1, overworld.getIntersectingChunks(new BoundingBox(1, 64, 1, 8, 66, 8)).size());

            RayTraceResult worldRay = overworld.rayTrace(cfg -> cfg
                    .start(new Location(overworld, 4.5, 75.0, 4.5))
                    .direction(new Vector(0, -1, 0))
                    .maxDistance(10.0)
                    .targets(RayTraceTarget.BLOCK));
            assertNotNull(worldRay);
            assertEquals(block, worldRay.getHitBlock());
            assertTrue(overworld.hasCollisionsIn(new BoundingBox(4.0, 70.0, 4.0, 5.0, 71.0, 5.0)));
            assertTrue(overworld.lineOfSightExists(
                    new Location(overworld, 0, 80, 0), new Location(overworld, 4, 80, 4)));

            assertTrue(block.breakNaturally(false, false));
            assertTrue(block.isEmpty());
        }
    }

    @Test
    void serverItemFactoryInventoryItemMetaAndInternalBridgeWork() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("ServerFactoryPlugin");
            server.enable(plugin);
            var srv = server.runtime().server();
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Hero", UUID.randomUUID(), true, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5)));

            // ItemFactory
            ItemFactory factory = Bukkit.getItemFactory();
            assertNotNull(factory);
            assertNull(factory.getItemMeta(Material.AIR));
            ItemMeta swordMeta = factory.getItemMeta(Material.DIAMOND_SWORD);
            assertNotNull(swordMeta);
            assertTrue(factory.equals(null, swordMeta));
            swordMeta.displayName(Component.text("Excalibur"));
            assertFalse(factory.equals(null, swordMeta));
            assertTrue(factory.isApplicable(swordMeta, Material.DIAMOND_SWORD));
            assertEquals(Color.fromRGB(0xA06540), factory.getDefaultLeatherColor());
            assertEquals(Material.OAK_LOG, factory.createItemStack("minecraft:oak_log").getType());
            assertEquals(Material.ZOMBIE_SPAWN_EGG, factory.getSpawnEgg(EntityType.ZOMBIE));
            ItemStack item = new ItemStack(Material.DIAMOND_SWORD, 1);
            item.setItemMeta(swordMeta);
            assertEquals(Component.text("Excalibur"), factory.displayName(item));
            assertEquals("Excalibur", factory.getI18NDisplayName(item));
            HoverEvent<HoverEvent.ShowItem> hover = factory.asHoverEvent(item, null);
            assertNotNull(hover);
            assertNotNull(factory.hoverContentOf(item));
            assertNotNull(factory.hoverContentOf(player, "Hero"));
            ItemStack enchanted = factory.enchantWithLevels(item, 30, true, new Random(42L));
            assertNotNull(enchanted);

            // Server config, TickManager, BanList, Profiles, Tags, Recipes, CommandSender
            assertEquals("update", srv.getUpdateFolder());
            assertEquals(29999984, srv.getMaxWorldSize());
            assertNotNull(srv.getPluginsFolder());
            assertNotNull(srv.getWorldContainer());
            assertNotNull(srv.getLevelDirectory());
            assertEquals(List.of("vanilla"), srv.getInitialEnabledPacks());
            assertEquals(List.of(player), srv.matchPlayer("her"));
            assertSame(player, srv.getEntity(player.getUniqueId()));
            assertSame(world, srv.getRespawnWorld());
            assertNotNull(srv.createWorldBorder());

            srv.setMotd("Welcome & Test");
            assertTrue(srv.getMotd().contains("Welcome"));
            assertFalse(srv.getShutdownMessage().isEmpty());
            assertFalse(srv.isStopping());

            List<Component> received = new ArrayList<>();
            var feedbackSender = srv.createCommandSender(received::add);
            feedbackSender.sendMessage(Component.text("ping"));
            assertEquals(List.of(Component.text("ping")), received);

            assertEquals(1, srv.broadcast(Component.text("op broadcast"), "some.op.perm"));
            srv.spigot().broadcast(new TextComponent("SpigotBroadcast"));

            var tickManager = srv.getServerTickManager();
            assertTrue(tickManager.isRunningNormally());
            tickManager.setTickRate(40.0f);
            assertEquals(40.0f, tickManager.getTickRate(), 1e-6f);
            tickManager.setFrozen(true);
            assertTrue(tickManager.isFrozen());
            assertTrue(tickManager.stepGameIfFrozen(5));
            assertTrue(tickManager.isStepping());

            var profile = srv.createProfile(UUID.randomUUID(), "SkinUser");
            profile.setProperty(new ProfileProperty("textures", "abc"));
            assertTrue(profile.hasProperty("textures"));
            var profileBans = srv.getBanList(BanListType.PROFILE);
            profileBans.addBan(profile, "Griefing", Duration.ofHours(1), "Admin");
            assertTrue(profileBans.isBanned(profile));
            assertFalse(srv.getBannedPlayers().isEmpty());
            profileBans.pardon(profile);
            assertFalse(profileBans.isBanned(profile));

            var ipBans = srv.getBanList(BanListType.IP);
            assertSame(ipBans, srv.getBanList(BanList.Type.IP));
            InetAddress addr = InetAddress.getByName("198.51.100.5");
            ipBans.addBan(addr, "Spam", (java.util.Date) null, "Console");
            assertTrue(ipBans.isBanned("198.51.100.5"));
            ipBans.pardon(addr);
            assertFalse(ipBans.isBanned(addr));

            NamespacedKey recipeKey = new NamespacedKey(plugin, "emerald_gem");
            ShapedRecipe recipe = new ShapedRecipe(recipeKey, new ItemStack(Material.EMERALD, 2))
                    .shape("D")
                    .setIngredient('D', Material.DIAMOND);
            assertTrue(srv.addRecipe(recipe));
            assertTrue(srv.hasRecipe(recipeKey));
            assertEquals(1, srv.getRecipesFor(new ItemStack(Material.EMERALD, 1)).size());
            assertTrue(srv.removeRecipe(recipeKey));

            Tag<Material> logs = srv.getTag(Tag.REGISTRY_BLOCKS, NamespacedKey.minecraft("logs"), Material.class);
            if (logs != null) {
                assertTrue(logs.isTagged(Material.OAK_LOG));
            }
            assertNotNull(srv.createBlockData("minecraft:stone[foo=bar]"));

            // PlayerInventory & ItemMeta
            var inv = player.getInventory();
            inv.setMaxStackSize(16);
            assertEquals(16, inv.getMaxStackSize());
            assertSame(player, inv.getHolder(false));
            assertTrue(inv.iterator().hasNext() || inv.getSize() == 41);
            assertEquals(41, inv.iterator(41).previousIndex() + 1);
            swordMeta.setVersion(1);
            assertTrue(swordMeta.getAsComponentString().contains("minecraft:display_name"));

            // InternalAPIBridge
            InternalAPIBridge apiBridge = java.util.ServiceLoader.load(InternalAPIBridge.class)
                    .findFirst()
                    .orElseThrow();
            SkinParts.Mutable allParts = apiBridge.allSkinParts();
            assertTrue(allParts.hasCapeEnabled());
            assertEquals(SpawnCategory.MONSTER, apiBridge.getSpawnCategory(EntityType.ZOMBIE));
            assertEquals("minecraft.custom:minecraft.jump", apiBridge.getStatisticCriteriaKey(Statistic.JUMP));
            assertNotNull(apiBridge.createPersistentDataKey(
                    Key.key("ferrite", "score"), PersistentDataType.INTEGER));
            assertEquals(Component.text("hello"), apiBridge.resolveWithContext(
                    Component.text("hello"), feedbackSender, player, false));
        }
    }
}
