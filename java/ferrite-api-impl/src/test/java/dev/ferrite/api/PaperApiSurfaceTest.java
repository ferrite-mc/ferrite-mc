package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerriteEntitySnapshot;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.PlayerState;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import io.papermc.paper.scoreboard.numbers.NumberFormat;
import java.time.Duration;
import java.util.List;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.TimeUnit;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.title.Title;
import net.kyori.adventure.title.TitlePart;
import net.md_5.bungee.api.chat.TextComponent;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.Color;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Particle;
import org.bukkit.Sound;
import org.bukkit.SoundCategory;
import org.bukkit.WorldBorder;
import org.bukkit.block.Block;
import org.bukkit.block.BlockState;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarFlag;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.BossBar;
import org.bukkit.boss.KeyedBossBar;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.metadata.FixedMetadataValue;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.DisplaySlot;
import org.bukkit.scoreboard.NameTagVisibility;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.RenderType;
import org.bukkit.scoreboard.Score;
import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.ScoreboardManager;
import org.bukkit.scoreboard.Team;
import org.junit.jupiter.api.Test;

@SuppressWarnings("removal")
class PaperApiSurfaceTest {

    @Test
    void worldBorderSupportsConfigurationContainmentAndReset() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerriteWorld other = server.runtime().world("missing");
            assertNull(other);

            WorldBorder border = world.getWorldBorder();
            assertNotNull(border);
            assertSame(border, world.getWorldBorder());
            assertSame(world, border.getWorld());

            assertEquals(border.getMaxSize(), border.getSize());
            assertEquals(0.0, border.getCenter().getX());
            assertEquals(0.0, border.getCenter().getZ());
            assertEquals(5.0, border.getDamageBuffer());
            assertEquals(0.2, border.getDamageAmount());
            assertEquals(15, border.getWarningTime());
            assertEquals(300, border.getWarningTimeTicks());
            assertEquals(5, border.getWarningDistance());

            border.setCenter(100.0, -50.0);
            Location centerCopy = border.getCenter();
            assertEquals(100.0, centerCopy.getX());
            assertEquals(-50.0, centerCopy.getZ());
            centerCopy.setX(9999.0);
            assertEquals(100.0, border.getCenter().getX());

            border.setCenter(new Location(world, 20.0, 64.0, -10.0));
            border.setSize(50.0);
            assertEquals(50.0, border.getSize());
            assertTrue(border.isInside(new Location(world, 20.0, 64.0, -10.0)));
            assertTrue(border.isInside(new Location(world, 45.0, 64.0, 15.0)));
            assertFalse(border.isInside(new Location(world, 45.1, 64.0, -10.0)));
            assertFalse(border.isInside(new Location(world, 20.0, 64.0, -35.1)));

            border.setSize(120.0, 5L);
            assertEquals(120.0, border.getSize());
            border.setSize(80.0, TimeUnit.SECONDS, 2L);
            assertEquals(80.0, border.getSize());
            border.changeSize(0.0, 10L);
            assertEquals(1.0, border.getSize());
            border.changeSize(1.0e12, 0L);
            assertEquals(border.getMaxSize(), border.getSize());

            border.setDamageBuffer(2.5);
            border.setDamageAmount(1.75);
            border.setWarningDistance(12);
            border.setWarningTime(30);
            assertEquals(2.5, border.getDamageBuffer());
            assertEquals(1.75, border.getDamageAmount());
            assertEquals(12, border.getWarningDistance());
            assertEquals(30, border.getWarningTime());
            assertEquals(600, border.getWarningTimeTicks());

            border.setWarningTimeTicks(100);
            assertEquals(100, border.getWarningTimeTicks());
            assertEquals(5, border.getWarningTime());

            assertThrows(IllegalArgumentException.class, () -> border.changeSize(Double.NaN, 0L));
            assertThrows(IllegalArgumentException.class, () -> border.changeSize(10.0, -1L));
            assertThrows(IllegalArgumentException.class, () -> border.setDamageAmount(-0.1));
            assertThrows(IllegalArgumentException.class, () -> border.setDamageBuffer(-1.0));
            assertThrows(IllegalArgumentException.class, () -> border.setWarningDistance(-1));
            assertThrows(IllegalArgumentException.class, () -> border.setWarningTimeTicks(-1));

            border.reset();
            assertEquals(border.getMaxSize(), border.getSize());
            assertEquals(0.0, border.getCenter().getX());
            assertEquals(0.0, border.getCenter().getZ());
            assertEquals(5.0, border.getDamageBuffer());
            assertEquals(0.2, border.getDamageAmount());
            assertEquals(300, border.getWarningTimeTicks());
            assertEquals(5, border.getWarningDistance());
        }
    }

    @Test
    @SuppressWarnings("deprecation")
    void blockStateSnapshotsUpdateSemanticsCopyAndMetadataWork() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("BlockStatePlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");

            Block block = world.getBlockAt(18, 64, -3);
            block.setType(Material.STONE);

            BlockState state = block.getState();
            assertTrue(state.isPlaced());
            assertEquals(Material.STONE, state.getType());
            assertEquals(Material.STONE, state.getBlockData().getMaterial());
            assertSame(world, state.getWorld());
            assertEquals(18, state.getX());
            assertEquals(64, state.getY());
            assertEquals(-3, state.getZ());
            assertEquals(block, state.getBlock());
            assertEquals(world.getChunkAt(1, -1), state.getChunk());
            assertEquals(new Location(world, 18, 64, -3), state.getLocation());

            Location reused = new Location(null, 0, 0, 0);
            assertSame(reused, state.getLocation(reused));
            assertEquals(18.0, reused.getX());

            // Modifying the snapshot does not touch the world until update.
            state.setType(Material.GOLD_BLOCK);
            assertEquals(Material.GOLD_BLOCK, state.getType());
            assertEquals(Material.STONE, block.getType());

            // Unforced update refuses when type changed relative to world block.
            assertFalse(state.update());
            assertEquals(Material.STONE, block.getType());

            // Forced update commits to the world.
            assertTrue(state.update(true, false));
            assertEquals(Material.GOLD_BLOCK, block.getType());

            // Same-type update succeeds with force=false.
            BlockState sameTypeState = block.getState();
            sameTypeState.setBlockData(Bukkit.createBlockData(Material.GOLD_BLOCK));
            assertTrue(sameTypeState.update());

            // External world change causes unforced update to return false.
            block.setType(Material.DIRT);
            assertFalse(sameTypeState.update(false));
            assertEquals(Material.DIRT, block.getType());
            assertTrue(sameTypeState.update(true));
            assertEquals(Material.GOLD_BLOCK, block.getType());

            // Unplaced copy and copy(Location).
            BlockState detached = state.copy();
            assertFalse(detached.isPlaced());
            assertEquals(Material.GOLD_BLOCK, detached.getType());
            assertThrows(IllegalStateException.class, detached::getWorld);
            assertThrows(IllegalStateException.class, detached::getBlock);

            Location targetLoc = new Location(world, 25, 70, 12);
            BlockState moved = state.copy(targetLoc);
            assertTrue(moved.isPlaced());
            assertEquals(25, moved.getX());
            assertEquals(70, moved.getY());
            assertEquals(12, moved.getZ());
            assertTrue(moved.update(true));
            assertEquals(Material.GOLD_BLOCK, world.getBlockAt(25, 70, 12).getType());

            // Metadata support on BlockState.
            state.setMetadata("tag", new FixedMetadataValue(plugin, "v1"));
            assertTrue(state.hasMetadata("tag"));
            assertEquals("v1", state.getMetadata("tag").get(0).asString());
            state.removeMetadata("tag", plugin);
            assertFalse(state.hasMetadata("tag"));

            // BlockData#createBlockState returns an unplaced state.
            BlockState fromData = Bukkit.createBlockData(Material.DIAMOND_BLOCK).createBlockState();
            assertFalse(fromData.isPlaced());
            assertEquals(Material.DIAMOND_BLOCK, fromData.getType());
            fromData.setRawData((byte) 3);
            assertEquals((byte) 3, fromData.getRawData());
        }
    }

    @Test
    @SuppressWarnings("deprecation")
    void scoreboardObjectivesTeamsScoresAndPlayerAssignmentWork() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            ScoreboardManager manager = server.runtime().server().getScoreboardManager();
            assertNotNull(manager);
            assertSame(manager.getMainScoreboard(), Bukkit.getScoreboardManager().getMainScoreboard());

            Scoreboard main = manager.getMainScoreboard();
            Scoreboard custom = manager.getNewScoreboard();
            assertNotSame(main, custom);

            // Criteria static constants and custom lookup.
            assertEquals("dummy", Criteria.DUMMY.getName());
            assertFalse(Criteria.DUMMY.isReadOnly());
            assertTrue(Criteria.HEALTH.isReadOnly());
            assertEquals(RenderType.HEARTS, Criteria.HEALTH.getDefaultRenderType());
            assertEquals("custom_stat", Criteria.create("custom_stat").getName());

            // Objectives & display slots.
            Objective kills = custom.registerNewObjective("kills", Criteria.DUMMY, Component.text("Kills"));
            assertEquals("kills", kills.getName());
            assertEquals(Component.text("Kills"), kills.displayName());
            assertEquals("Kills", kills.getDisplayName());
            assertEquals("dummy", kills.getCriteria());
            assertEquals(Criteria.DUMMY, kills.getTrackedCriteria());
            assertTrue(kills.isModifiable());
            assertEquals(RenderType.INTEGER, kills.getRenderType());

            kills.setDisplaySlot(DisplaySlot.SIDEBAR);
            assertEquals(DisplaySlot.SIDEBAR, kills.getDisplaySlot());
            assertSame(kills, custom.getObjective(DisplaySlot.SIDEBAR));

            Objective health = custom.registerNewObjective("hp", "health", "HP", RenderType.HEARTS);
            assertEquals(RenderType.HEARTS, health.getRenderType());
            assertFalse(health.isModifiable());
            health.setDisplaySlot(DisplaySlot.SIDEBAR);
            assertNull(kills.getDisplaySlot());
            assertSame(health, custom.getObjective(DisplaySlot.SIDEBAR));

            assertEquals(Set.of(kills), custom.getObjectivesByCriteria(Criteria.DUMMY));
            assertEquals(Set.of(health), custom.getObjectivesByCriteria("health"));
            assertEquals(2, custom.getObjectives().size());
            assertThrows(IllegalArgumentException.class,
                    () -> custom.registerNewObjective("kills", Criteria.DUMMY, Component.text("Dup")));

            custom.clearSlot(DisplaySlot.SIDEBAR);
            assertNull(health.getDisplaySlot());
            assertNull(custom.getObjective(DisplaySlot.SIDEBAR));

            // Scores, OfflinePlayer, and Entity entries.
            FerritePlayer alice = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "Alice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0, 64, 0)));
            Entity pig = FerriteEntitySnapshot.create(
                    world, UUID.randomUUID(), EntityType.PIG, "Pig", null, 1, 64, 1, 0, 0, 0, 0, 0, 10.0, 77);

            Score aliceScore = kills.getScore(alice);
            assertFalse(aliceScore.isScoreSet());
            assertEquals(0, aliceScore.getScore());
            aliceScore.setScore(15);
            assertTrue(aliceScore.isScoreSet());
            assertEquals(15, kills.getScore("Alice").getScore());
            aliceScore.customName(Component.text("Alice Label"));
            aliceScore.numberFormat(NumberFormat.blank());
            aliceScore.setTriggerable(true);
            assertEquals(Component.text("Alice Label"), aliceScore.customName());
            assertNotNull(aliceScore.numberFormat());
            assertTrue(aliceScore.isTriggerable());

            Score pigScore = kills.getScoreFor(pig);
            pigScore.setScore(3);
            assertEquals(Set.of("Alice", pig.getUniqueId().toString()), custom.getEntries());
            assertEquals(2, custom.getScores(alice).size());
            assertEquals(2, custom.getScoresFor(pig).size());

            custom.resetScoresFor(pig);
            assertFalse(kills.getScoreFor(pig).isScoreSet());
            assertEquals(Set.of("Alice"), custom.getEntries());

            // Teams & single-team-per-entry invariant.
            Team red = custom.registerNewTeam("red");
            Team blue = custom.registerNewTeam("blue");
            assertThrows(IllegalArgumentException.class, () -> custom.registerNewTeam("red"));

            red.displayName(Component.text("Red Team"));
            red.prefix(Component.text("[Red] "));
            red.suffix(Component.text("!"));
            assertFalse(red.hasColor());
            assertEquals(ChatColor.RESET, red.getColor());
            assertThrows(IllegalStateException.class, red::color);
            red.color(NamedTextColor.RED);
            assertTrue(red.hasColor());
            assertEquals(NamedTextColor.RED, red.color());
            assertEquals(ChatColor.RED, red.getColor());
            red.setColor(ChatColor.BLUE);
            assertEquals(NamedTextColor.BLUE, red.color());

            red.setAllowFriendlyFire(false);
            red.setCanSeeFriendlyInvisibles(false);
            assertFalse(red.allowFriendlyFire());
            assertFalse(red.canSeeFriendlyInvisibles());

            red.setNameTagVisibility(NameTagVisibility.HIDE_FOR_OTHER_TEAMS);
            assertEquals(NameTagVisibility.HIDE_FOR_OTHER_TEAMS, red.getNameTagVisibility());
            assertEquals(Team.OptionStatus.FOR_OTHER_TEAMS, red.getOption(Team.Option.NAME_TAG_VISIBILITY));
            red.setOption(Team.Option.COLLISION_RULE, Team.OptionStatus.NEVER);
            assertEquals(Team.OptionStatus.NEVER, red.getOption(Team.Option.COLLISION_RULE));

            red.addPlayer(alice);
            red.addEntity(pig);
            assertTrue(red.hasPlayer(alice));
            assertTrue(red.hasEntity(pig));
            assertSame(red, custom.getPlayerTeam(alice));
            assertSame(red, custom.getEntityTeam(pig));
            assertEquals(2, red.getSize());
            List<Object> teamAudiences = new java.util.ArrayList<>();
            red.audiences().forEach(teamAudiences::add);
            assertEquals(List.of(alice), teamAudiences);

            // Moving Alice to blue removes her from red.
            blue.addPlayer(alice);
            assertFalse(red.hasPlayer(alice));
            assertTrue(blue.hasPlayer(alice));
            assertSame(blue, custom.getPlayerTeam(alice));

            assertTrue(blue.removePlayer(alice));
            assertFalse(blue.removePlayer(alice));
            assertNull(custom.getPlayerTeam(alice));

            // Player scoreboard binding.
            assertSame(main, alice.getScoreboard());
            alice.setScoreboard(custom);
            assertSame(custom, alice.getScoreboard());
            assertThrows(IllegalArgumentException.class, () -> alice.setScoreboard(null));

            // Unregistering objectives and teams cleans up and rejects further mutations.
            kills.unregister();
            assertNull(custom.getObjective("kills"));
            assertThrows(IllegalStateException.class, () -> kills.getScore("Alice"));
            red.unregister();
            assertNull(custom.getTeam("red"));
            assertNull(custom.getEntityTeam(pig));
            assertThrows(IllegalStateException.class, () -> red.addEntry("Bob"));
        }
    }

    @Test
    void bossBarSupportsLifecycleFlagsProgressAndPlayers() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer alice = server.onMain(() -> server.runtime().createPlayer(
                    "BossViewer", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0, 64, 0));

            BossBar bar = server.runtime().server().createBossBar(
                    "Dragon", BarColor.PURPLE, BarStyle.SEGMENTED_10, BarFlag.DARKEN_SKY);
            assertEquals("Dragon", bar.getTitle());
            assertEquals(BarColor.PURPLE, bar.getColor());
            assertEquals(BarStyle.SEGMENTED_10, bar.getStyle());
            assertTrue(bar.hasFlag(BarFlag.DARKEN_SKY));
            assertFalse(bar.hasFlag(BarFlag.CREATE_FOG));
            assertEquals(1.0, bar.getProgress());
            assertTrue(bar.isVisible());

            bar.setTitle("Updated Dragon");
            bar.setColor(BarColor.RED);
            bar.setStyle(BarStyle.SOLID);
            bar.addFlag(BarFlag.PLAY_BOSS_MUSIC);
            bar.removeFlag(BarFlag.DARKEN_SKY);
            bar.setProgress(0.45);
            bar.hide();

            assertEquals("Updated Dragon", bar.getTitle());
            assertEquals(BarColor.RED, bar.getColor());
            assertEquals(BarStyle.SOLID, bar.getStyle());
            assertFalse(bar.hasFlag(BarFlag.DARKEN_SKY));
            assertTrue(bar.hasFlag(BarFlag.PLAY_BOSS_MUSIC));
            assertEquals(0.45, bar.getProgress(), 1e-9);
            assertFalse(bar.isVisible());
            bar.show();
            assertTrue(bar.isVisible());

            bar.addPlayer(alice);
            bar.addPlayer(alice);
            assertEquals(List.of(alice), bar.getPlayers());
            bar.removePlayer(alice);
            assertTrue(bar.getPlayers().isEmpty());
            bar.addPlayer(alice);
            bar.removeAll();
            assertTrue(bar.getPlayers().isEmpty());

            assertThrows(IllegalArgumentException.class, () -> bar.setProgress(-0.01));
            assertThrows(IllegalArgumentException.class, () -> bar.setProgress(1.01));
            assertThrows(IllegalArgumentException.class, () -> bar.setProgress(Double.NaN));

            NamespacedKey key = new NamespacedKey("ferrite", "world_boss");
            KeyedBossBar keyed = Bukkit.createBossBar(key, "Keyed", BarColor.BLUE, BarStyle.SEGMENTED_6);
            assertEquals(key, keyed.getKey());
            assertSame(keyed, Bukkit.getBossBar(key));
            assertTrue(Bukkit.getBossBars().hasNext());
            assertTrue(Bukkit.removeBossBar(key));
            assertNull(Bukkit.getBossBar(key));
        }
    }

    @Test
    @SuppressWarnings("deprecation")
    void playerTitlesActionBarsSoundsAndParticlesAreSafeAndStateBacked() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().playerJoined(
                    server.runtime().createPlayer(
                            "FeedbackPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 3.0, 64.0, -2.0)));
            PlayerState state = player.state();

            // Titles (Bukkit, Paper, Bungee, Adventure).
            player.sendTitle("Hello", "World", 5, 40, 10);
            assertNotNull(state.lastTitle());
            assertEquals("Hello", state.lastTitle().title());
            assertEquals("World", state.lastTitle().subtitle());
            assertEquals(5, state.lastTitle().fadeIn());
            assertEquals(40, state.lastTitle().stay());
            assertEquals(10, state.lastTitle().fadeOut());

            player.sendTitle("Quick", "Sub");
            assertEquals("Quick", state.lastTitle().title());
            assertEquals("Sub", state.lastTitle().subtitle());

            player.sendTitle(new com.destroystokyo.paper.Title("PaperTitle", "PaperSub", 2, 15, 3));
            assertEquals("§fPaperTitle", state.lastTitle().title());
            assertEquals("§fPaperSub", state.lastTitle().subtitle());
            assertEquals(15, state.lastTitle().stay());

            player.showTitle(Title.title(
                    Component.text("AdvTitle"),
                    Component.text("AdvSub"),
                    Title.Times.times(Duration.ofMillis(250), Duration.ofSeconds(2), Duration.ofMillis(500))));
            assertEquals("AdvTitle", state.lastTitle().title());
            assertEquals("AdvSub", state.lastTitle().subtitle());
            assertEquals(5, state.lastTitle().fadeIn());
            assertEquals(40, state.lastTitle().stay());
            assertEquals(10, state.lastTitle().fadeOut());

            player.sendTitlePart(TitlePart.SUBTITLE, Component.text("PartSub"));
            player.sendTitlePart(TitlePart.TITLE, Component.text("PartTitle"));
            assertEquals("PartTitle", state.lastTitle().title());
            assertEquals("PartSub", state.lastTitle().subtitle());

            player.clearTitle();
            assertNull(state.lastTitle());
            player.resetTitle();
            assertEquals(10, state.titleFadeIn());
            assertEquals(70, state.titleStay());
            assertEquals(20, state.titleFadeOut());

            // Action bars.
            player.sendActionBar("Legacy Bar");
            player.sendActionBar('&', "&aGreen Bar");
            player.sendActionBar(new TextComponent("Bungee Bar"));
            player.sendActionBar(Component.text("Adv Bar"));
            assertEquals(List.of("Legacy Bar", "§aGreen Bar", "§fBungee Bar", "Adv Bar"), state.actionBars());
            assertEquals("Adv Bar", state.lastActionBar());

            // Sounds (Bukkit Location/Entity + Adventure).
            Location loc = new Location(world, 10.0, 65.0, 20.0);
            player.playSound(loc, Sound.ENTITY_EXPERIENCE_ORB_PICKUP, 0.8f, 1.2f);
            player.playSound(loc, "minecraft:block.note_block.harp", SoundCategory.RECORDS, 1.0f, 1.0f, 42L);
            player.playSound(player, Sound.UI_BUTTON_CLICK, SoundCategory.UI, 0.5f, 1.5f);
            player.playSound(net.kyori.adventure.sound.Sound.sound(
                    Sound.ENTITY_PLAYER_LEVELUP.key(),
                    net.kyori.adventure.sound.Sound.Source.PLAYER,
                    1.0f,
                    1.0f));
            assertEquals(4, state.playedSounds().size());
            assertEquals("minecraft:entity.experience_orb.pickup", state.playedSounds().get(0).sound());
            assertEquals(SoundCategory.MASTER, state.playedSounds().get(0).category());
            assertEquals(42L, state.playedSounds().get(1).seed());
            assertEquals(SoundCategory.UI, state.playedSounds().get(2).category());
            assertEquals(SoundCategory.PLAYERS, state.playedSounds().get(3).category());

            player.stopSound(Sound.UI_BUTTON_CLICK);
            player.stopAllSounds();
            assertTrue(state.playedSounds().isEmpty());

            // Particles.
            player.spawnParticle(Particle.FLAME, loc, 5);
            player.spawnParticle(
                    Particle.DUST,
                    1.0,
                    66.0,
                    2.0,
                    8,
                    0.1,
                    0.2,
                    0.3,
                    0.05,
                    new Particle.DustOptions(Color.RED, 1.5f),
                    true);
            assertEquals(2, state.spawnedParticles().size());
            assertEquals(Particle.FLAME, state.spawnedParticles().get(0).particle());
            assertEquals(5, state.spawnedParticles().get(0).count());
            assertEquals(Particle.DUST, state.spawnedParticles().get(1).particle());
            assertTrue(state.spawnedParticles().get(1).force());
            assertThrows(IllegalArgumentException.class, () -> player.spawnParticle(Particle.FLAME, loc, -1));
        }
    }
}
